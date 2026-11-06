// Abrupt death through the actual Engine admission and creation callers.
mod admitted_engine_crash {
    use super::*;
    use std::os::unix::{fs::MetadataExt, process::ExitStatusExt};
    use std::time::{Duration, Instant};
    const ROOT: &str = "PULSAR_ADMITTED_ENGINE_CRASH_ROOT";
    const CUT: &str = "PULSAR_ADMITTED_ENGINE_CRASH_CUT";
    const MODE: &str = "PULSAR_ADMITTED_ENGINE_CRASH_MODE";
    const CASE: &str =
        "authority::tests::admitted_engine_crash::native_engine_creation_owner_death";

    fn config(root: &Path) -> EngineConfig {
        let mut config = EngineConfig::new(root.join("private-state"), root.join("worker"));
        config.max_jobs = 2;
        config
    }
    fn wait_json(path: &Path, mut alive: impl FnMut() -> bool) -> serde_json::Value {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(bytes) = fs::read(path) {
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    return value;
                }
            }
            assert!(alive(), "engine owner exited before acknowledging cut");
            assert!(
                Instant::now() < end,
                "engine creation cut deadline: {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn schema(root: &Path, scope: &str) {
        let db = Connection::open_with_flags(
            root.join("private-state/projects.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        let binding = crate::worker_fence_db::binding(&db, 20).unwrap().unwrap();
        assert_eq!(binding.scope, scope);
        assert_eq!(binding.minimum_ledger_version, 4);
    }
    fn child(root: &Path, mode: &str) -> ! {
        fs::write(
            root.join("worker"),
            b"nonexecuted engine creation crash worker",
        )
        .unwrap();
        let engine = Engine::open(config(root)).unwrap();
        let session = pair(&engine, "creation crash fixture");
        let project = match engine
            .handle(request(
                &session,
                None,
                None,
                Command::CreateProject {
                    name: "Creation crash".into(),
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(project) => project.project_id,
            _ => panic!("project response"),
        };
        if mode == "ordinary" {
            let response = engine.handle(request(&session, Some(&project), Some(0), Command::GenerateInput {
                input: GenerationInput::Text { prompt: "pattern=triangle axis=stroke duration=1s frequency=1hz amplitude=0.1 offset=0.5".into() },
            })).result.unwrap();
            assert!(matches!(response, ResponseBody::Job(_)));
        } else {
            assert_eq!(mode, "encoded");
            let mut options = capture_options();
            options.encoded_source = Some(LiveEncodedSourceOptions {
                format: LiveEncodedFormat::Yuv4mpegPipe,
                maximum_input_bytes: 1024,
                maximum_frames: 2,
            });
            let report = capture_report(
                engine
                    .handle(request(
                        &session,
                        Some(&project),
                        Some(0),
                        Command::StartLiveCapture {
                            engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                            options,
                        },
                    ))
                    .result
                    .unwrap(),
            );
            let handshake = LiveBulkHandshake {
                version: PROTOCOL_VERSION,
                channel: LiveBulkChannel::LiveEncodedSource,
                session: session.clone(),
                auth_token: TOKENS.with(|t| t.borrow()[session.as_str()].clone()),
                engine_epoch: report.engine_epoch,
                identity: report.identity,
            };
            let owner = engine.clone();
            std::thread::spawn(move || {
                let result = owner.encoded_source_open(&handshake);
                panic!(
                    "encoded creator returned before cut: {}",
                    result
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "unexpected success".into())
                );
            });
        }
        let ready = wait_json(&root.join("paused.json"), || true);
        schema(root, ready["scope"].as_str().unwrap());
        // One slot remains for interactive work. Occupy that slot briefly,
        // then prove the creator still owns the other one.
        let probe = engine.pool.reserve(1).unwrap();
        assert!(
            engine.pool.reserve(1).is_err(),
            "creator released its worker slot"
        );
        drop(probe);
        if mode == "encoded" {
            assert!(
                engine.pool.reserved_storage() > 0,
                "creator released staged storage"
            );
            assert!(ready["staging"].is_string());
        } else {
            assert!(ready["staging"].is_null());
        }
        fs::write(root.join("ready.tmp"), serde_json::to_vec(&ready).unwrap()).unwrap();
        fs::rename(root.join("ready.tmp"), root.join("ready.json")).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        panic!("owner not terminated at acknowledged engine cut");
    }
    struct Process(std::process::Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct GroupCleanup(Option<(PathBuf, u64, u64)>);
    impl Drop for GroupCleanup {
        fn drop(&mut self) {
            if let Some((path, device, inode)) = &self.0 {
                let _ = crate::cgroup::reclaim_recorded(path, *device, *inode, true);
            }
        }
    }
    fn protected(root: &Path, stage: Option<&Path>) -> Vec<(PathBuf, usize, String)> {
        let state = root.join("private-state");
        let mut paths = vec![
            state.join("projects.sqlite3"),
            state.join("projects.sqlite3-wal"),
            state.join("worker-fence.json"),
        ];
        if let Some(stage) = stage {
            for entry in fs::read_dir(stage).unwrap() {
                let path = entry.unwrap().path();
                assert!(fs::symlink_metadata(&path).unwrap().is_file());
                paths.push(path);
            }
        }
        paths.sort();
        paths
            .into_iter()
            .filter(|p| p.exists())
            .map(|p| {
                let bytes = fs::read(&p).unwrap();
                (p, bytes.len(), format!("{:x}", Sha256::digest(&bytes)))
            })
            .collect()
    }
    fn ledger(root: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(root.join("private-state/worker-fence.json")).unwrap())
            .unwrap()
    }
    #[test]
    #[ignore = "requires writable delegated cgroups, bwrap and pinned media tools; run explicitly serially"]
    fn native_engine_creation_owner_death() {
        if let Some(root) = std::env::var_os(ROOT) {
            child(Path::new(&root), &std::env::var(MODE).unwrap());
        }
        for mode in ["ordinary", "encoded"] {
            for cut in [
                "Ticket",
                "Intent",
                "Created",
                "Captured",
                "Bound",
                "Configured",
                "Registered",
            ] {
                let directory_owner = tempfile::tempdir().unwrap();
                let root = fs::canonicalize(directory_owner.path()).unwrap();
                // Keep the directory owner alive until every owned process is reaped.
                let mut process = Process(
                    std::process::Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            CASE,
                            "--ignored",
                            "--nocapture",
                            "--test-threads=1",
                        ])
                        .env(ROOT, &root)
                        .env(CUT, cut)
                        .env(MODE, mode)
                        .stdin(std::process::Stdio::null())
                        .spawn()
                        .unwrap(),
                );
                let ready = wait_json(&root.join("ready.json"), || {
                    process.0.try_wait().unwrap().is_none()
                });
                let scope = ready["scope"].as_str().unwrap();
                schema(&root, scope);
                let observer = Connection::open_with_flags(
                    root.join("private-state/projects.sqlite3"),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .unwrap();
                let jobs: i64 = observer
                    .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(jobs, i64::from(mode == "ordinary"));
                drop(observer);
                let declared = ledger(&root);
                let records = declared["creation"]["records"].as_object().unwrap();
                assert_eq!(
                    records.len(),
                    usize::from(!matches!(cut, "Ticket" | "Registered"))
                );
                assert_eq!(
                    declared["groups"].as_object().unwrap().len(),
                    usize::from(cut == "Registered")
                );
                if let Some(phase) = records.values().next() {
                    if matches!(cut, "Bound" | "Configured") {
                        assert!(phase["Bound"].is_object());
                    } else {
                        assert_eq!(phase, "Intent");
                    }
                }
                assert_eq!(
                    declared["staging"].as_object().unwrap().len(),
                    usize::from(mode == "encoded")
                );
                let path = PathBuf::from(ready["path"].as_str().unwrap());
                let identity = fs::metadata(&path).ok().map(|m| (m.dev(), m.ino()));
                let _cleanup = GroupCleanup(identity.map(|(d, i)| (path.clone(), d, i)));
                assert_eq!(identity.is_some(), !matches!(cut, "Ticket" | "Intent"));
                if let Some((d, i)) = identity {
                    crate::cgroup::check_recorded_empty(&path, d, i, false).unwrap();
                }
                if !ready["held_identity"].is_null() {
                    assert_eq!(
                        identity.unwrap(),
                        (
                            ready["held_identity"][0].as_u64().unwrap(),
                            ready["held_identity"][1].as_u64().unwrap()
                        )
                    );
                }
                let stage = ready["staging"].as_str().map(Path::new);
                let before = protected(&root, stage);
                let refusal = Engine::open(config(&root))
                    .err()
                    .expect("second engine acquired a live creator lease");
                assert!(
                    refusal
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock),
                    "{refusal:#}"
                );
                assert_eq!(
                    protected(&root, stage),
                    before,
                    "live lease refusal changed owned bytes"
                );
                process.0.kill().unwrap();
                assert_eq!(process.0.wait().unwrap().signal(), Some(libc::SIGKILL));
                if let Some((d, i)) = identity {
                    crate::cgroup::check_recorded_empty(&path, d, i, false).unwrap();
                }
                let before = protected(&root, stage);
                let recovered = Engine::open(config(&root));
                if matches!(cut, "Created" | "Captured") {
                    let refusal = recovered
                        .err()
                        .expect("existing unbound intent silently recovered");
                    assert!(
                        refusal
                            .to_string()
                            .contains("ambiguous cgroup creation intent"),
                        "{refusal:#}"
                    );
                    assert_eq!(
                        protected(&root, stage),
                        before,
                        "ambiguous restart mutated database, journal or staged bytes"
                    );
                    assert!(path.exists());
                    if let Some(stage) = stage {
                        assert!(stage.exists());
                    }
                    // Only the test's independent exact-identity observation permits
                    // this cleanup. Production restart must never infer it from a name.
                    let (d, i) = identity.unwrap();
                    crate::cgroup::reclaim_recorded(&path, d, i, false).unwrap();
                    drop(Engine::open(config(&root)).unwrap());
                } else {
                    drop(recovered.unwrap());
                }
                assert!(!path.exists());
                if let Some(stage) = stage {
                    assert!(!stage.exists());
                }
                let final_ledger = ledger(&root);
                assert_eq!(final_ledger["version"], 4);
                assert_eq!(final_ledger["scope"], scope);
                for field in ["groups", "retired", "staging"] {
                    assert!(
                        final_ledger[field].as_object().unwrap().is_empty(),
                        "uncleared {field}"
                    );
                }
                assert!(final_ledger["creation"]["records"]
                    .as_object()
                    .unwrap()
                    .is_empty());
                schema(&root, scope);
                eprintln!(
                    "engine creation owner-death observation: {}",
                    serde_json::json!({"mode":mode,"cut":cut,"scope":scope,"group_identity":identity,"ambiguous_refusal":matches!(cut,"Created"|"Captured"),"staged":stage.is_some(),"schema":20,"minimum_ledger":4,"recovered":true})
                );
                drop(process);
                drop(_cleanup);
                directory_owner.close().unwrap();
            }
        }
    }
}
