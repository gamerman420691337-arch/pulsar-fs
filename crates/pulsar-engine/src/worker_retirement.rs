//! Owned worker trees retain admission until exit AND cgroup emptiness are observed.
//! Restart admission also requires the engine scope binding and durable roster.
#![cfg(target_os = "linux")]
use crate::{cgroup::AttemptCgroup, resources::Reservation, worker_fence::WorkerFence};
use std::{
    any::Any,
    io,
    process::{Child, Command},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub(crate) type RetirementAssets = Vec<Arc<dyn Any + Send + Sync>>;

#[cfg(test)]
#[path = "staging_crash_tests.rs"]
mod staging_crash_tests;
#[cfg(test)]
#[path = "worker_staging_transfer_tests.rs"]
mod staging_transfer_tests;

/// Watchdogs signal only their held group and poll their direct child without
/// blocking wait. This alone never clears the roster or releases admission.
pub(crate) fn signal_and_poll_child(
    group: &AttemptCgroup,
    child: &Mutex<Child>,
    deadline: Instant,
) {
    group.kill();
    if let Ok(mut child) = child.try_lock() {
        let _ = child.kill();
    }
    loop {
        if let Ok(mut child) = child.try_lock() {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => {}
            }
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

trait Retiring: Send + 'static {
    fn signal(&mut self);
    fn retired(&mut self) -> io::Result<bool>;
}
pub(crate) struct WorkerTree {
    child: Option<Arc<Mutex<Child>>>,
    spawn_attempted: bool,
    pub(crate) group: Arc<AttemptCgroup>,
    fence: Option<Arc<WorkerFence>>,
    fence_pending: bool,
    threads: Vec<std::thread::JoinHandle<()>>,
    assets: Vec<Box<dyn Any + Send>>,
    _snapshots: Option<crate::staging_obligation::Staging>,
    cleanup: Option<crate::staging_cleanup::Cleanup>,
    // Normal retirement retains admission through staging/fence cleanup;
    // capture handoff releases CPU only after users and exact group retirement.
    _reservation: Arc<Reservation>,
    // Last field: completion guards cannot publish before other native assets
    // and admission have been released after confirmed retirement.
    _retirement_assets: RetirementAssets,
}
impl Retiring for WorkerTree {
    fn signal(&mut self) {
        self.group.kill();
        if let Some(child) = &self.child {
            if let Ok(mut child) = child.try_lock() {
                let _ = child.kill();
            }
        }
    }
    fn retired(&mut self) -> io::Result<bool> {
        if !self.users_retired()? {
            return Ok(false);
        }
        if let Some(snapshots) = self._snapshots.take() {
            self.cleanup = Some(
                snapshots
                    .cleanup_after_use_retired((self._reservation.clone(), self.fence.clone())),
            );
        }
        if self
            .cleanup
            .as_ref()
            .is_some_and(|cleanup| !cleanup.is_complete())
        {
            return Ok(false);
        }
        self.retire_group()
    }
}
impl WorkerTree {
    fn users_retired(&self) -> io::Result<bool> {
        // Observing the direct child alone cannot release a process-tree claim.
        if let Some(child) = &self.child {
            let mut child = match child.try_lock() {
                Ok(child) => child,
                Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
                Err(error) => return Err(io::Error::other(error.to_string())),
            };
            if child.try_wait()?.is_none() {
                return Ok(false);
            }
        }
        if self.threads.iter().any(|thread| !thread.is_finished()) {
            return Ok(false);
        }
        if !self.group.is_empty()? {
            return Ok(false);
        }
        Ok(true)
    }
    fn retire_group(&mut self) -> io::Result<bool> {
        if self.fence_pending {
            let fence = self.fence.as_ref().expect("pending worker fence");
            match self.child.as_mut() {
                Some(child) => {
                    let mut child = match child.try_lock() {
                        Ok(child) => child,
                        Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
                        Err(error) => return Err(io::Error::other(error.to_string())),
                    };
                    fence.retire(&mut child, &self.group)
                }
                None => fence.retire_unspawned(&self.group),
            }
            .map_err(io::Error::other)?;
            self.fence_pending = false;
        } else {
            self.group.reclaim()?;
        }
        Ok(true)
    }
}
struct Custody<T: Retiring> {
    held: Option<T>,
}
impl<T: Retiring> Custody<T> {
    fn retire(&mut self, deadline: Instant) -> io::Result<()> {
        let held = self.held.as_mut().expect("owned retirement state");
        held.signal();
        loop {
            if held.retired()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "worker tree retirement unconfirmed",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl<T: Retiring> Drop for Custody<T> {
    fn drop(&mut self) {
        let Some(mut held) = self.held.take() else {
            return;
        };
        if held.retired().unwrap_or(false) {
            return;
        }
        held.signal();
        let custody = Arc::new(Mutex::new(Some(held)));
        let reaper = custody.clone();
        if std::thread::Builder::new()
            .name("pulsar-worker-retirement".into())
            .spawn(move || {
                let Some(mut held) = reaper.lock().unwrap_or_else(|e| e.into_inner()).take() else {
                    return;
                };
                loop {
                    held.signal();
                    if held.retired().unwrap_or(false) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
            .is_err()
        {
            // Failed thread creation must not drop the only resource owner.
            std::mem::forget(custody);
        }
    }
}
pub(crate) struct WorkerCustody(Custody<WorkerTree>);
impl WorkerCustody {
    /// The admitted creator has already acknowledged registration. This transfer
    /// is infallible and installs process-tree custody before exposing a handle.
    pub(crate) fn from_admitted(admitted: crate::worker_fence::AdmittedGroup) -> Self {
        let (group, reservation, fence, snapshots, assets) = admitted.worker_parts();
        Self(Custody {
            held: Some(WorkerTree {
                child: None,
                spawn_attempted: false,
                group: Arc::new(group),
                fence: Some(fence),
                fence_pending: true,
                threads: Vec::new(),
                assets: Vec::new(),
                _snapshots: snapshots,
                cleanup: None,
                _reservation: reservation,
                _retirement_assets: assets,
            }),
        })
    }
    /// Take ownership before registration or fork. Failure drops into the same
    /// confirmed-retirement/reaper path as a running worker.
    pub(crate) fn prepare(
        group: AttemptCgroup,
        reservation: Arc<Reservation>,
        fence: Option<Arc<WorkerFence>>,
    ) -> anyhow::Result<Self> {
        Self::prepare_inner(group, reservation, fence, None, Vec::new())
    }
    #[cfg(test)]
    pub(crate) fn prepare_staged(
        group: AttemptCgroup,
        reservation: Arc<Reservation>,
        fence: Option<Arc<WorkerFence>>,
        snapshots: impl Into<crate::staging_obligation::Staging>,
    ) -> anyhow::Result<Self> {
        Self::prepare_inner(
            group,
            reservation,
            fence,
            Some(snapshots.into()),
            Vec::new(),
        )
    }
    pub(crate) fn prepare_staged_with_assets(
        group: AttemptCgroup,
        reservation: Arc<Reservation>,
        fence: Option<Arc<WorkerFence>>,
        snapshots: impl Into<crate::staging_obligation::Staging>,
        assets: RetirementAssets,
    ) -> anyhow::Result<Self> {
        Self::prepare_inner(group, reservation, fence, Some(snapshots.into()), assets)
    }
    fn prepare_inner(
        group: AttemptCgroup,
        reservation: Arc<Reservation>,
        fence: Option<Arc<WorkerFence>>,
        snapshots: Option<crate::staging_obligation::Staging>,
        assets: RetirementAssets,
    ) -> anyhow::Result<Self> {
        let custody = Self(Custody {
            held: Some(WorkerTree {
                child: None,
                spawn_attempted: false,
                group: Arc::new(group),
                _reservation: reservation,
                fence_pending: fence.is_some(),
                fence,
                threads: Vec::new(),
                assets: Vec::new(),
                _snapshots: snapshots,
                cleanup: None,
                _retirement_assets: assets,
            }),
        });
        let tree = custody.0.held.as_ref().expect("prepared worker tree");
        if let Some(fence) = &tree.fence {
            fence.lease_fd()?;
            fence.register(&tree.group)?;
        }
        Ok(custody)
    }
    pub(crate) fn spawn(&mut self, command: &mut Command) -> io::Result<()> {
        let tree = self.tree();
        if tree.spawn_attempted || (tree.fence.is_some() && !tree.fence_pending) {
            return Err(io::Error::other("worker custody cannot spawn again"));
        }
        if let Some(fence) = &tree.fence {
            fence.lease_fd().map_err(io::Error::other)?;
        }
        tree.spawn_attempted = true;
        tree.child = Some(Arc::new(Mutex::new(command.spawn()?)));
        Ok(())
    }
    pub(crate) fn child(&mut self) -> std::sync::MutexGuard<'_, Child> {
        self.tree()
            .child
            .as_ref()
            .expect("spawned worker child")
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
    pub(crate) fn child_handle(&mut self) -> Arc<Mutex<Child>> {
        self.tree()
            .child
            .as_ref()
            .expect("spawned worker child")
            .clone()
    }
    pub(crate) fn group_handle(&mut self) -> Arc<AttemptCgroup> {
        self.tree().group.clone()
    }
    /// Install descriptors/image owners before any fallible command preparation.
    /// Assets remain installed through failed spawn, transport completion, exact
    /// group retirement and the ordinary cleanup/reaper path.
    pub(crate) fn retain_asset<T: Any + Send>(&mut self, asset: T) -> &mut T {
        self.tree().assets.push(Box::new(asset));
        self.tree()
            .assets
            .last_mut()
            .unwrap()
            .downcast_mut::<T>()
            .expect("installed worker asset type")
    }
    /// Per-use completion guards remain last-drop obligations of this worker tree.
    pub(crate) fn retain_retirement_assets(&mut self, assets: RetirementAssets) {
        self.tree()._retirement_assets.extend(assets);
    }
    pub(crate) fn retain_thread(&mut self, thread: std::thread::JoinHandle<()>) {
        self.tree().threads.push(thread);
    }
    pub(crate) fn tree(&mut self) -> &mut WorkerTree {
        self.0.held.as_mut().expect("owned worker tree")
    }
    pub(crate) fn retire(&mut self, deadline: Instant) -> io::Result<()> {
        self.0.retire(deadline)
    }
    /// Return one uniquely typed asset only after ordinary cleanup and exact
    /// group retirement. Errors keep the complete reaper owner installed.
    pub(crate) fn retire_into_asset<T: Any + Send>(mut self, deadline: Instant) -> io::Result<T> {
        let indices = self
            .tree()
            .assets
            .iter()
            .enumerate()
            .filter_map(|(index, asset)| asset.is::<T>().then_some(index))
            .collect::<Vec<_>>();
        if indices.len() != 1 {
            return Err(io::Error::other(
                "worker asset handoff requires one matching owner",
            ));
        }
        self.0.retire(deadline)?;
        let mut tree = self.0.held.take().expect("retired worker custody");
        Ok(*tree
            .assets
            .remove(indices[0])
            .downcast::<T>()
            .expect("checked worker asset type"))
    }
    /// Transfer byte custody only after child, transports and the exact group
    /// have retired. Failure leaves the ordinary cleanup/reaper owner installed.
    pub(crate) fn retire_into_staging(
        mut self,
        deadline: Instant,
    ) -> io::Result<crate::staging_obligation::Staging> {
        if self.tree()._snapshots.is_none() {
            return Err(io::Error::other(
                "worker has no staging custody to transfer",
            ));
        }
        self.tree().signal();
        loop {
            if self.tree().users_retired()? && self.tree().retire_group()? {
                break;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "capture users retirement unconfirmed",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Nothing fallible follows removal of the installed worker owner.
        let mut tree = self.0.held.take().expect("retired worker custody");
        Ok(tree._snapshots.take().expect("checked capture staging"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ResourcePool;
    use std::sync::atomic::{AtomicBool, Ordering};
    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly"]
    fn native_retirement_receipt_follows_live_child_transport_and_deferred_reaper() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let root = fence_fixture();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        fence.enable_creation().unwrap();
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let reservation = Arc::new(
            pool.reserve(bytes)
                .unwrap()
                .with_storage(4096, bytes)
                .unwrap(),
        );
        let staging = tempfile::tempdir().unwrap();
        let path = staging.path().to_owned();
        let (receipt, asset) = crate::live_inference::CpuWorkerRetirement::new();
        let mut custody = fence
            .admit_staged_group_with_assets(reservation, bytes, 1, staging.into(), vec![asset])
            .unwrap()
            .into_worker();
        let group = custody.group_handle();
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) == 1 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            });
        }
        custody.spawn(&mut command).unwrap();
        assert!(custody.child().try_wait().unwrap().is_none());
        assert!(!receipt.joined());
        let (release, receive) = std::sync::mpsc::channel();
        struct Release(Option<std::sync::mpsc::Sender<()>>);
        impl Drop for Release {
            fn drop(&mut self) {
                if let Some(send) = self.0.take() {
                    let _ = send.send(());
                }
            }
        }
        let release = Release(Some(release));
        custody.retain_thread(std::thread::spawn(move || {
            let _ = receive.recv();
        }));
        let child = custody.child_handle();
        let held_child = child.lock().unwrap();
        drop(custody);
        assert!(
            !receipt.joined(),
            "Drop/cancel cannot infer child or transport retirement"
        );
        assert!(receipt.wait(Instant::now()).is_err());
        drop(held_child);
        assert!(
            !receipt.joined(),
            "live transport still owns original custody"
        );
        drop(release);
        receipt
            .wait(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(!path.exists());
        assert_eq!(pool.reserved_storage(), 0);
        assert!(pool.reserve(bytes).is_ok());
    }
    struct Evidence {
        child: Arc<AtomicBool>,
        tree: Arc<AtomicBool>,
        _reservation: Arc<Reservation>,
        error: Arc<AtomicBool>,
    }
    impl Retiring for Evidence {
        fn signal(&mut self) {}
        fn retired(&mut self) -> io::Result<bool> {
            if self.error.load(Ordering::Acquire) {
                return Err(io::Error::other("population observation failed"));
            }
            Ok(self.child.load(Ordering::Acquire) && self.tree.load(Ordering::Acquire))
        }
    }
    fn scenario(child_exited: bool, tree_empty: bool, observation_error: bool) {
        let pool = ResourcePool::new(1024, 1);
        let reservation = Arc::new(
            pool.reserve(1024)
                .unwrap()
                .with_storage(1024, 32 * 1024 * 1024)
                .unwrap(),
        );
        let error = Arc::new(AtomicBool::new(observation_error));
        let child = Arc::new(AtomicBool::new(child_exited));
        let tree = Arc::new(AtomicBool::new(tree_empty));
        let mut custody = Custody {
            held: Some(Evidence {
                child: child.clone(),
                tree: tree.clone(),
                _reservation: reservation.clone(),
                error: error.clone(),
            }),
        };
        drop(reservation);
        assert_eq!(
            custody.retire(Instant::now()).unwrap_err().kind(),
            if observation_error {
                io::ErrorKind::Other
            } else {
                io::ErrorKind::TimedOut
            }
        );
        drop(custody);
        assert_eq!(pool.reserved_storage(), 1024);
        assert!(
            pool.reserve(1024).is_err(),
            "unconfirmed retirement released capacity"
        );
        error.store(false, Ordering::Release);
        child.store(true, Ordering::Release);
        tree.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if pool.reserve(1024).is_ok() {
                assert_eq!(pool.reserved_storage(), 0);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn exited_child_with_populated_tree_retains_capacity() {
        scenario(true, false, false);
    }
    #[test]
    fn empty_tree_with_unreaped_child_retains_capacity() {
        scenario(false, true, false);
    }
    #[test]
    fn failed_population_observation_retains_capacity() {
        scenario(true, true, true);
    }
    #[test]
    fn confirmed_retirement_releases_on_drop() {
        let pool = ResourcePool::new(1024, 1);
        let reservation = Arc::new(pool.reserve(1024).unwrap());
        let mut custody = Custody {
            held: Some(Evidence {
                child: Arc::new(AtomicBool::new(true)),
                tree: Arc::new(AtomicBool::new(true)),
                _reservation: reservation,
                error: Arc::new(AtomicBool::new(false)),
            }),
        };
        custody.retire(Instant::now()).unwrap();
        assert!(pool.reserve(1024).is_err());
        drop(custody);
        assert!(pool.reserve(1024).is_ok());
    }
    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly"]
    fn native_orphan_retirement_keeps_admission_until_tree_empty() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        use std::process::{Command, Stdio};
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let reservation = Arc::new(pool.reserve(bytes).unwrap());
        let group = AttemptCgroup::create(
            &format!("retirement-test-{}", uuid::Uuid::new_v4()),
            bytes,
            1,
        )
        .unwrap();
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30 & exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut custody = WorkerCustody::prepare(group, reservation.clone(), None).unwrap();
        custody.spawn(&mut command).unwrap();
        drop(reservation);
        let deadline = Instant::now() + Duration::from_secs(3);
        while custody.child().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !custody.tree().group.is_empty().unwrap(),
            "fixture did not retain a descendant"
        );
        assert!(pool.reserve(bytes).is_err());
        custody.retire(deadline).unwrap();
        assert!(custody.tree().group.is_empty().unwrap());
        assert!(pool.reserve(bytes).is_err());
        drop(custody);
        assert!(pool.reserve(bytes).is_ok());
    }

    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly"]
    fn native_transport_thread_retains_staging_admission_and_lease_after_child_exit() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let root = fence_fixture();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        let scope = fence.scope().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let staged_path = staging.path().to_owned();
        std::fs::write(staged_path.join("admitted"), b"staged bytes").unwrap();
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let reservation = Arc::new(
            pool.reserve(bytes)
                .unwrap()
                .with_storage(4096, 32 * 1024 * 1024)
                .unwrap(),
        );
        let group =
            AttemptCgroup::create(&format!("live-custody-{}", uuid::Uuid::new_v4()), bytes, 1)
                .unwrap();
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/true");
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut custody =
            WorkerCustody::prepare_staged(group, reservation, Some(fence.clone()), staging)
                .unwrap();
        custody.spawn(&mut command).unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        custody.retain_thread(std::thread::spawn(move || {
            let _ = wait.recv();
        }));
        let deadline = Instant::now() + Duration::from_secs(3);
        while custody.child().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(custody.tree().group.is_empty().unwrap());
        assert_eq!(
            custody.retire(Instant::now()).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        drop(custody);
        drop(fence);
        assert!(staged_path.join("admitted").exists());
        assert_eq!(pool.reserved_storage(), 4096);
        assert!(pool.reserve(bytes).is_err());
        assert_eq!(roster_size(root.path()), 1);
        assert!(WorkerFence::open_bound(root.path(), Some(&scope)).is_err());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while pool.reserve(bytes).is_err() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(pool.reserved_storage(), 0);
        assert!(!staged_path.exists());
        assert_eq!(roster_size(root.path()), 0);
        WorkerFence::open_bound(root.path(), Some(&scope)).unwrap();
    }

    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly serially"]
    fn native_failed_staging_cleanup_retains_scope_and_admission_until_retry() {
        use std::os::{
            fd::AsRawFd,
            unix::{fs::PermissionsExt, process::CommandExt},
        };
        struct Restore(std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = fence_fixture();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        let scope = fence.scope().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let staged_path = staging.path().to_owned();
        let _restore = Restore(staged_path.clone());
        std::fs::write(staged_path.join("admitted"), b"staged bytes").unwrap();
        std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(
            std::fs::remove_dir_all(&staged_path).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let reservation = Arc::new(
            pool.reserve(bytes)
                .unwrap()
                .with_storage(4096, bytes)
                .unwrap(),
        );
        let group = owned_group(bytes);
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/true");
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut custody =
            WorkerCustody::prepare_staged(group, reservation, Some(fence.clone()), staging)
                .unwrap();
        custody.spawn(&mut command).unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        while custody.child().try_wait().unwrap().is_none() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(custody.tree().group.is_empty().unwrap());
        let retirement = custody.retire(Instant::now() + Duration::from_millis(100));
        drop(custody);
        drop(fence);
        assert!(staged_path.join("admitted").exists());
        assert_eq!(pool.reserved_storage(), 4096);
        assert!(pool.reserve(bytes).is_err());
        assert!(WorkerFence::open_bound(root.path(), Some(&scope)).is_err());
        assert!(
            retirement.is_err(),
            "failed staging deletion must refuse successful retirement"
        );
        std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        while pool.reserve(bytes).is_err() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(pool.reserved_storage(), 0);
        assert!(!staged_path.exists());
        assert_eq!(roster_size(root.path()), 0);
        WorkerFence::open_bound(root.path(), Some(&scope)).unwrap();
    }

    fn fence_fixture() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        root
    }
    fn roster_size(root: &std::path::Path) -> usize {
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("worker-fence.json")).unwrap())
                .unwrap();
        value["groups"].as_object().unwrap().len()
    }
    fn retired_roster_size(root: &std::path::Path) -> usize {
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("worker-fence.json")).unwrap())
                .unwrap();
        value["retired"].as_object().unwrap().len()
    }
    fn owned_group(bytes: u64) -> AttemptCgroup {
        AttemptCgroup::create(&format!("custody-test-{}", uuid::Uuid::new_v4()), bytes, 1).unwrap()
    }
    fn block_ledger(root: &std::path::Path) {
        std::fs::rename(
            root.join("worker-fence.json"),
            root.join("retained-ledger.json"),
        )
        .unwrap();
        std::fs::create_dir(root.join("worker-fence.json")).unwrap();
    }
    fn restore_ledger(root: &std::path::Path) {
        std::fs::remove_dir(root.join("worker-fence.json")).unwrap();
        std::fs::rename(
            root.join("retained-ledger.json"),
            root.join("worker-fence.json"),
        )
        .unwrap();
    }
    fn wait_for_capacity(pool: &ResourcePool, bytes: u64) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while pool.reserve(bytes).is_err() {
            assert!(
                Instant::now() < deadline,
                "reaper did not release confirmed custody"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn reopen_after_reaper(root: &std::path::Path) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match WorkerFence::open(root) {
                Ok(_) => break,
                Err(error)
                    if error
                        .downcast_ref::<io::Error>()
                        .is_some_and(|e| e.kind() == io::ErrorKind::WouldBlock)
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("fence reopening failed: {error:#}"),
            }
        }
    }
    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly serially"]
    fn native_failed_spawn_reclaims_group_before_releasing_capacity() {
        let root = fence_fixture();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let group = owned_group(bytes);
        let path = group.path().to_path_buf();
        let mut custody =
            WorkerCustody::prepare(group, Arc::new(pool.reserve(bytes).unwrap()), Some(fence))
                .unwrap();
        assert_eq!(roster_size(root.path()), 1);
        let mut command = Command::new(root.path().join("missing-executable"));
        assert_eq!(
            custody.spawn(&mut command).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(custody.spawn(&mut Command::new("/bin/true")).is_err());
        custody
            .retire(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert_eq!(roster_size(root.path()), 0);
        assert!(
            !path.exists(),
            "confirmed retirement must reclaim the cgroup"
        );
        assert_eq!(retired_roster_size(root.path()), 0);
        assert!(pool.reserve(bytes).is_err());
        drop(custody);
        assert!(!path.exists());
        assert!(pool.reserve(bytes).is_ok());
        reopen_after_reaper(root.path());
    }
    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly serially"]
    fn native_failed_registration_keeps_lease_group_and_capacity_until_durable_clear() {
        let root = fence_fixture();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let group = owned_group(bytes);
        let path = group.path().to_path_buf();
        block_ledger(root.path());
        assert!(
            WorkerCustody::prepare(group, Arc::new(pool.reserve(bytes).unwrap()), Some(fence))
                .is_err()
        );
        assert!(pool.reserve(bytes).is_err());
        assert!(path.exists());
        assert!(WorkerFence::open(root.path()).is_err());
        restore_ledger(root.path());
        wait_for_capacity(&pool, bytes);
        assert_eq!(roster_size(root.path()), 0);
        assert!(!path.exists());
        reopen_after_reaper(root.path());
    }
    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly serially"]
    fn native_exited_tree_keeps_custody_until_reaper_can_clear_durable_roster() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        use std::process::Stdio;
        let root = fence_fixture();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let group = owned_group(bytes);
        let path = group.path().to_path_buf();
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut custody =
            WorkerCustody::prepare(group, Arc::new(pool.reserve(bytes).unwrap()), Some(fence))
                .unwrap();
        custody.spawn(&mut command).unwrap();
        assert_eq!(roster_size(root.path()), 1);
        assert!(!custody.tree().group.is_empty().unwrap());
        block_ledger(root.path());
        assert!(custody
            .retire(Instant::now() + Duration::from_secs(3))
            .is_err());
        assert!(custody.child().try_wait().unwrap().is_some());
        assert!(custody.tree().group.is_empty().unwrap());
        drop(custody);
        assert!(pool.reserve(bytes).is_err());
        assert!(path.exists());
        assert!(WorkerFence::open(root.path()).is_err());
        restore_ledger(root.path());
        wait_for_capacity(&pool, bytes);
        assert_eq!(roster_size(root.path()), 0);
        assert!(!path.exists());
        reopen_after_reaper(root.path());
    }
    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly serially"]
    fn native_pre_exec_failure_retires_registered_forked_child() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let root = fence_fixture();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        let bytes = 32 * 1024 * 1024;
        let pool = ResourcePool::new(bytes, 1);
        let group = owned_group(bytes);
        let path = group.path().to_path_buf();
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/true");
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(io::Error::last_os_error());
                }
                Err(io::Error::from_raw_os_error(libc::EACCES))
            });
        }
        let mut custody =
            WorkerCustody::prepare(group, Arc::new(pool.reserve(bytes).unwrap()), Some(fence))
                .unwrap();
        assert_eq!(roster_size(root.path()), 1);
        assert_eq!(
            custody.spawn(&mut command).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        custody
            .retire(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert!(custody.tree().group.is_empty().unwrap());
        assert_eq!(roster_size(root.path()), 0);
        assert_eq!(retired_roster_size(root.path()), 0);
        assert!(
            !path.exists(),
            "confirmed retirement must reclaim the cgroup"
        );
        assert!(pool.reserve(bytes).is_err());
        drop(custody);
        assert!(!path.exists());
        assert!(pool.reserve(bytes).is_ok());
        reopen_after_reaper(root.path());
    }
}
