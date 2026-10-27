use super::*;
use std::os::{
    fd::AsRawFd,
    unix::{
        fs::PermissionsExt,
        process::{CommandExt, ExitStatusExt},
    },
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CHILD_ROOT: &str = "PULSAR_STAGING_CRASH_TEST_ROOT";
const DURABLE_CASE: &str = "worker_retirement::staging_crash_tests::native_restart_reconciles_durable_staging_after_owner_kill";
const CASE: &str = "worker_retirement::staging_crash_tests::native_restart_refuses_unreconciled_staging_after_owner_kill";

// The process guard is dropped before the fixture recovery guard, including on
// assertion failure. Only the owned, unreaped direct child can be signaled.
struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Recovery(PathBuf);
impl Drop for Recovery {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(self.0.join("staging")) {
            for entry in entries.flatten() {
                let _ =
                    std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o700));
            }
        }
        if let Ok(entries) = std::fs::read_dir(self.0.join("fence")) {
            for entry in entries.flatten().filter(|entry| {
                entry
                    .file_name()
                    .as_encoded_bytes()
                    .starts_with(b"pulsar-stage-")
            }) {
                let _ =
                    std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o700));
            }
        }
        // Recovery is limited to this test's private scope and recorded groups.
        let _ = WorkerFence::open(&self.0.join("fence"));
    }
}

fn owner(root: &Path, durable: bool) {
    let fence = Arc::new(WorkerFence::open(&root.join("fence")).unwrap());
    let scope = fence.scope().unwrap();
    let bytes = 32 * 1024 * 1024;
    let pool = crate::resources::ResourcePool::new(bytes, 1);
    let reservation = Arc::new(
        pool.reserve(bytes)
            .unwrap()
            .with_storage(4096, bytes)
            .unwrap(),
    );
    let staging = if durable {
        fence.enable_staging().unwrap();
        crate::staging_obligation::Staging::Durable(
            fence.create_staging(reservation.clone()).unwrap(),
        )
    } else {
        crate::staging_obligation::Staging::Legacy(
            tempfile::tempdir_in(root.join("staging")).unwrap(),
        )
    };
    let staged = staging.path();
    std::fs::write(staged.join("admitted"), b"owned crash fixture bytes").unwrap();
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(
        std::fs::remove_dir_all(&staged).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    let group = AttemptCgroup::create(&format!("crash-staging-{}", uuid::Uuid::new_v4()), bytes, 1)
        .unwrap();
    let group_path = group.path().to_owned();
    let group_identity = group.identity().unwrap();
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
        WorkerCustody::prepare_staged(group, reservation, Some(fence.clone()), staging).unwrap();
    custody.spawn(&mut command).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while custody.child().try_wait().unwrap().is_none() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(custody.tree().group.is_empty().unwrap());
    assert!(custody
        .retire(Instant::now() + Duration::from_millis(100))
        .is_err());
    drop(custody);
    drop(fence);
    assert_eq!(pool.reserved_storage(), 4096);
    assert!(pool.reserve(bytes).is_err());
    assert!(WorkerFence::open_bound(&root.join("fence"), Some(&scope)).is_err());
    let receipt = serde_json::json!({"scope":scope,"staging":staged,"group":group_path,"group_identity":group_identity,"storage_held":4096});
    std::fs::write(
        root.join("ready.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    // Bounded failsafe if the parent disappears; this does not signal any peer.
    std::thread::sleep(Duration::from_secs(60));
    std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o700)).unwrap();
    panic!("parent did not perform the owned crash cut");
}

#[test]
#[ignore = "requires delegated cgroups; unresolved durable staging crash regression; run explicitly serially"]
fn native_restart_refuses_unreconciled_staging_after_owner_kill() {
    exercise_crash(CASE, false);
}

#[test]
#[ignore = "actual owned-process termination and delegated cgroups; run serially"]
fn native_restart_reconciles_durable_staging_after_owner_kill() {
    exercise_crash(DURABLE_CASE, true);
}
fn exercise_crash(case: &str, durable: bool) {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        owner(Path::new(&root), durable);
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["fence", "staging"] {
        let path = root.path().join(name);
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let _recovery = Recovery(root.path().to_owned());
    let mut process = Process(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                case,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_ROOT, root.path())
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let end = Instant::now() + Duration::from_secs(10);
    let receipt: serde_json::Value = loop {
        if let Ok(bytes) = std::fs::read(root.path().join("ready.json")) {
            if let Ok(receipt) = serde_json::from_slice(&bytes) {
                break receipt;
            }
        }
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "owner exited before crash readiness"
        );
        assert!(
            Instant::now() < end,
            "owner did not acknowledge retained cleanup custody"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let staged = PathBuf::from(receipt["staging"].as_str().unwrap());
    assert!(staged.starts_with(root.path().join(if durable { "fence" } else { "staging" })));
    let scope = receipt["scope"].as_str().unwrap();
    assert!(WorkerFence::open_bound(&root.path().join("fence"), Some(scope)).is_err());
    process.0.kill().unwrap();
    assert_eq!(process.0.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert_eq!(
        std::fs::read(staged.join("admitted")).unwrap(),
        b"owned crash fixture bytes"
    );
    let reopened = WorkerFence::open_bound(&root.path().join("fence"), Some(scope));
    eprintln!(
        "crash cut: SIGKILL confirmed; staging bytes preserved; restart admitted={}",
        reopened.is_ok()
    );
    assert!(reopened.is_err(), "restart admitted an ownership scope with unaccounted staging bytes after its owner was killed");
    if durable {
        let before = std::fs::read(root.path().join("fence/worker-fence.json")).unwrap();
        assert_eq!(
            std::fs::read(staged.join("admitted")).unwrap(),
            b"owned crash fixture bytes"
        );
        assert!(
            WorkerFence::open_bound_required(&root.path().join("fence"), Some(scope), 3).is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join("fence/worker-fence.json")).unwrap(),
            before
        );
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o700)).unwrap();
        let recovered =
            WorkerFence::open_bound_required(&root.path().join("fence"), Some(scope), 3).unwrap();
        assert_eq!(recovered.version().unwrap(), 3);
        assert!(!staged.exists());
        let ledger: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.path().join("fence/worker-fence.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(ledger["staging"], serde_json::json!({}));
        assert_eq!(ledger["groups"], serde_json::json!({}));
        assert_eq!(ledger["retired"], serde_json::json!({}));
        eprintln!("durable production primitive: failed deletion refused restart; restored exact root recovered and roster cleared");
    }
}
