//! Immutable, optional attempt observations. These records grant no execution,
//! retry, fallback, provider qualification, or retirement authority.
//! Legacy attempts without an observation remain explicitly unknown.
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

type PResult<T> = std::result::Result<T, ProtocolError>;
const TABLE: &str = "job_attempt_outcomes";
const CREATE: &str = "CREATE TABLE job_attempt_outcomes(attempt TEXT PRIMARY KEY NOT NULL REFERENCES attempts(id),job TEXT NOT NULL REFERENCES jobs(id),manifest_sha256 TEXT NOT NULL,record TEXT NOT NULL)";
const MAX_RECORD: usize = 32 * 1024;
const MAX_MANIFEST: usize = 1024 * 1024;
const MAX_PAGE: usize = 512 * 1024;
const MAX_ATTEMPTS: i64 = 128;

fn internal(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, error.to_string())
}
fn mismatch(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn present(db: &Connection) -> Result<bool> {
    // Also discover a same-named view/index so initialization cannot repair it.
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
        [TABLE],
        |row| row.get(0),
    )?)
}
fn layout(db: &Connection) -> Result<()> {
    let sql: Option<String> = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",
        [TABLE], |row| row.get(0),
    ).optional()?;
    ensure!(
        sql.as_deref() == Some(CREATE),
        "job attempt outcome layout differs"
    );
    Ok(())
}

/// Read-only preflight. No global lifetime job limit and no JSON allocation.
/// Per-job reads and writes validate bounded manifests and record semantics.
pub(crate) fn validate(db: &Connection) -> Result<()> {
    if !present(db)? {
        return Ok(());
    }
    layout(db)?;
    let invalid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_attempt_outcomes WHERE typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(manifest_sha256)!='text' OR length(CAST(manifest_sha256 AS BLOB))!=64 OR manifest_sha256 GLOB '*[^0-9a-f]*' OR typeof(record)!='text' OR length(CAST(record AS BLOB)) NOT BETWEEN 1 AND 32768)",
        [], |row| row.get(0),
    )?;
    ensure!(
        !invalid,
        "job attempt outcome scalar or byte admission differs"
    );
    let populated: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_attempt_outcomes)",
        [],
        |row| row.get(0),
    )?;
    if !populated {
        return Ok(());
    }
    let orphan: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_attempt_outcomes o LEFT JOIN attempts a ON a.id=o.attempt AND a.job=o.job LEFT JOIN jobs j ON j.id=o.job WHERE a.id IS NULL OR j.id IS NULL)",
        [], |row| row.get(0),
    )?;
    ensure!(!orphan, "job attempt outcome membership differs");
    Ok(())
}

/// Auxiliary metadata only. Never changes any underlying or outer reader header.
pub(crate) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    if present(tx)? {
        validate(tx)?;
    } else {
        tx.execute_batch(CREATE)?;
    }
    Ok(())
}

struct JobContext {
    current: AttemptId,
    revision: u64,
    source: SourceVersionId,
}

fn job_context(db: &Connection, project: &ProjectId, job: &JobId) -> PResult<JobContext> {
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE id=?1 AND project=?2)",
            params![job.as_str(), project.as_str()],
            |row| row.get(0),
        )
        .map_err(internal)?;
    if !exists {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "job not found in project",
        ));
    }
    let row: Option<(String, i64, String, i64)> = db.query_row(
        "SELECT attempt,base_revision,source,attempts FROM jobs WHERE id=?1 AND project=?2 AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB)) BETWEEN 1 AND 128 AND typeof(base_revision)='integer' AND base_revision>=0 AND typeof(source)='text' AND length(CAST(source AS BLOB)) BETWEEN 1 AND 128 AND typeof(attempts)='integer' AND attempts BETWEEN 1 AND 128 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576",
        params![job.as_str(), project.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional().map_err(internal)?;
    let (current, revision, source, count) =
        row.ok_or_else(|| mismatch("job attempt header admission differs"))?;
    let actual: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM attempts WHERE job=?1 LIMIT 129)",
            [job.as_str()],
            |row| row.get(0),
        )
        .map_err(internal)?;
    if actual != count || actual > MAX_ATTEMPTS {
        return Err(mismatch(
            "job attempt counter differs from retained history",
        ));
    }
    let bound: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM jobs j JOIN attempts a ON a.id=j.attempt AND a.job=j.id WHERE j.id=?1 AND j.project=?2 AND a.manifest=j.manifest)",
        params![job.as_str(), project.as_str()], |row| row.get(0),
    ).map_err(internal)?;
    if !bound {
        return Err(mismatch(
            "current job attempt or manifest membership differs",
        ));
    }
    // Check all per-job scalars before selecting any caller-owned JSON or key.
    let invalid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM attempts WHERE job=?1 AND (typeof(id)!='text' OR length(CAST(id AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(state)!='text' OR length(CAST(state AS BLOB)) NOT BETWEEN 1 AND 64 OR typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 1048576))",
        [job.as_str()], |row| row.get(0),
    ).map_err(internal)?;
    if invalid {
        return Err(mismatch("job attempt scalar or manifest admission differs"));
    }
    Ok(JobContext {
        current: AttemptId::new(current).map_err(internal)?,
        revision: revision as u64,
        source: SourceVersionId::new(source).map_err(internal)?,
    })
}

