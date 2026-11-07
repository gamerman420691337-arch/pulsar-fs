//! Owned Unix listeners, connections and signal-driven Engine retirement.
use crate::{artifacts, task_group::TaskGroup, Engine, EngineConfig};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use pulsar_protocol::{local_socket_prelude::*, *};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
#[cfg(test)]
#[path = "daemon_test_response_gate.rs"]
mod test_response_gate;
#[cfg(test)]
pub(crate) use test_response_gate::{
    TestResponseGateHandle, TestResponseGateRelease, TestResponseGateScope,
    TestResponseGateSnapshot, TestResponseGateTarget,
};

struct Signals(Vec<signal_hook_registry::SigId>);
impl Signals {
    fn install(stop: &Arc<AtomicBool>) -> Result<Self> {
        let mut owned = Self(Vec::new());
        for signal in [libc::SIGTERM, libc::SIGINT] {
            let stop = stop.clone();
            // The handler only performs a lock-free atomic store. All cleanup
            // executes on ordinary threads after the listener loop observes it.
            owned.0.push(unsafe {
                signal_hook_registry::register(signal, move || stop.store(true, Ordering::Release))?
            });
        }
        Ok(owned)
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook_registry::unregister(id);
        }
    }
}

struct EndpointPath {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Drop for EndpointPath {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|m| {
            m.file_type().is_socket() && m.dev() == self.device && m.ino() == self.inode
        }) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Default)]
struct Connections {
    sockets: Mutex<HashMap<u64, OwnedFd>>,
    tasks: TaskGroup,
    #[cfg(test)]
    response_gate: Option<Arc<test_response_gate::ResponseGate>>,
}
struct ConnectionSlot {
    owner: Arc<Connections>,
    id: u64,
}
struct ConnectionCleanup(Arc<Connections>);
impl Drop for ConnectionCleanup {
    fn drop(&mut self) {
        let _ = self.0.finish();
    }
}
impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.owner
            .sockets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}
impl Connections {
    fn interrupt(&self) {
        #[cfg(test)]
        if let Some(gate) = &self.response_gate {
            gate.endpoint_stopping();
        }
        let sockets = self.sockets.lock().unwrap_or_else(|e| e.into_inner());
        for socket in sockets.values() {
            // This duplicate descriptor remains owned under the registry guard,
            // so concurrent stream retirement cannot redirect shutdown to a reused fd.
            unsafe {
                libc::shutdown(socket.as_raw_fd(), libc::SHUT_RDWR);
            }
        }
    }
    fn finish(&self) -> Result<()> {
        self.tasks.close();
        self.interrupt();
        self.tasks.join().context("join IPC connection workers")
    }
}

