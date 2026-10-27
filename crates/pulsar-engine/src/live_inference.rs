//! Persistent confined inference transport. Callers own capture authority and
//! result freshness; this module never commits motion or dispatches a device.
use crate::{live_frames::LiveRgbWork, resources::ResourcePool, EngineConfig};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
/// The ordinary live worker uses host runtime libraries. A selected captured
/// CPU image must never be downgraded to this route, including direct API calls.
pub(crate) fn require_host_cpu_runtime(config: &EngineConfig) -> Result<(), ProtocolError> {
    if config.cpu_owner_policy.is_some() {
        return Err(ProtocolError::unsupported(
            "ordinary live worker cannot use a configured captured CPU runtime",
        ));
    }
    Ok(())
}

fn error(message: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message.to_string())
}
fn profile_hash(manifest: &LiveInferenceManifest) -> Result<String, ProtocolError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest.profile).map_err(error)?)
    ))
}
fn pixels_identity(bytes: &[u8]) -> ArtifactIdentity {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).expect("hash"),
        sha256,
        byte_len: bytes.len() as u64,
    }
}
struct Input {
    header: LiveInferenceInput,
    pixels: Box<[u8]>,
}
/// Process-local observation completed only by the last actual lifecycle
/// asset owner. Cancellation, a timeout and public status cannot set it.
#[derive(Clone)]
pub(crate) struct CpuWorkerRetirement(Arc<AtomicBool>);
pub(crate) struct CpuRetirementGuard(Arc<AtomicBool>);
impl CpuWorkerRetirement {
    pub(crate) fn new() -> (Self, Arc<CpuRetirementGuard>) {
        let joined = Arc::new(AtomicBool::new(false));
        (Self(joined.clone()), Arc::new(CpuRetirementGuard(joined)))
    }
    pub(crate) fn joined(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub(crate) fn wait(&self, deadline: Instant) -> Result<(), ProtocolError> {
        while !self.joined() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| error("original CPU worker retirement remains unresolved"))?;
            std::thread::park_timeout(remaining.min(Duration::from_millis(1)));
        }
        Ok(())
    }
}
impl Drop for CpuRetirementGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
#[cfg(target_os = "linux")]
pub(crate) struct CpuWorkerTracking {
    pub fence: Option<Arc<crate::worker_fence::WorkerFence>>,
    pub assets: crate::worker_retirement::RetirementAssets,
}
/// Constructed only after this exact CPU worker's process, transport threads and
/// native resource custody have joined. History cannot recreate this proof.
pub(crate) struct CpuInputJoined {
    identity: LiveIdentity,
    attempt: AttemptId,
    profile_sha256: String,
    sequence: u64,
    completion: CpuInputCompletion,
}
enum CpuInputCompletion {
    HostRetired,
    #[allow(dead_code)] // Production captured execution is Linux-only.
    CapturedClean {
        manifest_sha256: String,
    },
}
impl CpuInputJoined {
    pub(crate) fn matches(
        &self,
        identity: &LiveIdentity,
        attempt: &AttemptId,
        profile: &str,
        completed: u64,
    ) -> bool {
        matches!(self.completion, CpuInputCompletion::HostRetired)
            && &self.identity == identity
            && &self.attempt == attempt
            && self.profile_sha256 == profile
            && self.sequence == completed
    }
    /// Captured input completion additionally requires observed clean stdout
    /// EOF and successful original child exit before whole-tree retirement.
    pub(crate) fn matches_captured(
        &self,
        identity: &LiveIdentity,
        attempt: &AttemptId,
        profile: &str,
        manifest_sha256: &str,
        completed: u64,
    ) -> bool {
        matches!(&self.completion, CpuInputCompletion::CapturedClean { manifest_sha256: original } if original == manifest_sha256)
            && &self.identity == identity
            && &self.attempt == attempt
            && self.profile_sha256 == profile
            && self.sequence == completed
    }
    #[cfg(test)]
    pub(crate) fn authored_for_test(
        identity: LiveIdentity,
        attempt: AttemptId,
        profile_sha256: String,
        sequence: u64,
    ) -> Self {
        Self {
            identity,
            attempt,
            profile_sha256,
            sequence,
            completion: CpuInputCompletion::HostRetired,
        }
    }
}

