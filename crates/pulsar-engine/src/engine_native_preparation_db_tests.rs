mod native_preparation_journal {
    use super::*;
    use crate::native_preparation_db as journal;
    fn declaration(f: &Fixture) -> NativeImagePreparationIntent {
        NativeImagePreparationIntent {
            operation_id: AttemptId::new("native-preparation").unwrap(),
            engine_epoch: AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
            owner: f.session.clone(),
            project: f.project.clone(),
            revision: 0,
            original_request_id: RequestId::new("native-preparation-request").unwrap(),
            options: NativeImagePreparationOptions {
                preparation_ms: 100,
                retention_ms: 200,
            },
            image_sha256: "a".repeat(64),
            owner_policy_sha256: "b".repeat(64),
            declared_bytes: 4096,
        }
    }
    fn persist(f: &Fixture) -> NativeImagePreparationReport {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        f.engine.arm_worker_fence(&tx).unwrap();
        let report = journal::begin(&tx, declaration(f), &"c".repeat(64)).unwrap();
        tx.commit().unwrap();
        report
    }
    #[test]
    fn native_preparation_engine_reopen_interrupts_intent_without_resource_authority() {
        let f = fixture();
        let initial = persist(&f);
        let config = f.engine.config.clone();
        assert_eq!(f.engine.pool.reserved_storage(), 0);
        drop(f.engine);
        let engine = reopen_after_transient_leases(config).unwrap();
        let db = engine.db.lock().unwrap();
        let current = journal::load(&db, initial.intent.operation_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(current.intent, initial.intent);
        assert_eq!(current.state, NativeImagePreparationState::Interrupted);
        assert!(!current.cleanup_confirmed);
        assert_eq!(current.generation, 1);
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            27
        );
        assert_eq!(engine.pool.reserved_storage(), 0);
        assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_ok());
    }
    #[test]
    fn native_preparation_engine_preserves_existing_archive_barrier_on_reopen() {
        let f = fixture();
        persist(&f);
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            crate::wasm_archive_db::arm(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(journal::version(&db).unwrap(), 26);
        }
        let config = f.engine.config.clone();
        drop(f.engine);
        let engine = reopen_after_transient_leases(config).unwrap();
        let db = engine.db.lock().unwrap();
        assert_eq!(journal::version(&db).unwrap(), 26);
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 27).unwrap(),
            20
        );
    }
    #[test]
    fn native_preparation_corruption_refusal_preserves_database_and_wal_bytes() {
        let f = fixture();
        persist(&f);
        let config = f.engine.config.clone();
        drop(f.engine);
        let path = config.state_dir.join("projects.sqlite3");
        let keeper = Connection::open(&path).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE native_image_preparations SET report=zeroblob(16385); UPDATE projects SET name=name || ' tail';").unwrap();
        let paths = [
            path,
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        let before = paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>();
        assert!(before[1].len() > 32);
        let error = reopen_after_transient_leases(config)
            .err()
            .expect("malformed durable intent accepted");
        assert!(format!("{error:#}").contains("native preparation row exceeds admission"));
        for (path, expected) in paths.iter().zip(before) {
            assert_eq!(fs::read(path).unwrap(), expected);
        }
    }
    #[test]
    #[ignore = "requires an explicitly pinned preceding reader executable; no CUDA or device work"]
    fn native_preparation_preceding_reader_refuses_before_database_wal_or_ledger_mutation() {
        let f = fixture();
        persist(&f);
        let config = f.engine.config.clone();
        drop(f.engine);
        let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE projects SET name=name || ' tail';").unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        let before = paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>();
        assert!(before[1].len() > 32);
        let reader =
            std::env::var_os("PULSAR_PUBLIC_CUDA_PRECEDING_READER").expect("pin preceding reader");
        let result = std::process::Command::new("/usr/bin/timeout")
            .args(["--signal=TERM", "5"])
            .arg(reader)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PULSAR_STATE_DIR", &config.state_dir)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration"));
        for (path, expected) in paths.iter().zip(before) {
            assert_eq!(fs::read(path).unwrap(), expected);
        }
        assert!(!config.state_dir.join("engine.sock").exists());
        if let Some(path) = std::env::var_os("PULSAR_NATIVE_PREPARATION_READER_EVIDENCE") {
            let path = PathBuf::from(path);
            assert!(path.is_absolute() && !path.exists());
            artifacts::private_directory(&path).unwrap();
            fs::write(path.join("preceding-reader-stderr.txt"), &result.stderr).unwrap();
            fs::write(
                path.join("checked.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "schema":27,"preceding_reader_exit_code":1,"database_wal_ledger_unchanged":true,
                    "wal_bytes":fs::metadata(&paths[1]).unwrap().len(),"socket_created":false,
                    "native_capture_performed":false,"physical_dispatch":false,"qualified":false
                }))
                .unwrap(),
            )
            .unwrap();
        }
        drop(keeper);
        let engine = reopen_after_transient_leases(config).unwrap();
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            27
        );
    }
}
