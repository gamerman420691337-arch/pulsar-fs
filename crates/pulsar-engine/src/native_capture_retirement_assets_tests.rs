//! Authored custody-order tests. Default tests create no process/cgroup and load
//! no native code. Ignored tests exercise owned cgroups and thread retirement only.
use super::*;
use crate::staging_obligation::{
    creation::{Admitted, Journal},
    Record, Staging,
};
use sha2::Digest;
use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

fn private_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

struct Marker(Arc<AtomicBool>);
impl Drop for Marker {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn assets() -> (Arc<AtomicBool>, CaptureRetirementAssets) {
    let released = Arc::new(AtomicBool::new(false));
    let marker: Arc<dyn std::any::Any + Send + Sync> = Arc::new(Marker(released.clone()));
    (released, CaptureRetirementAssets::new(vec![marker]))
}
fn wait(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            Instant::now() < end,
            "retirement test did not reach its acknowledged boundary"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
struct ControlledJournal {
    stores: AtomicUsize,
    fail_store: usize,
    clear_seen: AtomicBool,
    allow_clear: AtomicBool,
    current: Mutex<Option<Record>>,
}
impl Journal for ControlledJournal {
    fn store(&self, record: &Record) -> Result<()> {
        *self.current.lock().unwrap() = Some(record.clone());
        let call = self.stores.fetch_add(1, Ordering::AcqRel) + 1;
        ensure!(
            call != self.fail_store,
            "authored lost creation acknowledgement"
        );
        Ok(())
    }
    fn clear(&self, record: &Record) -> Result<()> {
        ensure!(
            self.current
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(|current| current == record),
            "wrong authored staging obligation"
        );
        self.clear_seen.store(true, Ordering::Release);
        ensure!(
            self.allow_clear.load(Ordering::Acquire),
            "authored clear acknowledgement withheld"
        );
        *self.current.lock().unwrap() = None;
        Ok(())
    }
}
fn journal(fail_store: usize) -> Arc<ControlledJournal> {
    Arc::new(ControlledJournal {
        stores: AtomicUsize::new(0),
        fail_store,
        clear_seen: AtomicBool::new(false),
        allow_clear: AtomicBool::new(false),
        current: Mutex::new(None),
    })
}
struct ReleaseOnDrop {
    journal: Arc<ControlledJournal>,
    released: Arc<AtomicBool>,
}
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.journal.allow_clear.store(true, Ordering::Release);
        let end = Instant::now() + Duration::from_secs(5);
        while !self.released.load(Ordering::Acquire) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
fn staged(
    parent: &Path,
    bridge: &CaptureRetirementAssets,
    journal: Arc<ControlledJournal>,
    claim: Arc<Reservation>,
) -> Staging {
    Staging::Durable(Admitted::create(parent, (claim.clone(), bridge.clone()), journal).unwrap())
        .retaining((claim, bridge.clone()))
}

#[test]
fn failed_intent_or_bound_creation_keeps_per_use_assets_until_clear_acknowledgement() {
    for fail_store in [1, 2] {
        let parent = private_tempdir();
        let pool = ResourcePool::new(1024, 1);
        let reservation = Arc::new(
            pool.reserve(1024)
                .unwrap()
                .with_storage(4096, 32 << 20)
                .unwrap(),
        );
        let (released, bridge) = assets();
        let journal = journal(fail_store);
        let _release = ReleaseOnDrop {
            journal: journal.clone(),
            released: released.clone(),
        };
        assert!(Admitted::create(
            parent.path(),
            (reservation.clone(), bridge.clone()),
            journal.clone()
        )
        .is_err());
        drop(reservation);
        drop(bridge);
        wait(|| journal.clear_seen.load(Ordering::Acquire));
        assert!(!released.load(Ordering::Acquire));
        assert_eq!(pool.reserved_storage(), 4096);
        assert!(pool.reserve(1024).is_err());
        journal.allow_clear.store(true, Ordering::Release);
        wait(|| released.load(Ordering::Acquire));
        assert_eq!(pool.reserved_storage(), 0);
        assert!(pool.reserve(1024).is_ok());
        assert!(journal.current.lock().unwrap().is_none());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }
}

#[test]
fn preparation_failure_keeps_guard_and_worker_claim_until_staging_cleanup_acknowledges() {
    let parent = private_tempdir();
    let pool = ResourcePool::new(1024, 1);
    let reservation = Arc::new(
        pool.reserve(1024)
            .unwrap()
            .with_storage(4096, 32 << 20)
            .unwrap(),
    );
    let (released, bridge) = assets();
    let journal = journal(usize::MAX);
    let _release = ReleaseOnDrop {
        journal: journal.clone(),
        released: released.clone(),
    };
    let staging = staged(parent.path(), &bridge, journal.clone(), reservation.clone());
    drop(Preparation {
        staging: Some(staging),
        worker: Some(reservation),
        retirement_assets: bridge.for_worker(),
    });
    drop(bridge);
    wait(|| journal.clear_seen.load(Ordering::Acquire));
    assert!(!released.load(Ordering::Acquire));
    assert_eq!(pool.reserved_storage(), 4096);
    assert!(pool.reserve(1024).is_err());
    journal.allow_clear.store(true, Ordering::Release);
    wait(|| released.load(Ordering::Acquire));
    assert_eq!(pool.reserved_storage(), 0);
    assert!(pool.reserve(1024).is_ok());
}

#[test]
fn failed_group_admission_retains_real_creation_custody_assets_through_staging_cleanup() {
    let parent = private_tempdir();
    let fence =
        Arc::new(crate::worker_fence::WorkerFence::open(&parent.path().join("fence")).unwrap());
    let stage_parent = parent.path().join("staging");
    artifacts::private_directory(&stage_parent).unwrap();
    let pool = ResourcePool::new(1024, 1);
    let reservation = Arc::new(
        pool.reserve(1024)
            .unwrap()
            .with_storage(4096, 32 << 20)
            .unwrap(),
    );
    let (released, bridge) = assets();
    let journal = journal(usize::MAX);
    let _release = ReleaseOnDrop {
        journal: journal.clone(),
        released: released.clone(),
    };
    let staging = staged(&stage_parent, &bridge, journal.clone(), reservation.clone());
    // The actual creation owner installs assets before rejecting this oversized
    // worker claim. No delegated cgroup lookup or process launch is reached.
    let error = fence
        .admit_staged_group_with_assets(reservation, 2048, 1, staging, bridge.for_worker())
        .err()
        .expect("oversized worker claim must fail");
    assert!(error.to_string().contains("exceeds admitted worker claim"));
    drop(bridge);
    wait(|| journal.clear_seen.load(Ordering::Acquire));
    assert!(!released.load(Ordering::Acquire));
    assert!(pool.reserve(1024).is_err());
    journal.allow_clear.store(true, Ordering::Release);
    wait(|| released.load(Ordering::Acquire));
    assert_eq!(pool.reserved_storage(), 0);
    assert!(pool.reserve(1024).is_ok());
}

#[test]
fn poisoned_bridge_preserves_the_original_assets_until_an_explicit_handoff() {
    let (released, bridge) = assets();
    let poisoned = bridge.clone();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _lock = poisoned.held.lock().unwrap();
        panic!("authored bridge poison before transfer");
    }))
    .is_err());
    drop(poisoned);
    let worker_assets = bridge.for_worker();
    assert_eq!(worker_assets.len(), 1);
    bridge.release_after_admission();
    assert!(!released.load(Ordering::Acquire));
    drop(worker_assets);
    assert!(released.load(Ordering::Acquire));
}

