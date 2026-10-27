//! Immutable GPU attempt intent and reader barrier, not launch authorization.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

const MAX_ROWS: i64 = 4096;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_BYTES: i64 = 64 * 1024 * 1024;
const TABLE: &str = "cuda_attempt_intents";
pub(crate) const CREATE:&str="CREATE TABLE cuda_attempt_intents(attempt TEXT PRIMARY KEY NOT NULL REFERENCES attempts(id),job TEXT NOT NULL REFERENCES jobs(id),engine_epoch TEXT NOT NULL,request_sha256 TEXT NOT NULL,manifest TEXT NOT NULL)";
// Detect both complete envelopes and incomplete declared CUDA profiles.
const CUDA_SQL:&str="(json_type(manifest,'$.cuda_execution') IS NOT NULL AND json_type(manifest,'$.cuda_execution')!='null') OR json_extract(manifest,'$.operation.arguments.settings.inference_profile.provider')='cuda' OR json_extract(manifest,'$.operation.arguments.inference_profile.provider')='cuda'";

fn exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [TABLE],
        |r| r.get(0),
    )?)
}
fn layout(db: &Connection) -> Result<()> {
    ensure!(exists(db)?, "missing CUDA attempt intent layout");
    let mut statement = db.prepare("PRAGMA table_info(cuda_attempt_intents)")?;
    let columns = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let expected = [
        "attempt",
        "job",
        "engine_epoch",
        "request_sha256",
        "manifest",
    ]
    .into_iter()
    .enumerate()
    .map(|(n, name)| {
        (
            name.to_string(),
            "TEXT".to_string(),
            1,
            None,
            i64::from(n == 0),
        )
    })
    .collect::<Vec<_>>();
    ensure!(columns == expected, "CUDA attempt intent columns differ");
    let mut statement = db.prepare("PRAGMA foreign_key_list(cuda_attempt_intents)")?;
    let mut foreign = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    foreign.sort();
    ensure!(
        foreign
            == vec![
                ("attempts".into(), "attempt".into(), "id".into()),
                ("jobs".into(), "job".into(), "id".into())
            ],
        "CUDA attempt intent foreign keys differ"
    );
    Ok(())
}
fn capacity(db: &Connection) -> Result<(i64, i64)> {
    let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM cuda_attempt_intents WHERE typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(engine_epoch)!='text' OR length(CAST(engine_epoch AS BLOB))!=36 OR typeof(request_sha256)!='text' OR length(CAST(request_sha256 AS BLOB))!=64 OR typeof(manifest)!='text')",[],|r|r.get(0))?;
    ensure!(!invalid, "CUDA intent scalar admission");
    let (count,total,maximum):(i64,i64,i64)=db.query_row("SELECT COUNT(*),COALESCE(SUM(length(CAST(manifest AS BLOB))),0),COALESCE(MAX(length(CAST(manifest AS BLOB))),0) FROM cuda_attempt_intents",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    ensure!(
        count <= MAX_ROWS && total <= MAX_TOTAL_BYTES && maximum <= MAX_RECORD_BYTES as i64,
        "CUDA attempt intent capacity exceeded"
    );
    Ok((count, total))
}
fn request(raw: &str) -> Result<WorkerRequest> {
    ensure!(
        raw.len() <= MAX_RECORD_BYTES,
        "CUDA intent record byte admission"
    );
    let request: WorkerRequest = serde_json::from_str(raw)?;
    request.validate()?;
    ensure!(
        request.cuda_execution.is_some(),
        "CUDA intent lacks execution declaration"
    );
    ensure!(
        serde_json::to_string(&request)? == raw,
        "CUDA intent manifest is not canonical"
    );
    Ok(request)
}
fn hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

