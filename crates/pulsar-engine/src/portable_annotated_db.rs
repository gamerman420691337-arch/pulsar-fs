//! Conditional portable annotated reader barrier, preserving the preceding layer.
use anyhow::{ensure, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};
const TABLE: &str = "portable_annotated_reader";
const CREATE:&str="CREATE TABLE portable_annotated_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 18 AND 23))";
fn exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [TABLE],
        |r| r.get(0),
    )?)
}
pub(crate) fn storage_version(db: &Connection) -> Result<i64> {
    let raw = crate::portable_wasm_db::storage_version(db)?;
    let present = exists(db)?;
    if raw == 24 || present {
        ensure!(
            raw == 24 && present,
            "portable annotated reader barrier missing or downgraded"
        );
        let sql:Option<String>=db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[TABLE],|r|r.get(0)).optional()?;
        ensure!(
            sql.as_deref() == Some(CREATE),
            "portable annotated reader layout differs"
        );
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM portable_annotated_reader LIMIT 2)",
            [],
            |r| r.get(0),
        )?;
        ensure!(count == 1, "portable annotated reader singleton differs");
        let version:i64=db.query_row("SELECT underlying_version FROM portable_annotated_reader WHERE typeof(singleton)='integer' AND singleton=1 AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 18 AND 23",[],|r|r.get(0))?;
        return Ok(version);
    }
    ensure!(raw < 24, "unknown portable annotated reader version");
    Ok(raw)
}
pub(crate) fn publish(tx: &Transaction<'_>, version: i64) -> Result<()> {
    let old = storage_version(tx)?;
    ensure!(
        (2..=23).contains(&version) && version >= old,
        "storage publication would downgrade the preserved reader"
    );
    if exists(tx)? {
        ensure!(
            version >= 18,
            "portable annotated reader requires portable base"
        );
        tx.execute(
            "UPDATE portable_annotated_reader SET underlying_version=?1 WHERE singleton=1",
            [version],
        )?;
    } else {
        crate::portable_wasm_db::publish(tx, version)?;
    }
    Ok(())
}
pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let underlying = storage_version(tx)?;
    ensure!(
        (18..=23).contains(&underlying),
        "portable annotated reader requires a supported portable base"
    );
    if !exists(tx)? {
        tx.execute_batch(CREATE)?;
        tx.execute(
            "INSERT INTO portable_annotated_reader VALUES(1,?1)",
            [underlying],
        )?;
        crate::portable_wasm_db::publish(tx, 24)?;
    }
    storage_version(tx)?;
    Ok(())
}
/// Detect stripped markers before any recovery, using SQL byte admission before JSON.
pub(crate) fn validate_metadata(db: &Connection) -> Result<()> {
    crate::portable_wasm_db::validate_metadata(db)?;
    storage_version(db)?;
    for table in ["project_import_archives", "project_package_exports"] {
        let present: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !present {
            continue;
        }
        let sql = if table == "project_import_archives" {
            "SELECT EXISTS(SELECT 1 FROM project_import_archives WHERE CASE WHEN typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(manifest) THEN 0 ELSE json_extract(manifest,'$.format_version')>=11 OR EXISTS(SELECT 1 FROM json_each(manifest,'$.imported_origins') e WHERE json_extract(e.value,'$.container.format_version')>=11) OR EXISTS(SELECT 1 FROM json_each(manifest,'$.generated_origins') e WHERE json_type(e.value,'$.annotated_model') IS NOT NULL AND json_type(e.value,'$.annotated_model')!='null') END)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM project_package_exports WHERE CASE WHEN typeof(status)!='text' OR length(CAST(status AS BLOB)) NOT BETWEEN 1 AND 1048576 OR NOT json_valid(status) THEN 0 ELSE json_extract(status,'$.artifact.format_version')>=11 END)"
        };
        let required: bool = db.query_row(sql, [], |r| r.get(0))?;
        ensure!(
            !required || exists(db)?,
            "portable annotated metadata requires its schema-24 barrier"
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_export_status_also_requires_the_barrier() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=18;CREATE TABLE project_package_exports(status TEXT);INSERT INTO project_package_exports VALUES('{\"artifact\":{\"format_version\":10}}');").unwrap();
        validate_metadata(&db).unwrap();
        db.execute(
            "UPDATE project_package_exports SET status=?1",
            ["{\"artifact\":{\"format_version\":11}}"],
        )
        .unwrap();
        assert!(validate_metadata(&db).is_err());
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        validate_metadata(&db).unwrap();
        db.execute_batch("DROP TABLE portable_annotated_reader;PRAGMA user_version=18")
            .unwrap();
        assert!(validate_metadata(&db).is_err());
    }
    #[test]
    fn wrapper_preserves_layers_and_transaction_rollback() {
        for base in 18..=23 {
            let mut db = Connection::open_in_memory().unwrap();
            db.pragma_update(None, "user_version", base).unwrap();
            {
                let tx = db.transaction().unwrap();
                arm(&tx).unwrap();
                assert_eq!(storage_version(&tx).unwrap(), base);
            }
            assert_eq!(storage_version(&db).unwrap(), base);
            assert!(!exists(&db).unwrap());
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            publish(&tx, 23).unwrap();
            assert_eq!(storage_version(&tx).unwrap(), 23);
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                24
            );
            assert!(publish(&tx, 20).is_err());
            tx.commit().unwrap();
        }
    }
    #[test]
    fn malformed_or_downgraded_wrapper_and_stripped_metadata_refuse() {
        for bad in ["PRAGMA user_version=18","DROP TABLE portable_annotated_reader","DELETE FROM portable_annotated_reader","PRAGMA ignore_check_constraints=ON;UPDATE portable_annotated_reader SET underlying_version=24","ALTER TABLE portable_annotated_reader ADD COLUMN extra INTEGER"] {
            let mut db=Connection::open_in_memory().unwrap();db.pragma_update(None,"user_version",18).unwrap();
            let tx=db.transaction().unwrap();arm(&tx).unwrap();tx.commit().unwrap();db.execute_batch(bad).unwrap();
            assert!(storage_version(&db).is_err(),"{bad}");
        }
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=18;CREATE TABLE project_import_archives(manifest TEXT);INSERT INTO project_import_archives VALUES('{\"format_version\":11}');").unwrap();
        assert!(validate_metadata(&db).is_err());
    }
}