pub(crate) struct Endpoint {
    stop: Arc<AtomicBool>,
    connections: Arc<Connections>,
    listener: Option<JoinHandle<Result<()>>>,
}
impl Endpoint {
    pub(crate) fn start(
        engine: Arc<Engine>,
        path: &Path,
        name: &'static str,
        maximum: usize,
        handle: fn(&Arc<Engine>, &mut LocalStream),
    ) -> Result<Self> {
        Self::start_inner(
            engine,
            path,
            name,
            maximum,
            handle,
            #[cfg(test)]
            None,
        )
    }
    fn start_inner(
        engine: Arc<Engine>,
        path: &Path,
        name: &'static str,
        maximum: usize,
        handle: fn(&Arc<Engine>, &mut LocalStream),
        #[cfg(test)] response_gate: Option<Arc<test_response_gate::ResponseGate>>,
    ) -> Result<Self> {
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
                bail!("refusing to replace an untrusted engine endpoint");
            }
            fs::remove_file(path)?;
        }
        let mut listener = ListenerOptions::new()
            .name(endpoint_name(path)?)
            .nonblocking(interprocess::local_socket::ListenerNonblockingMode::Accept)
            .create_sync()?;
        listener.do_not_reclaim_name_on_drop();
        let meta = fs::symlink_metadata(path)?;
        let path = EndpointPath {
            path: path.to_owned(),
            device: meta.dev(),
            inode: meta.ino(),
        };
        fs::set_permissions(&path.path, fs::Permissions::from_mode(0o600))?;
        artifacts::sync_dir(path.path.parent().context("endpoint parent absent")?)?;
        let stop = Arc::new(AtomicBool::new(false));
        let closing = stop.clone();
        let connections = Arc::new(Connections {
            #[cfg(test)]
            response_gate,
            ..Connections::default()
        });
        let clients = connections.clone();
        let listener = thread::Builder::new().name(name.into()).spawn(move || {
            let _cleanup = ConnectionCleanup(clients.clone());
            let _path = path;
            let outcome = (|| -> Result<()> {
                let mut sequence = 0u64;
                while !closing.load(Ordering::Acquire) && !engine.is_shutting_down() {
                    let mut stream = match listener.accept() {
                        Ok(stream) => stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(e.into()),
                    };
                    if closing.load(Ordering::Acquire) || engine.is_shutting_down() {
                        break;
                    }
                    let mut sockets = clients
                        .sockets
                        .lock()
                        .map_err(|_| anyhow::anyhow!("connection registry poisoned"))?;
                    if sockets.len() >= maximum {
                        continue;
                    }
                    let descriptor = match &stream {
                        LocalStream::UdSocket(socket) => socket.as_fd().try_clone_to_owned()?,
                    };
                    sequence = sequence
                        .checked_add(1)
                        .context("connection sequence exhausted")?;
                    sockets.insert(sequence, descriptor);
                    let slot = ConnectionSlot {
                        owner: clients.clone(),
                        id: sequence,
                    };
                    drop(sockets);
                    let engine = engine.clone();
                    #[cfg(test)]
                    let response_gate = clients.response_gate.clone();
                    clients.tasks.spawn(name, None, move || {
                        let _slot = slot;
                        #[cfg(test)]
                        if let Some(gate) = &response_gate {
                            control_inner(&engine, &mut stream, Some(gate));
                            return;
                        }
                        handle(&engine, &mut stream);
                    })?;
                }
                Ok(())
            })();
            drop(listener);
            let joined = clients.finish();
            outcome.and(joined)
        })?;
        Ok(Self {
            stop,
            connections,
            listener: Some(listener),
        })
    }
    fn finished(&self) -> bool {
        self.listener.as_ref().is_none_or(|t| t.is_finished())
    }
    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.connections.interrupt();
    }
    fn join(&mut self) -> Result<()> {
        self.request_stop();
        match self.listener.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| anyhow::anyhow!("IPC listener panicked"))?,
            None => Ok(()),
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

fn control(engine: &Arc<Engine>, stream: &mut LocalStream) {
    control_inner(
        engine,
        stream,
        #[cfg(test)]
        None,
    );
}
fn control_inner(
    engine: &Arc<Engine>,
    stream: &mut LocalStream,
    #[cfg(test)] response_gate: Option<&test_response_gate::ResponseGate>,
) {
    while !engine.is_shutting_down() {
        let mut reader = DeadlineStream::new(stream, Instant::now() + Duration::from_secs(30));
        let frame = match read_request(&mut reader) {
            Ok(frame) => frame,
            Err(_) => break,
        };
        #[cfg(test)]
        let received_at = Instant::now();
        if engine.is_shutting_down() {
            break;
        }
        #[cfg(test)]
        let mut captured_write = None;
        let response = match frame {
            RequestFrame::Current(request) => {
                #[cfg(test)]
                let selected = response_gate.and_then(|gate| gate.select(&request));
                let response = engine.handle(*request);
                #[cfg(test)]
                if let (Some(gate), Some(request_id)) = (response_gate, selected) {
                    // Engine handling has returned: no authority or peer guard
                    // is retained while this test-only wire response is held.
                    captured_write = gate.after_handle(engine, request_id, &response, received_at);
                }
                response
            }
            RequestFrame::Rejected { request_id, error } => Response::failure(request_id, error),
        };
        let mut writer = DeadlineStream::new(stream, Instant::now() + Duration::from_secs(30));
        let written = write_message(&mut writer, &response);
        #[cfg(test)]
        if let (Some(gate), Some(captured)) = (response_gate, captured_write) {
            gate.after_write(captured, written.is_ok());
        }
        if written.is_err() {
            break;
        }
    }
}

