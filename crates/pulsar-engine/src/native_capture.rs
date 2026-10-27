//! Independently supervised native byte capture. No libraries are loaded.
#![cfg(target_os = "linux")]
use crate::{
    artifacts,
    cgroup::AttemptCgroup,
    native_dependencies::CapturedNativeDependencies,
    resources::{Reservation, ResourcePool},
    EngineConfig,
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use std::{
    fs,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::{Component, Path},
    process::ExitStatus,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, Weak,
    },
    time::{Duration, Instant},
};

pub fn run_worker() -> Result<()> {
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1"),
        "native capture role requires parent-owned confinement"
    );
    let request: NativeCaptureRequest = read_message(&mut std::io::stdin().lock())?;
    let result = (|| -> Result<NativeCaptureCompletion> {
        let bytes = request.validate()?;
        use std::os::unix::fs::MetadataExt;
        for (library, expected) in request.image.libraries.iter().zip(&request.inputs) {
            let file = artifacts::open_regular(&library.path)?;
            let actual = file.metadata()?;
            ensure!(
                actual.dev() == expected.filesystem_device && actual.ino() == expected.inode,
                "mounted native capture input identity differs"
            );
        }
        let captured = CapturedNativeDependencies::capture(&request.image, Path::new("/capture"))?;
        let root = captured.keep_for_parent();
        let completion = NativeCaptureCompletion {
            root_name: root
                .file_name()
                .context("capture root name missing")?
                .to_str()
                .context("invalid capture root name")?
                .into(),
            declared_bytes: bytes,
            request_sha256: request.identity()?,
        };
        completion.validate(&request)?;
        Ok(completion)
    })()
    .map_err(|e| {
        ProtocolError::new(
            ErrorCode::DependencyMismatch,
            format!("native image capture failed: {e:#}"),
        )
    });
    write_message(
        &mut std::io::stdout().lock(),
        &NativeCaptureReply {
            version: PROTOCOL_VERSION,
            attempt_id: request.attempt_id,
            result,
        },
    )?;
    Ok(())
}

/// One use of a frozen native image. Storage remains owned until the final use
/// retires; every inference launch must still authenticate mounted bytes.
pub struct SupervisedNativeDependencies {
    owner: Arc<NativeImageOwner>,
}
struct NativeImageOwner {
    captured: Option<CapturedNativeDependencies>,
    staging: Mutex<Option<crate::staging_obligation::Staging>>,
    completion: NativeCaptureCompletion,
}
impl SupervisedNativeDependencies {
    fn new(
        captured: CapturedNativeDependencies,
        staging: crate::staging_obligation::Staging,
        completion: NativeCaptureCompletion,
    ) -> Self {
        Self {
            owner: Arc::new(NativeImageOwner {
                captured: Some(captured),
                staging: Mutex::new(Some(staging)),
                completion,
            }),
        }
    }
    pub fn captured(&self) -> &CapturedNativeDependencies {
        self.owner.captured.as_ref().expect("native image custody")
    }
    pub fn completion(&self) -> &NativeCaptureCompletion {
        &self.owner.completion
    }
    /// Release this use after its child tree retires. A remaining borrower keeps
    /// storage alive. The final explicit close waits for confirmed cleanup.
    pub fn close(self) -> Result<()> {
        if let Some(mut owner) = Arc::into_inner(self.owner) {
            let cleanup = owner.begin_cleanup().expect("native image cleanup custody");
            cleanup
                .wait(Instant::now() + Duration::from_secs(2))
                .context("supervised native capture staging cleanup unconfirmed")?;
        }
        Ok(())
    }
}
impl NativeImageOwner {
    fn begin_cleanup(&mut self) -> Option<crate::staging_cleanup::Cleanup> {
        let captured = self.captured.take()?;
        let staging = self
            .staging
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("native staging custody");
        // Identity-aware staging deletion owns the image and its storage claim.
        let _ = captured.keep_for_parent();
        Some(staging.cleanup_after_use_retired(()))
    }
}
impl Drop for NativeImageOwner {
    fn drop(&mut self) {
        drop(self.begin_cleanup());
    }
}

