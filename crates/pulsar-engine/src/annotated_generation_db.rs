//! Conditional reader barrier and immutable annotated-attempt membership.
//! Validation reads metadata only; it never opens historical worker paths.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

const MARKER: &str = "annotated_generation_reader";
const MEMBERS: &str = "annotated_generation_attempts";
const CREATE_MARKER: &str = "CREATE TABLE annotated_generation_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),schema_version INTEGER NOT NULL CHECK(schema_version=1))";
const CREATE_MEMBERS: &str = "CREATE TABLE annotated_generation_attempts(attempt TEXT PRIMARY KEY NOT NULL REFERENCES attempts(id),job TEXT NOT NULL REFERENCES jobs(id),manifest_sha256 TEXT NOT NULL)";
const MAX_ROWS: i64 = 4096;
const MAX_MANIFEST: usize = 1024 * 1024;
const MAX_BYTES: i64 = 64 * 1024 * 1024;

fn exists(db: &Connection, name: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}
fn layout(db: &Connection, name: &str, expected: &str) -> Result<()> {
    let sql:Option<String>=db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[name],|r|r.get(0)).optional()?;
    ensure!(
        sql.as_deref() == Some(expected),
        "annotated generation reader layout differs"
    );
    Ok(())
}

/// Resolve the preserved CUDA storage layer without accepting a downgraded
/// header or invoking its validators recursively. Full membership is separate.
pub(crate) fn storage_version(db: &Connection) -> Result<i64> {
    let actual = crate::portable_annotated_db::storage_version(db)?;
    let marker = exists(db, MARKER)?;
    let members = exists(db, MEMBERS)?;
    if actual == 23 || marker || members {
        ensure!(
            actual == 23 && marker && members,
            "annotated generation requires its exact schema-23 reader barrier"
        );
        layout(db, MARKER, CREATE_MARKER)?;
        layout(db, MEMBERS, CREATE_MEMBERS)?;
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM annotated_generation_reader LIMIT 2)",
            [],
            |r| r.get(0),
        )?;
        let valid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM annotated_generation_reader WHERE typeof(singleton)='integer' AND singleton=1 AND typeof(schema_version)='integer' AND schema_version=1)",[],|r|r.get(0))?;
        ensure!(
            count == 1 && valid,
            "annotated generation reader singleton differs"
        );
        let live = exists(db, "live_cuda_evidence")?;
        let capture = exists(db, "live_cuda_capture_evidence")?;
        ensure!(
            !capture || live,
            "annotated reader has an incomplete CUDA storage layer"
        );
        return Ok(if capture {
            22
        } else if live {
            21
        } else {
            20
        });
    }
    ensure!(actual < 23, "unknown annotated generation reader version");
    Ok(actual)
}

/// CUDA evidence can add its tables under schema 23 but cannot erase the
/// annotated reader requirement. Invoke after creating the corresponding tables.
pub(crate) fn publish_storage_version(tx: &Transaction<'_>, version: i64) -> Result<()> {
    ensure!(
        matches!(version, 21 | 22),
        "unsupported CUDA storage publication version"
    );
    let actual = crate::portable_annotated_db::storage_version(tx)?;
    if actual == 23 {
        ensure!(
            storage_version(tx)? == version,
            "annotated CUDA storage publication differs"
        );
    } else {
        ensure!(
            matches!(actual, 20..=22) && version >= actual,
            "CUDA storage publication cannot lower or replace an unknown reader barrier"
        );
        ensure!(
            !exists(tx, MARKER)? && !exists(tx, MEMBERS)?,
            "annotated reader cannot be downgraded"
        );
        ensure!(
            exists(tx, "live_cuda_evidence")?
                && exists(tx, "live_cuda_capture_evidence")? == (version == 22),
            "CUDA storage tables must precede reader publication"
        );
        crate::portable_annotated_db::publish(tx, version)?;
    }
    Ok(())
}

