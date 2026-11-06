//! Trusted byte preparation, independently observed by the original lease.
//! This task copies/hashes only; model execution remains in the confined worker.
#![cfg(target_os = "linux")]
use crate::{
    resources::{Reservation, ResourcePool},
    worker_fence::WorkerFence,
    EngineConfig,
};
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn error(message: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message.to_string())
}
fn preparation_now() -> Instant {
    #[cfg(test)]
    if let Some(clock) = test_clock::current() {
        return *clock.lock().unwrap();
    }
    Instant::now()
}

#[cfg(test)]
mod test_clock {
    use super::*;
    use std::cell::RefCell;
    type Clock = Arc<Mutex<Instant>>;
    thread_local! {
        static CLOCK: RefCell<Option<Clock>> = const { RefCell::new(None) };
    }
    pub(super) struct Installed(Option<Clock>);
    pub(super) fn current() -> Option<Clock> {
        CLOCK.with(|clock| clock.borrow().clone())
    }
    pub(super) fn install(clock: Option<Clock>) -> Installed {
        Installed(CLOCK.with(|slot| slot.replace(clock)))
    }
    impl Drop for Installed {
        fn drop(&mut self) {
            CLOCK.with(|slot| slot.replace(self.0.take()));
        }
    }
}

fn active(
    cancel: &AtomicBool,
    abandoned: &AtomicBool,
    deadline: Instant,
) -> Result<(), ProtocolError> {
    if cancel.load(Ordering::Acquire) || abandoned.load(Ordering::Acquire) {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "live staging cancelled",
        ));
    }
    if preparation_now() >= deadline {
        return Err(error("live staging deadline exceeded"));
    }
    Ok(())
}
struct Parts {
    snapshots: Option<crate::staging_obligation::Staging>,
    reservation: Option<Reservation>,
    claim: Option<Arc<Mutex<Option<Arc<Reservation>>>>>,
    // Keep the bound engine epoch alive while preparation or cleanup remains.
    _fence: Option<Arc<WorkerFence>>,
    assets: crate::worker_retirement::RetirementAssets,
}
pub(crate) struct Prepared<const N: usize> {
    parts: Option<Parts>,
    paths: [PathBuf; N],
    identities: [ArtifactIdentity; N],
}
/// Completed preparation whose CPU claim has retired. Snapshot storage remains
/// in the staging owner and cannot be used as ordinary worker admission.
pub(crate) struct GpuPrepared<const N: usize>(Prepared<N>);
impl<const N: usize> GpuPrepared<N> {
    pub(crate) fn identities(&self) -> &[ArtifactIdentity; N] {
        self.0.identities()
    }
    pub(crate) fn paths(&self) -> &[PathBuf; N] {
        self.0.paths()
    }
    pub(crate) fn into_reserved_worker(
        self,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
    ) -> Result<crate::worker_retirement::WorkerCustody, ProtocolError> {
        let fence = self
            .0
            .parts
            .as_ref()
            .expect("prepared custody")
            ._fence
            .clone()
            .ok_or_else(|| error("CUDA live snapshots require an owned worker fence"))?;
        let (snapshots, _, retired_preparation, assets) = self.0.into_parts();
        let mut custody = fence
            .admit_staged_group_with_assets(reservation, memory, threads, snapshots, assets)
            .map_err(error)?
            .into_worker();
        custody.retain_asset(retired_preparation);
        Ok(custody)
    }
}
impl<const N: usize> Prepared<N> {
    pub(crate) fn identities(&self) -> &[ArtifactIdentity; N] {
        &self.identities
    }
    /// `prepare` returns only after joining the completed preparation thread.
    /// Keep byte storage charged while retiring its now-unused RAM/worker slot.
    pub(crate) fn into_gpu(self) -> Result<GpuPrepared<N>, ProtocolError> {
        {
            let owner = self
                .parts
                .as_ref()
                .expect("prepared custody")
                .claim
                .as_ref()
                .expect("prepared claim");
            let mut held = owner
                .lock()
                .map_err(|_| error("preparation claim unavailable"))?;
            let claim = held.take().expect("prepared CPU claim");
            let mut claim = match Arc::try_unwrap(claim) {
                Ok(claim) => claim,
                Err(claim) => {
                    *held = Some(claim);
                    return Err(error("preparation CPU claim is still shared"));
                }
            };
            let result = claim.release_cpu_worker();
            *held = Some(Arc::new(claim));
            result?;
        }
        Ok(GpuPrepared(self))
    }
    pub(crate) fn paths(&self) -> &[PathBuf; N] {
        &self.paths
    }
    pub(crate) fn into_worker(
        self,
        attempt: &str,
        memory: u64,
        threads: u16,
    ) -> Result<crate::worker_retirement::WorkerCustody, ProtocolError> {
        let fence = self
            .parts
            .as_ref()
            .expect("prepared live ownership")
            ._fence
            .clone();
        match fence {
            Some(fence) => {
                let (snapshots, _, reservation, assets) = self.into_parts();
                Ok(fence
                    .admit_staged_group_with_assets(reservation, memory, threads, snapshots, assets)
                    .map_err(error)?
                    .into_worker())
            }
            None => {
                // Keep preparation custody installed through raw helper creation.
                let group = match crate::cgroup::AttemptCgroup::create(attempt, memory, threads) {
                    Ok(group) => group,
                    Err(failure) => {
                        // Legacy untracked creation cannot prove what remained
                        // after a partial failure. Never publish joined for it.
                        if let Some(parts) = self.parts.as_ref() {
                            std::mem::forget(parts.assets.clone());
                        }
                        return Err(error(failure));
                    }
                };
                let (snapshots, _, reservation, assets) = self.into_parts();
                crate::worker_retirement::WorkerCustody::prepare_staged_with_assets(
                    group,
                    reservation,
                    None,
                    snapshots,
                    assets,
                )
                .map_err(error)
            }
        }
    }
    fn into_parts(
        mut self,
    ) -> (
        crate::staging_obligation::Staging,
        [PathBuf; N],
        Arc<Reservation>,
        crate::worker_retirement::RetirementAssets,
    ) {
        let mut parts = self.parts.take().expect("prepared live ownership");
        (
            parts.snapshots.take().expect("prepared live snapshots"),
            self.paths.clone(),
            parts
                .claim
                .take()
                .expect("prepared resource claim")
                .lock()
                .expect("private preparation claim")
                .take()
                .expect("prepared worker claim"),
            std::mem::take(&mut parts.assets),
        )
    }
}
impl<const N: usize> Drop for Prepared<N> {
    fn drop(&mut self) {
        let Some(parts) = self.parts.take() else {
            return;
        };
        let mut parts = parts;
        if let Some(root) = parts.snapshots.take() {
            // The preparation task has ended; no worker received these bytes.
            drop(root.cleanup_after_use_retired(parts));
        }
    }
}
struct Abandon(Arc<AtomicBool>);
impl Drop for Abandon {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// The original cancellation token and absolute end; no clock is renewed here.
struct PreparationLifetime {
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}

/// Reserve the eventual CPU launch limits before preparation begins.
/// GPU preparation passes one thread and retires this claim before GPU handoff.
#[derive(Clone, Copy)]
pub(crate) struct PreparationBudget {
    pub memory_bytes: u64,
    pub cpu_threads: u16,
}
pub(crate) struct PreparationTracking {
    pub fence: Option<Arc<WorkerFence>>,
    pub assets: crate::worker_retirement::RetirementAssets,
}

pub(crate) fn prepare_tracked<const N: usize>(
    config: &EngineConfig,
    pool: &ResourcePool,
    refs: [ArtifactReference; N],
    budget: PreparationBudget,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    tracking: PreparationTracking,
) -> Result<Prepared<N>, ProtocolError> {
    prepare_inner_tracked(
        config,
        pool,
        refs,
        budget,
        PreparationLifetime { cancel, deadline },
        tracking,
        #[cfg(test)]
        None,
    )
}

pub(crate) fn prepare<const N: usize>(
    config: &EngineConfig,
    pool: &ResourcePool,
    refs: [ArtifactReference; N],
    budget: PreparationBudget,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    fence: Option<Arc<WorkerFence>>,
) -> Result<Prepared<N>, ProtocolError> {
    prepare_inner(
        config,
        pool,
        refs,
        budget,
        PreparationLifetime { cancel, deadline },
        fence,
        #[cfg(test)]
        None,
    )
}

fn prepare_inner<const N: usize>(
    config: &EngineConfig,
    pool: &ResourcePool,
    refs: [ArtifactReference; N],
    budget: PreparationBudget,
    lifetime: PreparationLifetime,
    fence: Option<Arc<WorkerFence>>,
    #[cfg(test)] post_reply: Option<Box<dyn FnOnce() + Send>>,
) -> Result<Prepared<N>, ProtocolError> {
    prepare_inner_tracked(
        config,
        pool,
        refs,
        budget,
        lifetime,
        PreparationTracking {
            fence,
            assets: Vec::new(),
        },
        #[cfg(test)]
        post_reply,
    )
}
fn prepare_inner_tracked<const N: usize>(
    config: &EngineConfig,
    pool: &ResourcePool,
    refs: [ArtifactReference; N],
    budget: PreparationBudget,
    lifetime: PreparationLifetime,
    tracking: PreparationTracking,
    #[cfg(test)] post_reply: Option<Box<dyn FnOnce() + Send>>,
) -> Result<Prepared<N>, ProtocolError> {
    let PreparationTracking { fence, assets } = tracking;
    let PreparationLifetime { cancel, deadline } = lifetime;
    if N == 0 {
        return Err(ProtocolError::invalid("live staging requires inputs"));
    }
    let abandoned = Arc::new(AtomicBool::new(false));
    let _abandon = Abandon(abandoned.clone());
    active(&cancel, &abandoned, deadline)?;
    let total = refs
        .iter()
        .try_fold(0u64, |n, v| n.checked_add(v.identity.byte_len))
        .ok_or_else(|| error("live snapshot sum overflow"))?;
    if refs
        .iter()
        .any(|v| v.identity.byte_len == 0 || v.identity.byte_len > 1024 * 1024 * 1024)
        || total > config.max_snapshot_bytes
    {
        return Err(ProtocolError::invalid(
            "live snapshots exceed file/storage bounds",
        ));
    }
    // Admission bounds outstanding preparation tasks, including blocked I/O.
    let mut prepared = Prepared {
        parts: Some(Parts {
            snapshots: None,
            reservation: Some(pool.reserve_worker(budget.memory_bytes, budget.cpu_threads)?),
            claim: None,
            _fence: fence,
            assets,
        }),
        paths: std::array::from_fn(|_| PathBuf::new()),
        identities: std::array::from_fn(|index| refs[index].identity.clone()),
    };
    let (send, receive) = mpsc::sync_channel(1);
    let task_cancel = cancel.clone();
    let task_abandoned = abandoned.clone();
    let task_assets = prepared
        .parts
        .as_ref()
        .expect("staging owner")
        .assets
        .clone();
    let join_assets = task_assets.clone();
    #[cfg(test)]
    let test_clock = test_clock::current();
    let task = std::thread::Builder::new()
        .name("pulsar-live-staging".into())
        .spawn(move || {
            let _task_assets = task_assets;
            #[cfg(test)]
            let _test_clock = test_clock::install(test_clock);
            // A channel reply is not proof that the producer has exited. Keep
            // independent claims through its epilogue, including receiver loss.
            let mut task_retention = None;
            let result = (|| -> Result<(), ProtocolError> {
                active(&task_cancel, &task_abandoned, deadline)?;
                let parts = prepared.parts.as_mut().expect("live staging ownership");
                let parent = parts
                    ._fence
                    .as_ref()
                    .map(|fence| fence.staging_parent().to_owned())
                    .unwrap_or_else(std::env::temp_dir);
                let mut reservation = parts
                    .reservation
                    .take()
                    .expect("pending staging claim")
                    .with_storage(total, fs2::available_space(&parent).map_err(error)?)?;
                let storage = Arc::new(reservation.split_storage()?);
                let claim = Arc::new(Mutex::new(Some(Arc::new(reservation))));
                parts.claim = Some(claim.clone());
                task_retention = Some((claim.clone(), storage.clone()));
                let root = match &parts._fence {
                    Some(fence) => crate::staging_obligation::Staging::Durable(
                        fence.create_staging((claim, storage)).map_err(error)?,
                    ),
                    None => crate::staging_obligation::Staging::Legacy(
                        tempfile::Builder::new()
                            .prefix("live-staging-")
                            .tempdir()
                            .map_err(error)?,
                    )
                    .retaining((claim, storage)),
                };
                parts.snapshots = Some(root);
                let directory = parts.snapshots.as_ref().unwrap().path();
                for (index, reference) in refs.into_iter().enumerate() {
                    active(&task_cancel, &task_abandoned, deadline)?;
                    let mut source =
                        crate::artifacts::open_regular(&reference.path).map_err(error)?;
                    let (path, hash, size) = crate::artifacts::snapshot_opened_reserved(
                        &mut source,
                        &directory,
                        reference.identity.byte_len,
                        0,
                    )
                    .map_err(error)?;
                    if hash != reference.identity.sha256 || size != reference.identity.byte_len {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "live snapshot differs from admitted bytes",
                        ));
                    }
                    prepared.paths[index] = path;
                }
                active(&task_cancel, &task_abandoned, deadline)?;
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    &prepared.paths[0],
                    std::fs::Permissions::from_mode(0o500),
                )
                .map_err(error)?;
                active(&task_cancel, &task_abandoned, deadline)
            })();
            let reply = result.map(|_| prepared);
            // A lost receiver drops the opaque bundle into independent cleanup.
            if send.send(reply).is_ok() {
                #[cfg(test)]
                if let Some(post_reply) = post_reply {
                    post_reply();
                }
            }
            drop(task_retention);
        })
        .map_err(error)?;
    loop {
        if let Err(failure) = active(&cancel, &abandoned, deadline) {
            retire_preparation::<N>(task, Err(failure.clone()), join_assets);
            return Err(failure);
        }
        let wait = deadline
            .saturating_duration_since(preparation_now())
            .min(Duration::from_millis(10));
        match receive.recv_timeout(wait) {
            Ok(reply) => {
                while !task.is_finished() {
                    if let Err(error) = active(&cancel, &abandoned, deadline) {
                        retire_preparation::<N>(task, reply, join_assets);
                        return Err(error);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                task.join()
                    .map_err(|_| error("live staging task panicked after reply"))?;
                active(&cancel, &abandoned, deadline)?;
                return reply;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let failure = error("live staging task terminated");
                retire_preparation::<N>(task, Err(failure.clone()), join_assets);
                return Err(failure);
            }
        }
    }
}

fn retire_preparation<const N: usize>(
    task: std::thread::JoinHandle<()>,
    reply: Result<Prepared<N>, ProtocolError>,
    assets: crate::worker_retirement::RetirementAssets,
) {
    let owner = Arc::new(Mutex::new(Some((task, reply, assets))));
    let retained = owner.clone();
    if std::thread::Builder::new()
        .name("pulsar-staging-retirement".into())
        .spawn(move || {
            let (task, reply, assets) = retained
                .lock()
                .expect("private staging retirement")
                .take()
                .unwrap();
            let _ = task.join();
            drop(reply);
            drop(assets);
        })
        .is_err()
    {
        // Losing retirement custody would release an outstanding task's claims.
        // Keep its lease/capacity until process exit if a reaper cannot start.
        std::mem::forget(owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_retirement_asset_waits_for_actual_staging_join_and_cleanup() {
        let (_root, config, reference, fence) = fixture();
        let pool = ResourcePool::new(1024, 1);
        let cancel = Arc::new(AtomicBool::new(false));
        let (receipt, asset) = crate::live_inference::CpuWorkerRetirement::new();
        let (entered_send, entered) = mpsc::channel();
        let (release_send, released) = mpsc::channel();
        struct Release(Option<mpsc::Sender<()>>);
        impl Drop for Release {
            fn drop(&mut self) {
                if let Some(send) = self.0.take() {
                    let _ = send.send(());
                }
            }
        }
        let release_guard = Release(Some(release_send));
        let task_cancel = cancel.clone();
        let task_pool = pool.clone();
        let caller = std::thread::spawn(move || {
            prepare_inner_tracked(
                &config,
                &task_pool,
                [reference],
                PreparationBudget {
                    memory_bytes: 1024,
                    cpu_threads: 1,
                },
                PreparationLifetime {
                    cancel: task_cancel,
                    deadline: Instant::now() + Duration::from_secs(5),
                },
                PreparationTracking {
                    fence: Some(fence),
                    assets: vec![asset],
                },
                Some(Box::new(move || {
                    entered_send.send(()).unwrap();
                    let _ = released.recv();
                })),
            )
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        cancel.store(true, Ordering::Release);
        assert!(caller.join().unwrap().is_err());
        assert!(
            !receipt.joined(),
            "abandoned reply is not producer join or staging cleanup"
        );
        assert!(receipt.wait(Instant::now()).is_err());
        drop(release_guard);
        receipt
            .wait(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(pool.reserved_storage(), 0);
        assert!(pool.reserve(1024).is_ok());
    }
    fn fixture() -> (
        tempfile::TempDir,
        EngineConfig,
        ArtifactReference,
        Arc<WorkerFence>,
    ) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::write(&source, b"authored snapshot").unwrap();
        let (sha256, byte_len) = crate::artifacts::digest_file(&source, 1024).unwrap();
        let reference = ArtifactReference {
            path: source.clone(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len,
            },
        };
        let state = root.path().join("state");
        crate::artifacts::private_directory(&state).unwrap();
        let fence = Arc::new(WorkerFence::open(&state).unwrap());
        fence.enable_staging().unwrap();
        (root, EngineConfig::new(state, source), reference, fence)
    }
    fn await_cleanup(pool: &ResourcePool, path: &std::path::Path) {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if pool.reserved_storage() == 0 && !path.exists() && pool.reserve(1024).is_ok() {
                break;
            }
            assert!(Instant::now() < end, "preparation custody failed to retire");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn gpu_handoff_releases_cpu_capacity_and_retains_exact_snapshot_storage() {
        for durable in [false, true] {
            let (_root, config, reference, fence) = fixture();
            let pool = ResourcePool::new(1024, 1);
            let size = reference.identity.byte_len;
            let prepared = prepare(
                &config,
                &pool,
                [reference],
                PreparationBudget {
                    memory_bytes: 1024,
                    cpu_threads: 1,
                },
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(5),
                durable.then(|| fence.clone()),
            )
            .unwrap();
            let path = prepared.paths()[0].clone();
            assert!(pool.reserve(1).is_err());
            assert_eq!(pool.reserved_storage(), size);
            let prepared = prepared.into_gpu().unwrap();
            let control = pool
                .reserve(1024)
                .expect("completed preparation must release its worker slot and RAM");
            assert_eq!(pool.reserved_storage(), size);
            assert_eq!(std::fs::read(&path).unwrap(), b"authored snapshot");
            drop(control);
            drop(prepared);
            await_cleanup(&pool, &path);
            let ledger: serde_json::Value = serde_json::from_slice(
                &std::fs::read(config.state_dir.join("worker-fence.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(ledger["staging"].as_object().unwrap().len(), 0);
        }
    }
    struct Release(Option<mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(send) = self.0.take() {
                let _ = send.send(());
            }
        }
    }
    fn blocked_epilogue(cancelled: bool) {
        let (_root, config, reference, fence) = fixture();
        let pool = ResourcePool::new(1024, 1);
        let size = reference.identity.byte_len;
        let cancel = Arc::new(AtomicBool::new(false));
        let (entered_send, entered) = mpsc::channel();
        let (release_send, release) = mpsc::channel();
        let guard = Release(Some(release_send));
        let (returned_send, returned) = mpsc::channel();
        // Freeze observation time until the actual producer has sent its reply.
        // Storage/scheduling delay must not expire the admitted lifetime before
        // this test reaches the post-reply state whose custody it checks.
        let now = Instant::now();
        let deadline = now + Duration::from_secs(2);
        let clock = Arc::new(Mutex::new(now));
        let caller_clock = clock.clone();
        let task_pool = pool.clone();
        let task_cancel = cancel.clone();
        let caller = std::thread::spawn(move || {
            let _clock = test_clock::install(Some(caller_clock));
            let result = prepare_inner(
                &config,
                &task_pool,
                [reference],
                PreparationBudget {
                    memory_bytes: 1024,
                    cpu_threads: 1,
                },
                PreparationLifetime {
                    cancel: task_cancel,
                    deadline,
                },
                Some(fence),
                Some(Box::new(move || {
                    entered_send.send(()).unwrap();
                    let _ = release.recv();
                })),
            );
            returned_send.send(result.err().map(|e| e.code)).unwrap();
        });
        if let Err(error) = entered.recv_timeout(Duration::from_secs(60)) {
            panic!("producer never reached post-reply hook: {error:?}; caller outcome before hook: {:?}; reserved storage: {}",
                returned.try_recv(), pool.reserved_storage());
        }
        assert!(matches!(
            returned.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(pool.reserved_storage(), size);
        if cancelled {
            cancel.store(true, Ordering::Release);
        } else {
            // The original deadline is immutable; only observed time advances.
            *clock.lock().unwrap() = deadline;
        }
        let code = returned
            .recv_timeout(Duration::from_secs(3))
            .expect("caller joined a still-live producer");
        assert_eq!(
            code,
            Some(if cancelled {
                ErrorCode::Cancelled
            } else {
                ErrorCode::Unavailable
            })
        );
        assert!(
            pool.reserve(1).is_err(),
            "producer released capacity before exiting"
        );
        assert_eq!(pool.reserved_storage(), size);
        drop(guard);
        caller.join().unwrap();
        // The returned bundle stays in the retirement owner until actual join.
        let end = Instant::now() + Duration::from_secs(5);
        while pool.reserved_storage() != 0 || pool.reserve(1024).is_err() {
            assert!(Instant::now() < end, "retirement did not release claims");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn expired_post_reply_producer_retains_capacity_until_join() {
        blocked_epilogue(false);
    }
    #[test]
    fn cancelled_post_reply_producer_retains_capacity_until_join() {
        blocked_epilogue(true);
    }
}