/// Per-Engine weak reuse with coalesced matching cold captures. No cache pin or
/// persistence. Every waiter retains its own cancellation and deadline. Supplier paths
/// are not reopened: workers prove the captured bytes independently on each use.
#[derive(Default)]
pub(crate) struct LiveNativeImageReuse {
    image: Mutex<Option<Weak<NativeImageOwner>>>,
    producing: Mutex<std::collections::BTreeSet<Vec<u8>>>,
    changed: Condvar,
}
impl LiveNativeImageReuse {
    pub(crate) fn capture_with<F>(
        &self,
        spec: &NativeDependencySpec,
        cancel: &AtomicBool,
        deadline: Instant,
        capture: F,
    ) -> std::result::Result<SupervisedNativeDependencies, ProtocolError>
    where
        F: FnOnce() -> std::result::Result<SupervisedNativeDependencies, ProtocolError>,
    {
        active(cancel, deadline)?;
        spec.validate().map_err(unavailable)?;
        let key = serde_json::to_vec(spec).map_err(unavailable)?;
        let mut producing = self.producing.lock().map_err(unavailable)?;
        loop {
            active(cancel, deadline)?;
            if let Some(image) = self.reuse(spec, cancel, deadline).map_err(unavailable)? {
                return Ok(image);
            }
            if producing.insert(key.clone()) {
                break;
            }
            // Cancellation is request-local; it must not cancel a producer used
            // by another authorized request. Waiting consumes the existing lease.
            producing = self
                .changed
                .wait_timeout(
                    producing,
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(10)),
                )
                .map_err(unavailable)?
                .0;
        }
        drop(producing);
        struct Producer<'a> {
            cache: &'a LiveNativeImageReuse,
            key: Vec<u8>,
        }
        impl Drop for Producer<'_> {
            fn drop(&mut self) {
                self.cache
                    .producing
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&self.key);
                self.cache.changed.notify_all();
            }
        }
        let _producer = Producer { cache: self, key };
        let image = capture()?;
        active(cancel, deadline)?;
        self.remember(&image).map_err(unavailable)?;
        Ok(image)
    }
    pub(crate) fn reuse(
        &self,
        spec: &NativeDependencySpec,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> Result<Option<SupervisedNativeDependencies>> {
        active(cancel, deadline)?;
        spec.validate()?;
        let owner = self
            .image
            .lock()
            .map_err(|_| anyhow::anyhow!("native reuse lock poisoned"))?
            .as_ref()
            .and_then(Weak::upgrade);
        let Some(owner) = owner else { return Ok(None) };
        let image = SupervisedNativeDependencies { owner };
        if serde_json::to_vec(image.captured().specification())? != serde_json::to_vec(spec)? {
            return Ok(None);
        }
        // Metadata only in authority; never substitutes for mounted byte proof.
        image.captured().verify_metadata()?;
        active(cancel, deadline)?;
        Ok(Some(image))
    }
    pub(crate) fn remember(&self, image: &SupervisedNativeDependencies) -> Result<()> {
        *self
            .image
            .lock()
            .map_err(|_| anyhow::anyhow!("native reuse lock poisoned"))? =
            Some(Arc::downgrade(&image.owner));
        Ok(())
    }
}

/// Bridge a per-use guard through staging creation, whose claim is otherwise
/// retained for the image's full shared lifetime. Failed creation keeps this
/// bridge populated until cleanup; successful worker admission owns separate
/// last-drop clones before the bridge is emptied.
#[derive(Clone)]
struct CaptureRetirementAssets {
    held: Arc<Mutex<crate::worker_retirement::RetirementAssets>>,
}
impl CaptureRetirementAssets {
    fn new(assets: crate::worker_retirement::RetirementAssets) -> Self {
        Self {
            held: Arc::new(Mutex::new(assets)),
        }
    }
    fn for_worker(&self) -> crate::worker_retirement::RetirementAssets {
        self.held
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    /// Call only after the exact clones from for_worker have moved into an
    /// installed WorkerCustody. No fallible work separates those two steps.
    fn release_after_admission(self) {
        self.held
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

struct Preparation {
    staging: Option<crate::staging_obligation::Staging>,
    worker: Option<Arc<Reservation>>,
    retirement_assets: crate::worker_retirement::RetirementAssets,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        if let Some(staging) = self.staging.take() {
            drop(staging.cleanup_after_use_retired((
                self.worker.take(),
                std::mem::take(&mut self.retirement_assets),
            )));
        }
    }
}
fn unavailable(e: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, e.to_string())
}
/// File cache is charged to the capture cgroup. Reserve the declared image size
/// up to the protocol's 1 GiB ceiling, retaining the existing 512 MiB floor.
/// This auxiliary role still passes aggregate RAM/worker admission; it never
/// changes the inference worker's budget or extends a live lease.
pub(crate) fn preparation_budget(
    total: u64,
    wall_time_ms: u64,
) -> std::result::Result<ResourceBudget, ProtocolError> {
    let output_bytes = total
        .checked_add(16 << 20)
        .ok_or_else(|| ProtocolError::invalid("native capture storage overflow"))?;
    let budget = ResourceBudget {
        memory_bytes: total.div_ceil(1 << 20).clamp(512, 1024) << 20,
        output_bytes,
        wall_time_ms,
        cpu_threads: 1,
    };
    budget.validate()?;
    Ok(budget)
}

fn active(cancel: &AtomicBool, deadline: Instant) -> std::result::Result<(), ProtocolError> {
    if cancel.load(Ordering::Acquire) {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "native capture cancelled",
        ));
    }
    if Instant::now() >= deadline {
        return Err(unavailable("native capture absolute deadline exceeded"));
    }
    Ok(())
}
fn nonblocking(fd: &impl AsRawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    ensure!(
        flags >= 0
            && unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "cannot establish nonblocking native capture pipe"
    );
    Ok(())
}
fn drain(reader: &mut Option<impl Read>, bytes: &mut Vec<u8>, limit: usize) -> Result<()> {
    let Some(input) = reader.as_mut() else {
        return Ok(());
    };
    let mut chunk = [0u8; 8192];
    match input.read(&mut chunk) {
        Ok(0) => {
            reader.take();
        }
        Ok(n) => {
            ensure!(
                bytes.len() + n <= limit,
                "native capture diagnostic/reply exceeds bound"
            );
            bytes.extend_from_slice(&chunk[..n]);
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) => {}
        Err(e) => return Err(e.into()),
    };
    Ok(())
}

pub fn capture(
    config: &EngineConfig,
    pool: &ResourcePool,
    spec: &NativeDependencySpec,
    parent: &Path,
    budget: ResourceBudget,
    address_space_bytes: u64,
    cancel: &AtomicBool,
) -> std::result::Result<SupervisedNativeDependencies, ProtocolError> {
    capture_inner(
        config,
        pool,
        spec,
        CaptureCustody {
            parent,
            fence: None,
            retirement_assets: Vec::new(),
        },
        CaptureLimits {
            budget,
            address_space_bytes,
        },
        cancel,
        None,
    )
}