fn manifest(
    raw: &str,
    project: &ProjectId,
    job: &JobId,
    attempt: &AttemptId,
    context: &JobContext,
) -> PResult<WorkerRequest> {
    if raw.is_empty() || raw.len() > MAX_MANIFEST {
        return Err(mismatch("job attempt manifest byte admission differs"));
    }
    let request: WorkerRequest = serde_json::from_str(raw).map_err(internal)?;
    request.validate()?;
    if request.project_id != *project
        || request.job_id != *job
        || request.attempt_id != *attempt
        || request.base_revision.value() != context.revision
        || request.source.source_version != context.source
        || serde_json::to_string(&request).map_err(internal)? != raw
    {
        return Err(mismatch("job attempt immutable manifest identity differs"));
    }
    Ok(request)
}

fn declared_profile(request: &WorkerRequest) -> Option<DeclaredInferenceProfile> {
    match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.clone(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref().cloned(),
        WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
    }
}

fn read_outcome(
    db: &Connection,
    request: &WorkerRequest,
    manifest_sha256: &str,
) -> PResult<Option<JobAttemptOutcome>> {
    let present = present(db).map_err(internal)?;
    if !present {
        return Ok(None);
    }
    layout(db).map_err(internal)?;
    // Scalar admission precedes get<String>, including when called without startup.
    let invalid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_attempt_outcomes WHERE attempt=?1 AND (typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(manifest_sha256)!='text' OR length(CAST(manifest_sha256 AS BLOB))!=64 OR manifest_sha256 GLOB '*[^0-9a-f]*' OR typeof(record)!='text' OR length(CAST(record AS BLOB)) NOT BETWEEN 1 AND 32768))",
        [request.attempt_id.as_str()], |row| row.get(0),
    ).map_err(internal)?;
    if invalid {
        return Err(mismatch("job attempt outcome byte admission differs"));
    }
    let row: Option<(String, String, String)> = db
        .query_row(
            "SELECT job,manifest_sha256,record FROM job_attempt_outcomes WHERE attempt=?1",
            [request.attempt_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((job, stored_digest, raw)) = row else {
        return Ok(None);
    };
    if job != request.job_id.as_str() || stored_digest != manifest_sha256 {
        return Err(mismatch("job attempt outcome indexed identity differs"));
    }
    let outcome: JobAttemptOutcome = serde_json::from_str(&raw).map_err(internal)?;
    outcome.validate_for(request)?;
    if outcome.manifest_sha256 != manifest_sha256
        || serde_json::to_string(&outcome).map_err(internal)? != raw
    {
        return Err(mismatch("job attempt outcome canonical identity differs"));
    }
    Ok(Some(outcome))
}

/// Retain a settled observation even after cancellation or a successor attempt.
/// Never writes jobs/attempts, infers retirement, or changes execution authority.
pub(crate) fn retain(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    outcome: &JobAttemptOutcome,
) -> PResult<()> {
    outcome.validate_for(request)?;
    let context = job_context(tx, &request.project_id, &request.job_id)?;
    let retained: Option<String> = tx
        .query_row(
            "SELECT manifest FROM attempts WHERE id=?1 AND job=?2",
            params![request.attempt_id.as_str(), request.job_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    let retained =
        retained.ok_or_else(|| mismatch("job attempt outcome has no retained attempt"))?;
    manifest(
        &retained,
        &request.project_id,
        &request.job_id,
        &request.attempt_id,
        &context,
    )?;
    if serde_json::to_string(request).map_err(internal)? != retained
        || digest(retained.as_bytes()) != outcome.manifest_sha256
    {
        return Err(mismatch(
            "job attempt outcome differs from retained manifest",
        ));
    }
    let raw = serde_json::to_string(outcome).map_err(internal)?;
    if raw.len() > MAX_RECORD {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "job attempt outcome exceeds record admission",
        ));
    }
    if let Some(old) = read_outcome(tx, request, &outcome.manifest_sha256)? {
        if serde_json::to_string(&old).map_err(internal)? == raw {
            return Ok(());
        }
        return Err(ProtocolError::new(
            ErrorCode::RequestConflict,
            "job attempt outcome is immutable",
        ));
    }
    // Engine startup initializes this layout. Do not create or repair it here.
    layout(tx).map_err(internal)?;
    tx.execute(
        "INSERT INTO job_attempt_outcomes(attempt,job,manifest_sha256,record) VALUES(?1,?2,?3,?4)",
        params![
            request.attempt_id.as_str(),
            request.job_id.as_str(),
            outcome.manifest_sha256,
            raw
        ],
    )
    .map_err(internal)?;
    Ok(())
}

/// Stable lexical AttemptId order, not a claimed chronological execution order.
/// The caller authenticates and requires current project Read authority.
#[cfg(test)]
pub(crate) fn page(
    db: &Connection,
    project: &ProjectId,
    job: &JobId,
    after: Option<&AttemptId>,
    limit: u16,
) -> PResult<JobAttemptPage> {
    page_with_fallback(db, project, job, after, limit, |_| Ok(None))
}

/// The caller resolves private, retained ledger data against each exact decoded
/// manifest. The result is historical metadata; this function creates no retry
/// authorization, evidence acceptance or current environment observation.
pub(crate) fn page_with_fallback(
    db: &Connection,
    project: &ProjectId,
    job: &JobId,
    after: Option<&AttemptId>,
    limit: u16,
    fallback: impl Fn(&WorkerRequest) -> PResult<Option<JobAttemptFallbackTrace>>,
) -> PResult<JobAttemptPage> {
    if !(1..=16).contains(&limit) {
        return Err(ProtocolError::invalid(
            "job attempt page limit must be 1..16",
        ));
    }
    let context = job_context(db, project, job)?;
    if let Some(after) = after {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND job=?2)",
                params![after.as_str(), job.as_str()],
                |row| row.get(0),
            )
            .map_err(internal)?;
        if !exists {
            return Err(ProtocolError::invalid(
                "job attempt cursor does not belong to this job",
            ));
        }
    }
    // Only bounded keys are collected. Parse one admitted manifest at a time.
    let mut statement = db
        .prepare(
            "SELECT id FROM attempts WHERE job=?1 AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT ?3",
        )
        .map_err(internal)?;
    let keys = statement
        .query_map(
            params![
                job.as_str(),
                after.map(AttemptId::as_str),
                i64::from(limit) + 1
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(internal)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(internal)?;
    let more = keys.len() > usize::from(limit);
    let mut attempts = Vec::with_capacity(keys.len().min(usize::from(limit)));
    for key in keys.into_iter().take(usize::from(limit)) {
        let attempt = AttemptId::new(key).map_err(internal)?;
        let (raw, state): (String, String) = db
            .query_row(
                "SELECT manifest,state FROM attempts WHERE id=?1 AND job=?2",
                params![attempt.as_str(), job.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(internal)?;
        let request = manifest(&raw, project, job, &attempt, &context)?;
        let manifest_sha256 = digest(raw.as_bytes());
        let profile = declared_profile(&request);
        let profile_sha256 = profile
            .as_ref()
            .map(|p| serde_json::to_vec(p).map(|bytes| digest(&bytes)))
            .transpose()
            .map_err(internal)?;
        let outcome = read_outcome(db, &request, &manifest_sha256)?;
        let fallback = fallback(&request)?;
        let tagged = request
            .dependencies
            .iter()
            .filter(|identity| {
                identity
                    .artifact_id
                    .as_str()
                    .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
            })
            .collect::<Vec<_>>();
        match (&fallback, tagged.as_slice()) {
            (None, []) => {}
            (Some(trace), [selection]) if trace.selection == **selection => {
                trace.validate_for_attempt(&request.attempt_id)?;
            }
            _ => {
                return Err(mismatch(
                    "fallback history differs from the retained selection dependency",
                ))
            }
        }
        let record = JobAttemptRecord {
            attempt_id: attempt,
            state,
            manifest_sha256,
            declared_profile: profile,
            declared_profile_sha256: profile_sha256,
            outcome,
            fallback,
        };
        record.validate()?;
        attempts.push(record);
    }
    let next_after = if more {
        attempts.last().map(|record| record.attempt_id.clone())
    } else {
        None
    };
    let page = JobAttemptPage {
        schema_version: 1,
        project_id: project.clone(),
        job_id: job.clone(),
        current_attempt: context.current,
        attempts,
        next_after,
        history_only: true,
    };
    page.validate()?;
    if serde_json::to_vec(&page).map_err(internal)?.len() > MAX_PAGE {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "job attempt page exceeds encoded admission",
        ));
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=20;CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT NOT NULL,project TEXT NOT NULL,base_revision INTEGER NOT NULL,source TEXT NOT NULL,manifest TEXT NOT NULL,state TEXT NOT NULL,attempts INTEGER NOT NULL);CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT NOT NULL REFERENCES jobs(id),manifest TEXT NOT NULL,state TEXT NOT NULL);").unwrap();
        db
    }
    fn request(attempt: &str) -> WorkerRequest {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        let artifact = ArtifactReference {
            identity: identity.clone(),
            path: "/nonexistent/metadata-only/tool".into(),
        };
        let request = WorkerRequest {
            cuda_execution: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new(attempt).unwrap(),
            job_id: JobId::new("job").unwrap(),
            project_id: ProjectId::new("project").unwrap(),
            base_revision: RevisionId::new(0),
            source: SourceArtifact {
                source_version: SourceVersionId::new("source").unwrap(),
                path: "/nonexistent/metadata-only/source".into(),
                identity,
            },
            independent_audio: None,
            output_dir: format!("/nonexistent/metadata-only/{attempt}").into(),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                settings: Box::default(),
                model: None,
            },
            budget: ResourceBudget {
                memory_bytes: 1024 * 1024,
                output_bytes: 1024 * 1024,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![],
            tools: WorkerTools {
                ffmpeg: artifact.clone(),
                ffprobe: artifact,
                onnx_runtime: None,
            },
        };
        request.validate().unwrap();
        request
    }
    fn seed(db: &Connection, request: &WorkerRequest, state: &str) {
        let raw = serde_json::to_string(request).unwrap();
        db.execute("INSERT INTO jobs VALUES(?1,?2,?3,?4,?5,?6,?7,1) ON CONFLICT(id) DO UPDATE SET attempt=excluded.attempt,manifest=excluded.manifest,state=excluded.state,attempts=jobs.attempts+1", params![request.job_id.as_str(),request.attempt_id.as_str(),request.project_id.as_str(),request.base_revision.value(),request.source.source_version.as_str(),raw,state]).unwrap();
        db.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,?4)",
            params![
                request.attempt_id.as_str(),
                request.job_id.as_str(),
                raw,
                state
            ],
        )
        .unwrap();
    }
    fn initialize_db(db: &mut Connection) {
        let tx = db.transaction().unwrap();
        initialize(&tx).unwrap();
        tx.commit().unwrap();
    }
    fn outcome(request: &WorkerRequest) -> JobAttemptOutcome {
        JobAttemptOutcome {
            schema_version: 1,
            manifest_sha256: digest(&serde_json::to_vec(request).unwrap()),
            engine_epoch: AttemptId::new("engine-epoch").unwrap(),
            validation_error: None,
            worker_retirement: None,
        }
    }
    fn one(db: &Connection, request: &WorkerRequest) -> JobAttemptRecord {
        page(db, &request.project_id, &request.job_id, None, 8)
            .unwrap()
            .attempts
            .remove(0)
    }
    fn record(
        db: &mut Connection,
        request: &WorkerRequest,
        outcome: &JobAttemptOutcome,
    ) -> PResult<()> {
        let tx = db.transaction().unwrap();
        retain(&tx, request, outcome)?;
        tx.commit().map_err(internal)
    }
    fn json(value: &impl serde::Serialize) -> serde_json::Value {
        serde_json::to_value(value).unwrap()
    }

    fn fallback_trace() -> JobAttemptFallbackTrace {
        let sha256 = "b".repeat(64);
        JobAttemptFallbackTrace {
            schema_version: 1,
            selection: ArtifactIdentity {
                artifact_id: ArtifactId::new(format!(
                    "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{sha256}"
                ))
                .unwrap(),
                sha256,
                byte_len: 100,
            },
            kind: JobAttemptFallbackKind::Initial,
            ordinal: 0,
            predecessor: None,
            reference: InferenceFallbackReference {
                dossier_id: ArtifactId::new("c".repeat(64)).unwrap(),
                expected_revision: 2,
                expected_report_sha256: "d".repeat(64),
            },
            observed_environment_sha256: "e".repeat(64),
            admitted_at_unix_ms: 1,
            expires_at_unix_ms: 100,
            history_only: true,
        }
    }
    fn fallback_request(attempt: &str) -> WorkerRequest {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.attempt_id = AttemptId::new(attempt).unwrap();
        request.cuda_execution = None;
        if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
            let profile = settings.inference_profile.as_mut().unwrap();
            profile.schema_version = 1;
            profile.provider = InferenceProvider::Cpu;
            profile.cuda_resources = None;
        }
        request.dependencies.push(fallback_trace().selection);
        request.validate().unwrap();
        request
    }

    #[test]
    fn fallback_pages_resolve_exact_manifests_once_and_keep_lexical_cursor() {
        use std::cell::RefCell;
        let db = database();
        for id in ["attempt-c", "attempt-b", "attempt-a"] {
            seed(&db, &fallback_request(id), "interrupted");
        }
        let request = fallback_request("attempt-a");
        let seen = RefCell::new(Vec::new());
        let resolver = |manifest: &WorkerRequest| {
            seen.borrow_mut().push(manifest.attempt_id.clone());
            assert_eq!(manifest.job_id, request.job_id);
            assert_eq!(manifest.project_id, request.project_id);
            assert_eq!(
                serde_json::to_vec(manifest).unwrap(),
                serde_json::to_vec(&fallback_request(manifest.attempt_id.as_str())).unwrap()
            );
            let mut trace = fallback_trace();
            if manifest.attempt_id.as_str() != "attempt-a" {
                trace.kind = JobAttemptFallbackKind::Fallback;
                trace.ordinal = 1;
                trace.predecessor = Some(AttemptId::new("attempt-a").unwrap());
            }
            Ok(Some(trace))
        };
        let mut after = None;
        let mut attempts = Vec::new();
        loop {
            let page = page_with_fallback(
                &db,
                &request.project_id,
                &request.job_id,
                after.as_ref(),
                1,
                resolver,
            )
            .unwrap();
            assert!(page.history_only);
            assert!(serde_json::to_vec(&page).unwrap().len() <= MAX_PAGE);
            assert_eq!(page.current_attempt, request.attempt_id);
            for record in &page.attempts {
                assert!(record.fallback.as_ref().unwrap().history_only);
                assert!(record.declared_profile.is_some());
            }
            attempts.extend(page.attempts.iter().map(|record| record.attempt_id.clone()));
            after = page.next_after;
            if after.is_none() {
                break;
            }
        }
        assert_eq!(attempts, seen.into_inner());
        assert_eq!(
            attempts.iter().map(AttemptId::as_str).collect::<Vec<_>>(),
            ["attempt-a", "attempt-b", "attempt-c"]
        );
        assert!(!present(&db).unwrap());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }

    #[test]
    fn fallback_pages_refuse_missing_rebound_or_invalid_trace_without_repair() {
        let db = database();
        let request = fallback_request("attempt-a");
        seed(&db, &request, "queued");
        assert!(page(&db, &request.project_id, &request.job_id, None, 8).is_err());
        let mut changed = fallback_trace();
        changed.selection.sha256 = "f".repeat(64);
        changed.selection.artifact_id = ArtifactId::new(format!(
            "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}",
            changed.selection.sha256
        ))
        .unwrap();
        assert!(
            page_with_fallback(&db, &request.project_id, &request.job_id, None, 8, |_| Ok(
                Some(changed.clone())
            ))
            .is_err()
        );
        let mut self_predecessor = fallback_trace();
        self_predecessor.kind = JobAttemptFallbackKind::Resume;
        self_predecessor.predecessor = Some(request.attempt_id.clone());
        assert!(
            page_with_fallback(&db, &request.project_id, &request.job_id, None, 8, |_| Ok(
                Some(self_predecessor.clone())
            ))
            .is_err()
        );
        let error = page_with_fallback(&db, &request.project_id, &request.job_id, None, 8, |_| {
            Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "retained ledger unavailable",
            ))
        })
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert_eq!(error.message, "retained ledger unavailable");
        assert!(!present(&db).unwrap());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn fallback_metadata_cannot_attach_to_an_untagged_or_implicit_attempt() {
        let db = database();
        let mut request = request("legacy");
        seed(&db, &request, "failed");
        let legacy = page(&db, &request.project_id, &request.job_id, None, 8).unwrap();
        assert!(legacy.attempts[0].fallback.is_none());
        assert!(
            page_with_fallback(&db, &request.project_id, &request.job_id, None, 8, |_| Ok(
                Some(fallback_trace())
            ))
            .is_err()
        );
        request.attempt_id = AttemptId::new("tagged-implicit").unwrap();
        request.dependencies.push(fallback_trace().selection);
        seed(&db, &request, "failed");
        assert!(page_with_fallback(
            &db,
            &request.project_id,
            &request.job_id,
            Some(&AttemptId::new("legacy").unwrap()),
            8,
            |_| Ok(Some(fallback_trace()))
        )
        .is_err());
        assert!(!present(&db).unwrap());
    }

    #[test]
    fn optional_legacy_history_is_unknown_without_schema_or_retirement_inference() {
        let mut db = database();
        let request = request("attempt");
        seed(&db, &request, "completed");
        validate(&db).unwrap();
        let before = one(&db, &request);
        assert!(before.outcome.is_none());
        assert!(before.declared_profile.is_none());
        assert!(before.declared_profile_sha256.is_none());
        assert!(!present(&db).unwrap());
        initialize_db(&mut db);
        initialize_db(&mut db);
        assert_eq!(json(&before), json(&one(&db, &request)));
        for state in [
            "cancelled",
            "interrupted",
            "failed",
            "running",
            "assistant_paused",
        ] {
            db.execute("UPDATE attempts SET state=?1", [state]).unwrap();
            let current = one(&db, &request);
            assert_eq!(current.state, state);
            assert!(current.outcome.is_none());
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }

    #[test]
    fn malformed_table_or_scalar_is_refused_without_repair() {
        for sql in [
            "CREATE TABLE job_attempt_outcomes(record TEXT)",
            "CREATE VIEW job_attempt_outcomes AS SELECT 1 AS record",
        ] {
            let mut db = database();
            db.execute_batch(sql).unwrap();
            let before: String = db
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name='job_attempt_outcomes'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(validate(&db).is_err());
            let tx = db.transaction().unwrap();
            assert!(initialize(&tx).is_err());
            tx.commit().unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT sql FROM sqlite_master WHERE name='job_attempt_outcomes'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                before
            );
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                20
            );
        }
        let mut db = database();
        let request = request("attempt");
        seed(&db, &request, "failed");
        initialize_db(&mut db);
        db.execute(
            "INSERT INTO job_attempt_outcomes VALUES(?1,?2,?3,?4)",
            params![
                request.attempt_id.as_str(),
                request.job_id.as_str(),
                "a".repeat(64),
                "x".repeat(MAX_RECORD + 1)
            ],
        )
        .unwrap();
        assert!(validate(&db).is_err());
        assert!(page(&db, &request.project_id, &request.job_id, None, 8).is_err());
        assert_eq!(
            db.query_row("SELECT length(record) FROM job_attempt_outcomes", [], |r| r
                .get::<_, usize>(0))
                .unwrap(),
            MAX_RECORD + 1
        );
    }

    #[test]
    fn immutable_retention_replays_exactly_and_refuses_conflicts() {
        let mut db = database();
        let request = request("attempt");
        seed(&db, &request, "failed");
        initialize_db(&mut db);
        let original = outcome(&request);
        record(&mut db, &request, &original).unwrap();
        record(&mut db, &request, &original).unwrap();
        let mut different = original.clone();
        different.validation_error = Some(JobAttemptDiagnostic::from_error(
            &ProtocolError::invalid("different observation"),
        ));
        assert_eq!(
            record(&mut db, &request, &different).unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert_eq!(json(&one(&db, &request).outcome.unwrap()), json(&original));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM job_attempt_outcomes", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn cancelled_and_stale_observations_do_not_change_attempt_or_current_job() {
        let mut db = database();
        let previous = request("attempt-a");
        let current = request("attempt-b");
        seed(&db, &previous, "cancelled");
        seed(&db, &current, "running");
        initialize_db(&mut db);
        let mut observed = outcome(&previous);
        observed.worker_retirement = Some(WorkerRetirementObservation {
            worker_error: Some(JobAttemptDiagnostic::from_error(&ProtocolError::new(
                ErrorCode::Cancelled,
                "worker observed cancellation",
            ))),
            provider_unavailable: None,
        });
        observed.validation_error = observed
            .worker_retirement
            .as_ref()
            .unwrap()
            .worker_error
            .clone();
        record(&mut db, &previous, &observed).unwrap();
        let page = page(&db, &current.project_id, &current.job_id, None, 8).unwrap();
        assert_eq!(page.current_attempt, current.attempt_id);
        assert_eq!(page.attempts[0].state, "cancelled");
        assert_eq!(page.attempts[1].state, "running");
        assert!(page.attempts[0]
            .outcome
            .as_ref()
            .unwrap()
            .worker_retirement
            .is_some());
        assert!(page.attempts[1].outcome.is_none());
        assert_eq!(
            db.query_row("SELECT state FROM jobs", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "running"
        );
    }

    #[test]
    fn manifest_digest_scope_and_counter_mismatches_refuse_without_publication() {
        let mut db = database();
        let request = request("attempt");
        seed(&db, &request, "failed");
        initialize_db(&mut db);
        let mut wrong = outcome(&request);
        wrong.manifest_sha256 = "b".repeat(64);
        assert!(record(&mut db, &request, &wrong).is_err());
        let mut other = request.clone();
        other.output_dir = "/different/metadata-only".into();
        assert!(record(&mut db, &other, &outcome(&other)).is_err());
        db.execute("UPDATE jobs SET attempts=2", []).unwrap();
        assert!(page(&db, &request.project_id, &request.job_id, None, 8).is_err());
        db.execute("UPDATE jobs SET attempts=1", []).unwrap();
        db.execute("UPDATE attempts SET manifest=manifest || ' '", [])
            .unwrap();
        assert!(record(&mut db, &request, &outcome(&request)).is_err());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM job_attempt_outcomes", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn project_scoped_pages_check_cursor_and_limits_and_never_create_history() {
        let db = database();
        let request = request("attempt");
        seed(&db, &request, "failed");
        let wrong = ProjectId::new("other-project").unwrap();
        assert_eq!(
            page(&db, &wrong, &request.job_id, None, 8)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        for limit in [0, 17, u16::MAX] {
            assert_eq!(
                page(&db, &request.project_id, &request.job_id, None, limit)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(
            page(
                &db,
                &request.project_id,
                &request.job_id,
                Some(&AttemptId::new("absent").unwrap()),
                8
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        assert!(!present(&db).unwrap());
        let mut foreign = request.clone();
        foreign.job_id = JobId::new("other-job").unwrap();
        foreign.attempt_id = AttemptId::new("other-attempt").unwrap();
        seed(&db, &foreign, "failed");
        assert_eq!(
            page(
                &db,
                &request.project_id,
                &request.job_id,
                Some(&foreign.attempt_id),
                8
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        let last = page(
            &db,
            &request.project_id,
            &request.job_id,
            Some(&request.attempt_id),
            8,
        )
        .unwrap();
        assert!(last.attempts.is_empty() && last.next_after.is_none() && last.history_only);
    }

    #[test]
    fn pagination_is_bounded_lexical_and_has_no_duplicate_or_missing_attempt() {
        let db = database();
        let mut expected = vec![];
        for n in (0..35).rev() {
            let request = request(&format!("attempt-{n:03}"));
            seed(&db, &request, "interrupted");
            expected.push(request.attempt_id);
        }
        expected.reverse();
        let request = request("attempt-000");
        let mut after = None;
        let mut seen = vec![];
        loop {
            let page = page(&db, &request.project_id, &request.job_id, after.as_ref(), 8).unwrap();
            assert!(page.attempts.len() <= 8);
            assert_eq!(page.current_attempt, request.attempt_id);
            seen.extend(page.attempts.into_iter().map(|a| a.attempt_id));
            after = page.next_after;
            if after.is_none() {
                break;
            }
        }
        assert_eq!(seen, expected);
    }

    #[test]
    fn lazy_semantic_validation_refuses_tampered_outcome_and_orphan_rows() {
        let mut db = database();
        let request = request("attempt");
        seed(&db, &request, "failed");
        initialize_db(&mut db);
        record(&mut db, &request, &outcome(&request)).unwrap();
        db.execute("UPDATE job_attempt_outcomes SET record='{}'", [])
            .unwrap();
        validate(&db).unwrap();
        assert!(page(&db, &request.project_id, &request.job_id, None, 8).is_err());
        db.execute_batch(
            "PRAGMA foreign_keys=OFF;UPDATE job_attempt_outcomes SET job='different';",
        )
        .unwrap();
        assert!(validate(&db).is_err());
    }

    #[test]
    fn provider_evidence_requires_exact_explicit_profile_and_original_worker_error() {
        let mut db = database();
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.cuda_execution = None;
        if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
            let profile = settings.inference_profile.as_mut().unwrap();
            profile.schema_version = 1;
            profile.provider = InferenceProvider::Cpu;
            profile.cuda_resources = None;
        }
        request.validate().unwrap();
        seed(&db, &request, "failed");
        initialize_db(&mut db);
        let profile = declared_profile(&request).unwrap();
        let hash = digest(&serde_json::to_vec(&profile).unwrap());
        let mut observed = outcome(&request);
        observed.worker_retirement = Some(WorkerRetirementObservation {
            worker_error: Some(JobAttemptDiagnostic::from_error(&ProtocolError::new(
                ErrorCode::Unavailable,
                "provider not listed",
            ))),
            provider_unavailable: Some(InferenceProviderUnavailable {
                schema_version: 1,
                provider: profile.provider,
                profile_sha256: hash.clone(),
                check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
            }),
        });
        observed.validation_error = observed
            .worker_retirement
            .as_ref()
            .unwrap()
            .worker_error
            .clone();
        let mut masked = observed.clone();
        masked.worker_retirement.as_mut().unwrap().worker_error = None;
        assert!(record(&mut db, &request, &masked).is_err());
        let mut retryable = observed.clone();
        retryable
            .worker_retirement
            .as_mut()
            .unwrap()
            .worker_error
            .as_mut()
            .unwrap()
            .retryable = true;
        assert!(record(&mut db, &request, &retryable).is_err());
        let mut wrong = observed.clone();
        wrong
            .worker_retirement
            .as_mut()
            .unwrap()
            .provider_unavailable
            .as_mut()
            .unwrap()
            .profile_sha256 = "b".repeat(64);
        assert!(record(&mut db, &request, &wrong).is_err());
        let mut wrong_provider = observed.clone();
        wrong_provider
            .worker_retirement
            .as_mut()
            .unwrap()
            .provider_unavailable
            .as_mut()
            .unwrap()
            .provider = InferenceProvider::Cuda;
        assert!(record(&mut db, &request, &wrong_provider).is_err());
        let mut wrong_error = observed.clone();
        wrong_error.worker_retirement.as_mut().unwrap().worker_error =
            Some(JobAttemptDiagnostic::from_error(&ProtocolError::new(
                ErrorCode::Cancelled,
                "cancelled",
            )));
        assert!(record(&mut db, &request, &wrong_error).is_err());
        let mut implicit = request.clone();
        if let WorkerOperation::Generate { settings, .. } = &mut implicit.operation {
            settings.inference_profile = None;
        }
        let mut implicit_observation = observed.clone();
        implicit_observation.manifest_sha256 = digest(&serde_json::to_vec(&implicit).unwrap());
        assert!(record(&mut db, &implicit, &implicit_observation).is_err());
        record(&mut db, &request, &observed).unwrap();
        let saved = one(&db, &request);
        assert_eq!(saved.declared_profile_sha256, Some(hash));
        assert_eq!(
            json(saved.declared_profile.as_ref().unwrap()),
            json(&profile)
        );
        assert_eq!(json(saved.outcome.as_ref().unwrap()), json(&observed));
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }
}
