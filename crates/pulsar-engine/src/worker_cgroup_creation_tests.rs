use super::*;
use crate::resources::ResourcePool;
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

const MEMORY: u64 = 32 * 1024 * 1024;
#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_failed_creation_keeps_retirement_receipt_pending_until_exact_cleanup() {
    let fixture = Fixture::new();
    let (receipt, asset) = crate::live_inference::CpuWorkerRetirement::new();
    let result = Admitted::create_inner_with_assets(
        fixture.owner(),
        fixture.claim(),
        MEMORY,
        1,
        None,
        vec![asset],
        &mut |at, _| {
            if at == Step::Registered {
                fixture.permissions(0o500);
                anyhow::bail!("authored postregistration failure");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    fixture.held();
    assert!(
        !receipt.joined(),
        "failed publication cannot release native cleanup receipt"
    );
    fixture.permissions(0o700);
    receipt
        .wait(Instant::now() + Duration::from_secs(5))
        .unwrap();
    fixture.released();
}
struct Fixture {
    directory: tempfile::TempDir,
    pool: ResourcePool,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            directory,
            pool: ResourcePool::new(MEMORY, 1),
        }
    }
    fn owner(&self) -> Arc<WorkerFence> {
        let fence = Arc::new(WorkerFence::open(self.directory.path()).unwrap());
        fence.enable_creation().unwrap();
        fence
    }
    fn claim(&self) -> Arc<Reservation> {
        Arc::new(
            self.pool
                .reserve(MEMORY)
                .unwrap()
                .with_storage(4096, MEMORY)
                .unwrap(),
        )
    }
    fn permissions(&self, mode: u32) {
        fs::set_permissions(self.directory.path(), fs::Permissions::from_mode(mode)).unwrap();
    }
    fn held(&self) {
        // Several cleanup retries must leave both the claim and scope held.
        std::thread::sleep(Duration::from_millis(180));
        assert_eq!(self.pool.reserved_storage(), 4096);
        assert!(self.pool.reserve(MEMORY).is_err());
        assert!(WorkerFence::open(self.directory.path()).is_err());
    }
    fn released(&self) {
        let end = Instant::now() + Duration::from_secs(5);
        while self.pool.reserved_storage() != 0 {
            assert!(
                Instant::now() < end,
                "creation cleanup did not release admission"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(self.pool.reserve(MEMORY).is_ok());
        let fence = WorkerFence::open(self.directory.path()).unwrap();
        let ledger = fence.ledger.lock().unwrap();
        assert!(ledger.groups.is_empty() && ledger.retired.is_empty());
        assert!(ledger.creation.as_ref().unwrap().records.is_empty());
        assert!(ledger.staging.as_ref().unwrap().is_empty());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::set_permissions(self.directory.path(), fs::Permissions::from_mode(0o700));
    }
}

#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_admitted_creation_transfers_registered_owner_before_spawn() {
    use std::os::unix::process::CommandExt;
    let fixture = Fixture::new();
    let fence = fixture.owner();
    let claim = fixture.claim();
    let snapshots =
        crate::staging_obligation::Staging::Durable(fence.create_staging(claim.clone()).unwrap());
    let snapshot_path = snapshots.path();
    fs::write(snapshot_path.join("authored"), b"success custody bytes").unwrap();
    let admitted = fence
        .admit_staged_group(claim, MEMORY, 1, snapshots)
        .unwrap();
    drop(fence);
    let state = admitted.state.as_ref().unwrap();
    let path = state.ticket.as_ref().unwrap().path.clone();
    {
        let ledger = state.fence.ledger.lock().unwrap();
        assert_eq!(ledger.groups.len(), 1);
        assert!(ledger.creation.as_ref().unwrap().records.is_empty());
    }
    let mut custody = admitted.into_worker();
    let group = custody.group_handle();
    assert_eq!(
        fs::read_to_string(path.join("memory.max")).unwrap().trim(),
        MEMORY.to_string()
    );
    let fd = group.procs.as_raw_fd();
    let mut command = std::process::Command::new("/bin/true");
    unsafe {
        command.pre_exec(move || {
            if libc::write(fd, b"0".as_ptr().cast(), 1) == 1 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    custody.spawn(&mut command).unwrap();
    custody.child().wait().unwrap();
    custody
        .retire(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert!(!path.exists());
    assert!(!snapshot_path.exists());
    assert_eq!(fixture.pool.reserved_storage(), 4096);
    drop(group);
    drop(custody);
    fixture.released();
}

#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_admitted_creation_failed_publication_retains_claim_and_lease() {
    for step in [
        Step::Ticket,
        Step::Captured,
        Step::Configured,
        Step::Registered,
    ] {
        let fixture = Fixture::new();
        let mut path = None;
        let result = Admitted::create_inner(
            fixture.owner(),
            fixture.claim(),
            MEMORY,
            1,
            None,
            &mut |at, state| {
                if at == step {
                    path = Some(state.ticket.as_ref().unwrap().path.clone());
                    fixture.permissions(0o500);
                    if at == Step::Registered {
                        anyhow::bail!("injected post-registration failure");
                    }
                }
                Ok(())
            },
        );
        assert!(result.is_err(), "{step:?}");
        fixture.held();
        if step != Step::Ticket {
            assert!(path.as_ref().unwrap().exists(), "{step:?}");
        }
        fixture.permissions(0o700);
        fixture.released();
        assert!(!path.unwrap().exists(), "{step:?}");
    }
}

#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_admitted_creation_uncaptured_intent_never_guesses_deletion_identity() {
    let fixture = Fixture::new();
    let mut identity = None;
    let result = Admitted::create_inner(
        fixture.owner(),
        fixture.claim(),
        MEMORY,
        1,
        None,
        &mut |at, state| {
            if at == Step::Created {
                identity = Some(group_identity(&state.ticket.as_ref().unwrap().path)?);
                anyhow::bail!("injected identity capture failure");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    fixture.held();
    let identity = identity.unwrap();
    assert_eq!(group_identity(&identity.path).unwrap(), identity);
    // Fixture authority is its independent observation of its exact empty root.
    crate::cgroup::reclaim_recorded(&identity.path, identity.device, identity.inode, false)
        .unwrap();
    fixture.released();
}

#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_admitted_creation_failed_deletion_retains_retirement_until_retry() {
    let fixture = Fixture::new();
    let mut blocker = None;
    let result = Admitted::create_inner(
        fixture.owner(),
        fixture.claim(),
        MEMORY,
        1,
        None,
        &mut |at, state| {
            if at == Step::Bound {
                let path = state.ticket.as_ref().unwrap().path.join("owned-blocker");
                fs::create_dir(&path)?;
                blocker = Some(path);
                anyhow::bail!("injected configuration failure with deletion blocked");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    fixture.held();
    let ledger: Ledger = serde_json::from_slice(
        &fs::read(fixture.directory.path().join("worker-fence.json")).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        ledger.creation.unwrap().records.values().next(),
        Some(Phase::Retired(_))
    ));
    fs::remove_dir(blocker.unwrap()).unwrap();
    fixture.released();
}

#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_admitted_creation_replacement_preserved_after_held_root_unlinked() {
    let fixture = Fixture::new();
    let mut replacement = None;
    let mut path = None;
    let result = Admitted::create_inner(
        fixture.owner(),
        fixture.claim(),
        MEMORY,
        1,
        None,
        &mut |at, state| {
            if at == Step::Captured {
                let ticket = state.ticket.as_ref().unwrap();
                let held = state.root.as_ref().unwrap().metadata()?;
                crate::cgroup::reclaim_recorded(&ticket.path, held.dev(), held.ino(), false)?;
                let name = ticket.name.strip_prefix("pulsar-worker-").unwrap();
                let group = AttemptCgroup::create(name, MEMORY, 1)?;
                assert_ne!(group.identity()?, (held.dev(), held.ino()));
                path = Some(ticket.path.clone());
                replacement = Some(group);
                anyhow::bail!("injected replacement before binding");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    fixture.held();
    let replacement = replacement.unwrap();
    assert_eq!(replacement.identity().unwrap(), {
        let current = group_identity(path.as_ref().unwrap()).unwrap();
        (current.device, current.inode)
    });
    replacement.reclaim().unwrap();
    drop(replacement);
    fixture.released();
}

#[test]
fn admitted_creation_refuses_nonworker_or_insufficient_claim_before_intent() {
    for memory_only in [false, true] {
        let fixture = Fixture::new();
        let fence = Arc::new(WorkerFence::open(fixture.directory.path()).unwrap());
        let claim = if memory_only {
            fixture.pool.reserve_memory(MEMORY)
        } else {
            fixture.pool.reserve(MEMORY / 2)
        }
        .unwrap();
        let result = Admitted::create(fence.clone(), Arc::new(claim), MEMORY, 1);
        assert!(result.is_err());
        assert!(fence.ledger.lock().unwrap().creation.is_none());
        drop(fence);
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            if fixture.pool.reserve(MEMORY).is_ok() {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_admitted_creation_failure_keeps_staged_bytes_until_cleanup_acknowledged() {
    for step in [Step::Ticket, Step::Configured] {
        let fixture = Fixture::new();
        let fence = fixture.owner();
        let claim = fixture.claim();
        let snapshots = crate::staging_obligation::Staging::Durable(
            fence.create_staging(claim.clone()).unwrap(),
        );
        let snapshot_path = snapshots.path();
        fs::write(snapshot_path.join("authored"), b"unexposed owned bytes").unwrap();
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
            }
        }
        let restore = Restore(snapshot_path.clone());
        fs::set_permissions(&snapshot_path, fs::Permissions::from_mode(0o500)).unwrap();
        let mut group_path = None;
        let result = Admitted::create_inner(
            fence,
            claim,
            MEMORY,
            1,
            Some(snapshots),
            &mut |at, state| {
                if at == step {
                    group_path = Some(state.ticket.as_ref().unwrap().path.clone());
                    anyhow::bail!("injected before worker handoff");
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        fixture.held();
        assert!(!group_path.unwrap().exists());
        assert_eq!(
            fs::read(snapshot_path.join("authored")).unwrap(),
            b"unexposed owned bytes"
        );
        let ledger: Ledger = serde_json::from_slice(
            &fs::read(fixture.directory.path().join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        assert!(ledger.creation.as_ref().unwrap().records.is_empty());
        let record =
            serde_json::to_value(ledger.staging.as_ref().unwrap().values().next().unwrap())
                .unwrap();
        assert!(record["phase"].get("Retired").is_some());
        drop(restore);
        fixture.released();
        assert!(!snapshot_path.exists());
    }
}