#[cfg(test)]
pub(crate) fn authored_preparation_fixture(
    parent: &Path,
    pool: &ResourcePool,
) -> SupervisedNativeDependencies {
    use sha2::{Digest, Sha256};
    let bytes = b"authored bytes; no native code";
    let input = parent.join("controller-authored-supplier");
    fs::write(&input, bytes).unwrap();
    let spec = NativeDependencySpec {
        schema_version: 1,
        libraries: vec![NativeLibrarySpec {
            path: input,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            byte_len: bytes.len() as u64,
            names: vec!["authored.so".into()],
        }],
    };
    let staging = tempfile::tempdir_in(parent).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let captured = CapturedNativeDependencies::capture(&spec, staging.path()).unwrap();
    SupervisedNativeDependencies::new(
        captured,
        crate::staging_obligation::Staging::Legacy(staging).retaining(
            pool.reserve_memory(1024)
                .unwrap()
                .with_storage(4096, 32 * 1024 * 1024)
                .unwrap(),
        ),
        NativeCaptureCompletion {
            root_name: "authored".into(),
            declared_bytes: bytes.len() as u64,
            request_sha256: "0".repeat(64),
        },
    )
}

/// Independent preparation limits, not capture or launch authority.
pub(crate) struct CaptureLimits {
    pub(crate) budget: ResourceBudget,
    pub(crate) address_space_bytes: u64,
}
struct CaptureCustody<'a> {
    parent: &'a Path,
    fence: Option<Arc<crate::worker_fence::WorkerFence>>,
    retirement_assets: crate::worker_retirement::RetirementAssets,
}

/// The original caller lease and its per-use retirement guards. These assets
/// protect capture work, not the reusable image's lifetime or future consumers.
pub(crate) struct CaptureLease {
    pub(crate) deadline: Instant,
    pub(crate) retirement_assets: crate::worker_retirement::RetirementAssets,
}

#[cfg(test)]
pub(crate) fn capture_tracked(
    config: &EngineConfig,
    pool: &ResourcePool,
    spec: &NativeDependencySpec,
    limits: CaptureLimits,
    cancel: &AtomicBool,
    fence: Arc<crate::worker_fence::WorkerFence>,
) -> std::result::Result<SupervisedNativeDependencies, ProtocolError> {
    capture_tracked_inner(config, pool, spec, limits, cancel, fence, None)
}

/// Retain the caller's absolute lease through preparation, copying and retirement.
/// This does not authorize capture or renew its independent resource budget.
pub(crate) fn capture_tracked_in_lease(
    config: &EngineConfig,
    pool: &ResourcePool,
    spec: &NativeDependencySpec,
    limits: CaptureLimits,
    cancel: &AtomicBool,
    fence: Arc<crate::worker_fence::WorkerFence>,
    lease_end: Instant,
) -> std::result::Result<SupervisedNativeDependencies, ProtocolError> {
    capture_tracked_in_lease_with_assets(
        config,
        pool,
        spec,
        limits,
        cancel,
        fence,
        CaptureLease {
            deadline: lease_end,
            retirement_assets: Vec::new(),
        },
    )
}

/// Keep per-use guards through cold capture and every failed staging/group/
/// transport cleanup. Successful images retain no guard from this call; each
/// later consumer must separately install its own retirement assets.
pub(crate) fn capture_tracked_in_lease_with_assets(
    config: &EngineConfig,
    pool: &ResourcePool,
    spec: &NativeDependencySpec,
    limits: CaptureLimits,
    cancel: &AtomicBool,
    fence: Arc<crate::worker_fence::WorkerFence>,
    lease: CaptureLease,
) -> std::result::Result<SupervisedNativeDependencies, ProtocolError> {
    capture_tracked_inner(config, pool, spec, limits, cancel, fence, Some(lease))
}

