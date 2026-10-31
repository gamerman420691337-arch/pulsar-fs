//! Conditional reader42 barrier for retained browser fallback intent/admission.
//! It preserves every older wrapper and never grants native execution authority.
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};

const NAME: &str = "browser_fallback_reader";
const MARKER: &str = "CREATE TABLE browser_fallback_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 41))";
pub(crate) const MAX_STORAGE_VERSION: i64 = 42;

fn header(db: &Connection) -> Result<(i64, bool)> {
    let raw: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    ensure!(
        raw == 0 || (2..=MAX_STORAGE_VERSION).contains(&raw),
        "unknown browser fallback reader version"
    );
    let temporary: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE)", [NAME], |r| r.get(0))?;
    ensure!(
        !temporary,
        "browser fallback reader temporary object differs"
    );
    let count: i64 = db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE LIMIT 2)", [NAME], |r| r.get(0))?;
    if raw != MAX_STORAGE_VERSION {
        ensure!(
            count == 0,
            "browser fallback reader downgraded or misplaced"
        );
        return Ok((raw, false));
    }
    ensure!(
        count == 1,
        "browser fallback reader marker missing or associated objects differ"
    );
    let exact: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)", rusqlite::params![NAME, MARKER.len() as i64, MARKER], |r| r.get(0))?;
    ensure!(exact, "browser fallback reader layout differs");
    let rows: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.browser_fallback_reader LIMIT 2)",
        [],
        |r| r.get(0),
    )?;
    ensure!(rows == 1, "browser fallback reader singleton differs");
    let underlying: Option<i64> = db.query_row("SELECT underlying_version FROM main.browser_fallback_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 41", [], |r| r.get(0)).optional()?;
    Ok((
        underlying.context("browser fallback reader scalar differs")?,
        true,
    ))
}

pub(crate) fn version(db: &Connection) -> Result<i64> {
    Ok(header(db)?.0)
}

pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let (old, armed) = header(db)?;
    ensure!(
        (2..=41).contains(&new_version) && new_version >= old,
        "browser fallback reader publication would downgrade or exceed storage"
    );
    if old == new_version {
        return Ok(());
    }
    if armed {
        let changed = db.execute("UPDATE main.browser_fallback_reader SET underlying_version=?1 WHERE singleton=1 AND underlying_version=?2", rusqlite::params![new_version, old])?;
        ensure!(
            changed == 1,
            "browser fallback reader lost its retained header"
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
        (2..=41).contains(&old),
        "browser fallback requires initialized supported storage"
    );
    if !armed {
        tx.execute_batch(MARKER)?;
        tx.execute(
            "INSERT INTO main.browser_fallback_reader(singleton,underlying_version) VALUES(1,?1)",
            [old],
        )?;
        tx.pragma_update(
            Some(rusqlite::DatabaseName::Main),
            "user_version",
            MAX_STORAGE_VERSION,
        )?;
    }
    ensure!(
        header(tx)? == (old, true),
        "browser fallback arming changed underlying storage"
    );
    Ok(())
}

pub(crate) fn remember(tx: &Transaction<'_>, body: &pulsar_protocol::ResponseBody) -> Result<()> {
    use pulsar_protocol::ResponseBody;
    let capture = match body {
        ResponseBody::LiveCapture(value) => Some(value.as_ref()),
        ResponseBody::BrowserAnalysisSuccessor(value) => Some(&value.successor),
        _ => None,
    };
    if let Some(capture) = capture.filter(|c| {
        c.options.inference_fallback.is_some()
            || c.inference.as_ref().is_some_and(|i| i.fallback.is_some())
    }) {
        capture
            .validate()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        arm(tx)?;
    }
    Ok(())
}