fn capacity(db: &Connection) -> Result<()> {
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM annotated_generation_attempts LIMIT 4097)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count <= MAX_ROWS,
        "annotated generation attempt capacity exceeded"
    );
    let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM annotated_generation_attempts WHERE typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(manifest_sha256)!='text' OR length(CAST(manifest_sha256 AS BLOB))!=64)",[],|r|r.get(0))?;
    ensure!(
        !invalid,
        "annotated generation member scalar admission differs"
    );
    let (missing,total):(i64,i64)=db.query_row("SELECT COALESCE(SUM(CASE WHEN a.id IS NULL OR typeof(a.state)!='text' OR a.state NOT IN ('queued','running','completed','failed','cancelled','interrupted','assistant_paused') OR typeof(a.job)!='text' OR length(CAST(a.job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(a.manifest)!='text' OR length(CAST(a.manifest AS BLOB)) NOT BETWEEN 1 AND 1048576 THEN 1 ELSE 0 END),0),COALESCE(SUM(length(CAST(a.manifest AS BLOB))),0) FROM annotated_generation_attempts m LEFT JOIN attempts a ON a.id=m.attempt",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        missing == 0 && total <= MAX_BYTES,
        "annotated generation manifest capacity or membership differs"
    );
    Ok(())
}

fn checked_request(raw: &str) -> Result<WorkerRequest> {
    ensure!(
        raw.len() <= MAX_MANIFEST,
        "annotated generation manifest byte admission"
    );
    let request: WorkerRequest = serde_json::from_str(raw)?;
    request.validate()?;
    ensure!(
        serde_json::to_string(&request)? == raw,
        "annotated generation manifest is not canonical"
    );
    let WorkerOperation::Generate {
        settings,
        model: Some(model),
        ..
    } = &request.operation
    else {
        anyhow::bail!("annotated member requires model-backed file generation");
    };
    ensure!(
        settings.annotated_view.is_some(),
        "annotated member has no source-bound declaration"
    );
    let profile = crate::inference_profiles::resolve_bound_profile(&request)?
        .context("annotated profile missing")?;
    let runtime = request
        .tools
        .onnx_runtime
        .as_ref()
        .context("annotated runtime missing")?;
    let profile_identity = crate::inference_profiles::profile_identity(&profile)?;
    for identity in [
        &profile_identity,
        &model.identity,
        &runtime.identity,
        &request.tools.ffmpeg.identity,
        &request.tools.ffprobe.identity,
    ] {
        ensure!(
            request.dependencies.contains(identity),
            "annotated attempt lacks a declared execution dependency"
        );
    }
    Ok(request)
}

fn encode_request(request: &WorkerRequest) -> Result<String> {
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self
                .0
                .len()
                .checked_add(bytes.len())
                .is_none_or(|n| n > MAX_MANIFEST)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "annotated generation manifest byte admission",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bytes = Bounded(Vec::new());
    serde_json::to_writer(&mut bytes, request)?;
    Ok(String::from_utf8(bytes.0)?)
}