fn capture_tracked_inner(
    config: &EngineConfig,
    pool: &ResourcePool,
    spec: &NativeDependencySpec,
    limits: CaptureLimits,
    cancel: &AtomicBool,
    fence: Arc<crate::worker_fence::WorkerFence>,
    lease: Option<CaptureLease>,
) -> std::result::Result<SupervisedNativeDependencies, ProtocolError> {
    let lease_end = lease.as_ref().map(|value| value.deadline);
    if let Some(end) = lease_end {
        active(cancel, end)?;
    }
    // The caller must commit admission before creation. Independently observe
    // the actual database binding, rather than upgrading a private helper scope.
    let db = rusqlite::Connection::open_with_flags(
        config.state_dir.join("projects.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(unavailable)?;
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(unavailable)?;
    let version = crate::live_cuda_evidence_db::base_version(&db, version).map_err(unavailable)?;
    let binding = crate::worker_fence_db::binding(&db, version).map_err(unavailable)?;
    if version != 20
        || binding.is_none_or(|b| {
            b.minimum_ledger_version != 4 || fence.scope().ok().as_deref() != Some(b.scope.as_str())
        })
        || fence.version().map_err(unavailable)? != 4
        || config.state_dir.canonicalize().map_err(unavailable)? != fence.staging_parent()
    {
        return Err(ProtocolError::invalid(
            "native capture requires committed format-4 engine admission",
        ));
    }
    drop(db);
    let parent = fence.staging_parent().to_owned();
    capture_inner(
        config,
        pool,
        spec,
        CaptureCustody {
            parent: &parent,
            fence: Some(fence),
            retirement_assets: lease
                .map(|value| value.retirement_assets)
                .unwrap_or_default(),
        },
        limits,
        cancel,
        lease_end,
    )
}

fn capture_inner(
    config: &EngineConfig,
    pool: &ResourcePool,
    spec: &NativeDependencySpec,
    custody: CaptureCustody<'_>,
    limits: CaptureLimits,
    cancel: &AtomicBool,
    lease_end: Option<Instant>,
) -> std::result::Result<SupervisedNativeDependencies, ProtocolError> {
    let CaptureCustody {
        parent,
        fence,
        retirement_assets,
    } = custody;
    // Declare before fallible preparation so local cleanup/claims retire before
    // the final local guard if no asynchronous owner was ever established.
    let retirement = CaptureRetirementAssets::new(retirement_assets);
    let CaptureLimits {
        budget,
        address_space_bytes,
    } = limits;
    let budget_end = Instant::now()
        .checked_add(Duration::from_millis(budget.wall_time_ms))
        .ok_or_else(|| ProtocolError::invalid("native capture deadline overflow"))?;
    let deadline = lease_end.map_or(budget_end, |end| end.min(budget_end));
    active(cancel, deadline)?;
    spec.validate()?;
    if !parent.is_absolute()
        || parent
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        || spec.libraries.iter().any(|l| {
            l.path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        })
    {
        return Err(ProtocolError::invalid(
            "native capture paths must be absolute without traversal",
        ));
    }
    use std::os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    };
    let mut held = Vec::new();
    let mut inputs = Vec::new();
    let mut guest = spec.clone();
    let mut bindings = Vec::new();
    for (index, library) in guest.libraries.iter_mut().enumerate() {
        active(cancel, deadline)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&library.path)
            .map_err(unavailable)?;
        let metadata = file.metadata().map_err(unavailable)?;
        if !metadata.is_file() || metadata.len() != library.byte_len {
            return Err(ProtocolError::invalid(
                "native capture requires exact-length regular supplier inputs",
            ));
        }
        inputs.push(NativeCaptureInputIdentity {
            filesystem_device: metadata.dev(),
            inode: metadata.ino(),
        });
        library.path = format!("/inputs/{index}").into();
        bindings.push((
            format!("/proc/self/fd/{}", file.as_raw_fd()).into(),
            library.path.clone(),
        ));
        held.push(file);
    }
    let request = NativeCaptureRequest {
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new(uuid::Uuid::new_v4().to_string()).map_err(unavailable)?,
        image: guest,
        inputs,
        budget,
        address_space_bytes,
    };
    request.validate()?;
    if !crate::launch::available() {
        return Err(ProtocolError::unsupported("native capture confinement"));
    }
    let reservation =
        pool.reserve_worker(request.budget.memory_bytes, request.budget.cpu_threads)?;
    artifacts::private_directory(parent).map_err(unavailable)?;
    let mut reservation = reservation.with_storage(
        request.budget.output_bytes,
        fs2::available_space(parent).map_err(unavailable)?,
    )?;
    active(cancel, deadline)?;
    let storage = Arc::new(reservation.split_storage()?);
    let staging = match &fence {
        Some(fence) => crate::staging_obligation::Staging::Durable(
            fence
                .create_staging((storage.clone(), retirement.clone()))
                .map_err(unavailable)?,
        ),
        None => crate::staging_obligation::Staging::Legacy(
            tempfile::Builder::new()
                .prefix("native-capture-")
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(parent)
                .map_err(unavailable)?,
        ),
    }
    .retaining((storage, retirement.clone()));
    let stage_path = staging.path();
    let mut prepared = Preparation {
        staging: Some(staging),
        worker: Some(Arc::new(reservation)),
        retirement_assets: retirement.for_worker(),
    };
    let mut owned = match fence {
        Some(fence) => fence
            .admit_staged_group_with_assets(
                prepared.worker.take().unwrap(),
                request.budget.memory_bytes,
                request.budget.cpu_threads,
                prepared.staging.take().unwrap(),
                std::mem::take(&mut prepared.retirement_assets),
            )
            .map_err(unavailable)?
            .into_worker(),
        None => {
            // Preparation keeps both claims and bytes through raw helper failure.
            let group = AttemptCgroup::create(
                request.attempt_id.as_str(),
                request.budget.memory_bytes,
                request.budget.cpu_threads,
            )
            .map_err(unavailable)?;
            crate::worker_retirement::WorkerCustody::prepare_staged_with_assets(
                group,
                prepared.worker.take().unwrap(),
                None,
                prepared.staging.take().unwrap(),
                std::mem::take(&mut prepared.retirement_assets),
            )
            .map_err(unavailable)?
        }
    };
    // Both constructors installed exact last-drop clones before success. Empty
    // the staging bridge now, so retire_into_staging cannot leak a per-use guard
    // into the shared NativeImageOwner. Failure above leaves it populated.
    retirement.release_after_admission();
    let group = owned.group_handle();
    active(cancel, deadline)?;
    let mut command = crate::confinement::native_capture_command(
        config,
        &request.budget,
        address_space_bytes,
        &group,
        &bindings,
        &stage_path,
    )
    .map_err(unavailable)?;
    let descriptors = held.iter().map(AsRawFd::as_raw_fd).collect::<Vec<_>>();
    unsafe {
        command.pre_exec(move || {
            for fd in &descriptors {
                let flags = libc::fcntl(*fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    owned.retain_asset(held);
    owned.spawn(&mut command).map_err(unavailable)?;
    let mut child = owned.child();
    let mut input = Some(
        child
            .stdin
            .take()
            .ok_or_else(|| unavailable("native capture stdin unavailable"))?,
    );
    let mut output = Some(
        child
            .stdout
            .take()
            .ok_or_else(|| unavailable("native capture stdout unavailable"))?,
    );
    let mut diagnostics = Some(
        child
            .stderr
            .take()
            .ok_or_else(|| unavailable("native capture stderr unavailable"))?,
    );
    drop(child);
    nonblocking(input.as_ref().unwrap()).map_err(unavailable)?;
    nonblocking(output.as_ref().unwrap()).map_err(unavailable)?;
    nonblocking(diagnostics.as_ref().unwrap()).map_err(unavailable)?;
    let mut frame = Vec::new();
    write_message(&mut frame, &request).map_err(unavailable)?;
    let mut sent = 0;
    let mut reply = Vec::new();
    let mut stderr = Vec::new();
    let mut status: Option<ExitStatus> = None;
    loop {
        active(cancel, deadline)?;
        if let Some(writer) = input.as_mut() {
            match writer.write(&frame[sent..]) {
                Ok(0) => return Err(unavailable("native capture input closed before manifest")),
                Ok(n) => {
                    sent += n;
                    if sent == frame.len() {
                        input.take();
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(unavailable(e)),
            }
        }
        drain(&mut output, &mut reply, 16384).map_err(unavailable)?;
        drain(&mut diagnostics, &mut stderr, 16384).map_err(unavailable)?;
        if status.is_none() {
            status = owned.child().try_wait().map_err(unavailable)?;
            if status.is_some() {
                group.kill();
            }
        }
        if status.is_some()
            && output.is_none()
            && diagnostics.is_none()
            && group.is_empty().map_err(unavailable)?
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // Only this observed terminal/empty state permits synchronous custody release.
    if !status.unwrap().success() {
        return Err(unavailable(format!(
            "native capture worker failed: {}",
            String::from_utf8_lossy(&stderr)
        )));
    }
    let mut cursor = std::io::Cursor::new(&reply);
    let response: NativeCaptureReply = read_message(&mut cursor).map_err(unavailable)?;
    if cursor.position() != reply.len() as u64
        || response.version != PROTOCOL_VERSION
        || response.attempt_id != request.attempt_id
    {
        return Err(ProtocolError::invalid(
            "native capture reply binding differs",
        ));
    }
    let completion = response.result?;
    completion.validate(&request)?;
    active(cancel, deadline)?;
    let root = stage_path.join(&completion.root_name);
    fs::symlink_metadata(&root).map_err(unavailable)?;
    artifacts::private_directory(&root).map_err(unavailable)?;
    // No rehash in the authority process. Trusted capture child completed its
    // full verification under supervision; later launch must verify mounted bytes.
    let captured = CapturedNativeDependencies::adopt_supervised(root, spec.clone());
    captured.verify_metadata().map_err(unavailable)?;
    let staging = owned.retire_into_staging(deadline).map_err(unavailable)?;
    Ok(SupervisedNativeDependencies::new(
        captured, staging, completion,
    ))
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn inherited_capture_expiry_or_cancel_refuses_before_database_or_staging_access() {
        let parent = tempfile::tempdir().unwrap();
        let state = parent.path().join("state");
        let fence = Arc::new(crate::worker_fence::WorkerFence::open(&state).unwrap());
        let before = fs::read(state.join("worker-fence.json")).unwrap();
        let config = EngineConfig::new(state.clone(), parent.path().join("missing-worker"));
        let pool = ResourcePool::new(1024, 1);
        let spec = NativeDependencySpec {
            schema_version: 1,
            libraries: vec![],
        };
        let budget = ResourceBudget {
            memory_bytes: 1024,
            output_bytes: 1024,
            wall_time_ms: 1000,
            cpu_threads: 1,
        };
        for wall_time_ms in [1000, 3_600_000] {
            let budget = ResourceBudget {
                wall_time_ms,
                ..budget.clone()
            };
            for cancelled in [false, true] {
                let error = capture_tracked_in_lease(
                    &config,
                    &pool,
                    &spec,
                    CaptureLimits {
                        budget: budget.clone(),
                        address_space_bytes: 4096,
                    },
                    &AtomicBool::new(cancelled),
                    fence.clone(),
                    if cancelled {
                        Instant::now() + Duration::from_secs(1)
                    } else {
                        Instant::now() - Duration::from_secs(1)
                    },
                )
                .err()
                .unwrap();
                assert!(error.message.contains(if cancelled {
                    "cancelled"
                } else {
                    "absolute deadline exceeded"
                }));
                assert!(!state.join("projects.sqlite3").exists());
                assert_eq!(fs::read(state.join("worker-fence.json")).unwrap(), before);
                assert_eq!(fs::read_dir(&state).unwrap().count(), 2);
                assert_eq!(pool.reserved_storage(), 0);
                assert!(pool.reserve(1024).is_ok());
            }
        }
    }

    fn reusable_image(
        parent: &Path,
    ) -> (
        SupervisedNativeDependencies,
        ResourcePool,
        std::path::PathBuf,
    ) {
        let bytes = b"authored bytes; no native code";
        let input = parent.join("supplier");
        fs::write(&input, bytes).unwrap();
        let spec = NativeDependencySpec {
            schema_version: 1,
            libraries: vec![NativeLibrarySpec {
                path: input,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                byte_len: bytes.len() as u64,
                names: vec!["authored.so".into()],
            }],
        };
        let staging = tempfile::tempdir_in(parent).unwrap();
        fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = staging.path().to_owned();
        let captured = CapturedNativeDependencies::capture(&spec, staging.path()).unwrap();
        let pool = ResourcePool::new(1024, 1);
        let image = SupervisedNativeDependencies::new(
            captured,
            crate::staging_obligation::Staging::Legacy(staging).retaining(
                pool.reserve(1024)
                    .unwrap()
                    .with_storage(4096, 32 * 1024 * 1024)
                    .unwrap(),
            ),
            NativeCaptureCompletion {
                root_name: "authored".into(),
                declared_bytes: bytes.len() as u64,
                request_sha256: "0".repeat(64),
            },
        );
        (image, pool, path)
    }

    fn retention_admission() -> crate::authority::live_cuda_preparation::RetentionAdmission {
        crate::authority::live_cuda_preparation::RetentionAdmission::reserve(
            &ResourcePool::new(512 * 1024, 1),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        )
        .unwrap()
    }

    #[test]
    fn prepared_image_release_keeps_worker_borrow_and_checked_storage_custody() {
        let parent = tempfile::tempdir().unwrap();
        let (image, pool, path) = reusable_image(parent.path());
        let cache = LiveNativeImageReuse::default();
        cache.remember(&image).unwrap();
        let spec = image.captured().specification().clone();
        let prepared = crate::PreparedLiveCudaImage::retain(
            image,
            AttemptId::new("prepared-boot").unwrap(),
            3000,
            retention_admission(),
        )
        .unwrap();
        assert!(prepared.available());
        assert_eq!(prepared.declared_bytes(), spec.validate().unwrap());
        assert_eq!(prepared.image_sha256().len(), 64);
        let borrowed = cache
            .reuse(
                &spec,
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(3),
            )
            .unwrap()
            .unwrap();
        assert!(prepared.release().unwrap());
        assert!(path.exists());
        assert_eq!(pool.reserved_storage(), 4096);
        borrowed.close().unwrap();
        wait_release(&pool, &path);
    }
    #[test]
    fn prepared_image_expires_without_revoking_worker_borrow() {
        let parent = tempfile::tempdir().unwrap();
        let (image, pool, path) = reusable_image(parent.path());
        let cache = LiveNativeImageReuse::default();
        cache.remember(&image).unwrap();
        let spec = image.captured().specification().clone();
        let borrowed = cache
            .reuse(
                &spec,
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(3),
            )
            .unwrap()
            .unwrap();
        let prepared = crate::PreparedLiveCudaImage::retain(
            image,
            AttemptId::new("expiring-boot").unwrap(),
            1,
            retention_admission(),
        )
        .unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        while prepared.available() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!prepared.release().unwrap());
        assert!(path.exists());
        assert_eq!(pool.reserved_storage(), 4096);
        borrowed.captured().verify().unwrap();
        borrowed.close().unwrap();
        wait_release(&pool, &path);
    }
    #[test]
    fn prepared_image_drop_does_not_pin_native_cache() {
        let parent = tempfile::tempdir().unwrap();
        let (image, pool, path) = reusable_image(parent.path());
        let cache = LiveNativeImageReuse::default();
        cache.remember(&image).unwrap();
        let spec = image.captured().specification().clone();
        let prepared = crate::PreparedLiveCudaImage::retain(
            image,
            AttemptId::new("dropped-boot").unwrap(),
            3000,
            retention_admission(),
        )
        .unwrap();
        drop(prepared);
        wait_release(&pool, &path);
        assert!(cache
            .reuse(
                &spec,
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(3)
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn reuse_releases_storage_only_after_last_use_and_never_pins_cache() {
        for explicit in [false, true] {
            let parent = tempfile::tempdir().unwrap();
            let (image, pool, path) = reusable_image(parent.path());
            let cache = LiveNativeImageReuse::default();
            cache.remember(&image).unwrap();
            let spec = image.captured().specification().clone();
            let cancel = AtomicBool::new(false);
            let end = Instant::now() + Duration::from_secs(3);
            let borrowed = cache.reuse(&spec, &cancel, end).unwrap().unwrap();
            assert_eq!(
                image.captured().library_directory(),
                borrowed.captured().library_directory()
            );
            // Reuse is of frozen captured bytes, not a new supplier snapshot.
            fs::remove_file(&spec.libraries[0].path).unwrap();
            borrowed.captured().verify().unwrap();
            if explicit {
                image.close().unwrap();
            } else {
                drop(image);
            }
            assert!(path.exists());
            assert_eq!(pool.reserved_storage(), 4096);
            assert!(pool.reserve(1024).is_err());
            borrowed.close().unwrap();
            wait_release(&pool, &path);
            assert!(cache.reuse(&spec, &cancel, end).unwrap().is_none());
        }
    }

    #[test]
    fn reuse_requires_exact_spec_active_lease_and_intact_metadata() {
        let parent = tempfile::tempdir().unwrap();
        let (image, pool, path) = reusable_image(parent.path());
        let cache = LiveNativeImageReuse::default();
        cache.remember(&image).unwrap();
        let spec = image.captured().specification().clone();
        let end = Instant::now() + Duration::from_secs(3);
        let cancel = AtomicBool::new(false);
        let mut different = spec.clone();
        different.libraries[0].names[0] = "other.so".into();
        assert!(cache.reuse(&different, &cancel, end).unwrap().is_none());
        assert!(cache.reuse(&spec, &AtomicBool::new(true), end).is_err());
        assert!(cache
            .reuse(&spec, &cancel, Instant::now() - Duration::from_secs(1))
            .is_err());
        let alias = image.captured().library_directory().join("authored.so");
        fs::remove_file(alias).unwrap();
        assert!(cache.reuse(&spec, &cancel, end).is_err());
        image.close().unwrap();
        wait_release(&pool, &path);
    }

    #[test]
    fn reuse_metadata_never_authenticates_changed_content() {
        let parent = tempfile::tempdir().unwrap();
        let (image, pool, path) = reusable_image(parent.path());
        let cache = LiveNativeImageReuse::default();
        cache.remember(&image).unwrap();
        let spec = image.captured().specification().clone();
        let alias = image.captured().library_directory().join("authored.so");
        fs::set_permissions(&alias, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&alias, vec![b'x'; spec.libraries[0].byte_len as usize]).unwrap();
        fs::set_permissions(&alias, fs::Permissions::from_mode(0o400)).unwrap();
        let borrowed = cache
            .reuse(
                &spec,
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(3),
            )
            .unwrap()
            .unwrap();
        assert!(borrowed.captured().verify().is_err());
        image.close().unwrap();
        borrowed.close().unwrap();
        wait_release(&pool, &path);
    }

    #[test]
    fn matching_cold_requests_run_one_producer_and_share_its_living_image() {
        let parent = tempfile::tempdir().unwrap();
        let (image, pool, path) = reusable_image(parent.path());
        let spec = image.captured().specification().clone();
        let cache = LiveNativeImageReuse::default();
        let start = std::sync::Barrier::new(2);
        let producers = std::sync::atomic::AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(3);
        let results = std::thread::scope(|scope| {
            let mut handles = vec![];
            for _ in 0..2 {
                handles.push(scope.spawn(|| {
                    start.wait();
                    cache
                        .capture_with(&spec, &cancel, deadline, || {
                            producers.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(100));
                            Ok(SupervisedNativeDependencies {
                                owner: image.owner.clone(),
                            })
                        })
                        .unwrap()
                }));
            }
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(producers.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&results[0].owner, &results[1].owner));
        drop(image);
        assert_eq!(pool.reserved_storage(), 4096);
        drop(results);
        wait_release(&pool, &path);
    }

    #[test]
    fn waiting_cancellation_and_expiry_do_not_cancel_or_extend_the_producer() {
        for cancelled in [false, true] {
            let parent = tempfile::tempdir().unwrap();
            let (image, pool, path) = reusable_image(parent.path());
            let spec = image.captured().specification().clone();
            let cache = LiveNativeImageReuse::default();
            let leader_cancel = AtomicBool::new(false);
            let waiter_cancel = AtomicBool::new(false);
            let (entered, started) = std::sync::mpsc::channel();
            let (release, released) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                let (cache_ref, spec_ref, cancel_ref, image_ref) =
                    (&cache, &spec, &leader_cancel, &image);
                let leader = scope.spawn(move || {
                    cache_ref
                        .capture_with(
                            spec_ref,
                            cancel_ref,
                            Instant::now() + Duration::from_secs(3),
                            || {
                                entered.send(()).unwrap();
                                released.recv_timeout(Duration::from_secs(2)).unwrap();
                                Ok(SupervisedNativeDependencies {
                                    owner: image_ref.owner.clone(),
                                })
                            },
                        )
                        .unwrap()
                });
                started.recv_timeout(Duration::from_secs(1)).unwrap();
                let waiter = scope.spawn(|| {
                    cache.capture_with(
                        &spec,
                        &waiter_cancel,
                        Instant::now() + Duration::from_millis(50),
                        || panic!("waiter became producer"),
                    )
                });
                if cancelled {
                    std::thread::sleep(Duration::from_millis(15));
                    waiter_cancel.store(true, Ordering::Release);
                }
                let error = waiter.join().unwrap().err().expect("waiter must retire");
                assert_eq!(
                    error.code,
                    if cancelled {
                        ErrorCode::Cancelled
                    } else {
                        ErrorCode::Unavailable
                    }
                );
                assert!(!leader_cancel.load(Ordering::Acquire));
                assert_eq!(cache.producing.lock().unwrap().len(), 1);
                release.send(()).unwrap();
                let held = leader.join().unwrap();
                assert!(Arc::ptr_eq(&image.owner, &held.owner));
                held.close().unwrap();
            });
            image.close().unwrap();
            wait_release(&pool, &path);
        }
    }

    #[test]
    fn failed_producer_releases_the_matching_capture_slot() {
        let parent = tempfile::tempdir().unwrap();
        let (image, pool, path) = reusable_image(parent.path());
        let spec = image.captured().specification().clone();
        let cache = LiveNativeImageReuse::default();
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(3);
        let error = cache
            .capture_with(&spec, &cancel, deadline, || {
                Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "authored producer failure",
                ))
            })
            .err()
            .unwrap();
        assert_eq!(error.message, "authored producer failure");
        assert!(cache.producing.lock().unwrap().is_empty());
        let retry = cache
            .capture_with(&spec, &cancel, deadline, || {
                Ok(SupervisedNativeDependencies {
                    owner: image.owner.clone(),
                })
            })
            .unwrap();
        image.close().unwrap();
        retry.close().unwrap();
        wait_release(&pool, &path);
        assert!(cache.reuse(&spec, &cancel, deadline).unwrap().is_none());
    }

    #[test]
    fn distinct_declarations_can_produce_concurrently() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let (a, pool_a, path_a) = reusable_image(left.path());
        let (b, pool_b, path_b) = reusable_image(right.path());
        let cache = LiveNativeImageReuse::default();
        let producers = std::sync::Barrier::new(2);
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(3);
        std::thread::scope(|scope| {
            let mut handles = vec![];
            for image in [&a, &b] {
                let producers = &producers;
                let cache = &cache;
                let cancel = &cancel;
                handles.push(scope.spawn(move || {
                    cache
                        .capture_with(image.captured().specification(), cancel, deadline, || {
                            producers.wait();
                            Ok(SupervisedNativeDependencies {
                                owner: image.owner.clone(),
                            })
                        })
                        .unwrap()
                }));
            }
            for handle in handles {
                handle.join().unwrap().close().unwrap();
            }
        });
        a.close().unwrap();
        b.close().unwrap();
        wait_release(&pool_a, &path_a);
        wait_release(&pool_b, &path_b);
    }

    #[test]
    fn preparation_budget_preserves_bounds_deadline_and_admitted_storage() {
        for (total, expected) in [
            (21, 512 << 20),
            (512 << 20, 512 << 20),
            ((512 << 20) + 1, 513 << 20),
            (1024 << 20, 1024 << 20),
            (MAX_NATIVE_TOTAL_BYTES, 1024 << 20),
        ] {
            let budget = preparation_budget(total, 30000).unwrap();
            assert_eq!(budget.memory_bytes, expected);
            assert_eq!(budget.output_bytes, total + (16 << 20));
            assert_eq!(budget.wall_time_ms, 30000);
            assert_eq!(budget.cpu_threads, 1);
            assert!(budget.memory_bytes <= 1536 << 20);
        }
        assert!(preparation_budget(u64::MAX, 30000).is_err());
        assert!(preparation_budget(21, 0).is_err());
    }
    #[test]
    fn larger_preparation_cannot_bypass_aggregate_memory_admission() {
        let budget = preparation_budget(1945994000, 30000).unwrap();
        let too_small = ResourcePool::new(512 << 20, 1);
        assert_eq!(
            too_small.reserve(budget.memory_bytes).err().unwrap().code,
            ErrorCode::ResourceExhausted
        );
        assert_eq!(too_small.reserved_storage(), 0);
        assert!(too_small.reserve(512 << 20).is_ok());
        let pool = ResourcePool::new(1536 << 20, 2);
        let other = pool.reserve((512 << 20) + 1).unwrap();
        assert!(pool.reserve(budget.memory_bytes).is_err());
        drop(other);
        let admitted = pool.reserve(budget.memory_bytes).unwrap();
        assert!(pool.reserve((512 << 20) + 1).is_err());
        drop(admitted);
        assert!(pool.reserve(1536 << 20).is_ok());
    }
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
    }
    fn wait_release(pool: &ResourcePool, path: &Path) {
        let end = Instant::now() + Duration::from_secs(3);
        while pool.reserve(1024).is_err() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(pool.reserved_storage(), 0);
        assert!(!path.exists());
    }
    #[test]
    fn native_image_explicit_and_drop_cleanup_keep_storage_until_confirmed() {
        for explicit in [false, true] {
            let parent = tempfile::tempdir().unwrap();
            let input = parent.path().join("authored-library");
            let bytes = b"authored bytes; no native code";
            fs::write(&input, bytes).unwrap();
            let spec = NativeDependencySpec {
                schema_version: 1,
                libraries: vec![NativeLibrarySpec {
                    path: input,
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                    byte_len: bytes.len() as u64,
                    names: vec!["authored.so".into()],
                }],
            };
            let staging = tempfile::tempdir_in(parent.path()).unwrap();
            fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let staged_path = staging.path().to_owned();
            let captured = CapturedNativeDependencies::capture(&spec, staging.path()).unwrap();
            let content = captured
                .library_directory()
                .parent()
                .unwrap()
                .join("content");
            let _restore = Restore(content.clone());
            fs::set_permissions(&content, fs::Permissions::from_mode(0o500)).unwrap();
            let pool = ResourcePool::new(1024, 1);
            let owner = SupervisedNativeDependencies::new(
                captured,
                crate::staging_obligation::Staging::Legacy(staging).retaining(
                    pool.reserve(1024)
                        .unwrap()
                        .with_storage(4096, 32 * 1024 * 1024)
                        .unwrap(),
                ),
                NativeCaptureCompletion {
                    root_name: "authored-cleanup".into(),
                    declared_bytes: bytes.len() as u64,
                    request_sha256: "0".repeat(64),
                },
            );
            if explicit {
                assert!(owner.close().is_err());
            } else {
                drop(owner);
            }
            assert_eq!(pool.reserved_storage(), 4096);
            assert!(pool.reserve(1024).is_err());
            assert!(content.exists());
            fs::set_permissions(&content, fs::Permissions::from_mode(0o700)).unwrap();
            wait_release(&pool, &staged_path);
        }
    }
    #[test]
    fn pre_spawn_capture_failure_keeps_staging_claim_until_cleanup() {
        let parent = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(parent.path()).unwrap();
        let path = staging.path().to_owned();
        let _restore = Restore(path.clone());
        fs::write(path.join("admitted"), b"owned bytes").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
        let pool = ResourcePool::new(1024, 1);
        drop(Preparation {
            staging: Some(crate::staging_obligation::Staging::Legacy(staging)),
            worker: Some(Arc::new(
                pool.reserve(1024)
                    .unwrap()
                    .with_storage(4096, 32 * 1024 * 1024)
                    .unwrap(),
            )),
            retirement_assets: Vec::new(),
        });
        assert_eq!(pool.reserved_storage(), 4096);
        assert!(pool.reserve(1024).is_err());
        assert!(path.join("admitted").exists());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        wait_release(&pool, &path);
    }
}

#[cfg(test)]
#[path = "native_capture_retirement_assets_tests.rs"]
mod retirement_assets_tests;