/// Detect a stripped barrier before startup recovery. Payload shape and exact
/// custody are separately validated by the owning stores and protocol types.
pub(crate) fn validate_metadata(db: &Connection) -> Result<()> {
    let (_, armed) = header(db)?;
    for (table, query) in [
        ("requests", "SELECT EXISTS(SELECT 1 FROM main.requests WHERE CASE WHEN NOT json_valid(response) THEN 0 ELSE json_extract(response,'$.kind') IN ('live_capture','browser_analysis_successor') AND EXISTS(SELECT 1 FROM json_tree(response) j WHERE j.key='inference_fallback') END)"),
        ("live_cpu_admissions", "SELECT EXISTS(SELECT 1 FROM main.live_cpu_admissions WHERE CASE WHEN NOT json_valid(CAST(origin AS TEXT)) THEN 0 ELSE EXISTS(SELECT 1 FROM json_tree(CAST(origin AS TEXT)) j WHERE j.key='inference_fallback') END)"),
        ("live_cuda_capture_evidence", "SELECT EXISTS(SELECT 1 FROM main.live_cuda_capture_evidence WHERE CASE WHEN NOT json_valid(CAST(origin AS TEXT)) THEN 0 ELSE json_type(CAST(origin AS TEXT),'$.options.inference_fallback') IS NOT NULL END)"),
    ] {
        let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1)", [table], |r| r.get(0))?;
        if exists {
            let required: bool = db.query_row(query, [], |r| r.get(0))?;
            ensure!(!required || armed, "browser fallback metadata requires reader42");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("browser_fallback_reader_native_tests.rs");
    #[test]
    fn browser_reader_preserves_every_underlying_header_and_rollback() {
        for base in 2..=41 {
            let mut db = Connection::open_in_memory().unwrap();
            db.pragma_update(None, "user_version", base).unwrap();
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            arm(&tx).unwrap();
            assert_eq!(version(&tx).unwrap(), base);
            tx.rollback().unwrap();
            assert_eq!(version(&db).unwrap(), base);
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
            publish(&db, 41).unwrap();
            assert_eq!(version(&db).unwrap(), 41);
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                42
            );
            assert!(publish(&db, 40).is_err());
            assert!(publish(&db, 42).is_err());
        }
    }
    #[test]
    fn browser_reader_composes_all720_orders_and_keeps_the_outer_floor() {
        fn visit(order: &mut Vec<usize>, checked: &mut usize) {
            if order.len() != 6 {
                for next in 0..6 {
                    if !order.contains(&next) {
                        order.push(next);
                        visit(order, checked);
                        order.pop();
                    }
                }
                return;
            }
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch("PRAGMA user_version=20").unwrap();
            let tx = db.transaction().unwrap();
            for n in order {
                match n {
                    0 => arm(&tx),
                    1 => crate::portable_fallback_graph_reader::arm(&tx),
                    2 => crate::portable_fallback_reader::arm(&tx),
                    3 => crate::portable_cpu_archives::arm(&tx),
                    4 => crate::live_cpu_reader::arm(&tx),
                    _ => crate::inference_fallback_reader::arm(&tx),
                }
                .unwrap();
            }
            assert_eq!(version(&tx).unwrap(), 41);
            assert_eq!(crate::inference_fallback_reader::version(&tx).unwrap(), 20);
            crate::inference_fallback_reader::publish(&tx, 36).unwrap();
            assert_eq!(crate::inference_fallback_reader::version(&tx).unwrap(), 36);
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                42
            );
            tx.commit().unwrap();
            *checked += 1;
        }
        let mut checked = 0;
        visit(&mut Vec::new(), &mut checked);
        assert_eq!(checked, 720);
    }
    #[test]
    fn browser_reader_refuses_missing_changed_temporary_and_downgraded_markers() {
        for fault in ["missing", "temporary", "scalar", "trigger", "downgrade"] {
            let mut db = Connection::open_in_memory().unwrap();
            db.pragma_update(None, "user_version", 25).unwrap();
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
            db.execute_batch(match fault {
                "missing" => "DROP TABLE browser_fallback_reader",
                "temporary" => "CREATE TEMP TABLE browser_fallback_reader(x)",
                "scalar" => "PRAGMA ignore_check_constraints=ON;UPDATE browser_fallback_reader SET underlying_version=42",
                "trigger" => "CREATE TRIGGER extra AFTER UPDATE ON browser_fallback_reader BEGIN SELECT 1; END",
                _ => "PRAGMA user_version=25",
            }).unwrap();
            assert!(version(&db).is_err(), "{fault}");
        }
    }
    #[test]
    fn retained_live_selection_requires_reader_but_file_selection_does_not() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=25;CREATE TABLE requests(response TEXT)")
            .unwrap();
        db.execute(
            "INSERT INTO requests VALUES(?1)",
            [r#"{"kind":"job","value":{"inference_fallback":{}}}"#],
        )
        .unwrap();
        validate_metadata(&db).unwrap();
        db.execute(
            "INSERT INTO requests VALUES(?1)",
            [r#"{"kind":"live_capture","value":{"options":{"inference_fallback":{}}}}"#],
        )
        .unwrap();
        assert!(validate_metadata(&db).is_err());
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        validate_metadata(&db).unwrap();
        db.execute_batch("DROP TABLE browser_fallback_reader;PRAGMA user_version=25")
            .unwrap();
        assert!(validate_metadata(&db).is_err());
    }
}