pub(crate) fn run(config: EngineConfig) -> Result<()> {
    crate::portable_fallback_reader::preflight_main_header(&config.state_dir)?;
    artifacts::private_directory(&config.state_dir)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(config.state_dir.join("engine.lock"))?;
    lock.try_lock_exclusive()
        .context("another per-user engine already owns this state directory")?;
    let stop = Arc::new(AtomicBool::new(false));
    let _signals = Signals::install(&stop)?;
    let engine = Engine::open_with_shutdown(config, stop.clone())?;
    let mut endpoints = Vec::new();
    let outcome = (|| -> Result<()> {
        endpoints.push(Endpoint::start(
            engine.clone(),
            &engine.config.endpoint(),
            "pulsar-control",
            32,
            control,
        )?);
        endpoints.push(crate::bulk::start(engine.clone())?);
        while !stop.load(Ordering::Acquire) {
            if endpoints.iter().any(Endpoint::finished) {
                bail!("IPC listener stopped unexpectedly");
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    engine.request_shutdown();
    for endpoint in &endpoints {
        endpoint.request_stop();
    }
    let mut failure = outcome.err();
    for endpoint in &mut endpoints {
        if let Err(error) = endpoint.join() {
            failure.get_or_insert(error);
        }
    }
    if let Err(error) = engine.finish_shutdown() {
        failure.get_or_insert(error);
    }
    drop(endpoints);
    drop(engine);
    // The instance lock remains held until every owned listener and task has joined.
    drop(lock);
    failure.map_or(Ok(()), Err)
}

/// Owned listeners for an already-open test Engine. The ignored rendered
/// fixture installs only its connection-local test permits before exposing IPC.
#[cfg(test)]
pub(crate) struct TestEndpoints(Vec<Endpoint>);
#[cfg(test)]
impl TestEndpoints {
    pub(crate) fn start(engine: Arc<Engine>) -> Result<Self> {
        anyhow::ensure!(
            !engine.config.endpoint().exists() && !engine.config.bulk_endpoint().exists(),
            "fresh test IPC paths required"
        );
        let mut owned = Self(Vec::new());
        owned.0.push(Endpoint::start(
            engine.clone(),
            &engine.config.endpoint(),
            "pulsar-test-control",
            32,
            control,
        )?);
        owned.0.push(crate::bulk::start(engine)?);
        Ok(owned)
    }
    pub(crate) fn start_with_response_gate(
        engine: Arc<Engine>,
        scope: TestResponseGateScope,
        maximum_hold: Duration,
    ) -> Result<(Self, TestResponseGateHandle)> {
        anyhow::ensure!(
            !engine.config.endpoint().exists() && !engine.config.bulk_endpoint().exists(),
            "fresh test IPC paths required"
        );
        let (gate, handle) = test_response_gate::ResponseGate::new(scope, maximum_hold)?;
        let mut owned = Self(Vec::new());
        owned.0.push(Endpoint::start_inner(
            engine.clone(),
            &engine.config.endpoint(),
            "pulsar-test-gated-control",
            32,
            control,
            Some(gate),
        )?);
        owned.0.push(crate::bulk::start(engine)?);
        Ok((owned, handle))
    }
    pub(crate) fn finish(&mut self) -> Result<()> {
        for endpoint in &self.0 {
            endpoint.request_stop();
        }
        let mut failure = None;
        for endpoint in &mut self.0 {
            if let Err(error) = endpoint.join() {
                failure.get_or_insert(error);
            }
        }
        self.0.clear();
        failure.map_or(Ok(()), Err)
    }
}
#[cfg(test)]
impl Drop for TestEndpoints {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
