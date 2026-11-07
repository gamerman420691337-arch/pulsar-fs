#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path, process::Command,
    };

    fn fingerprint(root: &Path) -> BTreeMap<String, serde_json::Value> {
        fn visit(
            root: &Path,
            path: &Path,
            rows: &mut BTreeMap<String, serde_json::Value>,
            bytes: &mut u64,
        ) {
            assert!(rows.len() < 512, "owned reader fixture inventory bound");
            let meta = fs::symlink_metadata(path).unwrap();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let value = if meta.is_dir() {
                for item in fs::read_dir(path).unwrap() {
                    visit(root, &item.unwrap().path(), rows, bytes);
                }
                json!({"kind":"directory","mode":meta.permissions().mode()})
            } else if meta.file_type().is_symlink() {
                json!({"kind":"link","mode":meta.permissions().mode(),"sha256":format!("{:x}",Sha256::digest(fs::read_link(path).unwrap().as_os_str().as_encoded_bytes()))})
            } else {
                *bytes = bytes.checked_add(meta.len()).unwrap();
                // Startup retains a worker image. Bound the whole fixture and
                // stream large files instead of allocating every snapshot.
                assert!(*bytes <= 1024 * 1024 * 1024, "owned fixture byte bound");
                use std::io::Read;
                let mut file = fs::File::open(path).unwrap();
                let mut digest = Sha256::new();
                let mut buffer = [0u8; 65536];
                let mut total = 0u64;
                loop {
                    let n = file.read(&mut buffer).unwrap();
                    if n == 0 {
                        break;
                    }
                    total += n as u64;
                    assert!(total <= meta.len(), "owned file changed while hashing");
                    digest.update(&buffer[..n]);
                }
                assert_eq!(total, meta.len());
                json!({"kind":"file","mode":meta.permissions().mode(),"byte_len":meta.len(),"sha256":format!("{:x}",digest.finalize())})
            };
            rows.insert(name, value);
        }
        let mut rows = BTreeMap::new();
        visit(root, root, &mut rows, &mut 0);
        rows
    }

    #[test]
    #[ignore = "explicit pinned reader41 App and durable proof path required; owned cold and committed-WAL fixtures only"]
    fn reader41_refuses_reader42_main_and_wal_without_mutation() {
        let app = std::path::PathBuf::from(
            std::env::var_os("PULSAR_BROWSER_FALLBACK_PRECEDING_APP").expect("pinned reader41 App"),
        );
        let expected = std::env::var("PULSAR_BROWSER_FALLBACK_PRECEDING_SHA256")
            .expect("pinned reader41 digest");
        let proof = std::path::PathBuf::from(
            std::env::var_os("PULSAR_BROWSER_FALLBACK_READER_PROOF").expect("retained proof path"),
        );
        assert!(app.is_absolute() && proof.is_absolute() && !proof.exists());
        let app_hash = format!("{:x}", Sha256::digest(fs::read(&app).unwrap()));
        assert_eq!(app_hash, expected);
        let mut cases = Vec::new();
        for hot_wal in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let state = root.path().join("state");
            let worker = std::env::current_exe().unwrap();
            let engine =
                crate::Engine::open(crate::EngineConfig::new(state.clone(), worker.clone()))
                    .unwrap();
            let config = crate::Engine::retire_for_test(engine);
            let mut db = Connection::open(state.join("projects.sqlite3")).unwrap();
            db.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
            let base: i64 = db
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                42
            );
            let connection = if hot_wal {
                Some(db)
            } else {
                drop(db);
                None
            };
            let main = fs::read(state.join("projects.sqlite3")).unwrap();
            let main_version = u32::from_be_bytes(main[60..64].try_into().unwrap());
            assert_eq!(i64::from(main_version), if hot_wal { base } else { 42 });
            if hot_wal {
                assert!(state.join("projects.sqlite3-wal").metadata().unwrap().len() > 32);
            }
            let before = fingerprint(&state);
            let output = Command::new("timeout")
                .args(["--kill-after=5s", "15s"])
                .arg(&app)
                .arg("engine")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PULSAR_STATE_DIR", &state)
                .env("PULSAR_WORKER_EXECUTABLE", &worker)
                .output()
                .unwrap();
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            assert!(
                !output.status.success() && output.status.code() != Some(124),
                "{diagnostic}"
            );
            assert!(
                diagnostic
                    .contains("project database schema requires an explicit recoverable migration")
                    || (hot_wal
                        && diagnostic.contains("unknown portable fallback graph reader version")),
                "{diagnostic}"
            );
            let after = fingerprint(&state);
            assert_eq!(before, after, "preceding reader changed owned state");
            drop(connection);
            let current = crate::Engine::open(config).unwrap();
            let _ = crate::Engine::retire_for_test(current);
            cases.push(json!({"case":if hot_wal {"committed_wal"} else {"main_file"},"main_reader_version":main_version,
                "sqlite_reader_version":42,"preceding_exit":output.status.code(),"preceding_diagnostic":diagnostic,
                "before":before,"after":after,"unchanged":true,"current_reader_reopened":true,"qualified":false}));
            root.close().unwrap();
        }
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(&app).unwrap())),
            expected
        );
        let receipt = json!({"status":"PASS","preceding_app":app,"preceding_sha256":expected,"cases":cases,
            "qualified":false,"physical_device_contacted":false,"source":"Actual Engine-created empty state plus conditional reader42 marker; no accepted dossier or execution authority is created."});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(proof)
            .unwrap();
        use std::io::Write;
        file.write_all(&serde_json::to_vec_pretty(&receipt).unwrap())
            .unwrap();
        file.sync_all().unwrap();
    }
}