fn member(db: &Connection, attempt: &str, job: &str, hash: &str, raw: &str) -> Result<()> {
    validate_sha256(hash)?;
    ensure!(
        format!("{:x}", Sha256::digest(raw.as_bytes())) == hash,
        "annotated generation manifest digest differs"
    );
    let request = checked_request(raw)?;
    ensure!(
        request.attempt_id.as_str() == attempt && request.job_id.as_str() == job,
        "annotated generation indexed identity differs"
    );
    let current:Option<(String,String,String,i64,String)>=db.query_row("SELECT attempt,manifest,project,base_revision,source FROM jobs WHERE id=?1 AND typeof(state)='text' AND state IN ('queued','running','completed','failed','cancelled','interrupted') AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB)) BETWEEN 1 AND 128 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 AND typeof(project)='text' AND length(CAST(project AS BLOB)) BETWEEN 1 AND 128 AND typeof(base_revision)='integer' AND base_revision>=0 AND typeof(source)='text' AND length(CAST(source AS BLOB)) BETWEEN 1 AND 128",[job],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let current = current.context("annotated generation job absent or oversized")?;
    ensure!(
        current.2 == request.project_id.as_str()
            && current.3 as u64 == request.base_revision.value()
            && current.4 == request.source.source_version.as_str(),
        "annotated generation job scope differs"
    );
    if current.0 == attempt {
        ensure!(current.1 == raw, "annotated current job manifest differs");
    } else {
        checked_request(&current.1).context("annotated current retry manifest differs")?;
    }
    let current_attempt:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND job=?2 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 AND manifest=?3)",params![current.0,job,current.1],|r|r.get(0))?;
    ensure!(
        current_attempt,
        "annotated current job differs from its immutable attempt"
    );
    let source:Option<(String,i64)>=db.query_row("SELECT sha256,bytes FROM sources WHERE project=?1 AND version=?2 AND typeof(sha256)='text' AND length(CAST(sha256 AS BLOB))=64 AND typeof(bytes)='integer' AND bytes>=0",params![request.project_id.as_str(),request.source.source_version.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    ensure!(
        source
            == Some((
                request.source.identity.sha256.clone(),
                i64::try_from(request.source.identity.byte_len)?
            )),
        "annotated generation captured source membership differs"
    );
    let revision:Option<i64>=db.query_row("SELECT revision FROM projects WHERE id=?1 AND typeof(revision)='integer' AND revision>=?2",params![request.project_id.as_str(),i64::try_from(request.base_revision.value())?],|r|r.get(0)).optional()?;
    ensure!(
        revision.is_some(),
        "annotated generation project revision membership differs"
    );
    Ok(())
}

/// Bounded typed allocation under resource admission; call before custody
/// recovery, writable SQLite startup or retry. Historical paths remain inert.
pub(crate) fn validate(db: &Connection, pool: &crate::resources::ResourcePool) -> Result<()> {
    storage_version(db)?;
    let present = exists(db, MEMBERS)?;
    // Existing CUDA validation applies the same per-record control bound.
    // Refuse malformed JSON rather than allowing it to hide a missing member.
    for table in ["jobs", "attempts"] {
        if !exists(db, table)? {
            ensure!(!present, "annotated generation worker tables missing");
            continue;
        }
        let invalid=format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE typeof(manifest)!='text' OR length(CAST(manifest AS BLOB))>1048576 OR NOT json_valid(manifest))");
        ensure!(
            !db.query_row(&invalid, [], |r| r.get::<_, bool>(0))?,
            "worker manifest is malformed or exceeds control admission"
        );
        let marker="(json_type(manifest,'$.operation.arguments.settings.annotated_view') IS NOT NULL AND json_type(manifest,'$.operation.arguments.settings.annotated_view')!='null') OR json_extract(manifest,'$.operation.arguments.settings.inference_profile.preprocessing.resize')='annotated_temporal_view'";
        let missing = if present {
            format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE ({marker}) AND NOT EXISTS(SELECT 1 FROM annotated_generation_attempts WHERE attempt={table}.{}))",if table=="jobs" {"attempt"}else{"id"})
        } else {
            format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {marker})")
        };
        ensure!(
            !db.query_row(&missing, [], |r| r.get::<_, bool>(0))?,
            "annotated generation lacks durable attempt membership"
        );
    }
    if !present {
        return Ok(());
    }
    capacity(db)?;
    let mut statement=db.prepare("SELECT m.attempt,m.job,m.manifest_sha256,a.job,a.manifest FROM annotated_generation_attempts m JOIN attempts a ON a.id=m.attempt ORDER BY m.attempt")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let _admission = pool.reserve_memory((MAX_MANIFEST * 8 + 2 * 1024 * 1024) as u64)?;
        let attempt: String = row.get(0)?;
        let job: String = row.get(1)?;
        let hash: String = row.get(2)?;
        let attempt_job: String = row.get(3)?;
        let raw: String = row.get(4)?;
        ensure!(
            job == attempt_job,
            "annotated generation attempt job differs"
        );
        member(db, &attempt, &job, &hash, &raw)?;
    }
    Ok(())
}

