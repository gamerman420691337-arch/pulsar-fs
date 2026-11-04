//! Outer reader barrier for retained local batch input and phase ownership.
use anyhow::{ensure, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};
const MARKER: &str = "CREATE TABLE library_batch_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 27))";
const RECORDS: &str = "CREATE TABLE library_batches(session TEXT NOT NULL REFERENCES sessions(id),request TEXT NOT NULL,record TEXT NOT NULL,PRIMARY KEY(session,request))";
const SOURCES: &str = "CREATE TABLE library_batch_sources(session TEXT NOT NULL,request TEXT NOT NULL,item INTEGER NOT NULL CHECK(item BETWEEN 0 AND 99),sha256 TEXT NOT NULL,PRIMARY KEY(session,request,item),FOREIGN KEY(session,request) REFERENCES library_batches(session,request))";
const MODELS: &str = "CREATE TABLE library_batch_models(session TEXT NOT NULL,request TEXT NOT NULL,sha256 TEXT NOT NULL,PRIMARY KEY(session,request),FOREIGN KEY(session,request) REFERENCES library_batches(session,request))";
const PACK_DOCUMENTS: &str = "CREATE TABLE library_batch_pack_documents(session TEXT NOT NULL,request TEXT NOT NULL,item INTEGER NOT NULL CHECK(item BETWEEN 0 AND 2),sha256 TEXT NOT NULL,PRIMARY KEY(session,request,item),FOREIGN KEY(session,request) REFERENCES library_batches(session,request))";
const MIGRATIONS: &str = "CREATE TABLE library_batch_migrations(session TEXT NOT NULL,request TEXT NOT NULL,parent_request TEXT NOT NULL,parent_sha256 TEXT NOT NULL,PRIMARY KEY(session,request),UNIQUE(session,parent_request),FOREIGN KEY(session,request) REFERENCES library_batches(session,request),FOREIGN KEY(session,parent_request) REFERENCES library_batches(session,request))";
pub(crate) const MAX_RECORD: usize = 1024 * 1024;
fn exists(db: &Connection, name: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}
fn layout(db: &Connection, name: &str, sql: &str) -> Result<()> {
    let actual:Option<String>=db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[name],|r|r.get(0)).optional()?;
    ensure!(
        actual.as_deref() == Some(sql),
        "local batch {name} layout differs"
    );
    Ok(())
}
pub(crate) fn version(db: &Connection) -> Result<i64> {
    let raw = crate::inference_equivalence_reader::version(db)?;
    let marker = exists(db, "library_batch_reader")?;
    let records = exists(db, "library_batches")?;
    let sources = exists(db, "library_batch_sources")?;
    let models = exists(db, "library_batch_models")?;
    let packs = exists(db, "library_batch_pack_documents")?;
    let migrations = exists(db, "library_batch_migrations")?;
    ensure!(
        migrations == (raw >= 32),
        "local batch migration reader barrier missing or downgraded"
    );
    if migrations {
        layout(db, "library_batch_migrations", MIGRATIONS)?;
    }
    ensure!(
        packs == (raw >= 31),
        "pack batch reader barrier missing or downgraded"
    );
    ensure!(
        models == (raw >= 30),
        "visual batch reader barrier missing or downgraded"
    );
    if matches!(raw, 28..=34) || marker || records || sources {
        ensure!(
            matches!(raw, 28..=34) && marker && records && sources,
            "local batch reader barrier missing or downgraded"
        );
        layout(db, "library_batch_reader", MARKER)?;
        layout(db, "library_batches", RECORDS)?;
        layout(db, "library_batch_sources", SOURCES)?;
        if models {
            layout(db, "library_batch_models", MODELS)?;
        }
        if packs {
            layout(db, "library_batch_pack_documents", PACK_DOCUMENTS)?;
        }
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM library_batch_reader LIMIT 2)",
            [],
            |r| r.get(0),
        )?;
        ensure!(count == 1, "local batch reader singleton differs");
        return Ok(db.query_row("SELECT underlying_version FROM library_batch_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 27",[],|r|r.get(0))?);
    }
    ensure!(raw < 28, "unknown local batch reader version");
    Ok(raw)
}
pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let old = version(db)?;
    ensure!(
        (2..=27).contains(&new_version) && new_version >= old,
        "local batch reader publication would downgrade storage"
    );
    if exists(db, "library_batch_reader")? {
        db.execute(
            "UPDATE library_batch_reader SET underlying_version=?1 WHERE singleton=1",
            [new_version],
        )?;
    } else {
        crate::inference_equivalence_reader::publish(db, new_version)?;
    }
    Ok(())
}
pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let old = version(tx)?;
    ensure!(
        (2..=27).contains(&old),
        "local batch requires supported storage"
    );
    if !exists(tx, "library_batch_reader")? {
        tx.execute_batch(MARKER)?;
        tx.execute_batch(RECORDS)?;
        tx.execute_batch(SOURCES)?;
        tx.execute("INSERT INTO library_batch_reader VALUES(1,?1)", [old])?;
        crate::inference_equivalence_reader::publish(tx, 28)?;
    }
    capacity(tx)?;
    Ok(())
}
pub(crate) fn arm_cancel(tx: &Transaction<'_>) -> Result<()> {
    arm(tx)?;
    let raw = crate::inference_equivalence_reader::version(tx)?;
    if raw < 29 {
        crate::inference_equivalence_reader::publish(tx, 29)?;
    }
    Ok(())
}
pub(crate) fn arm_visual(tx: &Transaction<'_>) -> Result<()> {
    arm(tx)?;
    if !exists(tx, "library_batch_models")? {
        tx.execute_batch(MODELS)?;
    }
    let raw = crate::inference_equivalence_reader::version(tx)?;
    if raw < 30 {
        crate::inference_equivalence_reader::publish(tx, 30)?;
    }
    Ok(())
}
pub(crate) fn arm_pack(tx: &Transaction<'_>) -> Result<()> {
    arm_visual(tx)?;
    if !exists(tx, "library_batch_pack_documents")? {
        tx.execute_batch(PACK_DOCUMENTS)?;
    }
    let raw = crate::inference_equivalence_reader::version(tx)?;
    if raw < 31 {
        crate::inference_equivalence_reader::publish(tx, 31)?;
    }
    Ok(())
}
pub(crate) fn arm_migration(tx: &Transaction<'_>) -> Result<()> {
    arm_pack(tx)?;
    if !exists(tx, "library_batch_migrations")? {
        tx.execute_batch(MIGRATIONS)?;
    }
    let raw = crate::inference_equivalence_reader::version(tx)?;
    if raw < 32 {
        crate::inference_equivalence_reader::publish(tx, 32)?;
    }
    Ok(())
}
pub(crate) fn arm_selected_audio(tx: &Transaction<'_>) -> Result<()> {
    arm_migration(tx)?;
    let raw = crate::inference_equivalence_reader::version(tx)?;
    if raw < 33 {
        crate::inference_equivalence_reader::publish(tx, 33)?;
    }
    Ok(())
}
pub(crate) fn arm_partial_audio(tx: &Transaction<'_>) -> Result<()> {
    arm_selected_audio(tx)?;
    crate::inference_equivalence_reader::publish(tx, 34)?;
    Ok(())
}
pub(crate) fn capacity(db: &Connection) -> Result<()> {
    version(db)?;
    if !exists(db, "library_batches")? {
        return Ok(());
    }
    let (count,total):(i64,i64)=db.query_row("SELECT count(*),COALESCE(sum(length(CAST(record AS BLOB))),0) FROM (SELECT record FROM library_batches LIMIT 129)",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        count <= 128 && total <= 32 * 1024 * 1024,
        "local batch ledger capacity exceeded"
    );
    let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM library_batches WHERE typeof(session)!='text' OR length(CAST(session AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(request)!='text' OR length(CAST(request AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(record)!='text' OR length(CAST(record AS BLOB)) NOT BETWEEN 1 AND 1048576) OR EXISTS(SELECT 1 FROM library_batch_sources WHERE typeof(session)!='text' OR typeof(request)!='text' OR typeof(item)!='integer' OR item NOT BETWEEN 0 AND 99 OR typeof(sha256)!='text' OR length(CAST(sha256 AS BLOB))!=64)",[],|r|r.get(0))?;
    ensure!(!invalid, "local batch scalar admission differs");
    let orphan:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM library_batches b LEFT JOIN sessions s ON b.session=s.id WHERE s.id IS NULL) OR EXISTS(SELECT 1 FROM library_batch_sources s LEFT JOIN library_batches b ON s.session=b.session AND s.request=b.request WHERE b.request IS NULL)",[],|r|r.get(0))?;
    ensure!(!orphan, "local batch owner/source membership missing");
    let count: i64 = db.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM library_batch_sources LIMIT 12801)",
        [],
        |r| r.get(0),
    )?;
    ensure!(count <= 12800, "local batch source capacity exceeded");
    if exists(db, "library_batch_models")? {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM library_batch_models LIMIT 129)",
            [],
            |r| r.get(0),
        )?;
        let invalid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM library_batch_models m LEFT JOIN library_batches b ON m.session=b.session AND m.request=b.request WHERE b.request IS NULL OR typeof(m.session)!='text' OR typeof(m.request)!='text' OR typeof(m.sha256)!='text' OR length(CAST(m.sha256 AS BLOB))!=64 OR m.sha256 GLOB '*[^0-9a-f]*')", [], |r| r.get(0))?;
        ensure!(
            count <= 128 && !invalid,
            "local batch model membership admission differs"
        );
    }

    if exists(db, "library_batch_pack_documents")? {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM library_batch_pack_documents LIMIT 385)",
            [],
            |r| r.get(0),
        )?;
        let invalid:bool = db.query_row("SELECT EXISTS(SELECT 1 FROM library_batch_pack_documents d LEFT JOIN library_batches b ON d.session=b.session AND d.request=b.request WHERE b.request IS NULL OR typeof(d.session)!='text' OR typeof(d.request)!='text' OR typeof(d.item)!='integer' OR d.item NOT BETWEEN 0 AND 2 OR typeof(d.sha256)!='text' OR length(CAST(d.sha256 AS BLOB))!=64 OR d.sha256 GLOB '*[^0-9a-f]*')",[],|r|r.get(0))?;
        ensure!(
            count <= 384 && !invalid,
            "pack batch document capacity/admission differs"
        );
    }

    if exists(db, "library_batch_migrations")? {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM library_batch_migrations LIMIT 129)",
            [],
            |r| r.get(0),
        )?;
        let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM library_batch_migrations m LEFT JOIN library_batches b ON m.session=b.session AND m.request=b.request LEFT JOIN library_batches p ON m.session=p.session AND m.parent_request=p.request WHERE b.request IS NULL OR p.request IS NULL OR m.request=m.parent_request OR typeof(m.session)!='text' OR typeof(m.request)!='text' OR typeof(m.parent_request)!='text' OR typeof(m.parent_sha256)!='text' OR length(CAST(m.parent_sha256 AS BLOB))!=64 OR m.parent_sha256 GLOB '*[^0-9a-f]*')", [], |r|r.get(0))?;
        ensure!(
            count <= 128 && !invalid,
            "local batch migration membership differs"
        );
    }
    Ok(())
}
pub(crate) fn is_retained_source(db: &Connection, sha: &str) -> Result<bool> {
    version(db)?;
    if !exists(db, "library_batch_sources")? {
        return Ok(false);
    }
    if exists(db, "library_batch_pack_documents")?
        && db.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_batch_pack_documents WHERE sha256=?1)",
            [sha],
            |r| r.get::<_, bool>(0),
        )?
    {
        return Ok(true);
    }
    if exists(db, "library_batch_models")?
        && db.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_batch_models WHERE sha256=?1)",
            [sha],
            |r| r.get::<_, bool>(0),
        )?
    {
        return Ok(true);
    }
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM library_batch_sources WHERE sha256=?1)",
        [sha],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=2;CREATE TABLE sessions(id TEXT PRIMARY KEY);INSERT INTO sessions VALUES('owner');").unwrap();
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        db
    }
    #[test]
    fn visual_barrier_preserves_model_retention_and_cannot_be_downgraded_by_cancel() {
        let mut db = db();
        let tx = db.transaction().unwrap();
        arm_visual(&tx).unwrap();
        tx.commit().unwrap();
        db.execute(
            "INSERT INTO library_batches VALUES('owner','visual','{}')",
            [],
        )
        .unwrap();
        let digest = "a".repeat(64);
        db.execute(
            "INSERT INTO library_batch_models VALUES('owner','visual',?1)",
            [&digest],
        )
        .unwrap();
        assert!(is_retained_source(&db, &digest).unwrap());
        capacity(&db).unwrap();
        let tx = db.transaction().unwrap();
        arm_cancel(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            30
        );
        db.execute_batch("PRAGMA user_version=29").unwrap();
        assert!(version(&db).is_err());
        db.execute_batch(
            "PRAGMA user_version=30;PRAGMA foreign_keys=OFF;DELETE FROM library_batches",
        )
        .unwrap();
        assert!(capacity(&db).is_err());
    }
    #[test]
    fn pack_documents_stay_pinned_and_reader_floor_survives_visual_and_cancel() {
        let mut db = db();
        let tx = db.transaction().unwrap();
        arm_pack(&tx).unwrap();
        tx.commit().unwrap();
        db.execute(
            "INSERT INTO library_batches VALUES('owner','pack','{}')",
            [],
        )
        .unwrap();
        let digest = "c".repeat(64);
        db.execute(
            "INSERT INTO library_batch_pack_documents VALUES('owner','pack',0,?1)",
            [&digest],
        )
        .unwrap();
        assert!(is_retained_source(&db, &digest).unwrap());
        capacity(&db).unwrap();
        let tx = db.transaction().unwrap();
        arm_visual(&tx).unwrap();
        arm_cancel(&tx).unwrap();
        publish(&tx, 20).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            31
        );
        db.execute_batch("PRAGMA user_version=30").unwrap();
        assert!(version(&db).is_err());
        db.execute_batch(
            "PRAGMA user_version=31;PRAGMA foreign_keys=OFF;DELETE FROM library_batches",
        )
        .unwrap();
        assert!(capacity(&db).is_err());
    }
    #[test]
    fn outer_barrier_survives_inner_publication_and_refuses_downgrade() {
        let db = db();
        assert_eq!(version(&db).unwrap(), 2);
        publish(&db, 20).unwrap();
        assert_eq!(version(&db).unwrap(), 20);
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            28
        );
        assert!(publish(&db, 19).is_err());
        db.execute_batch("PRAGMA user_version=27;").unwrap();
        assert!(version(&db).is_err());
    }
    #[test]
    fn scalar_admission_refuses_oversized_record_and_orphan_source_before_decode() {
        let db = db();
        db.execute(
            "INSERT INTO library_batches VALUES('owner','batch',?1)",
            ["x".repeat(MAX_RECORD + 1)],
        )
        .unwrap();
        assert!(capacity(&db).is_err());
        db.execute("DELETE FROM library_batches", []).unwrap();
        db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        db.execute(
            "INSERT INTO library_batch_sources VALUES('owner','absent',0,?1)",
            ["a".repeat(64)],
        )
        .unwrap();
        assert!(capacity(&db).is_err());
    }
    #[test]
    fn migration_reader_cannot_be_downgraded_by_pack_cancel_or_missing_lineage_table() {
        let mut db = db();
        let tx = db.transaction().unwrap();
        arm_migration(&tx).unwrap();
        arm_pack(&tx).unwrap();
        arm_cancel(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            32
        );
        capacity(&db).unwrap();
        db.execute_batch("PRAGMA user_version=31").unwrap();
        assert!(version(&db).is_err());
        db.execute_batch("PRAGMA user_version=32;DROP TABLE library_batch_migrations")
            .unwrap();
        assert!(version(&db).is_err());
    }

    #[test]
    fn equivalence_wrapper_preserves_batch_floors_and_all_feature_upgrades() {
        for with_batch in [false, true] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch("PRAGMA user_version=20;CREATE TABLE sessions(id TEXT PRIMARY KEY);INSERT INTO sessions VALUES('owner');").unwrap();
            let tx = db.transaction().unwrap();
            if with_batch {
                arm(&tx).unwrap();
            }
            crate::inference_equivalence_reader::arm(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&db).unwrap(),
                if with_batch { 28 } else { 20 }
            );
            assert_eq!(version(&db).unwrap(), 20);
            capacity(&db).unwrap();
            for table in [
                "library_batch_models",
                "library_batch_pack_documents",
                "library_batch_migrations",
            ] {
                assert!(!exists(&db, table).unwrap());
            }
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                28
            );
            arm_cancel(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                29
            );
            arm_visual(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                30
            );
            arm_pack(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                31
            );
            arm_migration(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                32
            );
            arm_selected_audio(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                33
            );
            arm_partial_audio(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                34
            );
            arm_cancel(&tx).unwrap();
            arm_visual(&tx).unwrap();
            arm_pack(&tx).unwrap();
            assert_eq!(
                crate::inference_equivalence_reader::version(&tx).unwrap(),
                34
            );
            publish(&tx, 21).unwrap();
            assert_eq!(version(&tx).unwrap(), 21);
            assert!(publish(&tx, 20).is_err());
            capacity(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                35
            );
        }
    }

    #[test]
    fn equivalence_wrapper_composes_with_portable_archive_and_inner_publication() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=18;CREATE TABLE sessions(id TEXT PRIMARY KEY);")
            .unwrap();
        let tx = db.transaction().unwrap();
        crate::portable_annotated_db::arm(&tx).unwrap();
        crate::portable_wasm_db::arm(&tx).unwrap();
        crate::wasm_archive_db::arm(&tx).unwrap();
        arm(&tx).unwrap();
        crate::inference_equivalence_reader::arm(&tx).unwrap();
        crate::portable_annotated_db::publish(&tx, 20).unwrap();
        arm_partial_audio(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            crate::inference_equivalence_reader::version(&db).unwrap(),
            34
        );
        assert_eq!(version(&db).unwrap(), 26);
        assert_eq!(crate::native_preparation_db::version(&db).unwrap(), 26);
        assert_eq!(crate::wasm_archive_db::version(&db).unwrap(), 25);
        assert_eq!(crate::portable_wasm_db::storage_version(&db).unwrap(), 24);
        assert_eq!(
            crate::portable_annotated_db::storage_version(&db).unwrap(),
            20
        );
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 35).unwrap(),
            20
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            35
        );
        let tx = db.transaction().unwrap();
        assert!(crate::portable_annotated_db::publish(&tx, 18).is_err());
        assert!(publish(&tx, 25).is_err());
        tx.rollback().unwrap();
        db.execute("DELETE FROM portable_wasm_reader", []).unwrap();
        assert!(crate::live_cuda_evidence_db::base_version(&db, 35).is_err());
    }
}
