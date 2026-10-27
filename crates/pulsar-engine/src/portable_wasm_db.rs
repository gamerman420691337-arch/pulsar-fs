//! Conditional portable wasm reader barrier, preserving the annotated and preceding layers.
use anyhow::{ensure, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};
const TABLE: &str = "portable_wasm_reader";
const CREATE:&str="CREATE TABLE portable_wasm_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 18 AND 24))";
fn exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [TABLE],
        |r| r.get(0),
    )?)
}
pub(crate) fn storage_version(db: &Connection) -> Result<i64> {
    let raw: i64 = crate::wasm_archive_db::version(db)?;
    let present = exists(db)?;
    if raw == 25 || present {
        ensure!(
            raw == 25 && present,
            "portable wasm reader barrier missing or downgraded"
        );
        let sql:Option<String>=db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[TABLE],|r|r.get(0)).optional()?;
        ensure!(
            sql.as_deref() == Some(CREATE),
            "portable wasm reader layout differs"
        );
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM portable_wasm_reader LIMIT 2)",
            [],
            |r| r.get(0),
        )?;
        ensure!(count == 1, "portable wasm reader singleton differs");
        let version:i64=db.query_row("SELECT underlying_version FROM portable_wasm_reader WHERE typeof(singleton)='integer' AND singleton=1 AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 18 AND 24",[],|r|r.get(0))?;
        return Ok(version);
    }
    ensure!(raw < 25, "unknown portable wasm reader version");
    Ok(raw)
}
pub(crate) fn publish(tx: &Transaction<'_>, version: i64) -> Result<()> {
    let old = storage_version(tx)?;
    ensure!(
        (2..=24).contains(&version) && version >= old,
        "storage publication would downgrade the preserved reader"
    );
    if exists(tx)? {
        ensure!(version >= 18, "portable wasm reader requires portable base");
        tx.execute(
            "UPDATE portable_wasm_reader SET underlying_version=?1 WHERE singleton=1",
            [version],
        )?;
    } else {
        crate::wasm_archive_db::publish(tx, version)?;
    }
    Ok(())
}
pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let underlying = storage_version(tx)?;
    ensure!(
        (18..=24).contains(&underlying),
        "portable wasm reader requires a supported portable base"
    );
    if !exists(tx)? {
        tx.execute_batch(CREATE)?;
        tx.execute(
            "INSERT INTO portable_wasm_reader VALUES(1,?1)",
            [underlying],
        )?;
        crate::wasm_archive_db::publish(tx, 25)?;
    }
    storage_version(tx)?;
    Ok(())
}
/// Detect stripped markers before any recovery, using SQL byte admission before JSON.
pub(crate) fn validate_metadata(db: &Connection) -> Result<()> {
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
            "SELECT EXISTS(SELECT 1 FROM project_import_archives WHERE CASE WHEN typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(manifest) THEN 0 ELSE json_extract(manifest,'$.format_version')>=12 OR EXISTS(SELECT 1 FROM json_each(manifest,'$.imported_origins') e WHERE json_extract(e.value,'$.container.format_version')>=12) OR EXISTS(SELECT 1 FROM json_each(manifest,'$.authored_lineage') e WHERE json_type(e.value,'$.wasm_invocation') IS NOT NULL AND json_type(e.value,'$.wasm_invocation')!='null') END)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM project_package_exports WHERE CASE WHEN typeof(status)!='text' OR length(CAST(status AS BLOB)) NOT BETWEEN 1 AND 1048576 OR NOT json_valid(status) THEN 0 ELSE json_extract(status,'$.artifact.format_version')>=12 END)"
        };
        let required: bool = db.query_row(sql, [], |r| r.get(0))?;
        ensure!(
            !required || exists(db)?,
            "portable wasm metadata requires its schema-25 barrier"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapper_preserves_annotated_and_underlying_publication() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=18").unwrap();
        let tx = db.transaction().unwrap();
        crate::portable_annotated_db::arm(&tx).unwrap();
        arm(&tx).unwrap();
        crate::portable_annotated_db::publish(&tx, 20).unwrap();
        tx.commit().unwrap();
        assert_eq!(storage_version(&db).unwrap(), 24);
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 25).unwrap(),
            20
        );
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 24).unwrap(),
            20
        );
        assert_eq!(
            crate::portable_annotated_db::storage_version(&db).unwrap(),
            20
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            25
        );
        let tx = db.transaction().unwrap();
        assert!(publish(&tx, 23).is_err());
        tx.rollback().unwrap();
        db.execute_batch("PRAGMA user_version=24").unwrap();
        assert!(storage_version(&db).is_err());
    }
    #[test]
    fn stripped_markers_and_unknown_layout_refuse_without_writes() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=18;CREATE TABLE project_import_archives(manifest TEXT);INSERT INTO project_import_archives VALUES('{\"format_version\":12}');").unwrap();
        let before = db.total_changes();
        assert!(validate_metadata(&db).is_err());
        assert_eq!(db.total_changes(), before);
        let tx = db.transaction().unwrap();
        crate::portable_annotated_db::arm(&tx).unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        validate_metadata(&db).unwrap();
        db.execute_batch("DROP TABLE portable_wasm_reader;PRAGMA user_version=24;")
            .unwrap();
        assert!(validate_metadata(&db).is_err());
        db.execute_batch("CREATE TABLE portable_wasm_reader(singleton INTEGER,underlying_version INTEGER);INSERT INTO portable_wasm_reader VALUES(1,24);PRAGMA user_version=25").unwrap();
        assert!(storage_version(&db).is_err());
    }
    #[test]
    fn nested_origins_and_field_presence_require_the_barrier() {
        for record in [
            "{\"format_version\":1,\"imported_origins\":[{\"container\":{\"format_version\":12}}]}",
            "{\"format_version\":1,\"authored_lineage\":[{\"wasm_invocation\":{}}]}",
        ] {
            let db = Connection::open_in_memory().unwrap();
            db.execute_batch(
                "PRAGMA user_version=18;CREATE TABLE project_import_archives(manifest TEXT)",
            )
            .unwrap();
            db.execute("INSERT INTO project_import_archives VALUES(?1)", [record])
                .unwrap();
            assert!(validate_metadata(&db).is_err());
        }
    }
}