#[cfg(target_os = "linux")]
enum ReplyBinding {
    Host,
    Captured(Box<LiveCpuReplyBinding>),
}
#[cfg(target_os = "linux")]
impl ReplyBinding {
    fn captured_sha256(&self) -> Option<&str> {
        match self {
            Self::Host => None,
            Self::Captured(binding) => Some(binding.manifest_sha256()),
        }
    }
    fn ready(
        &self,
        manifest: &LiveInferenceManifest,
        reply: &LiveInferenceReply,
    ) -> Result<(), ProtocolError> {
        match self {
            Self::Captured(binding) => binding
                .validate_ready(reply)
                .map_err(|error| ProtocolError::new(ErrorCode::DependencyMismatch, error.message)),
            Self::Host => match reply {
                LiveInferenceReply::Ready {
                    attempt_id,
                    identity,
                    profile_sha256,
                } if attempt_id == &manifest.attempt_id
                    && identity == &manifest.identity
                    && profile_sha256 == &profile_hash(manifest)? =>
                {
                    Ok(())
                }
                _ => Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "live worker readiness identity differs",
                )),
            },
        }
    }
    fn estimated(
        &self,
        manifest: &LiveInferenceManifest,
        input: &LiveInferenceInput,
        reply: &LiveInferenceReply,
    ) -> Result<(), ProtocolError> {
        match self {
            Self::Captured(binding) => binding
                .validate_estimated(input, reply)
                .map_err(|error| ProtocolError::new(ErrorCode::DependencyMismatch, error.message)),
            Self::Host => match reply {
                LiveInferenceReply::Estimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input: observed,
                    ..
                } if attempt_id == &manifest.attempt_id
                    && identity == &manifest.identity
                    && profile_sha256 == &profile_hash(manifest)?
                    && observed == input =>
                {
                    Ok(())
                }
                _ => Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "live worker result differs from issued identity/profile/pixels",
                )),
            },
        }
    }
}

#[cfg(target_os = "linux")]
fn inherited_live_deadline(end: u64, budget_ms: u64) -> Result<Instant, ProtocolError> {
    if !(1..=30_000).contains(&budget_ms) {
        return Err(ProtocolError::invalid(
            "captured live CPU deadline exceeds original live bound",
        ));
    }
    crate::cuda_bootstrap::deadline_instant(end, budget_ms).map_err(error)
}