/// Retain within the existing authorized job/attempt transaction. Public Engine
/// admission remains guarded until receipt and portable correspondence exists.
pub(crate) fn retain(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    pool: &crate::resources::ResourcePool,
) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_annotated_generation;")?;
    let result = (|| -> Result<()> {
        let _admission = pool.reserve_memory((MAX_MANIFEST * 8 + 2 * 1024 * 1024) as u64)?;
        let raw = encode_request(request)?;
        checked_request(&raw)?;
        let version = storage_version(tx)?;
        ensure!(
            matches!(version, 20..=22),
            "annotated generation requires creation custody"
        );
        ensure!(
            crate::worker_fence_db::binding(tx, version)?
                .is_some_and(|b| b.minimum_ledger_version == 4),
            "annotated generation ownership requirement missing"
        );
        crate::cuda_attempts::validate(tx, version)?;
        crate::cuda_acceptance::validate(tx, version)?;
        if !exists(tx, MARKER)? {
            tx.execute_batch(CREATE_MARKER)?;
            tx.execute_batch(CREATE_MEMBERS)?;
            tx.execute("INSERT INTO annotated_generation_reader VALUES(1,1)", [])?;
            crate::portable_annotated_db::publish(tx, 23)?;
        }
        capacity(tx)?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM annotated_generation_attempts",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            count < MAX_ROWS,
            "annotated generation attempt capacity exhausted"
        );
        let stored:Option<(String,String)>=tx.query_row("SELECT job,manifest FROM attempts WHERE id=?1 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576",[request.attempt_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        ensure!(
            stored == Some((request.job_id.to_string(), raw.clone())),
            "annotated generation differs from immutable stored attempt"
        );
        let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
        member(
            tx,
            request.attempt_id.as_str(),
            request.job_id.as_str(),
            &hash,
            &raw,
        )?;
        tx.execute(
            "INSERT INTO annotated_generation_attempts VALUES(?1,?2,?3)",
            params![request.attempt_id.as_str(), request.job_id.as_str(), hash],
        )?;
        validate(tx, pool)
    })();
    match result {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_annotated_generation;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch(
                "ROLLBACK TO pulsar_annotated_generation; RELEASE pulsar_annotated_generation;",
            )?;
            Err(error)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn pool() -> crate::resources::ResourcePool {
        crate::resources::ResourcePool::new(256 * 1024 * 1024, 4)
    }
    pub(crate) fn request() -> WorkerRequest {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.cuda_execution = None;
        let WorkerOperation::Generate {
            settings,
            model: Some(model),
            ..
        } = &mut request.operation
        else {
            panic!("model fixture");
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        profile.preprocessing.resize = InferenceResize::AnnotatedTemporalView;
        settings.annotated_view=Some(serde_json::from_value(serde_json::json!({
            "source_sha256":request.source.identity.sha256,"declaration":{"schema_version":1,
                "source_dimensions":[128,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"authored",
                "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}}}
        })).unwrap());
        request.dependencies = vec![
            crate::inference_profiles::profile_identity(profile).unwrap(),
            model.identity.clone(),
            request.tools.ffmpeg.identity.clone(),
            request.tools.ffprobe.identity.clone(),
            request
                .tools
                .onnx_runtime
                .as_ref()
                .unwrap()
                .identity
                .clone(),
        ];
        request.validate().unwrap();
        request
    }
    fn database() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=14;
            CREATE TABLE projects(id TEXT PRIMARY KEY,revision INTEGER);
            CREATE TABLE sources(project TEXT,version TEXT,sha256 TEXT,bytes INTEGER,PRIMARY KEY(project,version));
            CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT,project TEXT,base_revision INTEGER,source TEXT,manifest TEXT,state TEXT,candidate TEXT);
            CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT,manifest TEXT,state TEXT);
            CREATE TABLE candidates(id TEXT PRIMARY KEY,project TEXT,job TEXT,lineage TEXT,base_revision INTEGER);").unwrap();
        let tx = db.transaction().unwrap();
        crate::worker_fence_db::arm_creation(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
        tx.commit().unwrap();
        db
    }
    #[test]
    fn portable_wrapper_and_local_annotation_keep_both_reader_requirements() {
        for annotated_first in [false, true] {
            let mut db = database();
            let request = request();
            let pool = pool();
            let tx = db.transaction().unwrap();
            seed(&tx, &request);
            if annotated_first {
                retain(&tx, &request, &pool).unwrap();
            }
            crate::portable_annotated_db::arm(&tx).unwrap();
            if !annotated_first {
                retain(&tx, &request, &pool).unwrap();
            }
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                24
            );
            assert_eq!(
                crate::portable_annotated_db::storage_version(&tx).unwrap(),
                23
            );
            assert_eq!(storage_version(&tx).unwrap(), 20);
            validate(&tx, &pool).unwrap();
            assert_eq!(
                crate::worker_fence_db::binding(&tx, 24)
                    .unwrap()
                    .unwrap()
                    .minimum_ledger_version,
                4
            );
            tx.commit().unwrap();
            db.execute_batch("PRAGMA user_version=23").unwrap();
            assert!(validate(&db, &pool).is_err());
        }
    }
    pub(crate) fn seed(tx: &Transaction<'_>, request: &WorkerRequest) {
        let raw = serde_json::to_string(request).unwrap();
        tx.execute(
            "INSERT INTO projects(id,revision) VALUES(?1,?2)",
            params![request.project_id.as_str(), request.base_revision.value()],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO sources(project,version,sha256,bytes) VALUES(?1,?2,?3,?4)",
            params![
                request.project_id.as_str(),
                request.source.source_version.as_str(),
                request.source.identity.sha256,
                request.source.identity.byte_len
            ],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO jobs(id,attempt,project,base_revision,source,manifest,state,candidate) VALUES(?1,?2,?3,?4,?5,?6,'queued',NULL)",
            params![
                request.job_id.as_str(),
                request.attempt_id.as_str(),
                request.project_id.as_str(),
                request.base_revision.value(),
                request.source.source_version.as_str(),
                raw
            ],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![request.attempt_id.as_str(), request.job_id.as_str(), raw],
        )
        .unwrap();
    }
    fn committed() -> (Connection, WorkerRequest) {
        let mut db = database();
        let request = request();
        {
            let tx = db.transaction().unwrap();
            seed(&tx, &request);
            retain(&tx, &request, &pool()).unwrap();
            tx.commit().unwrap();
        }
        (db, request)
    }

    #[test]
    fn conditional_barrier_retains_exact_attempt_and_checks_without_opening_paths() {
        let (db, request) = committed();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            23
        );
        assert_eq!(storage_version(&db).unwrap(), 20);
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 23).unwrap(),
            20
        );
        for stale in [0, 17, 19, 21, 22, 24] {
            assert!(crate::live_cuda_evidence_db::base_version(&db, stale).is_err());
        }
        assert_eq!(
            crate::worker_fence_db::binding(&db, 23)
                .unwrap()
                .unwrap()
                .minimum_ledger_version,
            4
        );
        validate(&db, &pool()).unwrap();
        let hash: String = db
            .query_row(
                "SELECT manifest_sha256 FROM annotated_generation_attempts",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            hash,
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&request).unwrap())
            )
        );
        let poor = crate::resources::ResourcePool::new(1024, 1);
        assert!(validate(&db, &poor).is_err());
        validate(&db, &pool()).unwrap();
    }
    #[test]
    fn paused_attempt_history_reopens_but_paused_job_state_is_not_admitted() {
        let (db, _) = committed();
        db.execute_batch(
            "UPDATE jobs SET state='interrupted'; UPDATE attempts SET state='assistant_paused';",
        )
        .unwrap();
        validate(&db, &pool()).unwrap();
        db.execute_batch("UPDATE jobs SET state='assistant_paused';")
            .unwrap();
        assert!(validate(&db, &pool()).is_err());
    }

    #[test]
    fn missing_or_changed_membership_and_downgraded_headers_fail_closed() {
        for sql in [
            "PRAGMA user_version=20",
            "PRAGMA user_version=24",
            "DROP TABLE annotated_generation_reader",
            "DROP TABLE annotated_generation_attempts",
            "DELETE FROM annotated_generation_reader",
            "DELETE FROM annotated_generation_attempts",
            "UPDATE annotated_generation_attempts SET manifest_sha256=printf('%064d',0)",
            "UPDATE attempts SET manifest=manifest || ' '",
            "UPDATE attempts SET job=CAST(zeroblob(1048577) AS TEXT)",
            "UPDATE attempts SET manifest='not-json'",
            "UPDATE attempts SET manifest='{}'",
            "UPDATE attempts SET manifest=CAST(zeroblob(1048577) AS TEXT)",
            "UPDATE jobs SET manifest='{}'",
            "UPDATE jobs SET attempt='foreign'",
            "UPDATE jobs SET attempt='foreign',manifest='{}'",
            "UPDATE jobs SET source='foreign'",
            "UPDATE jobs SET state='future'",
            "UPDATE attempts SET state='future'",
            "UPDATE sources SET sha256=printf('%064d',0)",
            "UPDATE sources SET bytes=bytes+1",
            "UPDATE projects SET revision=-1",
        ] {
            let (db, _) = committed();
            db.execute_batch(sql).unwrap();
            assert!(validate(&db, &pool()).is_err(), "{sql}");
        }
    }
    #[test]
    fn retention_savepoint_and_outer_rollback_preserve_the_legacy_subject() {
        let mut db = database();
        let request = request();
        {
            let tx = db.transaction().unwrap();
            seed(&tx, &request);
            let mut wrong = request.clone();
            wrong.source.identity.sha256 = "b".repeat(64);
            assert!(retain(&tx, &wrong, &pool()).is_err());
            assert_eq!(storage_version(&tx).unwrap(), 20);
            assert!(!exists(&tx, MARKER).unwrap());
            // Failure after the barrier was created must also roll it back.
            tx.execute("UPDATE sources SET bytes=bytes+1", []).unwrap();
            assert!(retain(&tx, &request, &pool()).is_err());
            assert_eq!(storage_version(&tx).unwrap(), 20);
            assert!(!exists(&tx, MEMBERS).unwrap());
        }
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(storage_version(&db).unwrap(), 20);
        validate(&db, &pool()).unwrap();
    }
    #[test]
    fn source_bound_rows_without_the_reader_and_membership_are_not_legacy() {
        let mut db = database();
        let request = request();
        let tx = db.transaction().unwrap();
        seed(&tx, &request);
        tx.commit().unwrap();
        assert!(validate(&db, &pool()).is_err());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        assert!(!exists(&db, MARKER).unwrap());
    }

    #[test]
    fn retry_preserves_both_immutable_attempts_and_requires_current_membership() {
        let (mut db, previous) = committed();
        let mut next = previous.clone();
        next.attempt_id = AttemptId::new("authored-retry").unwrap();
        next.output_dir = "/inert-retry-output".into();
        let raw = serde_json::to_string(&next).unwrap();
        {
            let tx = db.transaction().unwrap();
            tx.execute(
                "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
                params![next.attempt_id.as_str(), next.job_id.as_str(), raw],
            )
            .unwrap();
            tx.execute(
                "UPDATE jobs SET attempt=?1,manifest=?2 WHERE id=?3",
                params![next.attempt_id.as_str(), raw, next.job_id.as_str()],
            )
            .unwrap();
            assert!(validate(&tx, &pool()).is_err());
            retain(&tx, &next, &pool()).unwrap();
            tx.commit().unwrap();
        }
        validate(&db, &pool()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM annotated_generation_attempts",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        let first: String = db
            .query_row(
                "SELECT manifest FROM attempts WHERE id=?1",
                [previous.attempt_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(first, serde_json::to_string(&previous).unwrap());
        db.execute(
            "DELETE FROM annotated_generation_attempts WHERE attempt=?1",
            [next.attempt_id.as_str()],
        )
        .unwrap();
        assert!(validate(&db, &pool()).is_err());
    }

    #[test]
    fn member_count_bound_refuses_before_loading_worker_payloads() {
        let (db, _) = committed();
        db.execute_batch("PRAGMA foreign_keys=OFF; WITH RECURSIVE n(i) AS(SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<4096) INSERT INTO annotated_generation_attempts SELECT printf('over-capacity-%d',i),'inert',printf('%064d',0) FROM n;").unwrap();
        assert!(validate(&db, &pool())
            .unwrap_err()
            .to_string()
            .contains("attempt capacity exceeded"));
        db.execute(
            "DELETE FROM annotated_generation_attempts WHERE job='inert'",
            [],
        )
        .unwrap();
        validate(&db, &pool()).unwrap();
    }

    #[test]
    fn serialization_is_bounded_and_storage_publication_never_downgrades() {
        let mut request = request();
        assert_eq!(
            encode_request(&request).unwrap(),
            serde_json::to_string(&request).unwrap()
        );
        request.output_dir = format!("/{}", "x".repeat(MAX_MANIFEST + 1)).into();
        assert!(encode_request(&request)
            .unwrap_err()
            .to_string()
            .contains("manifest byte admission"));
        for version in [19, 20, 22, 24] {
            let mut db = Connection::open_in_memory().unwrap();
            db.pragma_update(None, "user_version", version).unwrap();
            let tx = db.transaction().unwrap();
            assert!(publish_storage_version(&tx, 21).is_err());
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                version
            );
        }
    }
}
