//! Reader41: admitted fallback graph semantics, distinct from draft reader40.
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};
const READER_NAME: &str = "portable_fallback_graph_reader";
const READER_MARKER: &str = "CREATE TABLE portable_fallback_graph_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 40))";
const OUTER_VERSION: i64 = 41;
fn header(db: &Connection) -> Result<(i64, bool)> {
    let raw = crate::browser_fallback_reader::version(db)?;
    ensure!(
        raw == 0 || (2..=OUTER_VERSION).contains(&raw),
        "unknown portable fallback graph reader version"
    );
    let temporary: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    ensure!(
        !temporary,
        "portable fallback graph reader temporary object differs"
    );
    let objects: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE LIMIT 2)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    if raw != OUTER_VERSION {
        ensure!(
            objects == 0,
            "portable fallback graph reader barrier downgraded or misplaced"
        );
        return Ok((raw, false));
    }
    ensure!(
        objects == 1,
        "portable fallback graph reader marker missing or associated objects differ"
    );
    let layout: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)",
        rusqlite::params![READER_NAME, READER_MARKER.len() as i64, READER_MARKER],
        |r| r.get(0),
    )?;
    ensure!(layout, "portable fallback graph reader layout differs");
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.portable_fallback_graph_reader LIMIT 2)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count == 1,
        "portable fallback graph reader singleton differs"
    );
    let underlying: Option<i64> = db
        .query_row(
            "SELECT underlying_version FROM main.portable_fallback_graph_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 40",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok((
        underlying.context("portable fallback graph reader scalar admission differs")?,
        true,
    ))
}

pub(crate) fn version(db: &Connection) -> Result<i64> {
    Ok(header(db)?.0)
}

pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let (old, armed) = header(db)?;
    ensure!(
        (2..=40).contains(&new_version) && new_version >= old,
        "portable fallback graph reader publication would downgrade or exceed supported storage"
    );
    if new_version == old {
        return Ok(());
    }
    if armed {
        let changed = db.execute(
            "UPDATE main.portable_fallback_graph_reader SET underlying_version=?1 WHERE singleton=1 AND underlying_version=?2",
            rusqlite::params![new_version, old],
        )?;
        ensure!(
            changed == 1,
            "portable fallback graph reader publication lost its retained header"
        );
    } else {
        crate::browser_fallback_reader::publish(db, new_version)?;
    }
    Ok(())
}

pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let (old, armed) = header(tx)?;
    ensure!(
        (2..=40).contains(&old),
        "portable fallback graph reader requires supported initialized storage"
    );
    if !armed {
        tx.execute_batch(READER_MARKER)?;
        tx.execute(
            "INSERT INTO main.portable_fallback_graph_reader(singleton,underlying_version) VALUES(1,?1)",
            [old],
        )?;
        crate::browser_fallback_reader::publish(tx, OUTER_VERSION)?;
    }
    ensure!(
        header(tx)? == (old, true),
        "portable fallback graph reader arming did not preserve storage"
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
            "portable fallback metadata requires its schema-41 barrier"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader41_preserves_underlying_headers_and_rollback() {
        for base in 2..=40 {
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
            publish(&db, 40).unwrap();
            assert_eq!(version(&db).unwrap(), 40);
            assert!(publish(&db, 39).is_err());
            assert!(publish(&db, 41).is_err());
            db.execute_batch("PRAGMA query_only=ON").unwrap();
            assert_eq!(version(&db).unwrap(), 40);
        }
    }
    #[test]
    fn reader41_composes_all120_orders_and_keeps_the_graph_floor() {
        let mut checked = 0;
        for a in 0..5 {
            for b in 0..5 {
                for c in 0..5 {
                    for d in 0..5 {
                        for e in 0..5 {
                            let order = [a, b, c, d, e];
                            if order
                                .iter()
                                .copied()
                                .collect::<std::collections::BTreeSet<_>>()
                                .len()
                                != 5
                            {
                                continue;
                            }
                            let mut db = Connection::open_in_memory().unwrap();
                            db.execute_batch("PRAGMA user_version=20").unwrap();
                            let tx = db.transaction().unwrap();
                            for n in order {
                                match n {
                                    0 => arm(&tx).unwrap(),
                                    1 => crate::portable_fallback_reader::arm(&tx).unwrap(),
                                    2 => crate::portable_cpu_archives::arm(&tx).unwrap(),
                                    3 => crate::live_cpu_reader::arm(&tx).unwrap(),
                                    _ => crate::inference_fallback_reader::arm(&tx).unwrap(),
                                }
                            }
                            assert_eq!(version(&tx).unwrap(), 40);
                            assert_eq!(crate::inference_fallback_reader::version(&tx).unwrap(), 20);
                            crate::inference_fallback_reader::publish(&tx, 36).unwrap();
                            assert_eq!(crate::inference_fallback_reader::version(&tx).unwrap(), 36);
                            assert_eq!(
                                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                                    .unwrap(),
                                41
                            );
                            tx.commit().unwrap();
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 120);
    }
}
