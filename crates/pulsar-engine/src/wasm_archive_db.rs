//! Conditional reader barrier for archived completed invocation custody.
use anyhow::{ensure, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};
const CREATE:&str="CREATE TABLE wasm_archive_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 25))";
fn exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='wasm_archive_reader')",[],|r|r.get(0))?)
}
pub(crate) fn version(db: &Connection) -> Result<i64> {
    let raw: i64 = crate::native_preparation_db::version(db)?;
    let present = exists(db)?;
    if raw == 26 || present {
        ensure!(
            raw == 26 && present,
            "WASM archive reader barrier missing or downgraded"
        );
        let sql:Option<String>=db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='wasm_archive_reader' AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[],|r|r.get(0)).optional()?;
        ensure!(
            sql.as_deref() == Some(CREATE),
            "WASM archive reader layout differs"
        );
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM wasm_archive_reader LIMIT 2)",
            [],
            |r| r.get(0),
        )?;
        ensure!(count == 1, "WASM archive reader singleton differs");
        return Ok(db.query_row("SELECT underlying_version FROM wasm_archive_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 25",[],|r|r.get(0))?);
    }
    ensure!(raw < 26, "unknown WASM archive reader version");
    Ok(raw)
}
pub(crate) fn publish(db: &Connection, version: i64) -> Result<()> {
    let old = self::version(db)?;
    ensure!(
        (2..=25).contains(&version) && version >= old,
        "WASM archive reader publication would downgrade storage"
    );
    if exists(db)? {
        db.execute(
            "UPDATE wasm_archive_reader SET underlying_version=?1 WHERE singleton=1",
            [version],
        )?;
    } else {
        crate::native_preparation_db::publish(db, version)?;
    }
    Ok(())
}
pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let underlying = version(tx)?;
    ensure!(
        (2..=25).contains(&underlying),
        "WASM archive requires supported storage"
    );
    if !exists(tx)? {
        tx.execute_batch(CREATE)?;
        tx.execute("INSERT INTO wasm_archive_reader VALUES(1,?1)", [underlying])?;
        crate::native_preparation_db::publish(tx, 26)?;
    }
    version(tx)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_barrier_preserves_portable_wasm_underlying_version_without_downgrade() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 18).unwrap();
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        crate::portable_wasm_db::arm(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(version(&db).unwrap(), 25);
        assert_eq!(crate::portable_wasm_db::storage_version(&db).unwrap(), 18);
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 26).unwrap(),
            18
        );
        let tx = db.transaction().unwrap();
        crate::portable_wasm_db::publish(&tx, 24).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            26
        );
        assert_eq!(crate::portable_wasm_db::storage_version(&db).unwrap(), 24);
        assert!(publish(&db, 18).is_err());
    }
    #[test]
    fn stripped_downgraded_future_and_malformed_archive_readers_refuse_readonly() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 18).unwrap();
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        db.pragma_update(None, "user_version", 25).unwrap();
        assert!(version(&db).is_err());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            25
        );
        db.pragma_update(None, "user_version", 26).unwrap();
        db.execute_batch("DROP TABLE wasm_archive_reader").unwrap();
        assert!(version(&db).is_err());
        db.pragma_update(None, "user_version", 27).unwrap();
        assert!(version(&db).is_err());
    }
}
