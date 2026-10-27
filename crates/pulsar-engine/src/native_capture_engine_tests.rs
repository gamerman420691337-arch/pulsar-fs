// Actual confined capture using the held engine lease and committed admission.
mod native_capture_engine {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "requires actual confined worker execution and writable delegated cgroups; run explicitly serially"]
    fn native_capture_inherited_lease_reaps_idle_worker_without_renewal() {
        use std::os::unix::fs::PermissionsExt;
        let worker = std::env::var_os("PULSAR_BENCHMARK_WORKER").expect("pin actual native worker");
        let root = tempfile::tempdir().unwrap();
        let engine = Engine::open(EngineConfig::new(root.path().join("state"), worker.into())).unwrap();
        let input = root.path().join("authored-input");
        let bytes = b"authored capture deadline input; no library execution";
        fs::write(&input, bytes).unwrap();
        let spec = NativeDependencySpec { schema_version: 1, libraries: vec![NativeLibrarySpec {
            path: input, sha256: format!("{:x}", Sha256::digest(bytes)), byte_len: bytes.len() as u64,
            names: vec!["authored.so".into()],
        }] };
        let idle = root.path().join("idle-capture-worker");
        fs::write(&idle, b"#!/bin/sh\nprintf 'ready' > /capture/ready\nexec /bin/sleep 60\n").unwrap();
        fs::set_permissions(&idle, fs::Permissions::from_mode(0o500)).unwrap();
        let mut config = engine.config.clone(); config.worker_executable = idle;
        {
            let mut db = engine.db.lock().unwrap(); let tx = db.transaction().unwrap();
            engine.arm_worker_fence(&tx).unwrap(); tx.commit().unwrap();
        }
        let budget = ResourceBudget { memory_bytes: 512 << 20, output_bytes: (16 << 20) + bytes.len() as u64,
            wall_time_ms: 2000, cpu_threads: 1 };
        let origin = Instant::now(); let end = origin + Duration::from_millis(budget.wall_time_ms);
        std::thread::sleep(Duration::from_millis(1000));
        let started = Instant::now();
        let observed = std::thread::scope(|threads| {
            let observer = threads.spawn(|| {
                while Instant::now() < end {
                    for entry in fs::read_dir(&engine.config.state_dir).unwrap().flatten() {
                        if fs::read(entry.path().join("ready")).ok().as_deref() == Some(b"ready") {
                            assert!(engine.pool.reserved_storage() > 0);
                            assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_err());
                            return true;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                false
            });
            let error = crate::native_capture::capture_tracked_in_lease(&config, &engine.pool,
                &spec, crate::native_capture::CaptureLimits { budget, address_space_bytes: 1536 << 20 }, &AtomicBool::new(false), engine.worker_fence.clone(), end)
                .err().expect("idle capture unexpectedly completed");
            assert!(error.message.contains("absolute deadline exceeded"), "{error:?}");
            assert!(Instant::now() >= end && Instant::now() < end + Duration::from_millis(500));
            observer.join().unwrap()
        });
        assert!(observed, "fixture never reached its confined idle phase before expiry");
        let capture_elapsed = started.elapsed();
        let cleanup_end = Instant::now() + Duration::from_secs(5);
        loop {
            let ledger: serde_json::Value = serde_json::from_slice(
                &fs::read(engine.config.state_dir.join("worker-fence.json")).unwrap()).unwrap();
            if engine.pool.reserved_storage() == 0 && engine.pool.reserve(engine.config.max_ram_bytes).is_ok()
                && ["groups", "retired", "staging"].iter().all(|field| ledger[field].as_object().unwrap().is_empty())
                && ledger["creation"]["records"].as_object().unwrap().is_empty() { break; }
            assert!(Instant::now() < cleanup_end, "expired native capture custody did not retire");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!fs::read_dir(&engine.config.state_dir).unwrap().flatten().any(|entry|
            entry.file_name().to_string_lossy().starts_with("pulsar-stage-")));
        eprintln!("inherited capture lease: confined idle phase observed with held claims; original 2000ms lease, 1000ms prior elapsed, capture elapsed {:?}; full custody retired", capture_elapsed);
    }
    #[test]
    #[ignore = "requires pinned native worker and writable delegated cgroups; run explicitly serially"]
    fn native_capture_transfers_image_storage_after_exact_worker_retirement() {
        let worker = std::env::var_os("PULSAR_BENCHMARK_WORKER").expect("pin actual native worker");
        let root = tempfile::tempdir().unwrap();
        let engine =
            Engine::open(EngineConfig::new(root.path().join("state"), worker.into())).unwrap();
        let input = root.path().join("authored-input");
        let bytes = b"authored native capture custody bytes; no library execution";
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
        let budget = ResourceBudget {
            memory_bytes: 512 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024 + bytes.len() as u64,
            wall_time_ms: 10_000,
            cpu_threads: 1,
        };
        let capture = |spec: &NativeDependencySpec| {
            crate::native_capture::capture_tracked(
                &engine.config,
                &engine.pool,
                spec,
                crate::native_capture::CaptureLimits { budget: budget.clone(), address_space_bytes: 1536 * 1024 * 1024 },
                &AtomicBool::new(false),
                engine.worker_fence.clone(),
            )
        };
        assert!(
            capture(&spec).is_err(),
            "uncommitted engine admitted native creation"
        );
        assert_eq!(engine.pool.reserved_storage(), 0);
        {
            let mut db = engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            engine.arm_worker_fence(&tx).unwrap();
            tx.commit().unwrap();
        }
        let scope = engine.worker_fence.scope().unwrap();
        let image = capture(&spec).unwrap();
        image.captured().verify().unwrap();
        assert!(engine.pool.reserved_storage() > 0);
        // The worker's full RAM claim must be free while image storage remains.
        let probe = engine.pool.reserve(engine.config.max_ram_bytes).unwrap();
        drop(probe);
        let ledger: serde_json::Value = serde_json::from_slice(
            &fs::read(engine.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        assert!(ledger["groups"].as_object().unwrap().is_empty());
        assert!(ledger["retired"].as_object().unwrap().is_empty());
        assert!(ledger["creation"]["records"]
            .as_object()
            .unwrap()
            .is_empty());
        assert_eq!(ledger["staging"].as_object().unwrap().len(), 1);
        let content = image
            .captured()
            .library_directory()
            .parent()
            .unwrap()
            .join("content");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&content, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(image.close().is_err());
        assert!(engine.pool.reserved_storage() > 0);
        assert!(crate::worker_fence::WorkerFence::open(&engine.config.state_dir).is_err());
        assert!(content.exists());
        fs::set_permissions(&content, fs::Permissions::from_mode(0o700)).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let cpu_free = engine.pool.reserve(engine.config.max_ram_bytes).is_ok();
            if cpu_free && engine.pool.reserved_storage() == 0 {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut wrong = spec.clone();
        wrong.libraries[0].sha256 = "f".repeat(64);
        assert!(capture(&wrong).is_err());
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let cpu_free = engine.pool.reserve(engine.config.max_ram_bytes).is_ok();
            if cpu_free && engine.pool.reserved_storage() == 0 {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        // A successful image, not just a live Engine object, must retain the
        // durable staging lease until its own deletion is acknowledged.
        let surviving = capture(&spec).unwrap();
        let surviving_content = surviving
            .captured()
            .library_directory()
            .parent()
            .unwrap()
            .join("content");
        fs::set_permissions(&surviving_content, fs::Permissions::from_mode(0o500)).unwrap();
        let config = engine.config.clone();
        let pool = engine.pool.clone();
        drop(engine);
        assert!(crate::worker_fence::WorkerFence::open(&config.state_dir).is_err());
        assert!(surviving.close().is_err());
        assert!(pool.reserved_storage() > 0);
        let refusal = Engine::open(config.clone())
            .err()
            .expect("blocked image cleanup lost its engine lease");
        assert!(
            refusal
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock),
            "{refusal:#}"
        );
        fs::set_permissions(&surviving_content, fs::Permissions::from_mode(0o700)).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while pool.reserved_storage() != 0 {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        let reopened = reopen_after_transient_leases(config).unwrap();
        assert_eq!(reopened.worker_fence.scope().unwrap(), scope);
        let ledger: serde_json::Value = serde_json::from_slice(
            &fs::read(reopened.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        for field in ["groups", "retired", "staging"] {
            assert!(ledger[field].as_object().unwrap().is_empty());
        }
        assert!(ledger["creation"]["records"]
            .as_object()
            .unwrap()
            .is_empty());
        eprintln!("tracked native capture: committed admission, verified authored bytes, retired group/CPU, held storage/lease through blocked deletion, failed-capture cleanup, image-owned lease after engine drop and same-scope restart passed");
    }
}
