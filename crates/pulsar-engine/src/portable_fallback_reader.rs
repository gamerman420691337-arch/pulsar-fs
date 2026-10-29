//! Conditional schema-40 barrier for portable inert fallback history.
//! This module never opens a runtime path, adopts an owner policy, or creates
//! execution or acceptance authority.
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};

const READER_NAME: &str = "portable_fallback_reader";
const READER_MARKER: &str = "CREATE TABLE portable_fallback_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 39))";
pub(crate) const MAX_STORAGE_VERSION: i64 = 40;
const OUTER_VERSION: i64 = MAX_STORAGE_VERSION;

/// Fast negative check before SQLite or daemon lock creation. This inspects
/// only the main-file header; it never admits a schema or substitutes for the
/// normal SQLite/WAL and complete marker validation that follows.
pub(crate) fn preflight_main_header(state: &std::path::Path) -> Result<()> {
    use std::io::Read;
    let path = state.join("projects.sqlite3");
    let mut file = match crate::artifacts::open_regular(&path) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(())
        }
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() == 0 {
        return Ok(());
    }
    // SQLite's 100-byte header uses a big-endian user_version at offset60:
    // https://www.sqlite.org/fileformat.html#the_database_header
    let mut header = [0u8; 100];
    file.read_exact(&mut header)
        .context("read existing project database header")?;
    ensure!(
        &header[..16] == b"SQLite format 3\0",
        "invalid project database header; original was not modified"
    );
    let version = i64::from(u32::from_be_bytes(
        header[60..64].try_into().expect("fixed header slice"),
    ));
    ensure!(version == 0 || (2..=MAX_STORAGE_VERSION).contains(&version), "project database schema requires an explicit recoverable migration; original was not modified");
    Ok(())
}

fn header(db: &Connection) -> Result<(i64, bool)> {
    let raw: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    ensure!(
        raw == 0 || (2..=OUTER_VERSION).contains(&raw),
        "unknown portable fallback reader version"
    );
    let temporary: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    ensure!(
        !temporary,
        "portable fallback reader temporary object differs"
    );
    let objects: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE LIMIT 2)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    if raw != OUTER_VERSION {
        ensure!(
            objects == 0,
            "portable fallback reader barrier downgraded or misplaced"
        );
        return Ok((raw, false));
    }
    ensure!(
        objects == 1,
        "portable fallback reader marker missing or associated objects differ"
    );
    let layout: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)",
        rusqlite::params![READER_NAME, READER_MARKER.len() as i64, READER_MARKER],
        |r| r.get(0),
    )?;
    ensure!(layout, "portable fallback reader layout differs");
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.portable_fallback_reader LIMIT 2)",
        [],
        |r| r.get(0),
    )?;
    ensure!(count == 1, "portable fallback reader singleton differs");
    let underlying: Option<i64> = db
        .query_row(
            "SELECT underlying_version FROM main.portable_fallback_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 39",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok((
        underlying.context("portable fallback reader scalar admission differs")?,
        true,
    ))
}

pub(crate) fn version(db: &Connection) -> Result<i64> {
    Ok(header(db)?.0)
}

pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let (old, armed) = header(db)?;
    ensure!(
        (2..=39).contains(&new_version) && new_version >= old,
        "portable fallback reader publication would downgrade or exceed supported storage"
    );
    if new_version == old {
        return Ok(());
    }
    if armed {
        let changed = db.execute(
            "UPDATE main.portable_fallback_reader SET underlying_version=?1 WHERE singleton=1 AND underlying_version=?2",
            rusqlite::params![new_version, old],
        )?;
        ensure!(
            changed == 1,
            "portable fallback reader publication lost its retained header"
        );
    } else {
        db.pragma_update(
            Some(rusqlite::DatabaseName::Main),
            "user_version",
            new_version,
        )?;
    }
    Ok(())
}

pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let (old, armed) = header(tx)?;
    ensure!(
        (2..=39).contains(&old),
        "portable fallback reader requires supported initialized storage"
    );
    if !armed {
        tx.execute_batch(READER_MARKER)?;
        tx.execute(
            "INSERT INTO main.portable_fallback_reader(singleton,underlying_version) VALUES(1,?1)",
            [old],
        )?;
        tx.pragma_update(
            Some(rusqlite::DatabaseName::Main),
            "user_version",
            OUTER_VERSION,
        )?;
    }
    ensure!(
        header(tx)? == (old, true),
        "portable fallback reader arming did not preserve storage"
    );
    Ok(())
}

/// Read-only preflight before recovery. Declarations and object roles, including
/// inherited format16 containers, cannot silently lose their reader barrier.
pub(crate) fn validate_metadata(db: &Connection) -> Result<()> {
    let (_, armed) = header(db)?;
    for table in ["project_import_archives", "project_package_exports"] {
        let present: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !present {
            continue;
        }
        let sql = if table == "project_import_archives" {
            "SELECT EXISTS(SELECT 1 FROM main.project_import_archives WHERE CASE WHEN typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(manifest) THEN 0 ELSE json_extract(manifest,'$.format_version')>=16 OR EXISTS(SELECT 1 FROM json_each(manifest,'$.imported_origins') e WHERE json_extract(e.value,'$.container.format_version')>=16) OR EXISTS(SELECT 1 FROM json_each(manifest,'$.generated_origins') e WHERE json_type(e.value,'$.fallback_history') IS NOT NULL AND json_type(e.value,'$.fallback_history')!='null') OR EXISTS(SELECT 1 FROM json_each(manifest,'$.objects') o,json_each(o.value,'$.roles') r WHERE r.value='inference_fallback_history') END)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM main.project_package_exports WHERE CASE WHEN typeof(status)!='text' OR length(CAST(status AS BLOB)) NOT BETWEEN 1 AND 1048576 OR NOT json_valid(status) THEN 0 ELSE json_extract(status,'$.artifact.format_version')>=16 END)"
        };
        let required: bool = db.query_row(sql, [], |r| r.get(0))?;
        ensure!(
            !required || armed,
            "portable fallback metadata requires its schema-40 barrier"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_header_fast_refusal_is_read_only_and_does_not_admit_a_schema() {
        let tmp = tempfile::tempdir().unwrap();
        preflight_main_header(tmp.path()).unwrap();
        let path = tmp.path().join("projects.sqlite3");
        std::fs::write(&path, []).unwrap();
        preflight_main_header(tmp.path()).unwrap();
        std::fs::write(&path, b"short").unwrap();
        assert!(preflight_main_header(tmp.path()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"short");
        let mut header = [0u8; 100];
        header[..16].copy_from_slice(b"SQLite format 3\0");
        for raw in [0u32, 2, 25, 39, 40] {
            header[60..64].copy_from_slice(&raw.to_be_bytes());
            std::fs::write(&path, header).unwrap();
            preflight_main_header(tmp.path()).unwrap();
        }
        for raw in [1u32, 41, u32::MAX] {
            header[60..64].copy_from_slice(&raw.to_be_bytes());
            std::fs::write(&path, header).unwrap();
            assert!(preflight_main_header(tmp.path()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), header);
        }
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn main_header_fast_check_cannot_hide_a_newer_committed_wal_reader_version() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        crate::artifacts::private_directory(&state).unwrap();
        let path = state.join("projects.sqlite3");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;CREATE TABLE probe(x);PRAGMA user_version=25;PRAGMA wal_checkpoint(TRUNCATE);PRAGMA user_version=41").unwrap();
        let main = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_be_bytes(main[60..64].try_into().unwrap()), 25);
        let wal = std::fs::read(state.join("projects.sqlite3-wal")).unwrap();
        preflight_main_header(&state).unwrap();
        let error = crate::Engine::open(crate::EngineConfig::new(
            state.clone(),
            std::env::current_exe().unwrap(),
        ))
        .err()
        .expect("newer WAL version refused");
        assert!(
            error
                .to_string()
                .contains("unknown portable fallback reader version"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), main);
        assert_eq!(
            std::fs::read(state.join("projects.sqlite3-wal")).unwrap(),
            wal
        );
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    #[ignore = "requires explicit pinned preceding App executable and retained proof path; no device calls"]
    fn native_reader40_reports_prior_side_effects_and_checks_current_cold_refusal() {
        use sha2::{Digest, Sha256};
        use std::{
            collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path, process::Command,
        };
        fn fingerprint(root: &Path) -> BTreeMap<String, (u32, String)> {
            fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<String, (u32, String)>) {
                let metadata = fs::symlink_metadata(path).unwrap();
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let contents = if metadata.file_type().is_symlink() {
                    format!("link:{}", fs::read_link(path).unwrap().display())
                } else if metadata.is_dir() {
                    for entry in fs::read_dir(path).unwrap() {
                        visit(root, &entry.unwrap().path(), entries);
                    }
                    "directory".into()
                } else {
                    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
                };
                entries.insert(name, (metadata.permissions().mode(), contents));
            }
            let mut result = BTreeMap::new();
            visit(root, root, &mut result);
            result
        }
        let preceding =
            std::env::var_os("PULSAR_PRECEDING_READER").expect("explicit preceding executable");
        let proof = std::env::var_os("PULSAR_FALLBACK_READER_PROOF")
            .expect("explicit retained proof destination");
        let current =
            std::env::var_os("PULSAR_CURRENT_READER").expect("explicit current executable");
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let worker = std::env::current_exe().unwrap();
        let engine =
            crate::Engine::open(crate::EngineConfig::new(state.clone(), worker.clone())).unwrap();
        let config = crate::Engine::retire_for_test(engine);
        {
            let mut db = Connection::open(state.join("projects.sqlite3")).unwrap();
            let tx = db.transaction().unwrap();
            crate::portable_cpu_archives::arm(&tx).unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(version(&db).unwrap(), 39);
        }
        let before = fingerprint(&state);
        let output = Command::new("timeout")
            .arg("15s")
            .arg(&preceding)
            .arg("engine")
            .env("PULSAR_STATE_DIR", &state)
            .env("PULSAR_WORKER_EXECUTABLE", &worker)
            .env_remove("PULSAR_CPU_OWNER_POLICY")
            .env_remove("PULSAR_CUDA_OWNER_POLICY")
            .env_remove("PULSAR_ASSISTANT_OWNER_POLICY")
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .output()
            .unwrap();
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success() && output.status.code() != Some(124),
            "{diagnostic}"
        );
        assert!(
            diagnostic.contains("unknown portable CPU reader version"),
            "{diagnostic}"
        );
        let after = fingerprint(&state);
        for (path, identity) in &before {
            assert_eq!(
                after.get(path),
                Some(identity),
                "preceding reader changed original {path}"
            );
        }
        let added: Vec<_> = after
            .keys()
            .filter(|path| !before.contains_key(*path))
            .cloned()
            .collect();
        assert_eq!(
            added,
            [
                "engine.lock",
                "projects.sqlite3-shm",
                "projects.sqlite3-wal"
            ]
        );
        assert_eq!(fs::metadata(state.join("engine.lock")).unwrap().len(), 0);
        assert_eq!(
            fs::metadata(state.join("projects.sqlite3-wal"))
                .unwrap()
                .len(),
            0
        );
        let reopened = crate::Engine::open(config).unwrap();
        crate::Engine::retire_for_test(reopened);
        {
            let db = Connection::open(state.join("projects.sqlite3")).unwrap();
            assert_eq!(version(&db).unwrap(), 39);
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                40
            );
            db.pragma_update(None, "user_version", 41).unwrap();
        }
        // Only the empty lock created by the preceding CLI is removed. SQLite
        // must itself have checkpointed/closed its side files; never erase a WAL.
        fs::remove_file(state.join("engine.lock")).unwrap();
        assert!(!state.join("projects.sqlite3-wal").exists());
        assert!(!state.join("projects.sqlite3-shm").exists());
        let current_before = fingerprint(&state);
        let output_current = Command::new("timeout")
            .arg("15s")
            .arg(&current)
            .arg("engine")
            .env("PULSAR_STATE_DIR", &state)
            .env("PULSAR_WORKER_EXECUTABLE", &worker)
            .env_remove("PULSAR_CPU_OWNER_POLICY")
            .env_remove("PULSAR_CUDA_OWNER_POLICY")
            .env_remove("PULSAR_ASSISTANT_OWNER_POLICY")
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .output()
            .unwrap();
        let current_diagnostic = String::from_utf8_lossy(&output_current.stderr);
        assert!(
            !output_current.status.success() && output_current.status.code() != Some(124),
            "{current_diagnostic}"
        );
        assert!(
            current_diagnostic
                .contains("project database schema requires an explicit recoverable migration"),
            "{current_diagnostic}"
        );
        let current_after = fingerprint(&state);
        assert_eq!(
            current_before, current_after,
            "current cold refusal changed state"
        );
        let value = serde_json::json!({
            "status":"PARTIAL", "scope":"Preceding reader refuses40 but creates auxiliary files; current cold main-header41 refusal preserves state. Hot-WAL and full package integration remain open.",
            "preceding_executable":Path::new(&preceding),
            "preceding_sha256":format!("{:x}",Sha256::digest(fs::read(&preceding).unwrap())),
            "test_executable_sha256":format!("{:x}",Sha256::digest(fs::read(&worker).unwrap())),
            "preceding_exit":output.status.code(), "preceding_diagnostic":diagnostic,
            "before_state":before, "after_state":after,
            "old_reader_state_unchanged":false, "old_reader_existing_files_unchanged":true,
            "old_reader_added_files":added, "current_reader_reopened":true,
            "current_executable":Path::new(&current),
            "current_sha256":format!("{:x}",Sha256::digest(fs::read(&current).unwrap())),
            "current_exit":output_current.status.code(), "current_diagnostic":current_diagnostic,
            "current_before_state":current_before,"current_after_state":current_after,
            "current_cold_main_header_refusal_state_unchanged":true,
            "physical_device_contacted":false, "portable_fallback_complete":false
        });
        fs::write(proof, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }

    #[test]
    fn reader40_is_conditional_idempotent_monotonic_and_transactional() {
        for base in 2..=39 {
            let mut db = Connection::open_in_memory().unwrap();
            db.pragma_update(None, "user_version", base).unwrap();
            assert_eq!(version(&db).unwrap(), base);
            {
                let tx = db.transaction().unwrap();
                arm(&tx).unwrap();
                arm(&tx).unwrap();
                assert_eq!(version(&tx).unwrap(), base);
                assert_eq!(
                    tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    40
                );
                tx.rollback().unwrap();
            }
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                base
            );
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
            publish(&db, 39).unwrap();
            assert_eq!(version(&db).unwrap(), 39);
            assert!(publish(&db, 38).is_err());
            assert!(publish(&db, 40).is_err());
            db.execute_batch("PRAGMA query_only=ON").unwrap();
            assert_eq!(version(&db).unwrap(), 39);
        }
        let mut db = Connection::open_in_memory().unwrap();
        assert_eq!(version(&db).unwrap(), 0);
        assert!(arm(&db.transaction().unwrap()).is_err());
    }

    #[test]
    fn reader40_composes_all24_orders_with_cpu_live_and_fallback_barriers() {
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let order = [a, b, c, d];
                        if order
                            .iter()
                            .copied()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            != 4
                        {
                            continue;
                        }
                        let mut db = Connection::open_in_memory().unwrap();
                        db.execute_batch("PRAGMA user_version=20").unwrap();
                        let tx = db.transaction().unwrap();
                        for n in order {
                            match n {
                                0 => arm(&tx).unwrap(),
                                1 => crate::portable_cpu_archives::arm(&tx).unwrap(),
                                2 => crate::live_cpu_reader::arm(&tx).unwrap(),
                                _ => crate::inference_fallback_reader::arm(&tx).unwrap(),
                            }
                        }
                        assert_eq!(version(&tx).unwrap(), 39, "{order:?}");
                        assert_eq!(crate::portable_cpu_archives::version(&tx).unwrap(), 38);
                        assert_eq!(crate::live_cpu_reader::version(&tx).unwrap(), 37);
                        assert_eq!(crate::inference_fallback_reader::version(&tx).unwrap(), 20);
                        crate::inference_fallback_reader::publish(&tx, 36).unwrap();
                        assert_eq!(crate::inference_fallback_reader::version(&tx).unwrap(), 36);
                        assert_eq!(
                            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                                .unwrap(),
                            40
                        );
                        tx.commit().unwrap();
                        assert_eq!(crate::inference_fallback_reader::version(&db).unwrap(), 36);
                    }
                }
            }
        }
    }

    #[test]
    fn reader40_refuses_layout_shadow_downgrade_and_scalar_mutations_read_only() {
        for mutation in [
            "DROP TABLE portable_fallback_reader",
            "PRAGMA user_version=25",
            "DELETE FROM portable_fallback_reader",
            "ALTER TABLE portable_fallback_reader ADD COLUMN extra INTEGER",
            "CREATE TEMP TABLE portable_fallback_reader(x)",
            "CREATE INDEX extra ON portable_fallback_reader(underlying_version)",
            "CREATE TRIGGER extra AFTER UPDATE ON portable_fallback_reader BEGIN SELECT 1; END",
            "PRAGMA ignore_check_constraints=ON;UPDATE portable_fallback_reader SET underlying_version=40",
        ] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch("PRAGMA user_version=25").unwrap();
            let tx = db.transaction().unwrap(); arm(&tx).unwrap(); tx.commit().unwrap();
            db.execute_batch(mutation).unwrap();
            db.execute_batch("PRAGMA query_only=ON").unwrap();
            assert!(version(&db).is_err(), "{mutation}");
        }
        for raw in [1, 41] {
            let db = Connection::open_in_memory().unwrap();
            db.pragma_update(None, "user_version", raw).unwrap();
            assert!(version(&db).is_err());
        }
    }

    #[test]
    fn reader40_metadata_requires_marker_for_current_ancestor_field_and_role() {
        for (table, col, raw) in [
            (
                "project_import_archives",
                "manifest",
                r#"{"format_version":16}"#,
            ),
            (
                "project_import_archives",
                "manifest",
                r#"{"format_version":1,"imported_origins":[{"container":{"format_version":16}}]}"#,
            ),
            (
                "project_import_archives",
                "manifest",
                r#"{"format_version":1,"generated_origins":[{"fallback_history":{}}]}"#,
            ),
            (
                "project_import_archives",
                "manifest",
                r#"{"format_version":1,"objects":[{"roles":["inference_fallback_history"]}]}"#,
            ),
            (
                "project_package_exports",
                "status",
                r#"{"artifact":{"format_version":16}}"#,
            ),
        ] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch(&format!(
                "PRAGMA user_version=25;CREATE TABLE {table}({col} TEXT)"
            ))
            .unwrap();
            db.execute(&format!("INSERT INTO {table} VALUES(?1)"), [raw])
                .unwrap();
            assert!(validate_metadata(&db).is_err());
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
            validate_metadata(&db).unwrap();
            db.execute_batch("DROP TABLE portable_fallback_reader;PRAGMA user_version=25")
                .unwrap();
            assert!(validate_metadata(&db).is_err());
        }
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "PRAGMA user_version=25;CREATE TABLE project_import_archives(manifest TEXT)",
        )
        .unwrap();
        db.execute(
            "INSERT INTO project_import_archives VALUES(?1)",
            [r#"{"format_version":15,"generated_origins":[{"fallback_history":null}]}"#],
        )
        .unwrap();
        validate_metadata(&db).unwrap();
        assert_eq!(version(&db).unwrap(), 25);
    }
}
