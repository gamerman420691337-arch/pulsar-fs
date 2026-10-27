//! Durable fallback intent and attempt correspondence, never permission. The
//! Engine separately retains live settlement, acceptance, expiry and resources.
use super::*;
use std::collections::BTreeSet;

const JOBS: &str = "CREATE TABLE inference_fallback_jobs(job TEXT PRIMARY KEY NOT NULL REFERENCES jobs(id),selection_sha256 TEXT NOT NULL,selection TEXT NOT NULL,initial_attempt TEXT UNIQUE NOT NULL REFERENCES attempts(id))";
const ATTEMPTS: &str = "CREATE TABLE inference_fallback_attempts(attempt TEXT PRIMARY KEY NOT NULL REFERENCES attempts(id),job TEXT NOT NULL REFERENCES inference_fallback_jobs(job),manifest_sha256 TEXT NOT NULL,profile_sha256 TEXT NOT NULL,admission TEXT NOT NULL)";
const PENDING: &str = "CREATE TABLE inference_fallback_pending(job TEXT PRIMARY KEY NOT NULL REFERENCES inference_fallback_jobs(job),predecessor TEXT UNIQUE NOT NULL REFERENCES inference_fallback_attempts(attempt),engine_epoch TEXT NOT NULL,outcome_sha256 TEXT NOT NULL)";
const TABLES: [(&str, &str); 3] = [
    ("inference_fallback_jobs", JOBS),
    ("inference_fallback_attempts", ATTEMPTS),
    ("inference_fallback_pending", PENDING),
];
const MAX_SELECTION: usize = 64 * 1024;
const MAX_ADMISSION: usize = 128 * 1024;
const MAX_MANIFEST: usize = 1024 * 1024;
const MAX_ATTEMPTS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FallbackAdmissionKind {
    Initial,
    Resume,
    Fallback,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FallbackAdmission {
    pub(super) schema_version: u16,
    pub(super) kind: FallbackAdmissionKind,
    pub(super) ordinal: u8,
    pub(super) predecessor: Option<AttemptId>,
    pub(super) reference: InferenceFallbackReference,
    pub(super) observed_environment: InferenceEquivalenceEnvironment,
    pub(super) admitted_at_unix_ms: u64,
    pub(super) expires_at_unix_ms: u64,
}
#[derive(Clone, Debug)]
pub(super) struct RetainedFallbackJob {
    pub(super) selection: InferenceFallbackSelection,
    pub(super) admission: FallbackAdmission,
    pub(super) initial_attempt: AttemptId,
}
#[derive(Clone, Debug)]
pub(super) struct PendingFallback {
    pub(super) predecessor: AttemptId,
    pub(super) engine_epoch: AttemptId,
    pub(super) outcome_sha256: String,
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
fn conflict(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::RequestConflict, message)
}
fn ensure(value: bool, message: &str) -> PResult<()> {
    if value {
        Ok(())
    } else {
        Err(invalid(message))
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn bounded<T: Serialize>(value: &T, limit: usize) -> PResult<String> {
    let raw = encode(value)?;
    ensure(
        raw.len() <= limit,
        "fallback metadata exceeds byte admission",
    )?;
    Ok(raw)
}
fn equal<A: Serialize, B: Serialize>(a: &A, b: &B) -> PResult<bool> {
    Ok(encode(a)? == encode(b)?)
}
fn tagged(request: &WorkerRequest) -> PResult<Option<&ArtifactIdentity>> {
    let mut tags = request.dependencies.iter().filter(|d| {
        d.artifact_id
            .as_str()
            .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
    });
    let value = tags.next();
    ensure(tags.next().is_none(), "multiple fallback selection tags")?;
    Ok(value)
}
fn profile(request: &WorkerRequest) -> PResult<&DeclaredInferenceProfile> {
    let WorkerOperation::Generate {
        settings,
        model: Some(_),
        ..
    } = &request.operation
    else {
        return Err(invalid("fallback requires model-backed generation"));
    };
    settings
        .inference_profile
        .as_ref()
        .ok_or_else(|| invalid("fallback requires an explicit profile"))
}
fn profile_sha(request: &WorkerRequest) -> PResult<String> {
    Ok(digest(encode(profile(request)?)?.as_bytes()))
}

impl FallbackAdmission {
    pub(super) fn validate(&self) -> PResult<()> {
        ensure(
            self.schema_version == 1 && self.ordinal <= 8,
            "unsupported fallback admission schema or ordinal",
        )?;
        self.reference.validate()?;
        validate_sha256(self.reference.dossier_id.as_str())?;
        self.observed_environment.validate()?;
        ensure(
            self.admitted_at_unix_ms > 0
                && self.admitted_at_unix_ms < self.expires_at_unix_ms
                && self.expires_at_unix_ms <= i64::MAX as u64,
            "fallback admission clocks differ",
        )?;
        match self.kind {
            FallbackAdmissionKind::Initial => ensure(
                self.ordinal == 0 && self.predecessor.is_none(),
                "initial fallback admission has predecessor or ordinal",
            ),
            FallbackAdmissionKind::Resume => ensure(
                self.predecessor.is_some(),
                "resumed fallback admission lacks predecessor",
            ),
            FallbackAdmissionKind::Fallback => ensure(
                self.ordinal > 0 && self.predecessor.is_some(),
                "fallback admission lacks predecessor or ordinal",
            ),
        }
    }
    fn validate_for(
        &self,
        request: &WorkerRequest,
        selection: &InferenceFallbackSelection,
    ) -> PResult<()> {
        self.validate()?;
        request.validate()?;
        selection.validate()?;
        ensure(
            self.ordinal <= selection.maximum_fallbacks,
            "fallback admission exceeds selected limit",
        )?;
        let mut matches = 0;
        for reference in &selection.equivalents {
            if equal(reference, &self.reference)? {
                matches += 1;
            }
        }
        ensure(matches == 1, "fallback admission reference is not selected")?;
        let selected = selection.identity()?;
        ensure(
            tagged(request)? == Some(&selected) && selection.matches_worker_request(request)?,
            "fallback selection differs from immutable request",
        )?;
        let profile = profile(request)?;
        ensure(
            self.observed_environment
                .dependencies
                .contains(&profile.runtime)
                && request
                    .dependencies
                    .contains(&self.observed_environment.worker),
            "fallback environment runtime or worker correspondence differs",
        )?;
        bounded(self, MAX_ADMISSION)?;
        Ok(())
    }
}

fn layout(db: &Connection) -> PResult<bool> {
    crate::inference_fallback_reader::version(db).map_err(internal)?;
    let raw = crate::live_cpu_reader::version(db).map_err(internal)?;
    let temporary: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE (lower(name) GLOB 'inference_fallback_*' OR lower(tbl_name) GLOB 'inference_fallback_*') AND lower(name)!='inference_fallback_reader' AND lower(tbl_name)!='inference_fallback_reader')", [], |r|r.get(0)).map_err(internal)?;
    ensure(!temporary, "temporary fallback metadata is forbidden")?;
    let mut found = 0;
    for (name, sql) in TABLES {
        let count: i64 = db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE LIMIT 2)", [name], |r|r.get(0)).map_err(internal)?;
        if count != 0 {
            let exact:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)",params![name,sql.len() as i64,sql],|r|r.get(0)).map_err(internal)?;
            ensure(count == 1 && exact, "fallback metadata layout differs")?;
            found += 1;
        }
    }
    let objects:i64=db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE (lower(name) GLOB 'inference_fallback_*' OR lower(tbl_name) GLOB 'inference_fallback_*') AND lower(name)!='inference_fallback_reader' AND lower(tbl_name)!='inference_fallback_reader' AND sql IS NOT NULL LIMIT 5)",[],|r|r.get(0)).map_err(internal)?;
    ensure(
        objects == found && (found == 0 || found == 3),
        "partial or unexpected fallback metadata layout",
    )?;
    ensure(
        (found == 3) == (raw == 37),
        "fallback metadata requires exact reader37",
    )?;
    Ok(found == 3)
}
fn scalar_admission(db: &Connection) -> PResult<()> {
    for sql in [
        "SELECT EXISTS(SELECT 1 FROM main.inference_fallback_jobs WHERE typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(initial_attempt)!='text' OR length(CAST(initial_attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(selection_sha256)!='text' OR length(selection_sha256)!=64 OR selection_sha256 GLOB '*[^0-9a-f]*' OR typeof(selection)!='text' OR length(CAST(selection AS BLOB)) NOT BETWEEN 1 AND 65536)",
        "SELECT EXISTS(SELECT 1 FROM main.inference_fallback_attempts WHERE typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(manifest_sha256)!='text' OR length(manifest_sha256)!=64 OR manifest_sha256 GLOB '*[^0-9a-f]*' OR typeof(profile_sha256)!='text' OR length(profile_sha256)!=64 OR profile_sha256 GLOB '*[^0-9a-f]*' OR typeof(admission)!='text' OR length(CAST(admission AS BLOB)) NOT BETWEEN 1 AND 131072)",
        "SELECT EXISTS(SELECT 1 FROM main.inference_fallback_pending WHERE typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(predecessor)!='text' OR length(CAST(predecessor AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(engine_epoch)!='text' OR length(CAST(engine_epoch AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(outcome_sha256)!='text' OR length(outcome_sha256)!=64 OR outcome_sha256 GLOB '*[^0-9a-f]*')",
    ] { ensure(!db.query_row(sql,[],|r|r.get::<_,bool>(0)).map_err(internal)?, "fallback scalar or byte admission differs")?; }
    Ok(())
}
fn initialize(tx: &Transaction<'_>) -> PResult<()> {
    if layout(tx)? {
        return Ok(());
    }
    for (_, sql) in TABLES {
        tx.execute_batch(sql).map_err(internal)?;
    }
    crate::inference_fallback_reader::arm(tx).map_err(internal)?;
    Ok(())
}
fn savepoint<T>(tx: &Transaction<'_>, operation: impl FnOnce() -> PResult<T>) -> PResult<T> {
    tx.execute_batch("SAVEPOINT pulsar_fallback_ledger")
        .map_err(internal)?;
    match operation() {
        Ok(value) => {
            tx.execute_batch("RELEASE pulsar_fallback_ledger")
                .map_err(internal)?;
            Ok(value)
        }
        Err(error) => {
            tx.execute_batch("ROLLBACK TO pulsar_fallback_ledger; RELEASE pulsar_fallback_ledger")
                .map_err(internal)?;
            Err(error)
        }
    }
}

