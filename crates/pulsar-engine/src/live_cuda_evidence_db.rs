//! Atomic original live CUDA evidence and its conditional older-reader barrier.
//! Callers supply current Engine authority; this store checks durable consistency.
use crate::{
    live_cuda_evidence::{EvidenceAdmission, LiveCudaEvidence},
    resources::ResourcePool,
};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

const TABLE: &str = "live_cuda_evidence";
pub(crate) const CREATE: &str = "CREATE TABLE live_cuda_evidence(attempt TEXT PRIMARY KEY NOT NULL,project TEXT NOT NULL REFERENCES projects(id),epoch TEXT UNIQUE NOT NULL,source TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0),manifest BLOB NOT NULL,summary BLOB NOT NULL,receipt BLOB NOT NULL,trace BLOB NOT NULL,FOREIGN KEY(project,source) REFERENCES sources(project,version))";
const MAX_ROWS: i64 = 256;
const MAX_BYTES: i64 = 256 * 1024 * 1024;

fn exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [TABLE],
        |r| r.get(0),
    )?)
}
pub(crate) fn shape(db: &Connection) -> Result<(i64, i64)> {
    let sql: Option<String> = db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096", [TABLE], |r| r.get(0)).optional()?;
    ensure!(
        sql.as_deref() == Some(CREATE),
        "live CUDA evidence layout differs"
    );
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM live_cuda_evidence LIMIT 257)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count <= MAX_ROWS,
        "live CUDA evidence row capacity exceeded"
    );
    let invalid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM live_cuda_evidence WHERE typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(project)!='text' OR length(CAST(project AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(epoch)!='text' OR length(CAST(epoch AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(source)!='text' OR length(CAST(source AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(revision)!='integer' OR revision<0 OR typeof(manifest)!='blob' OR length(manifest) NOT BETWEEN 1 AND 1048576 OR typeof(summary)!='blob' OR length(summary) NOT BETWEEN 1 AND 4096 OR typeof(receipt)!='blob' OR length(receipt) NOT BETWEEN 1 AND 65536 OR typeof(trace)!='blob' OR length(trace) NOT BETWEEN 1 AND 8388608)", [], |r| r.get(0))?;
    ensure!(
        !invalid,
        "live CUDA evidence scalar or byte admission differs"
    );
    let total: i64 = db.query_row("SELECT COALESCE(SUM(length(manifest)+length(summary)+length(receipt)+length(trace)),0) FROM live_cuda_evidence", [], |r| r.get(0))?;
    ensure!(
        total <= MAX_BYTES,
        "live CUDA evidence aggregate capacity exceeded"
    );
    Ok((count, total))
}
/// Schema 21 extends schema 20 without changing any of its layouts. Delegate
/// those layouts using 20 only after validating the actual new reader barrier.
/// A table under a downgraded header, or a future schema, is always refused.
pub(crate) fn base_version(db: &Connection, expected: i64) -> Result<i64> {
    let expected = if expected == 42 {
        ensure!(
            db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))? == 42,
            "browser fallback delegation header differs"
        );
        crate::browser_fallback_reader::version(db)?
    } else {
        expected
    };
    let expected = if expected == 41 {
        ensure!(
            crate::browser_fallback_reader::version(db)? == 41,
            "fallback graph delegation header differs"
        );
        crate::portable_fallback_graph_reader::version(db)?
    } else {
        expected
    };
    let expected = if expected == 40 {
        ensure!(
            crate::portable_fallback_graph_reader::version(db)? == 40,
            "portable fallback delegation header differs"
        );
        crate::portable_fallback_reader::version(db)?
    } else {
        expected
    };
    let expected = if expected == 39 {
        ensure!(
            crate::portable_fallback_reader::version(db)? == 39,
            "portable CPU delegation header differs"
        );
        crate::portable_cpu_archives::version(db)?
    } else {
        expected
    };
    let expected = if expected == 38 {
        ensure!(
            crate::portable_cpu_archives::version(db)? == 38,
            "captured CPU live delegation header differs"
        );
        crate::live_cpu_reader::version(db)?
    } else {
        expected
    };
    let expected = if expected == 37 {
        ensure!(
            crate::live_cpu_reader::version(db)? == 37,
            "inference fallback delegation header differs"
        );
        crate::inference_fallback_reader::version(db)?
    } else {
        expected
    };
    let expected = if expected == 36 {
        ensure!(
            crate::inference_fallback_reader::version(db)? == 36,
            "closed CPU runtime delegation header differs"
        );
        crate::cpu_runtime_reader::version(db)?
    } else {
        expected
    };
    let expected = if expected == 35 {
        ensure!(
            crate::cpu_runtime_reader::version(db)? == 35,
            "inference equivalence delegation header differs"
        );
        crate::inference_equivalence_reader::version(db)?
    } else {
        expected
    };
    let stored = crate::portable_annotated_db::storage_version(db)?;
    let expected = if matches!(expected, 28..=34) {
        crate::library_batch_db::version(db)?
    } else {
        expected
    };
    let expected = if expected == 27 {
        ensure!(
            crate::library_batch_db::version(db)? == 27,
            "native preparation delegation header differs"
        );
        crate::native_preparation_db::version(db)?
    } else {
        expected
    };
    let expected = if expected == 26 {
        ensure!(
            crate::native_preparation_db::version(db)? == 26,
            "WASM archive delegation header differs"
        );
        crate::wasm_archive_db::version(db)?
    } else {
        expected
    };
    let expected = if expected == 25 {
        ensure!(
            crate::wasm_archive_db::version(db)? == 25,
            "portable WASM delegation header differs"
        );
        crate::portable_wasm_db::storage_version(db)?
    } else {
        expected
    };
    let expected = if expected == 24 {
        ensure!(
            crate::portable_wasm_db::storage_version(db)? == 24,
            "portable annotated delegation header differs"
        );
        stored
    } else {
        expected
    };
    let actual = crate::annotated_generation_db::storage_version(db)?;
    if stored == 23 {
        ensure!(
            expected == 23 || (matches!(expected, 20..=22) && expected <= actual),
            "annotated generation delegation requires its preserved storage version"
        );
    }
    let expected = if expected == 23 {
        ensure!(stored == 23, "annotated generation reader header differs");
        actual
    } else {
        expected
    };
    let present = exists(db)?;
    let capture_present = crate::live_cuda_capture_db::exists(db)?;
    if actual >= 21 || expected >= 21 || present || capture_present {
        ensure!(
            matches!(actual, 21 | 22)
                && matches!(expected, 20..=22)
                && expected <= actual
                && present,
            "live CUDA evidence requires its exact reader barrier"
        );
        ensure!(
            capture_present == (actual == 22),
            "live CUDA capture evidence reader barrier differs"
        );
        capacity(db)?;
        return Ok(20);
    }
    Ok(expected)
}
pub(crate) fn capacity(db: &Connection) -> Result<(i64, i64)> {
    let (mut count, mut total) = shape(db)?;
    if crate::live_cuda_capture_db::exists(db)? {
        let extra = crate::live_cuda_capture_db::shape(db)?;
        count += extra.0;
        total += extra.1;
        ensure!(
            count <= MAX_ROWS && total <= MAX_BYTES,
            "combined live CUDA evidence capacity exceeded"
        );
    }
    Ok((count, total))
}
fn membership(db: &Connection, evidence: &LiveCudaEvidence, current: bool) -> Result<()> {
    let identity = &evidence.manifest().stream.identity;
    let revision = i64::try_from(identity.revision.value())?;
    let present: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM projects p JOIN sources s ON s.project=p.id WHERE p.id=?1 AND s.version=?2 AND typeof(p.revision)='integer' AND p.revision>=?3 AND (?4=0 OR p.revision=?3))", params![identity.project.as_str(),identity.source.as_str(),revision,current], |r| r.get(0))?;
    ensure!(
        present,
        "live CUDA evidence project/source/revision membership differs"
    );
    Ok(())
}
/// Read-only, including all blob parsing; invoke before ledger recovery or WAL.
pub(crate) fn validate(db: &Connection, version: i64, pool: &ResourcePool) -> Result<()> {
    base_version(db, version)?;
    if !exists(db)? {
        return Ok(());
    }
    let binding = crate::worker_fence_db::binding(db, 21)?
        .context("live CUDA evidence ownership binding missing")?;
    ensure!(
        binding.minimum_ledger_version == 4,
        "live CUDA evidence requires creation custody"
    );
    crate::cuda_attempts::validate(db, 21)?;
    crate::cuda_acceptance::validate(db, 21)?;
    crate::live_cuda_capture_db::validate(db, pool)?;
    if shape(db)?.0 == 0 {
        return Ok(());
    }
    let mut statement = db.prepare("SELECT attempt,project,epoch,source,revision,manifest,summary,receipt,trace FROM live_cuda_evidence ORDER BY attempt")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        // No Rust blob allocation may precede this reservation.
        let admission = EvidenceAdmission::reserve(pool)?;
        let evidence = admission.read(row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)?;
        let stream = &evidence.manifest().stream;
        ensure!(
            row.get::<_, String>(0)? == stream.attempt_id.as_str()
                && row.get::<_, String>(1)? == stream.identity.project.as_str()
                && row.get::<_, String>(2)? == stream.identity.epoch.as_str()
                && row.get::<_, String>(3)? == stream.identity.source.as_str()
                && row.get::<_, i64>(4)? == i64::try_from(stream.identity.revision.value())?,
            "live CUDA evidence indexed identity differs from canonical bytes"
        );
        membership(db, &evidence, false)?;
    }
    Ok(())
}
/// The authorized Engine caller must retain evidence/storage admission through
/// outer transaction commit, and recheck grants, lease, fence and active attempt.
pub(crate) fn retain(tx: &Transaction<'_>, evidence: &LiveCudaEvidence) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_live_cuda_evidence;")?;
    let result = (|| -> Result<()> {
        let actual = crate::annotated_generation_db::storage_version(tx)?;
        ensure!(
            matches!(actual, 20..=22),
            "live CUDA evidence requires creation-custody schema"
        );
        base_version(tx, actual)?;
        let binding = crate::worker_fence_db::binding(tx, actual)?
            .context("live CUDA evidence ownership binding missing")?;
        ensure!(
            binding.minimum_ledger_version == 4,
            "live CUDA evidence requires creation custody"
        );
        crate::cuda_attempts::validate(tx, actual)?;
        crate::cuda_acceptance::validate(tx, actual)?;
        ensure!(
            evidence.storage_reserved(),
            "live CUDA evidence storage was not admitted"
        );
        membership(tx, evidence, true)?;
        let (count, total) = if actual >= 21 { capacity(tx)? } else { (0, 0) };
        ensure!(
            count < MAX_ROWS && evidence.byte_len() <= (MAX_BYTES - total) as u64,
            "live CUDA evidence publication capacity exhausted"
        );
        if actual == 20 {
            tx.execute_batch(CREATE)?;
        }
        let stream = &evidence.manifest().stream;
        if actual == 22 {
            let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM live_cuda_capture_evidence WHERE attempt=?1 OR epoch=?2)",params![stream.attempt_id.as_str(),stream.identity.epoch.as_str()],|r|r.get(0))?;
            ensure!(
                !duplicate,
                "legacy CUDA evidence conflicts with retained capture scope"
            );
        }
        tx.execute(
            "INSERT INTO live_cuda_evidence VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                stream.attempt_id.as_str(),
                stream.identity.project.as_str(),
                stream.identity.epoch.as_str(),
                stream.identity.source.as_str(),
                i64::try_from(stream.identity.revision.value())?,
                evidence.manifest_bytes(),
                evidence.summary_bytes(),
                evidence.receipt(),
                evidence.trace()
            ],
        )?;
        if actual == 20 {
            crate::annotated_generation_db::publish_storage_version(tx, 21)?;
        }
        base_version(tx, 21)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_live_cuda_evidence;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch(
                "ROLLBACK TO pulsar_live_cuda_evidence; RELEASE pulsar_live_cuda_evidence;",
            )?;
            Err(error)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn evidence(pool: &ResourcePool) -> LiveCudaEvidence {
        let admission = EvidenceAdmission::reserve(pool).unwrap();
        let (manifest, summary, receipt, trace) = crate::live_cuda_evidence::tests::original();
        admission
            .finished(&manifest, &summary, receipt, trace)
            .unwrap()
    }
    pub(crate) fn fixture(evidence: &LiveCudaEvidence) -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE projects(id TEXT PRIMARY KEY, revision INTEGER); CREATE TABLE sources(project TEXT,version TEXT,PRIMARY KEY(project,version)); CREATE TABLE jobs(id TEXT PRIMARY KEY, attempt TEXT, manifest TEXT, state TEXT,candidate TEXT,project TEXT,base_revision INTEGER,source TEXT); CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT,manifest TEXT,state TEXT); PRAGMA user_version=14;").unwrap();
        let tx = db.transaction().unwrap();
        crate::worker_fence_db::arm_creation(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
        tx.commit().unwrap();
        let identity = &evidence.manifest().stream.identity;
        db.execute(
            "INSERT INTO projects VALUES(?1,?2)",
            params![identity.project.as_str(), identity.revision.value()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sources VALUES(?1,?2)",
            params![identity.project.as_str(), identity.source.as_str()],
        )
        .unwrap();
        db
    }
    #[test]
    fn annotated_barrier_preserves_original_live_bytes_and_later_cuda_publication() {
        let pool = ResourcePool::new(3 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES, 2);
        let mut evidence = evidence(&pool);
        evidence.reserve_storage(u64::MAX).unwrap();
        // Arm schema 23 first, then publish the legacy CUDA evidence beneath it.
        let mut db = fixture(&evidence);
        db.execute_batch("ALTER TABLE sources ADD COLUMN sha256 TEXT; ALTER TABLE sources ADD COLUMN bytes INTEGER;").unwrap();
        let request = crate::annotated_generation_db::tests::request();
        {
            let tx = db.transaction().unwrap();
            crate::annotated_generation_db::tests::seed(&tx, &request);
            crate::annotated_generation_db::retain(&tx, &request, &pool).unwrap();
            assert_eq!(
                crate::annotated_generation_db::storage_version(&tx).unwrap(),
                20
            );
            retain(&tx, &evidence).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            23
        );
        assert_eq!(
            crate::annotated_generation_db::storage_version(&db).unwrap(),
            21
        );
        crate::annotated_generation_db::validate(&db, &pool).unwrap();
        validate(&db, 23, &pool).unwrap();
        let bytes: (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = db
            .query_row(
                "SELECT manifest,summary,receipt,trace FROM live_cuda_evidence",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            bytes,
            (
                evidence.manifest_bytes().to_vec(),
                evidence.summary_bytes().to_vec(),
                evidence.receipt().to_vec(),
                evidence.trace().to_vec()
            )
        );
    }
    #[test]
    fn conditional_barrier_commits_original_bytes_and_preserves_old_writers() {
        let pool = ResourcePool::new(2 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES, 1);
        let mut evidence = evidence(&pool);
        let mut db = fixture(&evidence);
        validate(&db, 20, &pool).unwrap();
        assert!(!exists(&db).unwrap());
        evidence.reserve_storage(u64::MAX).unwrap();
        {
            let tx = db.transaction().unwrap();
            retain(&tx, &evidence).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            21
        );
        let raw: (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = db
            .query_row(
                "SELECT manifest,summary,receipt,trace FROM live_cuda_evidence",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(raw.0, evidence.manifest_bytes());
        assert_eq!(raw.1, evidence.summary_bytes());
        assert_eq!(raw.2, evidence.receipt());
        assert_eq!(raw.3, evidence.trace());
        validate(&db, 21, &pool).unwrap();
        db.execute("UPDATE projects SET revision=revision+1", [])
            .unwrap();
        validate(&db, 21, &pool).unwrap();
        let scope = crate::worker_fence_db::binding(&db, 21)
            .unwrap()
            .unwrap()
            .scope;
        {
            let tx = db.transaction().unwrap();
            crate::worker_fence_db::arm_creation(&tx, &scope).unwrap();
            crate::portable_cuda_db::arm(&tx).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            21
        );
        let wrong = ResourcePool::new(crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES - 1, 1);
        assert!(validate(&db, 21, &wrong).is_err());
        drop(evidence);
        assert_eq!(pool.reserved_storage(), 0);
        assert!(pool
            .reserve_memory(2 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES)
            .is_ok());
    }
    #[test]
    fn publication_failure_and_outer_rollback_leave_no_barrier_or_rows() {
        let pool = ResourcePool::new(crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES, 1);
        let mut evidence = evidence(&pool);
        let mut db = fixture(&evidence);
        {
            let tx = db.transaction().unwrap();
            assert!(retain(&tx, &evidence).is_err());
            tx.commit().unwrap();
        }
        assert!(!exists(&db).unwrap());
        assert!(evidence.reserve_storage(0).is_err());
        assert_eq!(pool.reserved_storage(), 0);
        evidence.reserve_storage(u64::MAX).unwrap();
        {
            let tx = db.transaction().unwrap();
            retain(&tx, &evidence).unwrap();
            tx.rollback().unwrap();
        }
        assert!(!exists(&db).unwrap());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        db.execute("UPDATE projects SET revision=revision+1", [])
            .unwrap();
        {
            let tx = db.transaction().unwrap();
            assert!(retain(&tx, &evidence).is_err());
            tx.commit().unwrap();
        }
        assert!(!exists(&db).unwrap());
        db.execute("UPDATE projects SET revision=revision-1", [])
            .unwrap();
        {
            let tx = db.transaction().unwrap();
            retain(&tx, &evidence).unwrap();
            tx.commit().unwrap();
        }
        {
            let tx = db.transaction().unwrap();
            assert!(retain(&tx, &evidence).is_err());
            tx.commit().unwrap();
        }
        assert_eq!(shape(&db).unwrap().0, 1);
    }
    #[test]
    fn readback_refuses_downgrade_layout_identity_membership_and_blob_tampering() {
        let pool = ResourcePool::new(2 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES, 1);
        let mut evidence = evidence(&pool);
        evidence.reserve_storage(u64::MAX).unwrap();
        for sql in [
            "PRAGMA user_version=20",
            "PRAGMA user_version=22",
            "DROP TABLE live_cuda_evidence",
            "ALTER TABLE live_cuda_evidence ADD COLUMN extra TEXT",
            "UPDATE live_cuda_evidence SET attempt='rebound'",
            "UPDATE live_cuda_evidence SET epoch='rebound'",
            "UPDATE live_cuda_evidence SET project='rebound'",
            "UPDATE live_cuda_evidence SET source='rebound'",
            "UPDATE live_cuda_evidence SET revision=revision+1",
            "UPDATE live_cuda_evidence SET summary=CAST(summary AS TEXT)",
            "UPDATE live_cuda_evidence SET trace=CAST(trace || ' ' AS BLOB)",
            "UPDATE live_cuda_evidence SET manifest=x'00'",
            "UPDATE live_cuda_evidence SET receipt=x'00'",
            "DELETE FROM sources",
            "UPDATE projects SET revision=-1",
            "UPDATE live_cuda_evidence SET receipt=zeroblob(65537)",
        ] {
            let mut db = fixture(&evidence);
            {
                let tx = db.transaction().unwrap();
                retain(&tx, &evidence).unwrap();
                tx.commit().unwrap();
            }
            db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
            db.execute_batch(sql).unwrap();
            let actual = db
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert!(validate(&db, actual, &pool).is_err(), "{sql}");
            assert!(
                pool.reserve_memory(crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES)
                    .is_ok(),
                "{sql}"
            );
        }
    }
    #[test]
    fn bounded_layout_scan_refuses_row_and_aggregate_capacity_before_blob_loading() {
        let pool = ResourcePool::new(crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES, 1);
        let evidence = evidence(&pool);
        for (count, bytes, reason) in [
            (257, 1, "row capacity"),
            (33, 8 * 1024 * 1024, "aggregate capacity"),
        ] {
            let db = fixture(&evidence);
            db.execute_batch(CREATE).unwrap();
            let identity = &evidence.manifest().stream.identity;
            db.execute("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<?1) INSERT INTO live_cuda_evidence SELECT 'attempt-'||i,?2,'epoch-'||i,?3,0,x'01',x'01',x'01',zeroblob(?4) FROM n",params![count,identity.project.as_str(),identity.source.as_str(),bytes]).unwrap();
            db.execute_batch("PRAGMA user_version=21;").unwrap();
            let error = validate(&db, 21, &pool).unwrap_err();
            assert!(error.to_string().contains(reason), "{error:#}");
        }
    }
}