fn authored_capture(parent: &Path) -> (CapturedNativeDependencies, NativeDependencySpec) {
    let bytes = b"authored native-image bytes, never executed";
    let input = parent.join("authored-supplier");
    fs::write(&input, bytes).unwrap();
    let spec = NativeDependencySpec {
        schema_version: 1,
        libraries: vec![NativeLibrarySpec {
            path: input,
            sha256: format!("{:x}", sha2::Sha256::digest(bytes)),
            byte_len: bytes.len() as u64,
            names: vec!["authored.so".into()],
        }],
    };
    (
        CapturedNativeDependencies::capture(&spec, parent).unwrap(),
        spec,
    )
}
fn authored_image(
    captured: CapturedNativeDependencies,
    staging: Staging,
) -> SupervisedNativeDependencies {
    SupervisedNativeDependencies::new(
        captured,
        staging,
        NativeCaptureCompletion {
            root_name: "authored-image".into(),
            declared_bytes: 1,
            request_sha256: "0".repeat(64),
        },
    )
}

#[test]
fn emptied_staging_bridge_does_not_pin_per_use_assets_in_shared_weak_cache_image() {
    let parent = private_tempdir();
    let staging = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(parent.path())
        .unwrap();
    let path = staging.path().to_owned();
    let (captured, spec) = authored_capture(staging.path());
    let (released, bridge) = assets();
    // This unit test isolates the bridge/cache transfer. Real WorkerCustody
    // last-drop/transport behavior is exercised by the ignored tests below.
    let worker_assets = bridge.for_worker();
    let staging = Staging::Legacy(staging).retaining(bridge.clone());
    bridge.release_after_admission();
    let image = authored_image(captured, staging);
    let cache = LiveNativeImageReuse::default();
    cache.remember(&image).unwrap();
    let reused = cache
        .reuse(
            &spec,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap()
        .unwrap();
    assert!(!released.load(Ordering::Acquire));
    drop(worker_assets);
    assert!(
        released.load(Ordering::Acquire),
        "shared image retained the per-use guard"
    );
    assert!(path.exists());
    image.close().unwrap();
    assert!(path.exists());
    reused.close().unwrap();
    assert!(!path.exists());
    assert!(cache
        .reuse(
            &spec,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1)
        )
        .unwrap()
        .is_none());
}