struct Job {
    id: JobId,
    project: ProjectId,
    revision: u64,
    source: SourceVersionId,
    current: AttemptId,
    state: String,
    count: usize,
    selection: InferenceFallbackSelection,
    initial: AttemptId,
}
fn load_job(db: &Connection, id: &JobId) -> PResult<Option<Job>> {
    let row:Option<(String,String,String)>=db.query_row("SELECT selection_sha256,selection,initial_attempt FROM main.inference_fallback_jobs WHERE job=?1 AND typeof(selection)='text' AND length(CAST(selection AS BLOB)) BETWEEN 1 AND 65536 AND typeof(selection_sha256)='text' AND length(selection_sha256)=64 AND typeof(initial_attempt)='text' AND length(CAST(initial_attempt AS BLOB)) BETWEEN 1 AND 128",[id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
    let Some((sha, raw, initial)) = row else {
        let present: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM main.inference_fallback_jobs WHERE job=?1)",
                [id.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        ensure(!present, "fallback job row exceeds admission")?;
        return Ok(None);
    };
    let selection: InferenceFallbackSelection = decode(&raw)?;
    selection.validate()?;
    ensure(
        bounded(&selection, MAX_SELECTION)? == raw && selection.identity()?.sha256 == sha,
        "fallback selection canonical identity differs",
    )?;
    let row:Option<(String,i64,String,String,String,i64)>=db.query_row("SELECT project,base_revision,source,attempt,state,attempts FROM main.jobs WHERE id=?1 AND typeof(project)='text' AND length(CAST(project AS BLOB)) BETWEEN 1 AND 128 AND typeof(base_revision)='integer' AND base_revision>=0 AND typeof(source)='text' AND length(CAST(source AS BLOB)) BETWEEN 1 AND 128 AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB)) BETWEEN 1 AND 128 AND typeof(state)='text' AND length(CAST(state AS BLOB)) BETWEEN 1 AND 64 AND typeof(attempts)='integer' AND attempts BETWEEN 1 AND 128",[id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(internal)?;
    let (project, revision, source, current, state, count) =
        row.ok_or_else(|| invalid("fallback job membership differs"))?;
    ensure(
        matches!(
            state.as_str(),
            "queued"
                | "running"
                | "completed"
                | "failed"
                | "cancelled"
                | "interrupted"
                | "assistant_paused"
                | "fallback_pending"
        ),
        "fallback job state is unknown",
    )?;
    let actual: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM main.attempts WHERE job=?1 LIMIT 129)",
            [id.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    ensure(actual == count, "fallback attempt counter differs")?;
    let current_bound:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM main.jobs j JOIN main.attempts a ON a.id=j.attempt AND a.job=j.id WHERE j.id=?1 AND typeof(j.manifest)='text' AND length(CAST(j.manifest AS BLOB)) BETWEEN 1 AND 1048576 AND a.manifest=j.manifest)",[id.as_str()],|r|r.get(0)).map_err(internal)?;
    ensure(current_bound, "fallback current attempt manifest differs")?;
    Ok(Some(Job {
        id: id.clone(),
        project: ProjectId::new(project).map_err(internal)?,
        revision: revision as u64,
        source: SourceVersionId::new(source).map_err(internal)?,
        current: AttemptId::new(current).map_err(internal)?,
        state,
        count: count as usize,
        selection,
        initial: AttemptId::new(initial).map_err(internal)?,
    }))
}
fn request_row(
    db: &Connection,
    job: &Job,
    attempt: &AttemptId,
) -> PResult<(WorkerRequest, String)> {
    let row:Option<(String,String)>=db.query_row("SELECT manifest,state FROM main.attempts WHERE id=?1 AND job=?2 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 AND typeof(state)='text' AND length(CAST(state AS BLOB)) BETWEEN 1 AND 64",params![attempt.as_str(),job.id.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
    let (raw, state) =
        row.ok_or_else(|| invalid("fallback attempt membership or byte bound differs"))?;
    ensure(
        matches!(
            state.as_str(),
            "queued"
                | "running"
                | "completed"
                | "failed"
                | "cancelled"
                | "interrupted"
                | "assistant_paused"
        ),
        "fallback attempt state is unknown",
    )?;
    let request: WorkerRequest = decode(&raw)?;
    request.validate()?;
    ensure(
        bounded(&request, MAX_MANIFEST)? == raw
            && request.job_id == job.id
            && request.attempt_id == *attempt
            && request.project_id == job.project
            && request.base_revision.value() == job.revision
            && request.source.source_version == job.source,
        "fallback immutable request identity differs",
    )?;
    ensure(
        job.selection.matches_worker_request(&request)?
            && tagged(&request)? == Some(&job.selection.identity()?),
        "fallback attempt lost selected policy",
    )?;
    Ok((request, state))
}
struct Attempt {
    request: WorkerRequest,
    state: String,
    profile_sha: String,
    admission: FallbackAdmission,
}
fn load_attempt(db: &Connection, job: &Job, id: &AttemptId) -> PResult<Attempt> {
    let row:Option<(String,String,String)>=db.query_row("SELECT manifest_sha256,profile_sha256,admission FROM main.inference_fallback_attempts WHERE attempt=?1 AND job=?2 AND typeof(manifest_sha256)='text' AND length(manifest_sha256)=64 AND typeof(profile_sha256)='text' AND length(profile_sha256)=64 AND typeof(admission)='text' AND length(CAST(admission AS BLOB)) BETWEEN 1 AND 131072",params![id.as_str(),job.id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
    let (manifest_sha, stored_profile, raw) =
        row.ok_or_else(|| invalid("fallback attempt declaration absent or oversized"))?;
    let (request, state) = request_row(db, job, id)?;
    let admission: FallbackAdmission = decode(&raw)?;
    admission.validate_for(&request, &job.selection)?;
    ensure(
        bounded(&admission, MAX_ADMISSION)? == raw
            && manifest_sha == digest(encode(&request)?.as_bytes())
            && stored_profile == profile_sha(&request)?,
        "fallback attempt canonical identities differ",
    )?;
    Ok(Attempt {
        request,
        state,
        profile_sha: stored_profile,
        admission,
    })
}
fn validate_pair(previous: &Attempt, next: &Attempt) -> PResult<()> {
    ensure(
        next.admission.predecessor.as_ref() == Some(&previous.request.attempt_id)
            && next.admission.admitted_at_unix_ms >= previous.admission.admitted_at_unix_ms,
        "fallback predecessor or observed clock order differs",
    )?;
    match next.admission.kind {
        FallbackAdmissionKind::Initial => Err(invalid("initial fallback record has a predecessor")),
        FallbackAdmissionKind::Fallback => {
            validate_inference_fallback_successor(&previous.request, &next.request)?;
            ensure(
                previous.admission.ordinal.checked_add(1) == Some(next.admission.ordinal)
                    && previous.profile_sha != next.profile_sha,
                "fallback ordinal/profile transition differs",
            )
        }
        FallbackAdmissionKind::Resume => {
            let old = &previous.request;
            let new = &next.request;
            ensure(
                old.attempt_id != new.attempt_id
                    && old.job_id == new.job_id
                    && old.project_id == new.project_id
                    && old.base_revision == new.base_revision
                    && old.source.source_version == new.source.source_version
                    && old.source.identity == new.source.identity
                    && equal(&old.independent_audio, &new.independent_audio)?
                    && equal(&old.operation, &new.operation)?
                    && equal(&old.budget, &new.budget)?
                    && old.tools.ffmpeg.identity == new.tools.ffmpeg.identity
                    && old.tools.ffprobe.identity == new.tools.ffprobe.identity
                    && previous.profile_sha == next.profile_sha
                    && previous.admission.ordinal == next.admission.ordinal,
                "fallback resume changed its exact generation basis",
            )
        }
    }
}
/// Traverses one bounded manifest/admission at a time; no lifetime job cap.
fn chain(db: &Connection, job: &Job, tail: &AttemptId, expected_count: usize) -> PResult<()> {
    ensure(
        expected_count > 0 && expected_count <= MAX_ATTEMPTS,
        "fallback chain count exceeds admission",
    )?;
    let mut ids = BTreeSet::new();
    let mut profiles = BTreeSet::new();
    let mut current = load_attempt(db, job, tail)?;
    loop {
        ensure(
            ids.insert(current.request.attempt_id.clone()) && ids.len() <= MAX_ATTEMPTS,
            "fallback predecessor chain cycles or exceeds bound",
        )?;
        if current.admission.kind != FallbackAdmissionKind::Resume {
            ensure(
                profiles.insert(current.profile_sha.clone()),
                "fallback profile cycle",
            )?;
        }
        let Some(previous) = &current.admission.predecessor else {
            ensure(
                current.admission.kind == FallbackAdmissionKind::Initial
                    && current.request.attempt_id == job.initial
                    && ids.len() == expected_count,
                "fallback initial/chain membership differs",
            )?;
            break;
        };
        let previous = load_attempt(db, job, previous)?;
        validate_pair(&previous, &current)?;
        current = previous;
    }
    let declared:i64=db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_fallback_attempts WHERE job=?1 LIMIT 129)",[job.id.as_str()],|r|r.get(0)).map_err(internal)?;
    ensure(
        declared == expected_count as i64,
        "fallback declarations contain detached attempts",
    )
}
fn outcome(db: &Connection, request: &WorkerRequest) -> PResult<(JobAttemptOutcome, String)> {
    crate::job_attempt_history::validate(db).map_err(internal)?;
    let row:Option<(String,String,String)>=db.query_row("SELECT job,manifest_sha256,record FROM main.job_attempt_outcomes WHERE attempt=?1 AND typeof(job)='text' AND length(CAST(job AS BLOB)) BETWEEN 1 AND 128 AND typeof(manifest_sha256)='text' AND length(manifest_sha256)=64 AND typeof(record)='text' AND length(CAST(record AS BLOB)) BETWEEN 1 AND 32768",[request.attempt_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
    let (job, sha, raw) =
        row.ok_or_else(|| invalid("fallback predecessor lacks persisted settlement"))?;
    let value: JobAttemptOutcome = decode(&raw)?;
    value.validate_for(request)?;
    ensure(
        job == request.job_id.as_str() && sha == value.manifest_sha256 && encode(&value)? == raw,
        "fallback settlement indexed/canonical identity differs",
    )?;
    ensure(
        value.worker_retirement.as_ref().is_some_and(|retired| {
            retired.provider_unavailable.is_some()
                && retired
                    .worker_error
                    .as_ref()
                    .is_some_and(|error| error.code == ErrorCode::Unavailable && !error.retryable)
        }),
        "fallback requires original settled provider absence",
    )?;
    Ok((value, digest(raw.as_bytes())))
}
fn pending_row(db: &Connection, id: &JobId) -> PResult<Option<PendingFallback>> {
    let row:Option<(String,String,String)>=db.query_row("SELECT predecessor,engine_epoch,outcome_sha256 FROM main.inference_fallback_pending WHERE job=?1 AND typeof(predecessor)='text' AND length(CAST(predecessor AS BLOB)) BETWEEN 1 AND 128 AND typeof(engine_epoch)='text' AND length(CAST(engine_epoch AS BLOB)) BETWEEN 1 AND 128 AND typeof(outcome_sha256)='text' AND length(outcome_sha256)=64",[id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
    match row {
        Some((prior, epoch, sha)) => {
            validate_sha256(&sha)?;
            Ok(Some(PendingFallback {
                predecessor: AttemptId::new(prior).map_err(internal)?,
                engine_epoch: AttemptId::new(epoch).map_err(internal)?,
                outcome_sha256: sha,
            }))
        }
        None => {
            let exists: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM main.inference_fallback_pending WHERE job=?1)",
                    [id.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            ensure(!exists, "fallback pending row exceeds admission")?;
            Ok(None)
        }
    }
}
fn validate_pending(db: &Connection, job: &Job) -> PResult<()> {
    match pending_row(db, &job.id)? {
        None => ensure(
            job.state != "fallback_pending",
            "fallback pending job lost its intent",
        ),
        Some(pending) => {
            let predecessor = load_attempt(db, job, &pending.predecessor)?;
            let (observed, sha) = outcome(db, &predecessor.request)?;
            ensure(
                job.state == "fallback_pending"
                    && job.current == pending.predecessor
                    && predecessor.state == "failed"
                    && observed.engine_epoch == pending.engine_epoch
                    && sha == pending.outcome_sha256
                    && predecessor.admission.ordinal < job.selection.maximum_fallbacks,
                "fallback pending intent no longer binds current failed predecessor",
            )
        }
    }
}

pub(super) fn retained(
    db: &Connection,
    request: &WorkerRequest,
) -> PResult<Option<RetainedFallbackJob>> {
    request.validate()?;
    if !layout(db)? {
        ensure(
            tagged(request)?.is_none(),
            "fallback selection has no retained ledger",
        )?;
        return Ok(None);
    }
    let Some(job) = load_job(db, &request.job_id)? else {
        ensure(
            tagged(request)?.is_none(),
            "fallback selection has no retained job",
        )?;
        return Ok(None);
    };
    chain(db, &job, &job.current, job.count)?;
    validate_pending(db, &job)?;
    let attempt = load_attempt(db, &job, &request.attempt_id)?;
    ensure(
        equal(&attempt.request, request)?,
        "fallback retained request differs",
    )?;
    Ok(Some(RetainedFallbackJob {
        selection: job.selection,
        admission: attempt.admission,
        initial_attempt: job.initial,
    }))
}
pub(super) fn pending(db: &Connection, job: &JobId) -> PResult<Option<PendingFallback>> {
    if !layout(db)? {
        return Ok(None);
    }
    let Some(context) = load_job(db, job)? else {
        return Ok(None);
    };
    chain(db, &context, &context.current, context.count)?;
    validate_pending(db, &context)?;
    pending_row(db, job)
}
fn insert_attempt(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    admission: &FallbackAdmission,
) -> PResult<()> {
    tx.execute(
        "INSERT INTO main.inference_fallback_attempts VALUES(?1,?2,?3,?4,?5)",
        params![
            request.attempt_id.as_str(),
            request.job_id.as_str(),
            digest(encode(request)?.as_bytes()),
            profile_sha(request)?,
            bounded(admission, MAX_ADMISSION)?
        ],
    )
    .map_err(internal)?;
    Ok(())
}
pub(super) fn declare_initial(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    selection: &InferenceFallbackSelection,
    admission: &FallbackAdmission,
) -> PResult<()> {
    savepoint(tx, || {
        admission.validate_for(request, selection)?;
        ensure(
            admission.kind == FallbackAdmissionKind::Initial,
            "initial fallback declaration has wrong kind",
        )?;
        initialize(tx)?;
        if let Some(job) = load_job(tx, &request.job_id)? {
            let old = load_attempt(tx, &job, &request.attempt_id)?;
            ensure(
                job.initial == request.attempt_id
                    && equal(&job.selection, selection)?
                    && equal(&old.request, request)?
                    && equal(&old.admission, admission)?,
                "initial fallback declaration is immutable",
            )?;
            chain(tx, &job, &job.current, job.count)?;
            return validate_pending(tx, &job);
        }
        let raw = bounded(selection, MAX_SELECTION)?;
        tx.execute(
            "INSERT INTO main.inference_fallback_jobs VALUES(?1,?2,?3,?4)",
            params![
                request.job_id.as_str(),
                selection.identity()?.sha256,
                raw,
                request.attempt_id.as_str()
            ],
        )
        .map_err(internal)?;
        let job =
            load_job(tx, &request.job_id)?.ok_or_else(|| invalid("new fallback job absent"))?;
        let (stored, state) = request_row(tx, &job, &request.attempt_id)?;
        ensure(
            job.count == 1
                && job.current == request.attempt_id
                && job.state == "queued"
                && state == "queued"
                && equal(&stored, request)?,
            "initial fallback requires exact first queued attempt",
        )?;
        insert_attempt(tx, request, admission)?;
        chain(tx, &job, &job.current, job.count)?;
        validate_pending(tx, &job)
    })
}
pub(super) fn mark_pending(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    epoch: &AttemptId,
) -> PResult<bool> {
    savepoint(tx, || {
        let Some(retained) = retained(tx, request)? else {
            return Ok(false);
        };
        if retained.admission.ordinal >= retained.selection.maximum_fallbacks {
            return Ok(false);
        }
        let job =
            load_job(tx, &request.job_id)?.ok_or_else(|| invalid("fallback job disappeared"))?;
        let predecessor = load_attempt(tx, &job, &request.attempt_id)?;
        let (observed, sha) = outcome(tx, request)?;
        ensure(
            observed.engine_epoch == *epoch,
            "fallback settlement engine epoch differs",
        )?;
        if let Some(existing) = pending_row(tx, &job.id)? {
            ensure(
                job.state == "fallback_pending"
                    && existing.predecessor == request.attempt_id
                    && existing.engine_epoch == *epoch
                    && existing.outcome_sha256 == sha,
                "fallback pending intent conflicts",
            )?;
            return Ok(true);
        }
        ensure(
            job.current == request.attempt_id
                && job.state == "failed"
                && predecessor.state == "failed",
            "fallback pending requires exact current failed attempt",
        )?;
        tx.execute(
            "INSERT INTO main.inference_fallback_pending VALUES(?1,?2,?3,?4)",
            params![
                job.id.as_str(),
                request.attempt_id.as_str(),
                epoch.as_str(),
                sha
            ],
        )
        .map_err(internal)?;
        let changed=tx.execute("UPDATE main.jobs SET state='fallback_pending',error=NULL WHERE id=?1 AND attempt=?2 AND state='failed'",params![job.id.as_str(),request.attempt_id.as_str()]).map_err(internal)?;
        ensure(changed == 1, "fallback pending compare-and-swap changed")?;
        Ok(true)
    })
}
fn record_next(
    tx: &Transaction<'_>,
    previous: &WorkerRequest,
    next: &WorkerRequest,
    admission: &FallbackAdmission,
    fallback: bool,
) -> PResult<()> {
    savepoint(tx, || {
        ensure(layout(tx)?, "fallback ledger absent")?;
        let job = load_job(tx, &next.job_id)?.ok_or_else(|| invalid("fallback job absent"))?;
        let old = load_attempt(tx, &job, &previous.attempt_id)?;
        ensure(
            equal(&old.request, previous)? && job.id == previous.job_id && job.count >= 2,
            "fallback previous manifest differs",
        )?;
        let (stored, state) = request_row(tx, &job, &next.attempt_id)?;
        ensure(
            job.current == next.attempt_id
                && job.state == "queued"
                && state == "queued"
                && equal(&stored, next)?,
            "successor requires the committed-in-transaction current queued attempt",
        )?;
        admission.validate_for(next, &job.selection)?;
        let expected_kind = if fallback {
            FallbackAdmissionKind::Fallback
        } else {
            FallbackAdmissionKind::Resume
        };
        ensure(
            admission.kind == expected_kind,
            "successor admission kind differs",
        )?;
        let fresh = Attempt {
            request: next.clone(),
            state,
            profile_sha: profile_sha(next)?,
            admission: admission.clone(),
        };
        validate_pair(&old, &fresh)?;
        let existing: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM main.inference_fallback_attempts WHERE attempt=?1)",
                [next.attempt_id.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if existing {
            let existing = load_attempt(tx, &job, &next.attempt_id)?;
            if !equal(&existing.admission, admission)? {
                return Err(conflict("fallback successor declaration is immutable"));
            }
            chain(tx, &job, &job.current, job.count)?;
            return validate_pending(tx, &job);
        }
        chain(tx, &job, &previous.attempt_id, job.count - 1)?;
        if fallback {
            let pending = pending_row(tx, &job.id)?
                .ok_or_else(|| invalid("fallback successor lacks pending intent"))?;
            let (observed, sha) = outcome(tx, previous)?;
            ensure(
                old.state == "failed"
                    && pending.predecessor == previous.attempt_id
                    && pending.engine_epoch == observed.engine_epoch
                    && pending.outcome_sha256 == sha,
                "fallback pending predecessor or outcome differs",
            )?;
        } else {
            ensure(
                matches!(old.state.as_str(), "interrupted" | "assistant_paused")
                    && pending_row(tx, &job.id)?.is_none(),
                "resume requires interrupted predecessor without pending fallback",
            )?;
        }
        insert_attempt(tx, next, admission)?;
        if fallback {
            tx.execute(
                "DELETE FROM main.inference_fallback_pending WHERE job=?1 AND predecessor=?2",
                params![job.id.as_str(), previous.attempt_id.as_str()],
            )
            .map_err(internal)?;
        }
        chain(tx, &job, &job.current, job.count)?;
        validate_pending(tx, &job)
    })
}
pub(super) fn record_successor(
    tx: &Transaction<'_>,
    previous: &WorkerRequest,
    next: &WorkerRequest,
    admission: &FallbackAdmission,
) -> PResult<()> {
    record_next(tx, previous, next, admission, true)
}
pub(super) fn record_resume(
    tx: &Transaction<'_>,
    previous: &WorkerRequest,
    next: &WorkerRequest,
    admission: &FallbackAdmission,
) -> PResult<()> {
    record_next(tx, previous, next, admission, false)
}
pub(super) fn clear_pending(tx: &Transaction<'_>, job: &JobId) -> PResult<()> {
    if !layout(tx)? {
        return Ok(());
    }
    tx.execute(
        "DELETE FROM main.inference_fallback_pending WHERE job=?1",
        [job.as_str()],
    )
    .map_err(internal)?;
    Ok(())
}
/// Restart does not reconstruct the live predecessor settlement or an execution lease.
/// Preserve the already-failed attempt and make its interrupted selection terminal.
pub(super) fn retire_pending_after_restart(tx: &Transaction<'_>) -> PResult<()> {
    if !layout(tx)? {
        return Ok(());
    }
    tx.execute("UPDATE jobs SET state='failed',error='automatic fallback interrupted by Engine restart; a fresh request is required' WHERE state='fallback_pending' AND id IN (SELECT job FROM inference_fallback_pending)", []).map_err(internal)?;
    tx.execute("DELETE FROM inference_fallback_pending", [])
        .map_err(internal)?;
    Ok(())
}

pub(super) fn ordinary_attempt_count(db: &Connection, job: &JobId) -> PResult<i64> {
    if !layout(db)? {
        return db
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE job=?1 AND state!='assistant_paused'",
                [job.as_str()],
                |r| r.get(0),
            )
            .map_err(internal);
    }
    let Some(context) = load_job(db, job)? else {
        return db
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE job=?1 AND state!='assistant_paused'",
                [job.as_str()],
                |r| r.get(0),
            )
            .map_err(internal);
    };
    chain(db, &context, &context.current, context.count)?;
    let mut count = 0;
    let mut current = load_attempt(db, &context, &context.current)?;
    loop {
        if current.state != "assistant_paused"
            && current.admission.kind != FallbackAdmissionKind::Fallback
        {
            count += 1;
        }
        match &current.admission.predecessor {
            Some(prior) => current = load_attempt(db, &context, prior)?,
            None => break,
        }
    }
    Ok(count)
}

/// Startup reads bounded records one job/attempt at a time. Missing history is
/// legacy only when neither a selection tag nor fallback_pending state survives.
pub(super) fn validate(db: &Connection) -> PResult<()> {
    let present = layout(db)?;
    let tables:i64=db.query_row("SELECT COUNT(*) FROM main.sqlite_master WHERE type='table' AND name IN ('jobs','attempts')",[],|r|r.get(0)).map_err(internal)?;
    ensure(
        tables == 0 || tables == 2,
        "incomplete job history for fallback validation",
    )?;
    if tables == 0 {
        ensure(!present, "fallback ledger lacks job history")?;
        return Ok(());
    }
    if present {
        scalar_admission(db)?;
        let orphan:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM main.inference_fallback_jobs f LEFT JOIN main.jobs j ON j.id=f.job WHERE j.id IS NULL) OR EXISTS(SELECT 1 FROM main.inference_fallback_attempts f LEFT JOIN main.inference_fallback_jobs j ON j.job=f.job LEFT JOIN main.attempts a ON a.id=f.attempt AND a.job=f.job WHERE j.job IS NULL OR a.id IS NULL) OR EXISTS(SELECT 1 FROM main.inference_fallback_pending p LEFT JOIN main.inference_fallback_jobs j ON j.job=p.job LEFT JOIN main.inference_fallback_attempts a ON a.attempt=p.predecessor AND a.job=p.job WHERE j.job IS NULL OR a.attempt IS NULL)",[],|r|r.get(0)).map_err(internal)?;
        ensure(
            !orphan,
            "fallback ledger membership contains orphan records",
        )?;
        let mut statement = db
            .prepare("SELECT job FROM main.inference_fallback_jobs ORDER BY job")
            .map_err(internal)?;
        let mut rows = statement.query([]).map_err(internal)?;
        while let Some(row) = rows.next().map_err(internal)? {
            let id = JobId::new(row.get::<_, String>(0).map_err(internal)?).map_err(internal)?;
            let job = load_job(db, &id)?
                .ok_or_else(|| invalid("fallback job disappeared during validation"))?;
            chain(db, &job, &job.current, job.count)?;
            validate_pending(db, &job)?;
        }
    }
    for (table, member) in [("jobs", "attempt"), ("attempts", "job")] {
        let invalid_json:bool=db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM main.{table} WHERE CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN NOT json_valid(manifest) ELSE 1 END)"),[],|r|r.get(0)).map_err(internal)?;
        ensure(
            !invalid_json,
            "fallback history manifest byte/JSON admission differs",
        )?;
        let sql=format!("SELECT CASE WHEN typeof(id)='text' AND length(CAST(id AS BLOB)) BETWEEN 1 AND 128 THEN id ELSE NULL END,CASE WHEN typeof({member})='text' AND length(CAST({member} AS BLOB)) BETWEEN 1 AND 128 THEN {member} ELSE NULL END,manifest FROM main.{table} WHERE EXISTS(SELECT 1 FROM json_tree(manifest) item WHERE item.key='artifact_id' AND item.type='text' AND item.value GLOB 'inference-fallback-selection-*') OR state='fallback_pending'");
        let mut statement = db.prepare(&sql).map_err(internal)?;
        let mut rows = statement.query([]).map_err(internal)?;
        while let Some(row) = rows.next().map_err(internal)? {
            ensure(present, "fallback tagged history lacks reader37 ledger")?;
            let id: String = row.get(0).map_err(internal)?;
            let member: String = row.get(1).map_err(internal)?;
            let raw: String = row.get(2).map_err(internal)?;
            let request: WorkerRequest = decode(&raw)?;
            let indexed = if table == "jobs" {
                id == request.job_id.as_str() && member == request.attempt_id.as_str()
            } else {
                id == request.attempt_id.as_str() && member == request.job_id.as_str()
            };
            ensure(
                bounded(&request, MAX_MANIFEST)? == raw && indexed,
                "fallback history is not canonical or indexed identity differs",
            )?;
            // Whole chains were checked once above. Bind this indexed row
            // directly instead of traversing its job again for every attempt.
            let job = load_job(db, &request.job_id)?
                .ok_or_else(|| invalid("fallback tag lacks retained policy"))?;
            let record = load_attempt(db, &job, &request.attempt_id)?;
            ensure(
                equal(&record.request, &request)?
                    && tagged(&request)? == Some(&job.selection.identity()?),
                "fallback history indexed tag differs",
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use sha2::{Digest, Sha256};

    fn fixture_identity(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 1,
        }
    }

    fn fixture_reference(n: u8) -> InferenceFallbackReference {
        InferenceFallbackReference {
            dossier_id: ArtifactId::new(format!("{n:064x}")).unwrap(),
            expected_revision: 1,
            expected_report_sha256: format!("{n:064x}"),
        }
    }

    fn fixture_selection() -> InferenceFallbackSelection {
        InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![fixture_reference(1), fixture_reference(2)],
            maximum_fallbacks: 2,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: None,
                independent_audio: InferenceFallbackIndependentAudio::Any,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        }
    }

    fn fixture_profile(runtime: u8) -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: fixture_identity(2),
            runtime: fixture_identity(runtime),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        }
    }

    fn fixture_request(attempt: &str, runtime: u8) -> WorkerRequest {
        let profile = fixture_profile(runtime);
        let request = WorkerRequest {
            cuda_execution: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new(attempt).unwrap(),
            job_id: JobId::new("fixture-job").unwrap(),
            project_id: ProjectId::new("fixture-project").unwrap(),
            base_revision: RevisionId::new(1),
            source: SourceArtifact {
                source_version: SourceVersionId::new("fixture-source").unwrap(),
                path: "/nonexistent/fallback-fixture/source".into(),
                identity: fixture_identity(1),
            },
            independent_audio: None,
            output_dir: format!("/nonexistent/fallback-fixture/{attempt}").into(),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                settings: Box::new(GenerationSettings {
                    model_input: Some(profile.preprocessing.input.clone()),
                    inference_profile: Some(profile),
                    ..GenerationSettings::default()
                }),
                model: Some(SourceArtifact {
                    source_version: SourceVersionId::new("fixture-model").unwrap(),
                    path: "/nonexistent/fallback-fixture/model".into(),
                    identity: fixture_identity(2),
                }),
            },
            budget: ResourceBudget {
                memory_bytes: 128,
                output_bytes: 128,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![
                fixture_identity(1),
                fixture_identity(2),
                fixture_identity(runtime),
                fixture_identity(12),
                fixture_selection().identity().unwrap(),
            ],
            tools: WorkerTools {
                ffmpeg: ArtifactReference {
                    identity: fixture_identity(4),
                    path: "/nonexistent/fallback-fixture/ffmpeg".into(),
                },
                ffprobe: ArtifactReference {
                    identity: fixture_identity(5),
                    path: "/nonexistent/fallback-fixture/ffprobe".into(),
                },
                onnx_runtime: Some(ArtifactReference {
                    identity: fixture_identity(runtime),
                    path: "/nonexistent/fallback-fixture/runtime".into(),
                }),
            },
        };
        request.validate().unwrap();
        assert!(fixture_selection()
            .matches_worker_request(&request)
            .unwrap());
        request
    }

    fn fixture_environment(runtime: u8) -> InferenceEquivalenceEnvironment {
        InferenceEquivalenceEnvironment {
            engine: fixture_identity(11),
            worker: fixture_identity(12),
            os: "linux".into(),
            architecture: "x86_64".into(),
            platform_build: fixture_identity(13),
            hardware: fixture_identity(14),
            dependencies: vec![fixture_identity(runtime)],
        }
    }

    fn fixture_admission(
        kind: FallbackAdmissionKind,
        ordinal: u8,
        predecessor: Option<AttemptId>,
        runtime: u8,
    ) -> FallbackAdmission {
        FallbackAdmission {
            schema_version: 1,
            kind,
            ordinal,
            predecessor,
            reference: fixture_reference(1),
            observed_environment: fixture_environment(runtime),
            admitted_at_unix_ms: 10,
            expires_at_unix_ms: 100,
        }
    }

    fn fixture_database(request: &WorkerRequest) -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=20;CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT NOT NULL,project TEXT NOT NULL,base_revision INTEGER NOT NULL,session TEXT NOT NULL,source TEXT NOT NULL,state TEXT NOT NULL,manifest TEXT NOT NULL,candidate TEXT,error TEXT,attempts INTEGER NOT NULL DEFAULT 1);CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT NOT NULL REFERENCES jobs(id),manifest TEXT NOT NULL,state TEXT NOT NULL);").unwrap();
        let raw = serde_json::to_string(request).unwrap();
        db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest,attempts) VALUES(?1,?2,?3,?4,'fixture-owner',?5,'queued',?6,1)", params![request.job_id.as_str(),request.attempt_id.as_str(),request.project_id.as_str(),request.base_revision.value(),request.source.source_version.as_str(),raw]).unwrap();
        db.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![request.attempt_id.as_str(), request.job_id.as_str(), raw],
        )
        .unwrap();
        let tx = db.transaction().unwrap();
        crate::job_attempt_history::initialize(&tx).unwrap();
        tx.commit().unwrap();
        db
    }

    fn fixture_declare(db: &mut Connection, request: &WorkerRequest) {
        let tx = db.transaction().unwrap();
        declare_initial(
            &tx,
            request,
            &fixture_selection(),
            &fixture_admission(FallbackAdmissionKind::Initial, 0, None, 3),
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn fixture_state(db: &Connection, request: &WorkerRequest, state: &str) {
        db.execute(
            "UPDATE jobs SET state=?1 WHERE id=?2 AND attempt=?3",
            params![state, request.job_id.as_str(), request.attempt_id.as_str()],
        )
        .unwrap();
        db.execute(
            "UPDATE attempts SET state=?1 WHERE id=?2",
            params![state, request.attempt_id.as_str()],
        )
        .unwrap();
    }

    fn fixture_job_state(db: &Connection, request: &WorkerRequest) -> String {
        db.query_row(
            "SELECT state FROM jobs WHERE id=?1",
            [request.job_id.as_str()],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn fixture_outcome(request: &WorkerRequest, epoch: &str, settled: bool) -> JobAttemptOutcome {
        // Authored structural history only: no native worker was run or retired.
        let diagnostic = JobAttemptDiagnostic::from_error(&ProtocolError::new(
            ErrorCode::Unavailable,
            "authored provider absence fixture",
        ));
        let profile = match &request.operation {
            WorkerOperation::Generate { settings, .. } => {
                settings.inference_profile.as_ref().unwrap()
            }
            _ => unreachable!(),
        };
        let outcome = JobAttemptOutcome {
            schema_version: 1,
            manifest_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(request).unwrap())),
            engine_epoch: AttemptId::new(epoch).unwrap(),
            validation_error: Some(diagnostic.clone()),
            worker_retirement: settled.then(|| WorkerRetirementObservation {
                worker_error: Some(diagnostic),
                provider_unavailable: Some(InferenceProviderUnavailable {
                    schema_version: 1,
                    provider: profile.provider,
                    profile_sha256: format!(
                        "{:x}",
                        Sha256::digest(serde_json::to_vec(profile).unwrap())
                    ),
                    check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
                }),
            }),
        };
        outcome.validate_for(request).unwrap();
        outcome
    }

    fn fixture_retain_outcome(
        db: &Connection,
        request: &WorkerRequest,
        outcome: &JobAttemptOutcome,
    ) {
        db.execute(
            "INSERT INTO job_attempt_outcomes VALUES(?1,?2,?3,?4)",
            params![
                request.attempt_id.as_str(),
                request.job_id.as_str(),
                outcome.manifest_sha256,
                serde_json::to_string(outcome).unwrap()
            ],
        )
        .unwrap();
    }

    fn fixture_pending(db: &mut Connection, request: &WorkerRequest) {
        fixture_state(db, request, "failed");
        db.execute(
            "UPDATE jobs SET error='original provider failure' WHERE id=?1",
            [request.job_id.as_str()],
        )
        .unwrap();
        fixture_retain_outcome(
            db,
            request,
            &fixture_outcome(request, "fixture-epoch", true),
        );
        let tx = db.transaction().unwrap();
        assert!(mark_pending(&tx, request, &AttemptId::new("fixture-epoch").unwrap()).unwrap());
        assert!(tx
            .query_row(
                "SELECT error FROM jobs WHERE id=?1",
                [request.job_id.as_str()],
                |row| row.get::<_, Option<String>>(0)
            )
            .unwrap()
            .is_none());
        let observed: String = tx
            .query_row(
                "SELECT record FROM job_attempt_outcomes WHERE attempt=?1",
                [request.attempt_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(observed.contains("authored provider absence fixture"));
        tx.commit().unwrap();
    }

    fn fixture_insert_successor(
        tx: &rusqlite::Transaction<'_>,
        previous: &WorkerRequest,
        next: &WorkerRequest,
    ) {
        let raw = serde_json::to_string(next).unwrap();
        tx.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![next.attempt_id.as_str(), next.job_id.as_str(), raw],
        )
        .unwrap();
        assert_eq!(tx.execute("UPDATE jobs SET attempt=?1,manifest=?2,state='queued',attempts=attempts+1 WHERE id=?3 AND attempt=?4", params![next.attempt_id.as_str(),raw,previous.job_id.as_str(),previous.attempt_id.as_str()]).unwrap(),1);
    }

    fn fixture_transition(db: &mut Connection, previous: &WorkerRequest, next: &WorkerRequest) {
        let tx = db.transaction().unwrap();
        fixture_insert_successor(&tx, previous, next);
        record_successor(
            &tx,
            previous,
            next,
            &fixture_admission(
                FallbackAdmissionKind::Fallback,
                1,
                Some(previous.attempt_id.clone()),
                8,
            ),
        )
        .unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn initial_declaration_replays_exactly_and_rejects_missing_tag_or_conflicting_admission() {
        let request = fixture_request("initial", 3);
        let mut db = fixture_database(&request);
        fixture_declare(&mut db, &request);
        fixture_declare(&mut db, &request);
        validate(&db).unwrap();
        assert_eq!(ordinary_attempt_count(&db, &request.job_id).unwrap(), 1);
        let original = retained(&db, &request).unwrap().unwrap();
        assert_eq!(original.initial_attempt, request.attempt_id);
        assert_eq!(original.admission.ordinal, 0);
        assert_eq!(
            serde_json::to_value(&original.selection).unwrap(),
            serde_json::to_value(fixture_selection()).unwrap()
        );
        let tx = db.transaction().unwrap();
        let mut different = fixture_admission(FallbackAdmissionKind::Initial, 0, None, 3);
        different.admitted_at_unix_ms += 1;
        assert!(declare_initial(&tx, &request, &fixture_selection(), &different).is_err());
        tx.commit().unwrap();
        assert_eq!(
            retained(&db, &request)
                .unwrap()
                .unwrap()
                .admission
                .admitted_at_unix_ms,
            10
        );
        fixture_state(&db, &request, "failed");
        fixture_declare(&mut db, &request);
        assert_eq!(fixture_job_state(&db, &request), "failed");

        let mut untagged = request.clone();
        untagged.dependencies.retain(|dependency| {
            !dependency
                .artifact_id
                .as_str()
                .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
        });
        let mut db = fixture_database(&untagged);
        let tx = db.transaction().unwrap();
        assert!(declare_initial(
            &tx,
            &untagged,
            &fixture_selection(),
            &fixture_admission(FallbackAdmissionKind::Initial, 0, None, 3)
        )
        .is_err());
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            20
        );
        assert_eq!(fixture_job_state(&db, &untagged), "queued");
    }

    #[test]
    fn retained_fallback_requires_all_tables_and_its_reader_barrier() {
        for missing in [
            "inference_fallback_jobs",
            "inference_fallback_attempts",
            "inference_fallback_pending",
            "inference_fallback_reader",
        ] {
            let request = fixture_request("initial", 3);
            let mut db = fixture_database(&request);
            fixture_declare(&mut db, &request);
            db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
            db.execute_batch(&format!("DROP TABLE {missing}")).unwrap();
            db.execute_batch("PRAGMA query_only=ON").unwrap();
            assert!(validate(&db).is_err());
            assert!(retained(&db, &request).is_err());
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                37
            );
        }
        let request = fixture_request("initial", 3);
        let mut db = fixture_database(&request);
        fixture_declare(&mut db, &request);
        db.execute_batch("PRAGMA user_version=20;PRAGMA query_only=ON")
            .unwrap();
        assert!(validate(&db).is_err());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }

    #[test]
    fn pending_requires_persisted_settled_outcome_from_the_same_epoch() {
        for observation in [
            None,
            Some(("fixture-epoch", false)),
            Some(("other-epoch", true)),
        ] {
            let request = fixture_request("initial", 3);
            let mut db = fixture_database(&request);
            fixture_declare(&mut db, &request);
            fixture_state(&db, &request, "failed");
            if let Some((epoch, settled)) = observation {
                fixture_retain_outcome(&db, &request, &fixture_outcome(&request, epoch, settled));
            }
            let tx = db.transaction().unwrap();
            assert!(!matches!(
                mark_pending(&tx, &request, &AttemptId::new("fixture-epoch").unwrap()),
                Ok(true)
            ));
            tx.commit().unwrap();
            assert_eq!(fixture_job_state(&db, &request), "failed");
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM inference_fallback_pending",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
        }
        let request = fixture_request("initial", 3);
        let mut db = fixture_database(&request);
        fixture_declare(&mut db, &request);
        fixture_pending(&mut db, &request);
        assert_eq!(fixture_job_state(&db, &request), "fallback_pending");
        validate(&db).unwrap();
    }

    #[test]
    fn fallback_requires_next_ordinal_distinct_profile_and_no_profile_cycle() {
        for (runtime, ordinal) in [(3, 1), (8, 2)] {
            let previous = fixture_request("initial", 3);
            let next = fixture_request("next", runtime);
            let mut db = fixture_database(&previous);
            fixture_declare(&mut db, &previous);
            fixture_pending(&mut db, &previous);
            let tx = db.transaction().unwrap();
            fixture_insert_successor(&tx, &previous, &next);
            assert!(record_successor(
                &tx,
                &previous,
                &next,
                &fixture_admission(
                    FallbackAdmissionKind::Fallback,
                    ordinal,
                    Some(previous.attempt_id.clone()),
                    runtime
                )
            )
            .is_err());
            tx.rollback().unwrap();
            assert_eq!(fixture_job_state(&db, &previous), "fallback_pending");
            assert_eq!(ordinary_attempt_count(&db, &previous.job_id).unwrap(), 1);
        }
        let first = fixture_request("initial", 3);
        let second = fixture_request("fallback", 8);
        let mut db = fixture_database(&first);
        fixture_declare(&mut db, &first);
        fixture_pending(&mut db, &first);
        fixture_transition(&mut db, &first, &second);
        assert_eq!(
            retained(&db, &second).unwrap().unwrap().admission.ordinal,
            1
        );
        assert_eq!(ordinary_attempt_count(&db, &first.job_id).unwrap(), 1);
        fixture_pending(&mut db, &second);
        let cycle = fixture_request("cycle", 3);
        let mut admission = fixture_admission(
            FallbackAdmissionKind::Fallback,
            2,
            Some(second.attempt_id.clone()),
            3,
        );
        admission.reference = fixture_reference(2);
        let tx = db.transaction().unwrap();
        fixture_insert_successor(&tx, &second, &cycle);
        assert!(record_successor(&tx, &second, &cycle, &admission).is_err());
        tx.rollback().unwrap();
        assert_eq!(fixture_job_state(&db, &second), "fallback_pending");
        validate(&db).unwrap();
    }

    #[test]
    fn resume_preserves_profile_and_fallback_ordinal_but_counts_as_an_ordinary_attempt() {
        let first = fixture_request("initial", 3);
        let second = fixture_request("fallback", 8);
        let resumed = fixture_request("resume", 8);
        let mut db = fixture_database(&first);
        fixture_declare(&mut db, &first);
        fixture_pending(&mut db, &first);
        fixture_transition(&mut db, &first, &second);
        fixture_state(&db, &second, "interrupted");
        let tx = db.transaction().unwrap();
        fixture_insert_successor(&tx, &second, &resumed);
        record_resume(
            &tx,
            &second,
            &resumed,
            &fixture_admission(
                FallbackAdmissionKind::Resume,
                1,
                Some(second.attempt_id.clone()),
                8,
            ),
        )
        .unwrap();
        tx.commit().unwrap();
        let retained = retained(&db, &resumed).unwrap().unwrap();
        assert_eq!(retained.admission.ordinal, 1);
        assert_eq!(retained.initial_attempt, first.attempt_id);
        assert_eq!(ordinary_attempt_count(&db, &first.job_id).unwrap(), 2);
        validate(&db).unwrap();
    }

    #[test]
    fn cancelling_pending_clears_intent_and_cannot_record_a_successor() {
        let previous = fixture_request("initial", 3);
        let next = fixture_request("after-cancel", 8);
        let mut db = fixture_database(&previous);
        fixture_declare(&mut db, &previous);
        fixture_pending(&mut db, &previous);
        let tx = db.transaction().unwrap();
        fixture_state(&tx, &previous, "cancelled");
        clear_pending(&tx, &previous.job_id).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM inference_fallback_pending",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        let tx = db.transaction().unwrap();
        fixture_insert_successor(&tx, &previous, &next);
        assert!(record_successor(
            &tx,
            &previous,
            &next,
            &fixture_admission(
                FallbackAdmissionKind::Fallback,
                1,
                Some(previous.attempt_id.clone()),
                8
            )
        )
        .is_err());
        tx.rollback().unwrap();
        assert_eq!(fixture_job_state(&db, &previous), "cancelled");
        assert_eq!(ordinary_attempt_count(&db, &previous.job_id).unwrap(), 1);
        assert_eq!(
            db.query_row("SELECT count(*) FROM attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        validate(&db).unwrap();
    }
}