/// Read-only startup gate, invoked before writable SQLite/WAL startup.
pub fn validate(db: &Connection, version: i64) -> Result<()> {
    let version = crate::live_cuda_evidence_db::base_version(db, version)?;
    if version == 0 {
        return Ok(());
    }
    for table in ["jobs", "attempts"] {
        let sql=format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE typeof(manifest)!='text' OR length(CAST(manifest AS BLOB))>1048576)");
        ensure!(
            !db.query_row(&sql, [], |r| r.get::<_, bool>(0))?,
            "worker manifest exceeds durable control admission"
        );
    }
    crate::portable_cuda_db::validate(db, version)?;
    let present = exists(db)?;
    if version < 15 || matches!(version, 18..=20) && !present {
        if present {
            layout(db)?;
            let (count, _) = capacity(db)?;
            ensure!(count == 0, "CUDA intent requires schema-15 reader barrier");
        }
        for table in ["jobs", "attempts"] {
            let sql = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {CUDA_SQL})");
            ensure!(
                !db.query_row(&sql, [], |r| r.get::<_, bool>(0))?,
                "CUDA manifest requires schema-15 reader barrier"
            );
        }
        return Ok(());
    }
    ensure!(
        matches!(version, 15..=20),
        "unknown CUDA intent database version"
    );
    layout(db)?;
    capacity(db)?;
    let mut statement=db.prepare("SELECT attempt,job,engine_epoch,request_sha256,manifest FROM cuda_attempt_intents ORDER BY attempt")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let attempt: String = row.get(0)?;
        let job: String = row.get(1)?;
        let epoch: String = row.get(2)?;
        let digest: String = row.get(3)?;
        let raw: String = row.get(4)?;
        ensure!(
            uuid::Uuid::parse_str(&epoch)
                .context("invalid CUDA intent engine epoch")?
                .to_string()
                == epoch,
            "CUDA intent epoch is not canonical"
        );
        let request = request(&raw)?;
        ensure!(
            attempt == request.attempt_id.as_str()
                && job == request.job_id.as_str()
                && digest == hash(&raw),
            "CUDA intent identity differs"
        );
        let pair:Option<(String,String)>=db.query_row("SELECT job,manifest FROM attempts WHERE id=?1 AND typeof(job)='text' AND length(CAST(job AS BLOB))<=128",[&attempt],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        ensure!(
            pair == Some((job.clone(), raw.clone())),
            "CUDA intent differs from immutable attempt"
        );
        let current:Option<(String,String,String,i64,String)>=db.query_row("SELECT attempt,manifest,project,base_revision,source FROM jobs WHERE id=?1 AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB))<=128 AND typeof(project)='text' AND length(CAST(project AS BLOB))<=128 AND typeof(source)='text' AND length(CAST(source AS BLOB))<=128",[&job],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let current = current.context("CUDA intent job missing")?;
        ensure!(
            current.2 == request.project_id.as_str()
                && current.3 >= 0
                && current.3 as u64 == request.base_revision.value()
                && current.4 == request.source.source_version.as_str(),
            "CUDA intent job scope differs"
        );
        if current.0 == attempt {
            ensure!(current.1 == raw, "current CUDA job manifest differs");
        }
    }
    for (table, key) in [("attempts", "id"), ("jobs", "attempt")] {
        let sql=format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE ({CUDA_SQL}) AND NOT EXISTS(SELECT 1 FROM cuda_attempt_intents WHERE attempt={table}.{key} AND manifest={table}.manifest))");
        ensure!(
            !db.query_row(&sql, [], |r| r.get::<_, bool>(0))?,
            "CUDA manifest lacks immutable intent membership"
        );
    }
    Ok(())
}