#[cfg(target_os = "linux")]
fn clean_stream_eof(reader: &mut impl std::io::Read) -> Result<(), ProtocolError> {
    let mut extra = [0u8; 1];
    if reader.read(&mut extra).map_err(error)? != 0 {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "captured CPU stream has undeclared trailing output",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn require_clean_completion_live(
    cancel: &AtomicBool,
    stop: &AtomicBool,
    deadline: Instant,
) -> Result<(), ProtocolError> {
    if cancel.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "captured CPU clean completion cancelled",
        ));
    }
    if Instant::now() >= deadline {
        return Err(error("captured CPU clean completion deadline exceeded"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn require_successful_captured_exit(status: std::process::ExitStatus) -> Result<(), ProtocolError> {
    if !status.success() {
        return Err(error(format!(
            "captured CPU final verification exited {status}"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub struct LiveInferenceWorker {
    manifest: LiveInferenceManifest,
    reply_binding: ReplyBinding,
    sequence: u64,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    child: Arc<std::sync::Mutex<std::process::Child>>,
    group: Arc<crate::cgroup::AttemptCgroup>,
    input: Option<std::sync::mpsc::SyncSender<Input>>,
    output: Option<std::sync::mpsc::Receiver<Result<LiveInferenceReply, ProtocolError>>>,
    completion: Option<std::sync::mpsc::Receiver<Result<(), ProtocolError>>>,
    custody: Option<crate::worker_retirement::WorkerCustody>,
}
#[cfg(not(target_os = "linux"))]
pub struct LiveInferenceWorker;
#[cfg(not(target_os = "linux"))]
impl LiveInferenceWorker {
    pub fn start(
        _config: &EngineConfig,
        _pool: &ResourcePool,
        _manifest: LiveInferenceManifest,
        _cancel: Arc<AtomicBool>,
    ) -> Result<Self, ProtocolError> {
        Err(ProtocolError::unsupported(
            "persistent native confinement on this platform",
        ))
    }
    pub fn start_in_lease(
        _config: &EngineConfig,
        _pool: &ResourcePool,
        _manifest: LiveInferenceManifest,
        _cancel: Arc<AtomicBool>,
        _origin: Instant,
        _lease_end: Instant,
    ) -> Result<Self, ProtocolError> {
        Err(ProtocolError::unsupported(
            "persistent native confinement on this platform",
        ))
    }
    pub fn estimate(&mut self, _work: &LiveRgbWork) -> Result<LiveInferenceReply, ProtocolError> {
        Err(ProtocolError::unsupported(
            "persistent native confinement on this platform",
        ))
    }
    pub fn process_id(&self) -> Result<u32, ProtocolError> {
        Err(ProtocolError::unsupported(
            "persistent native confinement on this platform",
        ))
    }
    pub fn peak_memory_bytes(&self) -> Result<u64, ProtocolError> {
        Err(ProtocolError::unsupported(
            "persistent native confinement on this platform",
        ))
    }
}
#[cfg(target_os = "linux")]
impl LiveInferenceWorker {
    pub fn start(
        config: &EngineConfig,
        pool: &ResourcePool,
        manifest: LiveInferenceManifest,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, ProtocolError> {
        manifest.validate()?;
        let origin = Instant::now();
        let end = origin + Duration::from_millis(manifest.budget.wall_time_ms);
        Self::start_in_lease(config, pool, manifest, cancel, origin, end)
    }
    /// Preserve the existing absolute lease end across staging and readiness.
    pub fn start_in_lease(
        config: &EngineConfig,
        pool: &ResourcePool,
        manifest: LiveInferenceManifest,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
    ) -> Result<Self, ProtocolError> {
        Self::start_owned(
            config,
            pool,
            manifest,
            cancel,
            origin,
            lease_end,
            CpuWorkerTracking {
                fence: None,
                assets: Vec::new(),
            },
        )
    }
    pub(crate) fn start_tracked_in_lease(
        config: &EngineConfig,
        pool: &ResourcePool,
        manifest: LiveInferenceManifest,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
        fence: Arc<crate::worker_fence::WorkerFence>,
    ) -> Result<Self, ProtocolError> {
        Self::start_owned(
            config,
            pool,
            manifest,
            cancel,
            origin,
            lease_end,
            CpuWorkerTracking {
                fence: Some(fence),
                assets: Vec::new(),
            },
        )
    }
    pub(crate) fn start_with_tracking(
        config: &EngineConfig,
        pool: &ResourcePool,
        manifest: LiveInferenceManifest,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
        tracking: CpuWorkerTracking,
    ) -> Result<Self, ProtocolError> {
        Self::start_owned(config, pool, manifest, cancel, origin, lease_end, tracking)
    }
    fn start_owned(
        config: &EngineConfig,
        pool: &ResourcePool,
        mut manifest: LiveInferenceManifest,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
        tracking: CpuWorkerTracking,
    ) -> Result<Self, ProtocolError> {
        manifest.validate()?;
        require_host_cpu_runtime(config)?;
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "live worker cancelled before launch",
            ));
        }
        if !crate::launch::available() {
            return Err(ProtocolError::unsupported("persistent worker confinement"));
        }
        if manifest.budget.memory_bytes > config.max_ram_bytes
            || Duration::from_millis(manifest.budget.wall_time_ms) > config.worker_timeout
        {
            return Err(ProtocolError::invalid(
                "live worker exceeds configured limits",
            ));
        }
        let deadline = crate::live_worker_lifetime::deadline(
            origin,
            lease_end,
            manifest.budget.wall_time_ms,
            Instant::now(),
        )?;
        let refs = [
            ArtifactReference {
                path: config.worker_executable.clone(),
                identity: manifest.worker.clone(),
            },
            manifest.model.clone(),
            manifest.runtime.clone(),
        ];
        let prepared = crate::live_staging::prepare_tracked(
            config,
            pool,
            refs,
            crate::live_staging::PreparationBudget {
                memory_bytes: manifest.budget.memory_bytes,
                cpu_threads: manifest.budget.cpu_threads,
            },
            cancel.clone(),
            deadline,
            crate::live_staging::PreparationTracking {
                fence: tracking.fence,
                assets: tracking.assets,
            },
        )?;
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "live staging cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Err(error("live staging deadline exceeded before launch"));
        }
        let mut config = config.clone();
        config.worker_executable = prepared.paths()[0].clone();
        manifest.model.path = "/input/model".into();
        manifest.runtime.path = "/input/runtime".into();
        let staged = prepared.paths().clone();
        let mut custody = prepared.into_worker(
            manifest.attempt_id.as_str(),
            manifest.budget.memory_bytes,
            manifest.budget.cpu_threads,
        )?;
        let group = custody.group_handle();
        let bindings = vec![
            (staged[1].clone(), manifest.model.path.clone()),
            (staged[2].clone(), manifest.runtime.path.clone()),
        ];
        let command = crate::confinement::command(
            &config,
            &manifest.budget,
            manifest.budget.memory_bytes,
            &group,
            &bindings,
            std::path::Path::new("/work"),
            "live-worker",
        )
        .map_err(error)?;
        let guest = manifest.clone();
        Self::start_transport(
            manifest,
            ReplyBinding::Host,
            cancel,
            deadline,
            custody,
            command,
            move |stdin| write_bulk_header(stdin, &guest).map_err(error),
        )
    }

    /// Consumes an already-owned captured mount plan. Its original absolute
    /// deadline is the only lifetime source; this never stages a host runtime.
    pub(crate) fn start_captured(
        plan: crate::live_cpu_mounts::OwnedLiveCpuMountCommand,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, ProtocolError> {
        let (command, custody, witness) = plan.into_parts();
        let binding = witness.manifest.reply_binding()?;
        if witness.manifest_sha256 != binding.manifest_sha256() {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "captured CPU mount manifest identity differs",
            ));
        }
        let deadline = inherited_live_deadline(
            witness.deadline_ns,
            witness.manifest.stream.budget.wall_time_ms,
        )?;
        let manifest = witness.manifest.stream.clone();
        Self::start_transport(
            manifest,
            ReplyBinding::Captured(Box::new(binding)),
            cancel,
            deadline,
            custody,
            command,
            move |stdin| write_message(stdin, &witness).map_err(error),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_transport(
        manifest: LiveInferenceManifest,
        reply_binding: ReplyBinding,
        cancel: Arc<AtomicBool>,
        deadline: Instant,
        mut custody: crate::worker_retirement::WorkerCustody,
        mut command: std::process::Command,
        initialize: impl FnOnce(&mut std::process::ChildStdin) -> Result<(), ProtocolError>
            + Send
            + 'static,
    ) -> Result<Self, ProtocolError> {
        use std::{
            io::{Read, Write},
            sync::mpsc,
        };
        let group = custody.group_handle();
        let captured = reply_binding.captured_sha256().is_some();
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "live worker cancelled before spawn",
            ));
        }
        if Instant::now() >= deadline {
            return Err(error("live worker deadline exceeded before spawn"));
        }
        custody.spawn(&mut command).map_err(error)?;
        let child = custody.child_handle();
        let pipes = {
            let mut child = custody.child();
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        };
        let (Some(mut stdin), Some(mut stdout), Some(stderr)) = pipes else {
            return Err(error("live worker pipes absent"));
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel::<Input>(1);
        let (reply_tx, output) = mpsc::sync_channel(1);
        let (completion_tx, completion) = mpsc::sync_channel(1);
        let transport = std::thread::Builder::new()
            .name("pulsar-live-transport".into())
            .spawn(move || {
                let init = (|| {
                    initialize(&mut stdin)?;
                    read_bulk_header(&mut stdout).map_err(error)
                })();
                let ok = init.is_ok();
                if reply_tx.send(init).is_err() || !ok {
                    return;
                }
                while let Ok(input) = rx.recv() {
                    let result = (|| {
                        write_bulk_header(&mut stdin, &input.header).map_err(error)?;
                        stdin.write_all(&input.pixels).map_err(error)?;
                        read_bulk_header(&mut stdout).map_err(error)
                    })();
                    let ok = result.is_ok();
                    if reply_tx.send(result).is_err() || !ok {
                        return;
                    }
                }
                if captured {
                    // Closing the input is a request for final verification, not
                    // cancellation. The original watchdog still bounds this read.
                    drop(stdin);
                    let result = clean_stream_eof(&mut stdout);
                    let _ = completion_tx.send(result);
                }
            })
            .map_err(error)?;
        custody.retain_thread(transport);
        let diagnostics = std::thread::Builder::new()
            .name("pulsar-live-diagnostics".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let _ = stderr.take(16 * 1024).read_to_end(&mut bytes);
            })
            .map_err(error)?;
        custody.retain_thread(diagnostics);
        let watchdog = {
            let child = child.clone();
            let group = group.clone();
            let stop = stop.clone();
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("pulsar-live-watchdog".into())
                .spawn(move || {
                    while !stop.load(Ordering::Acquire)
                        && !cancel.load(Ordering::Acquire)
                        && Instant::now() < deadline
                    {
                        if child.try_lock().is_ok_and(|mut child| {
                            child.try_wait().map_or(true, |status| status.is_some())
                        }) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    crate::worker_retirement::signal_and_poll_child(
                        &group,
                        &child,
                        Instant::now() + Duration::from_millis(250),
                    );
                })
                .map_err(error)?
        };
        custody.retain_thread(watchdog);
        let mut worker = Self {
            manifest,
            reply_binding,
            sequence: 0,
            deadline,
            cancel,
            stop,
            child,
            group,
            input: Some(tx),
            output: Some(output),
            completion: captured.then_some(completion),
            custody: Some(custody),
        };
        let ready = worker.receive()?;
        if let LiveInferenceReply::Error(error) = ready {
            return Err(error);
        }
        worker.reply_binding.ready(&worker.manifest, &ready)?;
        Ok(worker)
    }

    pub(crate) fn captured_manifest_sha256(&self) -> Option<&str> {
        self.reply_binding.captured_sha256()
    }

    fn receive(&mut self) -> Result<LiveInferenceReply, ProtocolError> {
        loop {
            if self.cancel.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
                return Err(ProtocolError::new(
                    ErrorCode::Cancelled,
                    "live worker cancelled or retired",
                ));
            }
            if Instant::now() >= self.deadline {
                return Err(error("live worker absolute deadline exceeded"));
            }
            match self
                .output
                .as_ref()
                .ok_or_else(|| error("live worker closed"))?
                .recv_timeout(Duration::from_millis(10))
            {
                Ok(reply) => return reply,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(error("live worker stream closed")),
            }
        }
    }
    pub fn process_id(&self) -> Result<u32, ProtocolError> {
        Ok(self.child.lock().map_err(error)?.id())
    }
    pub fn peak_memory_bytes(&self) -> Result<u64, ProtocolError> {
        self.group.peak_memory_bytes().map_err(error)
    }
    /// Ends only this worker, not the capture's source cancellation flag. The
    /// caller has already settled all input and direct delivery. Failed join
    /// retains exact custody and cannot produce a proof or sealed input.
    pub(crate) fn close_input_and_join(
        &mut self,
        deadline: Instant,
    ) -> Result<CpuInputJoined, ProtocolError> {
        if self.reply_binding.captured_sha256().is_some() {
            let result = self.close_captured_input_and_join(deadline.min(self.deadline));
            if result.is_err() {
                self.close();
            }
            return result;
        }
        self.stop.store(true, Ordering::Release);
        self.input.take();
        self.output.take();
        self.custody
            .as_mut()
            .ok_or_else(|| error("CPU input worker custody is absent"))?
            .retire(deadline.min(self.deadline))
            .map_err(error)?;
        self.custody.take();
        Ok(CpuInputJoined {
            identity: self.manifest.identity.clone(),
            attempt: self.manifest.attempt_id.clone(),
            profile_sha256: profile_hash(&self.manifest)?,
            sequence: self.sequence,
            completion: CpuInputCompletion::HostRetired,
        })
    }

    fn close_captured_input_and_join(
        &mut self,
        deadline: Instant,
    ) -> Result<CpuInputJoined, ProtocolError> {
        require_clean_completion_live(&self.cancel, &self.stop, deadline)?;
        self.input.take();
        self.output.take();
        loop {
            require_clean_completion_live(&self.cancel, &self.stop, deadline)?;
            match self
                .completion
                .as_ref()
                .ok_or_else(|| error("captured CPU completion transport absent"))?
                .recv_timeout(Duration::from_millis(10))
            {
                Ok(result) => {
                    result?;
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(error("captured CPU clean EOF was not observed")),
            }
        }
        loop {
            require_clean_completion_live(&self.cancel, &self.stop, deadline)?;
            if let Some(status) = self
                .child
                .lock()
                .map_err(error)?
                .try_wait()
                .map_err(error)?
            {
                require_successful_captured_exit(status)?;
                break;
            }
            std::thread::park_timeout(Duration::from_millis(1));
        }
        self.custody
            .as_mut()
            .ok_or_else(|| error("captured CPU input worker custody is absent"))?
            .retire(deadline)
            .map_err(error)?;
        require_clean_completion_live(&self.cancel, &self.stop, deadline)?;
        self.custody.take();
        self.completion.take();
        self.stop.store(true, Ordering::Release);
        let ReplyBinding::Captured(binding) = &self.reply_binding else {
            return Err(error("captured CPU clean completion mode differs"));
        };
        Ok(CpuInputJoined {
            identity: self.manifest.identity.clone(),
            attempt: self.manifest.attempt_id.clone(),
            profile_sha256: binding.profile_sha256().into(),
            sequence: self.sequence,
            completion: CpuInputCompletion::CapturedClean {
                manifest_sha256: binding.manifest_sha256().into(),
            },
        })
    }

    /// Reads opaque issued pixels; the caller retains work for freshness/completion.
    /// Any failed exchange permanently closes this worker, with no replay/substitution.
    pub fn estimate(&mut self, work: &LiveRgbWork) -> Result<LiveInferenceReply, ProtocolError> {
        let result = self.estimate_inner(work);
        if result.is_err() {
            self.close();
        }
        result
    }
    fn estimate_inner(&mut self, work: &LiveRgbWork) -> Result<LiveInferenceReply, ProtocolError> {
        if self.stop.load(Ordering::Acquire) || self.cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "live worker retired or cancelled",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(error("live worker expired"));
        }
        if work.token().frame().identity != self.manifest.identity {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "issued live work belongs to another capture",
            ));
        }
        let header = LiveInferenceInput {
            sequence: self
                .sequence
                .checked_add(1)
                .ok_or_else(|| error("live sequence overflow"))?,
            pair: work.declaration(),
            previous: pixels_identity(work.previous()),
            current: pixels_identity(work.current()),
        };
        header.validate(&self.manifest)?;
        let pixels = [work.previous(), work.current()]
            .concat()
            .into_boxed_slice();
        self.input
            .as_ref()
            .ok_or_else(|| error("live worker closed"))?
            .try_send(Input {
                header: header.clone(),
                pixels,
            })
            .map_err(error)?;
        let result = self.receive()?;
        if let LiveInferenceReply::Error(error) = &result {
            return Err(error.clone());
        }
        self.reply_binding
            .estimated(&self.manifest, &header, &result)?;
        self.sequence = header.sequence;
        Ok(result)
    }
    fn close(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.input.take();
        self.output.take();
        self.completion.take();
        if let Some(mut custody) = self.custody.take() {
            let _ = custody.retire(Instant::now() + Duration::from_millis(250));
        }
    }
}
#[cfg(target_os = "linux")]
impl Drop for LiveInferenceWorker {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod retirement_receipt_tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn selected_closed_cpu_mode_refuses_public_live_start_before_staging() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("uncreated-state");
        let mut config = EngineConfig::new(state.clone(), root.path().join("absent-worker"));
        config.cpu_owner_policy = Some(root.path().join("owner-policy.json"));
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        let epoch = AttemptId::new("closed-cpu-live-refusal").unwrap();
        let manifest = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("refused-live-worker").unwrap(),
            identity: LiveIdentity {
                epoch: epoch.clone(),
                project: ProjectId::new("authored-project").unwrap(),
                revision: RevisionId::new(0),
                source: SourceVersionId::new("authored-source").unwrap(),
            },
            clock: LivePresentationClock {
                epoch,
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            },
            model: ArtifactReference {
                path: root.path().join("absent-model"),
                identity: artifact.clone(),
            },
            runtime: ArtifactReference {
                path: root.path().join("absent-runtime"),
                identity: artifact.clone(),
            },
            worker: artifact.clone(),
            profile: DeclaredInferenceProfile {
                schema_version: 1,
                model: artifact.clone(),
                runtime: artifact,
                preprocessing: InferencePreprocessing {
                    input: ModelInputContract {
                        width: 64,
                        height: 64,
                        channels: 6,
                        layout: TensorLayout::Nchw,
                        class_count: 1,
                        decoder: DetectorDecoder::TemporalScriptPosition,
                    },
                    resize: InferenceResize::NearestStretch,
                    color: InferenceColor::RgbFramePair,
                    scale: vec![1.0 / 255.0; 6],
                    bias: vec![0.0; 6],
                    padding_byte: 0,
                },
                provider: InferenceProvider::Cpu,
                minimum_memory_bytes: 1,
                minimum_output_bytes: 1,
                cpu_threads: 1,
                cuda_resources: None,
            },
            budget: ResourceBudget {
                memory_bytes: 64 * 1024 * 1024,
                output_bytes: 1024 * 1024,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
        };
        manifest.validate().unwrap();
        let pool = ResourcePool::new(config.max_ram_bytes, 2);
        let result =
            LiveInferenceWorker::start(&config, &pool, manifest, Arc::new(AtomicBool::new(false)));
        let error = match result {
            Ok(_) => panic!("closed CPU policy launched an ordinary live worker"),
            Err(error) => error,
        };
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error
            .message
            .contains("ordinary live worker cannot use a configured captured CPU runtime"));
        assert!(!state.exists());
        assert_eq!(pool.reserved_storage(), 0);
        assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
    }

    #[test]
    fn original_retirement_receipt_waits_for_every_owned_asset() {
        let (receipt, original) = CpuWorkerRetirement::new();
        let retained = original.clone();
        drop(original);
        assert!(!receipt.joined());
        assert!(receipt.wait(Instant::now()).is_err());
        drop(retained);
        assert!(receipt.joined());
        receipt.wait(Instant::now()).unwrap();
        let (other, other_owner) = CpuWorkerRetirement::new();
        assert!(
            !other.joined(),
            "another original launch cannot borrow completion"
        );
        drop(other_owner);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod captured_transport_tests {
    use super::*;
    fn artifact(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 10,
        }
    }
    fn manifest() -> LiveCpuInferenceManifest {
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let WorkerOperation::Generate {
            settings, model, ..
        } = request.operation
        else {
            unreachable!()
        };
        let mut profile = settings.inference_profile.unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        let mut libraries = vec![
            CapturedNativeLibrary {
                identity: artifact(1),
                names: vec!["ld-linux-x86-64.so.2".into()],
            },
            CapturedNativeLibrary {
                identity: artifact(2),
                names: vec!["libc.so.6".into()],
            },
            CapturedNativeLibrary {
                identity: profile.runtime.clone(),
                names: vec!["libonnxruntime.so.1".into()],
            },
        ];
        libraries.sort_by(|a, b| a.identity.sha256.cmp(&b.identity.sha256));
        let image = CpuRuntimeImage {
            schema_version: 1,
            owner_policy: artifact(9),
            libraries,
        };
        let identity = LiveIdentity {
            epoch: AttemptId::new("authored-epoch").unwrap(),
            project: request.project_id,
            revision: request.base_revision,
            source: request.source.source_version,
        };
        let stream = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("authored-attempt").unwrap(),
            clock: LivePresentationClock {
                epoch: identity.epoch.clone(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            },
            identity,
            model: ArtifactReference {
                path: LIVE_CPU_MODEL_PATH.into(),
                identity: model.unwrap().identity,
            },
            runtime: ArtifactReference {
                path: LIVE_CPU_RUNTIME_PATH.into(),
                identity: profile.runtime.clone(),
            },
            worker: artifact(7),
            profile,
            budget: request.budget,
        };
        LiveCpuInferenceManifest::new(stream, image).unwrap()
    }
    fn input(manifest: &LiveInferenceManifest) -> LiveInferenceInput {
        let pixels = pixels_identity(&vec![0; LIVE_FRAME_BYTES]);
        LiveInferenceInput {
            sequence: 1,
            pair: RawFramePair {
                source_epoch: manifest.identity.epoch.clone(),
                previous_frame: FrameId::new(0),
                current_frame: FrameId::new(1),
                previous_time: SourceTimestamp::new(0, 1).unwrap(),
                current_time: SourceTimestamp::new(1, 10).unwrap(),
                presentation_clock: Some(manifest.clock.clone()),
            },
            previous: pixels.clone(),
            current: pixels,
        }
    }
    fn captured_ready(manifest: &LiveCpuInferenceManifest) -> LiveInferenceReply {
        let binding = manifest.reply_binding().unwrap();
        LiveInferenceReply::CapturedCpuReady {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: binding.profile_sha256().into(),
            manifest_sha256: binding.manifest_sha256().into(),
        }
    }

    #[test]
    fn host_and_captured_ready_are_not_interchangeable_even_with_equal_profiles() {
        let manifest = manifest();
        let captured = ReplyBinding::Captured(Box::new(manifest.reply_binding().unwrap()));
        let host = LiveInferenceReply::Ready {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: profile_hash(&manifest.stream).unwrap(),
        };
        let reply = captured_ready(&manifest);
        ReplyBinding::Host.ready(&manifest.stream, &host).unwrap();
        captured.ready(&manifest.stream, &reply).unwrap();
        assert!(ReplyBinding::Host.ready(&manifest.stream, &reply).is_err());
        assert!(captured.ready(&manifest.stream, &host).is_err());
        let mut other = manifest.clone();
        other.image.owner_policy = artifact(8);
        let other = LiveCpuInferenceManifest::new(other.stream, other.image).unwrap();
        assert_eq!(
            profile_hash(&other.stream).unwrap(),
            profile_hash(&manifest.stream).unwrap()
        );
        assert!(captured
            .ready(&manifest.stream, &captured_ready(&other))
            .is_err());
    }

    #[test]
    fn captured_estimate_preserves_variant_and_requires_original_image_and_issued_pixels() {
        let manifest = manifest();
        let captured = ReplyBinding::Captured(Box::new(manifest.reply_binding().unwrap()));
        let issued = input(&manifest.stream);
        let reply = LiveInferenceReply::CapturedCpuEstimated {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: profile_hash(&manifest.stream).unwrap(),
            manifest_sha256: manifest.identity_sha256().unwrap(),
            input: issued.clone(),
            position: NormalizedPosition::new(0.5).unwrap(),
            pair_analysis_elapsed_ns: 1,
        };
        captured
            .estimated(&manifest.stream, &issued, &reply)
            .unwrap();
        assert!(matches!(
            &reply,
            LiveInferenceReply::CapturedCpuEstimated { .. }
        ));
        assert!(ReplyBinding::Host
            .estimated(&manifest.stream, &issued, &reply)
            .is_err());
        let mut different = issued.clone();
        different.current.sha256 = "0".repeat(64);
        different.current.artifact_id = ArtifactId::new(different.current.sha256.clone()).unwrap();
        assert!(captured
            .estimated(&manifest.stream, &different, &reply)
            .is_err());
        let host = LiveInferenceReply::Estimated {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: profile_hash(&manifest.stream).unwrap(),
            input: issued.clone(),
            position: NormalizedPosition::new(0.5).unwrap(),
            pair_analysis_elapsed_ns: 1,
        };
        assert!(captured
            .estimated(&manifest.stream, &issued, &host)
            .is_err());
    }

    #[test]
    fn inherited_captured_deadline_never_restores_the_original_duration() {
        let original = Instant::now() + Duration::from_millis(500);
        let encoded = crate::cuda_bootstrap::deadline_ns_in_execution(original, 1000).unwrap();
        let first = inherited_live_deadline(encoded, 1000).unwrap();
        let second = inherited_live_deadline(encoded, 1000).unwrap();
        assert!(first <= original && second <= original);
        assert!(inherited_live_deadline(0, 1000).is_err());
        assert!(inherited_live_deadline(encoded, 0).is_err());
        assert!(inherited_live_deadline(encoded, 30_001).is_err());
        assert!(inherited_live_deadline(u64::MAX, 1000).is_err());
    }

    #[test]
    fn captured_completion_requires_real_eof_not_trailing_data_or_transport_failure() {
        clean_stream_eof(&mut std::io::empty()).unwrap();
        assert!(clean_stream_eof(&mut std::io::Cursor::new([0u8])).is_err());
        struct Failed;
        impl std::io::Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("authored transport error"))
            }
        }
        assert!(clean_stream_eof(&mut Failed).is_err());
    }

    #[test]
    fn cancellation_expiry_or_unsuccessful_exit_never_means_clean_captured_completion() {
        use std::os::unix::process::ExitStatusExt;
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let end = Instant::now() + Duration::from_secs(1);
        require_clean_completion_live(&cancel, &stop, end).unwrap();
        cancel.store(true, Ordering::Release);
        assert!(require_clean_completion_live(&cancel, &stop, end).is_err());
        cancel.store(false, Ordering::Release);
        stop.store(true, Ordering::Release);
        assert!(require_clean_completion_live(&cancel, &stop, end).is_err());
        stop.store(false, Ordering::Release);
        assert!(require_clean_completion_live(&cancel, &stop, Instant::now()).is_err());
        require_successful_captured_exit(std::process::ExitStatus::from_raw(0)).unwrap();
        assert!(
            require_successful_captured_exit(std::process::ExitStatus::from_raw(1 << 8)).is_err()
        );
        assert!(
            require_successful_captured_exit(std::process::ExitStatus::from_raw(libc::SIGKILL))
                .is_err()
        );
    }

    #[test]
    fn input_joined_matchers_keep_host_retirement_and_captured_clean_completion_distinct() {
        // Authored value-shape test only; no process or custody is created.
        let manifest = manifest();
        let profile = profile_hash(&manifest.stream).unwrap();
        let hash = manifest.identity_sha256().unwrap();
        let host = CpuInputJoined::authored_for_test(
            manifest.stream.identity.clone(),
            manifest.stream.attempt_id.clone(),
            profile.clone(),
            1,
        );
        assert!(host.matches(
            &manifest.stream.identity,
            &manifest.stream.attempt_id,
            &profile,
            1
        ));
        assert!(!host.matches_captured(
            &manifest.stream.identity,
            &manifest.stream.attempt_id,
            &profile,
            &hash,
            1
        ));
        let captured = CpuInputJoined {
            identity: manifest.stream.identity.clone(),
            attempt: manifest.stream.attempt_id.clone(),
            profile_sha256: profile.clone(),
            sequence: 1,
            completion: CpuInputCompletion::CapturedClean {
                manifest_sha256: hash.clone(),
            },
        };
        assert!(!captured.matches(
            &manifest.stream.identity,
            &manifest.stream.attempt_id,
            &profile,
            1
        ));
        assert!(captured.matches_captured(
            &manifest.stream.identity,
            &manifest.stream.attempt_id,
            &profile,
            &hash,
            1
        ));
        assert!(!captured.matches_captured(
            &manifest.stream.identity,
            &manifest.stream.attempt_id,
            &profile,
            &"0".repeat(64),
            1
        ));
        assert!(!captured.matches_captured(
            &manifest.stream.identity,
            &manifest.stream.attempt_id,
            &profile,
            &hash,
            2
        ));
    }
}
