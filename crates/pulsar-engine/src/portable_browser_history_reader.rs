//! Reader43: portable browser recording evidence graphs remain inert.
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};
const READER_NAME: &str = "portable_browser_history_reader";
const READER_MARKER: &str = "CREATE TABLE portable_browser_history_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 42))";
const OUTER_VERSION: i64 = 43;
fn header(db: &Connection) -> Result<(i64, bool)> {
    let raw: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    ensure!(
        raw == 0 || (2..=OUTER_VERSION).contains(&raw),
        "unknown portable browser history reader version"
    );
    let temporary: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    ensure!(
        !temporary,
        "portable browser history reader temporary object differs"
    );
    let objects: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE LIMIT 2)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    if raw != OUTER_VERSION {
        ensure!(
            objects == 0,
            "portable browser history reader barrier downgraded or misplaced"
        );
        return Ok((raw, false));
    }
    ensure!(
        objects == 1,
        "portable browser history reader marker missing or associated objects differ"
    );
    let layout: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)",
        rusqlite::params![READER_NAME, READER_MARKER.len() as i64, READER_MARKER],
        |r| r.get(0),
    )?;
    ensure!(layout, "portable browser history reader layout differs");
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.portable_browser_history_reader LIMIT 2)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count == 1,
        "portable browser history reader singleton differs"
    );
    let underlying: Option<i64> = db
        .query_row(
            "SELECT underlying_version FROM main.portable_browser_history_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 42",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok((
        underlying.context("portable browser history reader scalar admission differs")?,
        true,
    ))
}

pub(crate) fn version(db: &Connection) -> Result<i64> {
    Ok(header(db)?.0)
}

pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let (old, armed) = header(db)?;
    ensure!(
        (2..=42).contains(&new_version) && new_version >= old,
        "portable browser history reader publication would downgrade or exceed supported storage"
    );
    if new_version == old {
        return Ok(());
    }
    if armed {
        let changed = db.execute(
            "UPDATE main.portable_browser_history_reader SET underlying_version=?1 WHERE singleton=1 AND underlying_version=?2",
            rusqlite::params![new_version, old],
        )?;
        ensure!(
            changed == 1,
            "portable browser history reader publication lost its retained header"
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
        (2..=42).contains(&old),
        "portable browser history reader requires supported initialized storage"
    );
    if !armed {
        tx.execute_batch(READER_MARKER)?;
        tx.execute(
            "INSERT INTO main.portable_browser_history_reader(singleton,underlying_version) VALUES(1,?1)",
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
        "portable browser history reader arming did not preserve storage"
    );
    Ok(())
}

/// Read-only preflight before recovery. Declarations and object roles, including
/// inherited format17 containers, cannot silently lose their reader barrier.
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
            "SELECT EXISTS(SELECT 1 FROM main.project_import_archives WHERE CASE WHEN typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(manifest) THEN 0 ELSE json_extract(manifest,'$.format_version')>=17 OR EXISTS(SELECT 1 FROM json_each(manifest,'$.imported_origins') e WHERE json_extract(e.value,'$.container.format_version')>=17) OR EXISTS(SELECT 1 FROM json_each(manifest,'$.sources') e WHERE json_type(e.value,'$.browser_fallback_history') IS NOT NULL AND json_type(e.value,'$.browser_fallback_history')!='null') OR EXISTS(SELECT 1 FROM json_each(manifest,'$.objects') o,json_each(o.value,'$.roles') r WHERE r.value='browser_inference_fallback_history') END)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM main.project_package_exports WHERE CASE WHEN typeof(status)!='text' OR length(CAST(status AS BLOB)) NOT BETWEEN 1 AND 1048576 OR NOT json_valid(status) THEN 0 ELSE json_extract(status,'$.artifact.format_version')>=17 END)"
        };
        let required: bool = db.query_row(sql, [], |r| r.get(0))?;
        ensure!(
            !required || armed,
            "portable fallback metadata requires its schema-43 barrier"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("portable_browser_history_reader_native_tests.rs");
    #[test]
    fn browser_archive_reader_preserves_both_wrapper_orders_and_rollback() {
        for base in 2..=41 {
            for outer_first in [false, true] {
                let mut db = Connection::open_in_memory().unwrap();
                db.pragma_update(None, "user_version", base).unwrap();
                let tx = db.transaction().unwrap();
                if outer_first {
                    arm(&tx).unwrap();
                    crate::browser_fallback_reader::arm(&tx).unwrap();
                } else {
                    crate::browser_fallback_reader::arm(&tx).unwrap();
                    arm(&tx).unwrap();
                }
                arm(&tx).unwrap();
                assert_eq!(version(&tx).unwrap(), 42);
                assert_eq!(crate::browser_fallback_reader::version(&tx).unwrap(), base);
                tx.rollback().unwrap();
                assert_eq!(version(&db).unwrap(), base);
            }
        }
    }
    #[test]
    fn browser_archive_reader_delegates_live_cuda_base_without_inventing_evidence() {
        for browser in [false, true] {
            let mut db = Connection::open_in_memory().unwrap();
            db.pragma_update(None, "user_version", 20).unwrap();
            let tx = db.transaction().unwrap();
            if browser {
                crate::browser_fallback_reader::arm(&tx).unwrap();
            }
            arm(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                crate::live_cuda_evidence_db::base_version(&db, 43).unwrap(),
                20
            );
            assert!(crate::live_cuda_evidence_db::base_version(&db, 44).is_err());
            db.execute_batch("DROP TABLE portable_browser_history_reader;")
                .unwrap();
            assert!(crate::live_cuda_evidence_db::base_version(&db, 43).is_err());
        }
    }

    #[test]
    fn browser_archive_reader_refuses_stripping_shadows_and_undeclared_metadata() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 25).unwrap();
        db.execute_batch("CREATE TABLE project_import_archives(manifest TEXT);INSERT INTO project_import_archives VALUES('{\"format_version\":17}');").unwrap();
        assert!(validate_metadata(&db).is_err());
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        validate_metadata(&db).unwrap();
        db.execute_batch("CREATE TEMP TABLE portable_browser_history_reader(x);")
            .unwrap();
        assert!(validate_metadata(&db).is_err());
        db.execute_batch("DROP TABLE temp.portable_browser_history_reader;DROP TABLE main.portable_browser_history_reader;").unwrap();
        assert!(validate_metadata(&db).is_err());
    }
}