/// Call inside the same job/attempt transaction after independent admission.
/// Persisting declared intent does not prove reservations, mounts or launch.
pub fn declare(tx: &Transaction<'_>, request: &WorkerRequest, engine_epoch: &str) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_cuda_intent;")?;
    match declare_inner(tx, request, engine_epoch) {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_cuda_intent;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch("ROLLBACK TO pulsar_cuda_intent; RELEASE pulsar_cuda_intent;")?;
            Err(error)
        }
    }
}
fn declare_inner(tx: &Transaction<'_>, request: &WorkerRequest, engine_epoch: &str) -> Result<()> {
    request.validate()?;
    ensure!(
        request.cuda_execution.is_some(),
        "CPU attempts do not create GPU intent"
    );
    ensure!(
        uuid::Uuid::parse_str(engine_epoch)?.to_string() == engine_epoch,
        "CUDA engine epoch is not canonical"
    );
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = crate::live_cuda_evidence_db::base_version(tx, version)?;
    ensure!(
        (2..=20).contains(&version),
        "unsupported CUDA intent writer schema"
    );
    crate::portable_cuda_db::validate(tx, version)?;
    let raw = serde_json::to_string(request)?;
    ensure!(
        raw.len() <= MAX_RECORD_BYTES,
        "CUDA intent record byte admission"
    );
    let pair:Option<(String,String,String)>=tx.query_row("SELECT job,manifest,state FROM attempts WHERE id=?1 AND typeof(job)='text' AND length(CAST(job AS BLOB))<=128 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB))<=1048576 AND typeof(state)='text' AND length(CAST(state AS BLOB))<=32",[request.attempt_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    ensure!(
        pair == Some((request.job_id.to_string(), raw.clone(), "queued".into())),
        "CUDA intent requires exact queued attempt"
    );
    let current:Option<(String,String,String)>=tx.query_row("SELECT attempt,manifest,state FROM jobs WHERE id=?1 AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB))<=128 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB))<=1048576 AND typeof(state)='text' AND length(CAST(state AS BLOB))<=32",[request.job_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    ensure!(
        current == Some((request.attempt_id.to_string(), raw.clone(), "queued".into())),
        "CUDA intent requires exact current queued job"
    );
    if !exists(tx)? {
        tx.execute_batch(CREATE)?;
    }
    layout(tx)?;
    let (count, total) = capacity(tx)?;
    let prior:Option<(String,String,String,String)>=tx.query_row("SELECT job,engine_epoch,request_sha256,manifest FROM cuda_attempt_intents WHERE attempt=?1",[request.attempt_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let desired = (
        request.job_id.to_string(),
        engine_epoch.to_string(),
        hash(&raw),
        raw.clone(),
    );
    if let Some(prior) = prior {
        ensure!(
            matches!(version, 15..=20) && prior == desired,
            "CUDA intent cannot be replaced or replayed in another engine epoch"
        );
        return validate(tx, version);
    }
    ensure!(
        count < MAX_ROWS && total + raw.len() as i64 <= MAX_TOTAL_BYTES,
        "CUDA attempt intent capacity exhausted"
    );
    tx.execute(
        "INSERT INTO cuda_attempt_intents VALUES(?1,?2,?3,?4,?5)",
        params![
            request.attempt_id.as_str(),
            request.job_id.as_str(),
            engine_epoch,
            desired.2,
            raw
        ],
    )?;
    if version < 15 {
        crate::portable_annotated_db::publish(tx, 15)?;
    }
    if version >= 18 {
        crate::portable_cuda_db::arm(tx)?;
    }
    validate(tx, version.max(15))
}

/// Completion must belong to this engine epoch and the exact current running
/// request. Durable intent alone grants no authority to launch or publish.
pub fn require_running(db: &Connection, request: &WorkerRequest, engine_epoch: &str) -> Result<()> {
    request.validate()?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    validate(db, version)?;
    let raw = serde_json::to_string(request)?;
    let desired = (
        request.job_id.to_string(),
        engine_epoch.to_string(),
        hash(&raw),
        raw.clone(),
    );
    let intent:Option<(String,String,String,String)>=db.query_row("SELECT job,engine_epoch,request_sha256,manifest FROM cuda_attempt_intents WHERE attempt=?1",[request.attempt_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    ensure!(
        intent == Some(desired),
        "CUDA completion differs from current engine intent"
    );
    let running:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM jobs j JOIN attempts a ON a.id=j.attempt WHERE j.id=?1 AND j.attempt=?2 AND j.state='running' AND a.state='running' AND j.manifest=?3 AND a.manifest=?3)",params![request.job_id.as_str(),request.attempt_id.as_str(),raw],|r|r.get(0))?;
    ensure!(
        running,
        "CUDA completion requires exact running job and attempt"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> WorkerRequest {
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap()
    }
    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA user_version=14;
        CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT,project TEXT,base_revision INTEGER,source TEXT,manifest TEXT,state TEXT);
        CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT REFERENCES jobs(id),manifest TEXT,state TEXT);").unwrap();
        db
    }
    fn seed(tx: &Transaction<'_>, r: &WorkerRequest) {
        let raw = serde_json::to_string(r).unwrap();
        tx.execute(
            "INSERT INTO jobs VALUES(?1,?2,?3,?4,?5,?6,'queued')",
            params![
                r.job_id.as_str(),
                r.attempt_id.as_str(),
                r.project_id.as_str(),
                r.base_revision.value(),
                r.source.source_version.as_str(),
                raw
            ],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![r.attempt_id.as_str(), r.job_id.as_str(), raw],
        )
        .unwrap();
    }
    #[test]
    fn archival_reader_barrier_survives_new_cuda_intent_and_rejects_missing_table() {
        let mut db = database();
        let request = fixture();
        let epoch = uuid::Uuid::new_v4().to_string();
        {
            let tx = db.transaction().unwrap();
            crate::portable_cuda_db::arm(&tx).unwrap();
            seed(&tx, &request);
            declare(&tx, &request, &epoch).unwrap();
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                18
            );
            assert!(
                crate::portable_cuda_db::validate(&tx, 18)
                    .unwrap()
                    .unwrap()
                    .cuda_intents
            );
            validate(&tx, 18).unwrap();
            tx.commit().unwrap();
        }
        db.execute_batch("DROP TABLE cuda_attempt_intents;")
            .unwrap();
        assert!(validate(&db, 18).is_err());
    }
    fn committed() -> (Connection, WorkerRequest, String) {
        let mut db = database();
        let r = fixture();
        let epoch = uuid::Uuid::new_v4().to_string();
        let tx = db.transaction().unwrap();
        seed(&tx, &r);
        declare(&tx, &r, &epoch).unwrap();
        tx.commit().unwrap();
        (db, r, epoch)
    }
    #[test]
    fn cpu_legacy_open_does_not_create_or_advance_gpu_schema() {
        let db = database();
        validate(&db, 14).unwrap();
        assert!(!exists(&db).unwrap());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
    }
    #[test]
    fn declaration_and_reader_barrier_rollback_together() {
        let mut db = database();
        let r = fixture();
        let tx = db.transaction().unwrap();
        seed(&tx, &r);
        declare(&tx, &r, &uuid::Uuid::new_v4().to_string()).unwrap();
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            15
        );
        drop(tx);
        assert!(!exists(&db).unwrap());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        validate(&db, 14).unwrap();
    }
    #[test]
    fn exact_replay_is_idempotent_and_new_epoch_cannot_replace_intent() {
        let (mut db, r, epoch) = committed();
        let tx = db.transaction().unwrap();
        declare(&tx, &r, &epoch).unwrap();
        assert!(declare(&tx, &r, &uuid::Uuid::new_v4().to_string())
            .unwrap_err()
            .to_string()
            .contains("cannot be replaced"));
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM cuda_attempt_intents", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        validate(&db, 15).unwrap();
    }
    #[test]
    fn restart_interruption_keeps_immutable_intent_without_reapproval() {
        let (mut db, r, epoch) = committed();
        db.execute_batch(
            "UPDATE jobs SET state='interrupted'; UPDATE attempts SET state='interrupted';",
        )
        .unwrap();
        validate(&db, 15).unwrap();
        let tx = db.transaction().unwrap();
        assert!(declare(&tx, &r, &epoch).is_err());
        drop(tx);
        validate(&db, 15).unwrap();
    }
    #[test]
    fn old_schema_hidden_gpu_intent_and_missing_layout_refuse() {
        let (db, _, _) = committed();
        assert!(validate(&db, 14).is_err());
        let mut db = database();
        let tx = db.transaction().unwrap();
        seed(&tx, &fixture());
        tx.commit().unwrap();
        assert!(validate(&db, 14).is_err());
        assert!(validate(&db, 15).is_err());
    }
    #[test]
    fn corrupted_identity_membership_scope_and_oversized_bytes_refuse_read_only() {
        for sql in [
            "UPDATE cuda_attempt_intents SET request_sha256=printf('%064d',0)",
            "DELETE FROM cuda_attempt_intents",
            "UPDATE attempts SET manifest='{}'",
            "UPDATE jobs SET project='another-project'",
            "UPDATE cuda_attempt_intents SET manifest=zeroblob(1048577)",
        ] {
            let (db, _, _) = committed();
            db.execute_batch(sql).unwrap();
            let before = db
                .query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
                .unwrap();
            assert!(validate(&db, 15).is_err(), "{sql}");
            assert_eq!(
                db.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                before
            );
        }
    }
    #[test]
    fn altered_layout_and_unknown_manifest_fields_refuse() {
        let (db, _, _) = committed();
        db.execute_batch("ALTER TABLE cuda_attempt_intents ADD COLUMN admitted INTEGER;")
            .unwrap();
        assert!(validate(&db, 15).is_err());
        let (db, r, _) = committed();
        let mut value = serde_json::to_value(r).unwrap();
        value["launch_authorized"] = true.into();
        let raw = serde_json::to_string(&value).unwrap();
        db.execute(
            "UPDATE cuda_attempt_intents SET manifest=?1,request_sha256=?2",
            params![raw, hash(&raw)],
        )
        .unwrap();
        assert!(validate(&db, 15)
            .unwrap_err()
            .to_string()
            .contains("unknown field"));
    }
    #[test]
    fn cpu_or_missing_attempt_cannot_create_intent_table() {
        let mut db = database();
        let tx = db.transaction().unwrap();
        assert!(declare(&tx, &fixture(), &uuid::Uuid::new_v4().to_string()).is_err());
        assert!(!exists(&tx).unwrap());
        let mut r = fixture();
        r.cuda_execution = None;
        assert!(declare(&tx, &r, &uuid::Uuid::new_v4().to_string()).is_err());
        assert!(!exists(&tx).unwrap());
    }
    #[test]
    fn failed_declaration_rolls_back_its_barrier_without_rolling_back_caller_work() {
        let mut db = database();
        let r = fixture();
        let tx = db.transaction().unwrap();
        seed(&tx, &r);
        tx.execute_batch("CREATE TABLE caller_note(value INTEGER); INSERT INTO caller_note VALUES(7); UPDATE jobs SET project='another-project';").unwrap();
        assert!(declare(&tx, &r, &uuid::Uuid::new_v4().to_string()).is_err());
        assert!(!exists(&tx).unwrap());
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        assert_eq!(
            tx.query_row("SELECT value FROM caller_note", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
    }
    #[test]
    fn row_and_aggregate_caps_refuse_before_manifest_parsing_independently() {
        for (rows, bytes) in [(4096, 2), (64, 1048576)] {
            let (db, _, epoch) = committed();
            db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
            db.execute("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM numbers WHERE n<?1) INSERT INTO cuda_attempt_intents SELECT 'extra-'||n,'authored-gpu-job',?2,printf('%064d',0),printf('%.*c',?3,'x') FROM numbers",params![rows,epoch,bytes]).unwrap();
            assert!(validate(&db, 15)
                .unwrap_err()
                .to_string()
                .contains("capacity exceeded"));
        }
    }
}
