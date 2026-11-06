use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}
fn owner(root: &Path) -> WorkerFence {
    let fence = WorkerFence::open(root).unwrap();
    fence.enable_creation().unwrap();
    fence
}
fn group(path: &Path) -> AttemptCgroup {
    let name = path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("pulsar-worker-")
        .unwrap();
    let group = AttemptCgroup::create(name, 32 * 1024 * 1024, 1).unwrap();
    assert_eq!(group.path(), path);
    group
}
fn fake_ledger() -> Ledger {
    let root = fixture();
    let fence = WorkerFence::open(root.path()).unwrap();
    fence.enable_staging().unwrap();
    let mut ledger = fence.ledger.lock().unwrap().clone();
    ledger.version = 4;
    ledger.creation = Some(Roster {
        parent: Group {
            path: "/sys/fs/cgroup/authored-parent".into(),
            device: 1,
            inode: 2,
        },
        boot: uuid::Uuid::new_v4().to_string(),
        records: BTreeMap::new(),
    });
    ledger
}
#[test]
fn format4_requires_creation_and_staging_rosters_without_weakening_legacy_bytes() {
    let ledger = fake_ledger();
    validate(&ledger).unwrap();
    for field in ["parent", "boot", "records"] {
        let mut malformed = serde_json::to_value(&ledger).unwrap();
        malformed["creation"].as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Ledger>(malformed).is_err(),
            "{field}"
        );
    }
    for missing in ["creation", "staging"] {
        let mut broken = ledger.clone();
        if missing == "creation" {
            broken.creation = None;
        } else {
            broken.staging = None;
        }
        assert!(validate(&broken).is_err(), "{missing}");
    }
    let mut broken = ledger.clone();
    broken.version = 3;
    assert!(validate(&broken).is_err());
    let root = fixture();
    let fence = WorkerFence::open(root.path()).unwrap();
    fence.enable_staging().unwrap();
    let bytes = fs::read(root.path().join("worker-fence.json")).unwrap();
    let decoded: Ledger = serde_json::from_slice(&bytes).unwrap();
    assert!(decoded.creation.is_none());
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    assert!(!String::from_utf8(bytes).unwrap().contains("creation"));
}
#[test]
fn format4_creation_shape_rejects_overlap_scope_escape_and_changed_binding() {
    let mut ledger = fake_ledger();
    let name = format!("{}{}", prefix(&ledger.scope), uuid::Uuid::new_v4());
    let group = Group {
        path: ledger.creation.as_ref().unwrap().parent.path.join(&name),
        device: 1,
        inode: 3,
    };
    ledger
        .creation
        .as_mut()
        .unwrap()
        .records
        .insert(name.clone(), Phase::Bound(group.clone()));
    validate(&ledger).unwrap();
    let mut changed = ledger.clone();
    changed.groups.insert(name.clone(), group.clone());
    assert!(validate(&changed).is_err());
    let mut changed = ledger.clone();
    changed
        .creation
        .as_mut()
        .unwrap()
        .records
        .insert("outside-scope".into(), Phase::Intent);
    assert!(validate(&changed).is_err());
    let mut changed = ledger.clone();
    let mut other = group;
    other.path = "/sys/fs/cgroup/other".into();
    changed
        .creation
        .as_mut()
        .unwrap()
        .records
        .insert(name, Phase::Bound(other));
    assert!(validate(&changed).is_err());
    let mut changed = ledger.clone();
    changed.creation.as_mut().unwrap().boot = "not-a-boot".into();
    assert!(validate(&changed).is_err());
    let mut changed = ledger;
    changed.creation.as_mut().unwrap().parent.inode = 0;
    assert!(validate(&changed).is_err());
}
#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_creation_primitive_transfers_and_retires_only_its_scope() {
    let root = fixture();
    let fence = owner(root.path());
    let scope = fence.scope().unwrap();
    let foreign = AttemptCgroup::create(
        &format!("{}-{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()),
        32 * 1024 * 1024,
        1,
    )
    .unwrap();
    let foreign_identity = group_identity(foreign.path()).unwrap();
    let path = fence.declare_creation().unwrap();
    let created = group(&path);
    assert!(
        fence.register(&created).is_err(),
        "registration bypassed creation binding"
    );
    assert!(fence.register_creation(&created).is_err());
    fence.bind_creation(&created).unwrap();
    fence.bind_creation(&created).unwrap();
    fence.register_creation(&created).unwrap();
    fence.register(&created).unwrap();
    assert!(fence.bind_creation(&created).is_err());
    fence.retire_creation(&created).unwrap();
    assert!(!path.exists());
    let pending = fence.declare_creation().unwrap();
    assert!(!pending.exists());
    drop(fence);
    let reopened = WorkerFence::open_bound_required(root.path(), Some(&scope), 4).unwrap();
    let ledger = reopened.ledger.lock().unwrap();
    assert_eq!(ledger.version, 4);
    assert!(ledger.creation.as_ref().unwrap().records.is_empty());
    assert!(ledger.groups.is_empty() && ledger.retired.is_empty());
    assert_eq!(group_identity(foreign.path()).unwrap(), foreign_identity);
}
#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_creation_recovery_preflights_all_records_before_cleanup() {
    for case in [
        "existing-intent",
        "missing-bound",
        "replaced-bound",
        "changed-boot",
        "changed-parent",
        "missing-roster",
        "unrecorded",
    ] {
        let root = fixture();
        let fence = owner(root.path());
        let good_path = fence.declare_creation().unwrap();
        let good = group(&good_path);
        fence.bind_creation(&good).unwrap();
        let bad_path = fence.declare_creation().unwrap();
        let bad = group(&bad_path);
        let mut replacement = None;
        if case != "existing-intent" {
            fence.bind_creation(&bad).unwrap();
        }
        match case {
            "missing-bound" => bad.reclaim().unwrap(),
            "replaced-bound" => {
                bad.reclaim().unwrap();
                replacement = Some(group(&bad_path));
            }
            "changed-boot" => {
                let mut ledger = fence.ledger.lock().unwrap();
                ledger.creation.as_mut().unwrap().boot = uuid::Uuid::new_v4().to_string();
                save(&fence.directory, &ledger).unwrap();
            }
            "changed-parent" => {
                let mut ledger = fence.ledger.lock().unwrap();
                ledger.creation.as_mut().unwrap().parent.inode += 1;
                save(&fence.directory, &ledger).unwrap();
            }
            "missing-roster" => {
                let mut ledger = fence.ledger.lock().unwrap().clone();
                ledger.creation = None;
                fs::write(
                    fence.directory.join("worker-fence.json"),
                    serde_json::to_vec(&ledger).unwrap(),
                )
                .unwrap();
            }
            "unrecorded" => {
                let mut ledger = fence.ledger.lock().unwrap();
                ledger
                    .creation
                    .as_mut()
                    .unwrap()
                    .records
                    .remove(bad_path.file_name().unwrap().to_str().unwrap());
                save(&fence.directory, &ledger).unwrap();
            }
            _ => (),
        }
        let bytes = fs::read(root.path().join("worker-fence.json")).unwrap();
        drop(fence);
        assert!(WorkerFence::open(root.path()).is_err(), "{case}");
        assert_eq!(
            fs::read(root.path().join("worker-fence.json")).unwrap(),
            bytes,
            "{case}"
        );
        assert!(
            good_path.exists(),
            "{case} cleaned a good record before complete preflight"
        );
        if case != "missing-bound" {
            assert!(bad_path.exists(), "{case}");
        }
        drop(replacement);
    }
}
#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_creation_capacity_refuses_without_poisoning_attempted_ledger() {
    let root = fixture();
    let fence = owner(root.path());
    {
        let mut ledger = fence.ledger.lock().unwrap();
        let scope = ledger.scope.clone();
        for _ in 0..4096 {
            ledger.creation.as_mut().unwrap().records.insert(
                format!("{}{}", prefix(&scope), uuid::Uuid::new_v4()),
                Phase::Intent,
            );
        }
        save(&fence.directory, &ledger).unwrap();
    }
    let before = fs::read(root.path().join("worker-fence.json")).unwrap();
    assert!(fence.declare_creation().is_err());
    assert_eq!(
        serde_json::to_vec(&*fence.ledger.lock().unwrap()).unwrap(),
        before
    );
    assert_eq!(
        fs::read(root.path().join("worker-fence.json")).unwrap(),
        before
    );
}
#[test]
#[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
fn native_creation_failed_publication_retains_attempted_identity_until_retirement() {
    let root = fixture();
    let fence = owner(root.path());
    let path = fence.declare_creation().unwrap();
    let created = group(&path);
    let before = fs::read(root.path().join("worker-fence.json")).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let bound = fence.bind_creation(&created);
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(bound.is_err());
    assert_eq!(
        fs::read(root.path().join("worker-fence.json")).unwrap(),
        before
    );
    assert!(matches!(
        fence
            .ledger
            .lock()
            .unwrap()
            .creation
            .as_ref()
            .unwrap()
            .records
            .values()
            .next(),
        Some(Phase::Bound(_))
    ));
    fence.retire_creation(&created).unwrap();
    assert!(!path.exists());
    assert!(fence
        .ledger
        .lock()
        .unwrap()
        .creation
        .as_ref()
        .unwrap()
        .records
        .is_empty());
}
#[test]
#[ignore = "requires owned writable delegated cgroups and an owned child; run explicitly serially"]
fn native_creation_populated_record_refuses_recovery_without_signalling() {
    use std::os::unix::process::CommandExt;
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = fixture();
    let fence = owner(root.path());
    let path = fence.declare_creation().unwrap();
    let created = group(&path);
    fence.bind_creation(&created).unwrap();
    let fd = created.procs.as_raw_fd();
    let mut cmd = std::process::Command::new("/bin/sleep");
    cmd.arg("30");
    unsafe {
        cmd.pre_exec(move || {
            if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = Child(cmd.spawn().unwrap());
    assert!(!created.is_empty().unwrap());
    let before = fs::read(root.path().join("worker-fence.json")).unwrap();
    drop(fence);
    assert!(WorkerFence::open(root.path()).is_err());
    assert!(child.0.try_wait().unwrap().is_none());
    assert_eq!(
        fs::read(root.path().join("worker-fence.json")).unwrap(),
        before
    );
    created.kill();
    child.0.wait().unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while !created.is_empty().unwrap() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    WorkerFence::open(root.path()).unwrap();
    assert!(!path.exists());
}
#[test]
#[ignore = "actual owned-process termination and delegated cgroups; run explicitly serially"]
fn native_creation_owner_death_cuts_reconcile_or_refuse_exact_obligations() {
    crash(false);
}
#[test]
#[ignore = "unresolved legacy mkdir-before-registration counterexample; run explicitly serially"]
fn native_legacy_unregistered_creation_refuses_owner_death() {
    crash(true);
}
fn crash(legacy: bool) {
    const ROOT: &str = "PULSAR_CGROUP_CREATION_CRASH_ROOT";
    const CUT: &str = "PULSAR_CGROUP_CREATION_CRASH_CUT";
    const CASE:&str="worker_fence::creation::tests::native_creation_owner_death_cuts_reconcile_or_refuse_exact_obligations";
    const LEGACY: &str =
        "worker_fence::creation::tests::native_legacy_unregistered_creation_refuses_owner_death";
    if let Some(root) = std::env::var_os(ROOT) {
        let root = Path::new(&root);
        let cut = std::env::var(CUT).unwrap();
        let fence = WorkerFence::open(root).unwrap();
        if legacy {
            fence.enable_staging().unwrap();
        } else {
            fence.enable_creation().unwrap();
        }
        let path = if legacy {
            crate::cgroup::delegated_root().unwrap().join(format!(
                "{}{}",
                prefix(&fence.scope().unwrap()),
                uuid::Uuid::new_v4()
            ))
        } else {
            fence.declare_creation().unwrap()
        };
        let created = if cut == "intent-absent" {
            None
        } else {
            Some(group(&path))
        };
        if matches!(cut.as_str(), "bound" | "retired-missing" | "registered") {
            fence.bind_creation(created.as_ref().unwrap()).unwrap();
        }
        if cut == "registered" {
            fence.register_creation(created.as_ref().unwrap()).unwrap();
        }
        if cut == "retired-missing" {
            let mut ledger = fence.ledger.lock().unwrap();
            let name = path.file_name().unwrap().to_str().unwrap();
            let phase = ledger
                .creation
                .as_ref()
                .unwrap()
                .records
                .get(name)
                .unwrap()
                .clone();
            let Phase::Bound(group) = phase else {
                panic!("bound cut")
            };
            ledger
                .creation
                .as_mut()
                .unwrap()
                .records
                .insert(name.into(), Phase::Retired(group));
            save(root, &ledger).unwrap();
            created.as_ref().unwrap().reclaim().unwrap();
        }
        let identity = created.as_ref().map(|g| g.identity().unwrap());
        fs::write(root.join("ready.json"),serde_json::to_vec(&serde_json::json!({"path":path,"identity":identity,"scope":fence.scope().unwrap()})).unwrap()).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        panic!("owner was not terminated at its acknowledged cut");
    }
    struct Process(std::process::Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Cleanup(Option<(PathBuf, u64, u64)>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some((path, device, inode)) = &self.0 {
                let _ = crate::cgroup::reclaim_recorded(path, *device, *inode, true);
            }
        }
    }
    let cuts: &[&str] = if legacy {
        &["legacy"]
    } else {
        &[
            "intent-absent",
            "intent-existing",
            "bound",
            "retired-missing",
            "registered",
        ]
    };
    for cut in cuts {
        let root = fixture();
        let mut child = Process(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    if legacy { LEGACY } else { CASE },
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(ROOT, root.path())
                .env(CUT, cut)
                .stdin(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let end = Instant::now() + Duration::from_secs(10);
        let ready: serde_json::Value = loop {
            if let Ok(bytes) = fs::read(root.path().join("ready.json")) {
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    break value;
                }
            }
            assert!(child.0.try_wait().unwrap().is_none());
            assert!(Instant::now() < end, "owner readiness deadline");
            std::thread::sleep(Duration::from_millis(10));
        };
        let path = PathBuf::from(ready["path"].as_str().unwrap());
        let identity = if ready["identity"].is_array() {
            Some((
                ready["identity"][0].as_u64().unwrap(),
                ready["identity"][1].as_u64().unwrap(),
            ))
        } else {
            None
        };
        let _cleanup = Cleanup(identity.map(|(d, i)| (path.clone(), d, i)));
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        if path.exists() {
            let observed = group_identity(&path).unwrap();
            assert_eq!(Some((observed.device, observed.inode)), identity);
            assert!(empty(&observed).unwrap());
        }
        let before = fs::read(root.path().join("worker-fence.json")).unwrap();
        {
            use sha2::{Digest, Sha256};
            eprintln!(
                "creation cut evidence: {}",
                serde_json::json!({"cut":cut,"ready":ready,"ledger_before_sha256":format!("{:x}",Sha256::digest(&before))})
            );
        }
        let recovered = WorkerFence::open(root.path());
        eprintln!(
            "creation cut observation: {}",
            serde_json::json!({"cut":cut,"refused":recovered.is_err(),"path_exists":path.exists()})
        );
        if legacy || *cut == "intent-existing" {
            assert!(
                recovered.is_err(),
                "{cut}: restart forgot the unbound created cgroup"
            );
            assert!(path.exists());
            assert_eq!(
                fs::read(root.path().join("worker-fence.json")).unwrap(),
                before
            );
            let (device, inode) = identity.unwrap();
            crate::cgroup::reclaim_recorded(&path, device, inode, false).unwrap();
            let recovered = WorkerFence::open(root.path()).unwrap();
            let ledger = recovered.ledger.lock().unwrap();
            assert!(ledger
                .creation
                .as_ref()
                .is_none_or(|roster| roster.records.is_empty()));
            assert!(ledger.groups.is_empty() && ledger.retired.is_empty());
        } else {
            let recovered = recovered.unwrap();
            assert_eq!(recovered.scope().unwrap(), ready["scope"].as_str().unwrap());
            assert!(!path.exists());
            let ledger = recovered.ledger.lock().unwrap();
            assert!(
                ledger.groups.is_empty()
                    && ledger.retired.is_empty()
                    && ledger.creation.as_ref().unwrap().records.is_empty()
            );
        }
        eprintln!(
            "actual owner termination cut {cut}: checked recovery/refusal and exact group identity"
        );
    }
}