#[test]
fn expired_or_cancelled_capture_drops_assets_without_creating_native_work() {
    let parent = private_tempdir();
    let fence =
        Arc::new(crate::worker_fence::WorkerFence::open(&parent.path().join("state")).unwrap());
    let config = EngineConfig::new(
        parent.path().join("state"),
        parent.path().join("never-launched-worker"),
    );
    let pool = ResourcePool::new(1024, 1);
    for cancelled in [false, true] {
        let (released, bridge) = assets();
        let assets = bridge.for_worker();
        drop(bridge);
        let result = capture_tracked_in_lease_with_assets(
            &config,
            &pool,
            &NativeDependencySpec {
                schema_version: 1,
                libraries: vec![],
            },
            CaptureLimits {
                budget: ResourceBudget {
                    memory_bytes: 1024,
                    output_bytes: 1024,
                    wall_time_ms: 1000,
                    cpu_threads: 1,
                },
                address_space_bytes: 4096,
            },
            &AtomicBool::new(cancelled),
            fence.clone(),
            CaptureLease {
                deadline: if cancelled {
                    Instant::now() + Duration::from_secs(1)
                } else {
                    Instant::now() - Duration::from_secs(1)
                },
                retirement_assets: assets,
            },
        );
        assert!(result.is_err());
        assert!(released.load(Ordering::Acquire));
        assert!(!config.state_dir.join("projects.sqlite3").exists());
        assert_eq!(pool.reserved_storage(), 0);
    }
}

struct AdmittedTestWorker {
    custody: crate::worker_retirement::WorkerCustody,
    fence: Arc<crate::worker_fence::WorkerFence>,
    pool: ResourcePool,
    path: PathBuf,
    released: Arc<AtomicBool>,
}
fn admitted_empty_worker(parent: &Path) -> AdmittedTestWorker {
    let fence = Arc::new(crate::worker_fence::WorkerFence::open(&parent.join("state")).unwrap());
    fence.enable_creation().unwrap();
    let bytes = 32 << 20;
    let pool = ResourcePool::new(bytes, 1);
    let mut reservation = pool
        .reserve(bytes)
        .unwrap()
        .with_storage(4096, bytes)
        .unwrap();
    let storage = Arc::new(reservation.split_storage().unwrap());
    let (released, bridge) = assets();
    let staging = Staging::Durable(
        fence
            .create_staging((storage.clone(), bridge.clone()))
            .unwrap(),
    )
    .retaining((storage, bridge.clone()));
    let path = staging.path();
    let custody = fence
        .admit_staged_group_with_assets(
            Arc::new(reservation),
            bytes,
            1,
            staging,
            bridge.for_worker(),
        )
        .unwrap()
        .into_worker();
    bridge.release_after_admission();
    AdmittedTestWorker {
        custody,
        fence,
        pool,
        path,
        released,
    }
}

#[test]
#[ignore = "requires an owned writable delegated cgroup; no child process/native code is launched"]
fn capture_retirement_assets_wait_for_actual_transport_reaper_and_storage_cleanup() {
    let parent = private_tempdir();
    let AdmittedTestWorker {
        mut custody,
        fence,
        pool,
        path,
        released,
    } = admitted_empty_worker(parent.path());
    fs::write(path.join("authored"), b"retained bytes").unwrap();
    let (send, receive) = std::sync::mpsc::channel::<()>();
    custody.retain_thread(std::thread::spawn(move || {
        let _ = receive.recv();
    }));
    // Dropping the sender is also the failure cleanup path if an assertion panics.
    drop(custody);
    assert!(!released.load(Ordering::Acquire));
    assert!(path.exists());
    assert_eq!(pool.reserved_storage(), 4096);
    drop(send);
    wait(|| released.load(Ordering::Acquire));
    assert!(!path.exists());
    assert_eq!(pool.reserved_storage(), 0);
    assert!(pool.reserve(32 << 20).is_ok());
    assert!(!fence.has_pending_worker_custody().unwrap());
}

#[test]
#[ignore = "requires an owned writable delegated cgroup; no child process/native code is launched"]
fn actual_capture_staging_handoff_detaches_assets_before_shared_image_reuse() {
    let parent = private_tempdir();
    let AdmittedTestWorker {
        custody,
        fence,
        pool,
        path,
        released,
    } = admitted_empty_worker(parent.path());
    let (captured, spec) = authored_capture(&path);
    let staging = custody
        .retire_into_staging(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert!(released.load(Ordering::Acquire));
    assert_eq!(pool.reserved_storage(), 4096);
    assert!(pool.reserve(32 << 20).is_ok());
    let image = authored_image(captured, staging);
    let cache = LiveNativeImageReuse::default();
    cache.remember(&image).unwrap();
    let reused = cache
        .reuse(
            &spec,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap()
        .unwrap();
    assert!(released.load(Ordering::Acquire));
    image.close().unwrap();
    assert!(path.exists());
    reused.close().unwrap();
    assert!(!path.exists());
    assert_eq!(pool.reserved_storage(), 0);
    assert!(!fence.has_pending_worker_custody().unwrap());
}
