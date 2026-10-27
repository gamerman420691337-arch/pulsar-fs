use super::*;
use crate::{resources::ResourcePool, staging_obligation::Staging};
use std::os::{fd::AsRawFd, unix::process::CommandExt};

#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_failed_image_handoff_retains_transport_bytes_claims_and_lease() {
    let root = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
    fence.enable_creation().unwrap();
    let memory = 32 * 1024 * 1024;
    let pool = ResourcePool::new(memory, 1);
    let mut worker = pool
        .reserve(memory)
        .unwrap()
        .with_storage(4096, 32 * 1024 * 1024)
        .unwrap();
    let storage = Arc::new(worker.split_storage().unwrap());
    let stage = Staging::Durable(fence.create_staging(storage.clone()).unwrap()).retaining(storage);
    let stage_path = stage.path();
    std::fs::write(
        stage_path.join("authored"),
        b"unretired transport must retain these bytes",
    )
    .unwrap();
    let mut custody = fence
        .admit_staged_group(Arc::new(worker), memory, 1, stage)
        .unwrap()
        .into_worker();
    let group = custody.group_handle();
    let path = group.path().to_owned();
    let fd = group.procs.as_raw_fd();
    let mut command = Command::new("/bin/true");
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
    custody.child().wait().unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    custody.retain_thread(std::thread::spawn(move || {
        let _ = wait.recv();
    }));
    drop(group);
    drop(fence);
    let error = custody
        .retire_into_staging(Instant::now() + Duration::from_millis(20))
        .err()
        .expect("unretired transport transferred image custody");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(pool.reserved_storage(), 4096);
    assert!(pool.reserve(memory).is_err());
    assert!(WorkerFence::open(root.path()).is_err());
    assert_eq!(
        std::fs::read(stage_path.join("authored")).unwrap(),
        b"unretired transport must retain these bytes"
    );
    assert!(path.exists());
    release.send(()).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if pool.reserve(memory).is_ok() && pool.reserved_storage() == 0 {
            break;
        }
        assert!(
            Instant::now() < end,
            "failed image handoff did not finish acknowledged cleanup"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!path.exists() && !stage_path.exists());
    let fence = WorkerFence::open(root.path()).unwrap();
    let bytes = std::fs::read(root.path().join("worker-fence.json")).unwrap();
    let ledger: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for field in ["groups", "retired", "staging"] {
        assert!(ledger[field].as_object().unwrap().is_empty());
    }
    assert!(ledger["creation"]["records"]
        .as_object()
        .unwrap()
        .is_empty());
    drop(fence);
}
