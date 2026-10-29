//! Owner-authored equivalence history and durable withdrawal intent. No runtime
//! admission, provider substitution, native pin or automatic acceptance exists here.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const MAX_DOSSIERS: u64 = 32;
const MAX_EVIDENCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DECISIONS: u64 = 4096;
const MAX_INTENTS: u64 = 4096;
const MAX_RECEIPTS: u64 = 8192;
const MAX_RECEIPT_BYTES: u64 = 128 * 1024 * 1024;
const REPORT_LIMIT: usize = 256 * 1024;
const RECEIPT_LIMIT: usize = 512 * 1024;
const SCHEMA: &str = "CREATE TABLE inference_equivalence_schema(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),version INTEGER NOT NULL CHECK(version=1),clock_floor_unix_ms INTEGER NOT NULL CHECK(clock_floor_unix_ms>=0))";
const RECORDS: &str = "CREATE TABLE inference_equivalence_records(id TEXT PRIMARY KEY NOT NULL,revision INTEGER NOT NULL,report TEXT NOT NULL)";
const EVIDENCE: &str = "CREATE TABLE inference_equivalence_evidence(dossier TEXT NOT NULL REFERENCES inference_equivalence_records(id),kind TEXT NOT NULL,sha256 TEXT NOT NULL,data BLOB NOT NULL,PRIMARY KEY(dossier,kind))";
const DECISIONS: &str = "CREATE TABLE inference_equivalence_decisions(dossier TEXT NOT NULL REFERENCES inference_equivalence_records(id),revision INTEGER NOT NULL,actor TEXT NOT NULL,request TEXT NOT NULL,report_sha256 TEXT NOT NULL,PRIMARY KEY(dossier,revision),UNIQUE(actor,request),FOREIGN KEY(actor,request) REFERENCES requests(session,request) DEFERRABLE INITIALLY DEFERRED)";
const INTENTS: &str = "CREATE TABLE inference_equivalence_withdrawals(actor TEXT NOT NULL,request TEXT NOT NULL,dossier TEXT NOT NULL REFERENCES inference_equivalence_records(id),expected_revision INTEGER NOT NULL,fingerprint TEXT NOT NULL,reason TEXT NOT NULL,completed_revision INTEGER,PRIMARY KEY(actor,request),UNIQUE(dossier,expected_revision),FOREIGN KEY(actor,request) REFERENCES request_identities(session,request),FOREIGN KEY(dossier,completed_revision) REFERENCES inference_equivalence_decisions(dossier,revision) DEFERRABLE INITIALLY DEFERRED)";
const TABLES: [(&str, &str); 5] = [
    ("inference_equivalence_schema", SCHEMA),
    ("inference_equivalence_records", RECORDS),
    ("inference_equivalence_evidence", EVIDENCE),
    ("inference_equivalence_decisions", DECISIONS),
    ("inference_equivalence_withdrawals", INTENTS),
];

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn revision(value: u64) -> PResult<i64> {
    i64::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("equivalence revision exceeds storage admission"))
}
fn bounded_report(value: &InferenceEquivalenceReport) -> PResult<String> {
    value.validate()?;
    let raw = encode(value)?;
    if raw.len() > REPORT_LIMIT {
        return Err(invalid("equivalence report exceeds storage admission"));
    }
    Ok(raw)
}
fn conflict(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::RequestConflict, message)
}
fn changed() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::RevisionConflict,
        "equivalence review revision changed",
    )
}

fn layout(db: &Connection) -> PResult<bool> {
    crate::inference_equivalence_reader::version(db).map_err(internal)?;
    let temporary: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE (lower(name) GLOB 'inference_equivalence_*' OR lower(tbl_name) GLOB 'inference_equivalence_*') AND lower(name)!='inference_equivalence_reader' AND lower(tbl_name)!='inference_equivalence_reader')", [], |r| r.get(0)).map_err(internal)?;
    if temporary {
        return Err(invalid(
            "temporary equivalence storage objects are forbidden",
        ));
    }
    let mut found = 0;
    for (name, sql) in TABLES {
        let count: u64 = db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE LIMIT 2)", [name], |r| r.get(0)).map_err(internal)?;
        if count != 0 {
            let exact: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE name=?1 AND tbl_name=?1 AND type='table' AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)", params![name,sql.len() as i64,sql], |r|r.get(0)).map_err(internal)?;
            if count != 1 || !exact {
                return Err(invalid("equivalence storage layout differs"));
            }
            found += 1;
        }
    }
    let objects: u64 = db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE (lower(name) GLOB 'inference_equivalence_*' OR lower(tbl_name) GLOB 'inference_equivalence_*') AND lower(name)!='inference_equivalence_reader' AND lower(tbl_name)!='inference_equivalence_reader' AND sql IS NOT NULL LIMIT 7)", [], |r|r.get(0)).map_err(internal)?;
    if (found != 0 && found != TABLES.len()) || objects != found as u64 {
        return Err(invalid(
            "incomplete or unexpected equivalence storage objects",
        ));
    }
    if found == 0 {
        return Ok(false);
    }
    let rows: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_schema LIMIT 2)",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    let scalar: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.inference_equivalence_schema WHERE typeof(singleton)='integer' AND singleton=1 AND typeof(version)='integer' AND version=1 AND typeof(clock_floor_unix_ms)='integer' AND clock_floor_unix_ms>=0)", [], |r|r.get(0)).map_err(internal)?;
    if rows != 1 || !scalar {
        return Err(invalid("equivalence schema or clock floor differs"));
    }
    Ok(true)
}

fn capacity(db: &Connection) -> PResult<()> {
    for (query, maximum) in [
        ("SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_records LIMIT 33)", MAX_DOSSIERS),
        ("SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_evidence LIMIT 257)", MAX_DOSSIERS * 8),
        ("SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_decisions LIMIT 4097)", MAX_DECISIONS),
        ("SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_withdrawals LIMIT 4097)", MAX_INTENTS),
    ] {
        let count: u64 = db.query_row(query, [], |r|r.get(0)).map_err(internal)?;
        if count > maximum { return Err(invalid("equivalence row capacity exceeded")); }
    }
    let bytes: u64 = db
        .query_row(
            "SELECT COALESCE(SUM(length(data)),0) FROM main.inference_equivalence_evidence",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if bytes > MAX_EVIDENCE_BYTES {
        return Err(invalid("equivalence evidence capacity exceeded"));
    }
    Ok(())
}
fn require_reader(db: &Connection) -> PResult<()> {
    crate::inference_equivalence_reader::version(db).map_err(internal)?;
    let raw = crate::cpu_runtime_reader::version(db).map_err(internal)?;
    if raw != 35 {
        return Err(invalid(
            "equivalence decisions and pending withdrawals require reader 35",
        ));
    }
    Ok(())
}
fn history_requires_reader(db: &Connection) -> PResult<()> {
    let history: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.inference_equivalence_decisions) OR EXISTS(SELECT 1 FROM main.inference_equivalence_withdrawals) OR EXISTS(SELECT 1 FROM main.inference_equivalence_records WHERE revision!=1) OR EXISTS(SELECT 1 FROM main.inference_equivalence_schema WHERE clock_floor_unix_ms>0)", [], |r|r.get(0)).map_err(internal)?;
    if history {
        require_reader(db)?;
    }
    Ok(())
}
fn identity_claim(
    db: &Connection,
    actor: &SessionId,
    request: &RequestId,
    expected: &str,
) -> PResult<()> {
    validate_sha256(expected)?;
    let stored: Option<String> = db.query_row("SELECT CASE WHEN typeof(fingerprint)='text' AND length(fingerprint)=64 THEN fingerprint ELSE NULL END FROM main.request_identities WHERE session=?1 AND request=?2",params![actor.as_str(),request.as_str()],|r|r.get(0)).optional().map_err(internal)?;
    if stored.as_deref() != Some(expected) {
        return Err(invalid(
            "equivalence request identity claim is missing or changed",
        ));
    }
    Ok(())
}

fn saved_receipt(
    db: &Connection,
    actor: &SessionId,
    request: &RequestId,
) -> PResult<(String, InferenceEquivalenceReceipt)> {
    let (fingerprint,raw):(String,String)=db.query_row("SELECT CASE WHEN typeof(fingerprint)='text' AND length(fingerprint)=64 THEN fingerprint ELSE NULL END,CASE WHEN typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND 524288 THEN response ELSE NULL END FROM main.requests WHERE session=?1 AND request=?2",params![actor.as_str(),request.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
    identity_claim(db, actor, request, &fingerprint)?;
    let ResponseBody::InferenceEquivalenceSaved(receipt) = decode(&raw)? else {
        return Err(invalid("equivalence decision receipt type differs"));
    };
    receipt.validate()?;
    if receipt.request_id != *request {
        return Err(invalid("equivalence receipt request identity differs"));
    }
    Ok((fingerprint, *receipt))
}
fn decision_link(db: &Connection, value: &InferenceEquivalenceReport) -> PResult<()> {
    let Some(decision) = &value.decision else {
        return Ok(());
    };
    require_reader(db)?;
    let (actor,request,hash):(String,String,String)=db.query_row("SELECT CASE WHEN typeof(actor)='text' AND length(CAST(actor AS BLOB)) BETWEEN 1 AND 128 THEN actor ELSE NULL END,CASE WHEN typeof(request)='text' AND length(CAST(request AS BLOB)) BETWEEN 1 AND 128 THEN request ELSE NULL END,CASE WHEN typeof(report_sha256)='text' AND length(report_sha256)=64 THEN report_sha256 ELSE NULL END FROM main.inference_equivalence_decisions WHERE dossier=?1 AND revision=?2", params![value.dossier_id.as_str(),revision(value.revision)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(internal)?;
    if actor != decision.actor_session.as_str()
        || request != decision.request_id.as_str()
        || hash != digest(bounded_report(value)?.as_bytes())
    {
        return Err(invalid("equivalence decision membership differs"));
    }
    let (_, receipt) = saved_receipt(db, &decision.actor_session, &decision.request_id)?;
    if receipt.input_sha256.is_some() || bounded_report(&receipt.report)? != bounded_report(value)?
    {
        return Err(invalid(
            "equivalence decision and original receipt disagree",
        ));
    }
    Ok(())
}

pub(super) fn initialize(tx: &Transaction<'_>) -> PResult<()> {
    if layout(tx)? {
        capacity(tx)?;
        history_requires_reader(tx)?;
        return Ok(());
    }
    scan_receipts(tx, |_, _, _, _| {
        Err(invalid("equivalence receipt exists without its registry"))
    })?;
    for (_, sql) in TABLES {
        tx.execute_batch(sql).map_err(internal)?;
    }
    tx.execute(
        "INSERT INTO main.inference_equivalence_schema VALUES(1,1,0)",
        [],
    )
    .map_err(internal)?;
    Ok(())
}
pub(super) fn clock_floor(db: &Connection) -> PResult<Option<u64>> {
    if !layout(db)? {
        return Ok(None);
    }
    history_requires_reader(db)?;
    let value: u64 = db
        .query_row(
            "SELECT clock_floor_unix_ms FROM main.inference_equivalence_schema WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    Ok((value != 0).then_some(value))
}
fn stored_revision(db: &Connection, id: &ArtifactId) -> PResult<u64> {
    validate_sha256(id.as_str())?;
    let value:Option<u64>=db.query_row("SELECT CASE WHEN typeof(revision)='integer' AND revision BETWEEN 1 AND 9223372036854775807 THEN revision ELSE NULL END FROM main.inference_equivalence_records WHERE id=?1",[id.as_str()],|r|r.get(0)).optional().map_err(internal)?;
    value.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "inference equivalence dossier not found",
        )
    })
}
pub(super) fn report(db: &Connection, id: &ArtifactId) -> PResult<InferenceEquivalenceReport> {
    validate_sha256(id.as_str())?;
    if !layout(db)? {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "inference equivalence dossier not found",
        ));
    }
    capacity(db)?;
    history_requires_reader(db)?;
    let expected = stored_revision(db, id)?;
    let raw:String=db.query_row("SELECT CASE WHEN typeof(report)='text' AND length(CAST(report AS BLOB)) BETWEEN 1 AND 262144 THEN report ELSE NULL END FROM main.inference_equivalence_records WHERE id=?1",[id.as_str()],|r|r.get(0)).map_err(internal)?;
    let value: InferenceEquivalenceReport = decode(&raw)?;
    value.validate()?;
    if value.dossier_id != *id || value.revision != expected {
        return Err(invalid("equivalence record identity or revision differs"));
    }
    decision_link(db, &value)?;
    Ok(value)
}
pub(super) fn verify_evidence(
    db: &Connection,
    value: &InferenceEquivalenceReport,
) -> PResult<InferenceEquivalenceAssessment> {
    value.validate()?;
    let count:u64=db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_evidence WHERE dossier=?1 LIMIT 9)",[value.dossier_id.as_str()],|r|r.get(0)).map_err(internal)?;
    if count != 8 {
        return Err(invalid("equivalence evidence membership differs"));
    }
    let mut docs = BTreeMap::new();
    for item in &value.manifest.evidence {
        let (hash,bytes):(String,Vec<u8>)=db.query_row("SELECT CASE WHEN typeof(sha256)='text' AND length(sha256)=64 THEN sha256 ELSE NULL END,CASE WHEN typeof(data)='blob' AND length(data) BETWEEN 1 AND 4194304 THEN data ELSE NULL END FROM main.inference_equivalence_evidence WHERE dossier=?1 AND kind=?2",params![value.dossier_id.as_str(),encode(&item.kind)?],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
        if hash != item.identity.sha256
            || bytes.len() as u64 != item.identity.byte_len
            || digest(&bytes) != hash
        {
            return Err(invalid("retained equivalence evidence changed"));
        }
        docs.insert(item.kind, bytes);
    }
    validate_evidence(&value.manifest, &docs)
}

/// Original decision bytes and documents for inert archival use. This does not
/// observe current acceptance, advance a clock, fence withdrawal or grant a
/// lifetime. The caller owns snapshot and aggregate allocation admission.
pub(super) fn archival_decision(
    db: &Connection,
    reference: &InferenceFallbackReference,
    remaining: &mut usize,
) -> PResult<(InferenceEquivalenceReport, BTreeMap<String, Vec<u8>>)> {
    reference.validate()?;
    validate_sha256(reference.dossier_id.as_str())?;
    validate(db)?;
    let current = report(db, &reference.dossier_id)?;
    let (actor, request, hash): (String, String, String) = db.query_row(
        "SELECT CASE WHEN typeof(actor)='text' AND length(CAST(actor AS BLOB)) BETWEEN 1 AND 128 THEN actor ELSE NULL END,CASE WHEN typeof(request)='text' AND length(CAST(request AS BLOB)) BETWEEN 1 AND 128 THEN request ELSE NULL END,CASE WHEN typeof(report_sha256)='text' AND length(report_sha256)=64 THEN report_sha256 ELSE NULL END FROM main.inference_equivalence_decisions WHERE dossier=?1 AND revision=?2",
        params![reference.dossier_id.as_str(), revision(reference.expected_revision)?],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).map_err(internal)?;
    let actor = SessionId::new(actor).map_err(internal)?;
    let request = RequestId::new(request).map_err(internal)?;
    let (_, receipt) = saved_receipt(db, &actor, &request)?;
    let original = receipt.report.clone();
    let receipt_raw: String = db.query_row(
        "SELECT CASE WHEN typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND 524288 THEN response ELSE NULL END FROM main.requests WHERE session=?1 AND request=?2",
        params![actor.as_str(),request.as_str()], |r|r.get(0),
    ).map_err(internal)?;
    if encode(&ResponseBody::InferenceEquivalenceSaved(Box::new(receipt)))? != receipt_raw {
        return Err(invalid(
            "archival equivalence original receipt is not canonical",
        ));
    }
    let canonical = bounded_report(&original)?;
    if hash != reference.expected_report_sha256
        || digest(canonical.as_bytes()) != hash
        || original.dossier_id != reference.dossier_id
        || original.revision != reference.expected_revision
        || original.state != InferenceEquivalenceState::Accepted
        || original.manifest.subject.origin != InferenceEquivalenceOrigin::Measured
        || encode(&original.manifest)? != encode(&current.manifest)?
    {
        return Err(invalid(
            "archival equivalence decision identity or original acceptance differs",
        ));
    }
    decision_link(db, &original)?;
    *remaining = remaining.checked_sub(canonical.len()).ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "archival equivalence metadata exceeds capture admission",
        )
    })?;
    let mut documents = BTreeMap::new();
    let mut by_kind = BTreeMap::new();
    for item in &original.manifest.evidence {
        let length: u64 = db.query_row(
            "SELECT CASE WHEN typeof(data)='blob' AND length(data) BETWEEN 1 AND 4194304 AND typeof(sha256)='text' AND sha256=?3 THEN length(data) ELSE NULL END FROM main.inference_equivalence_evidence WHERE dossier=?1 AND kind=?2",
            params![reference.dossier_id.as_str(),encode(&item.kind)?,item.identity.sha256], |r|r.get(0),
        ).map_err(internal)?;
        if length != item.identity.byte_len {
            return Err(invalid("archival equivalence document length differs"));
        }
        *remaining = remaining.checked_sub(length as usize).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "archival equivalence evidence exceeds capture admission",
            )
        })?;
        let data: Vec<u8> = db.query_row(
            "SELECT data FROM main.inference_equivalence_evidence WHERE dossier=?1 AND kind=?2 AND typeof(data)='blob' AND length(data)=?3 AND sha256=?4",
            params![reference.dossier_id.as_str(),encode(&item.kind)?,length,item.identity.sha256], |r|r.get(0),
        ).map_err(internal)?;
        if digest(&data) != item.identity.sha256 {
            return Err(invalid("archival equivalence document digest differs"));
        }
        by_kind.insert(item.kind, data.clone());
        documents.insert(item.identity.sha256.clone(), data);
    }
    let assessment = validate_evidence(&original.manifest, &by_kind)?;
    let decision = original
        .decision
        .as_ref()
        .ok_or_else(|| invalid("archival equivalence decision absent"))?;
    assessment.require_acceptance(
        decision
            .recorded_at_unix_ms
            .ok_or_else(|| invalid("archival equivalence decision clock absent"))?,
    )?;
    if decision.review_identity.as_ref() != Some(assessment.review_identity()) {
        return Err(invalid("archival equivalence review identity differs"));
    }
    Ok((original, documents))
}

/// Fresh storage observations only. The caller must separately own authorization,
/// runtime/environment correspondence, withdrawal fencing and worker admission.
/// Keeping or cloning report data never extends this observation's check time.
pub(super) struct CurrentAcceptedEquivalence {
    report: InferenceEquivalenceReport,
    assessment: InferenceEquivalenceAssessment,
    report_sha256: String,
    checked_at_unix_ms: u64,
}
impl std::fmt::Debug for CurrentAcceptedEquivalence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CurrentAcceptedEquivalence")
    }
}
impl CurrentAcceptedEquivalence {
    pub(super) fn report(&self) -> &InferenceEquivalenceReport {
        &self.report
    }
    pub(super) fn assessment(&self) -> &InferenceEquivalenceAssessment {
        &self.assessment
    }
    pub(super) fn report_sha256(&self) -> &str {
        &self.report_sha256
    }
    pub(super) fn checked_at_unix_ms(&self) -> u64 {
        self.checked_at_unix_ms
    }
}

/// Read the exact current accepted decision without mutating any clock or header.
/// Call inside the caller's transaction when this observation is used for admission.
pub(super) fn read_current_accepted(
    db: &Connection,
    id: &ArtifactId,
    expected_revision: u64,
    expected_report_sha256: &str,
    now_unix_ms: u64,
) -> PResult<CurrentAcceptedEquivalence> {
    validate_sha256(id.as_str())?;
    revision(expected_revision)?;
    validate_sha256(expected_report_sha256)?;
    if now_unix_ms == 0 || now_unix_ms > i64::MAX as u64 {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "equivalence execution clock is unavailable",
        ));
    }
    // Retain the full decision/request history guarantees, not only a row whose
    // current state happens to say Accepted. Evidence is rehashed from storage.
    validate(db)?;
    let value = report(db, id)?;
    if value.revision != expected_revision {
        return Err(changed());
    }
    let canonical = bounded_report(&value)?;
    let raw: String = db.query_row(
        "SELECT CASE WHEN typeof(report)='text' AND length(CAST(report AS BLOB)) BETWEEN 1 AND 262144 THEN report ELSE NULL END FROM main.inference_equivalence_records WHERE id=?1",
        [id.as_str()], |row| row.get(0),
    ).map_err(internal)?;
    let report_sha256 = digest(canonical.as_bytes());
    if raw != canonical || report_sha256 != expected_report_sha256 {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "accepted equivalence canonical report identity changed",
        ));
    }
    if value.state != InferenceEquivalenceState::Accepted
        || value.manifest.subject.origin != InferenceEquivalenceOrigin::Measured
    {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "equivalence dossier has no current measured acceptance",
        ));
    }
    if pending_withdrawal(db, id)?.is_some() {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "equivalence withdrawal is pending",
        ));
    }
    let floor = clock_floor(db)?
        .ok_or_else(|| invalid("accepted equivalence has no retained clock floor"))?;
    if now_unix_ms < floor {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "equivalence execution clock predates its retained floor",
        ));
    }
    let assessment = verify_evidence(db, &value)?;
    let decision = value
        .decision
        .as_ref()
        .ok_or_else(|| invalid("accepted equivalence decision absent"))?;
    let accepted_at = decision
        .recorded_at_unix_ms
        .ok_or_else(|| invalid("accepted equivalence decision time absent"))?;
    // A resealed historical decision must have been eligible at its own time,
    // as well as remaining eligible at this new observation time.
    assessment.require_acceptance(accepted_at)?;
    assessment.require_acceptance(now_unix_ms)?;
    if assessment.manifest_id() != id
        || decision.review_identity.as_ref() != Some(assessment.review_identity())
    {
        return Err(invalid(
            "accepted equivalence assessment or supplied review differs",
        ));
    }
    let (_, receipt) = saved_receipt(db, &decision.actor_session, &decision.request_id)?;
    let receipt_raw: String = db.query_row(
        "SELECT CASE WHEN typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND 524288 THEN response ELSE NULL END FROM main.requests WHERE session=?1 AND request=?2",
        params![decision.actor_session.as_str(),decision.request_id.as_str()], |row| row.get(0),
    ).map_err(internal)?;
    if encode(&ResponseBody::InferenceEquivalenceSaved(Box::new(receipt)))? != receipt_raw {
        return Err(invalid(
            "accepted equivalence original decision receipt bytes are not canonical",
        ));
    }
    Ok(CurrentAcceptedEquivalence {
        report: value,
        assessment,
        report_sha256,
        checked_at_unix_ms: now_unix_ms,
    })
}

/// Resolve and retain a successful admission's observed wall-clock floor in the
/// actual caller-owned transaction. No report, decision or reader is changed.
/// Rollback removes the floor advance; this value is still not execution authority.
pub(super) fn resolve_current_accepted(
    tx: &Transaction<'_>,
    id: &ArtifactId,
    expected_revision: u64,
    expected_report_sha256: &str,
    now_unix_ms: u64,
) -> PResult<CurrentAcceptedEquivalence> {
    let value = read_current_accepted(
        tx,
        id,
        expected_revision,
        expected_report_sha256,
        now_unix_ms,
    )?;
    let floor =
        clock_floor(tx)?.ok_or_else(|| invalid("accepted equivalence clock floor disappeared"))?;
    if now_unix_ms > floor {
        let rows = tx.execute(
            "UPDATE main.inference_equivalence_schema SET clock_floor_unix_ms=?1 WHERE singleton=1 AND clock_floor_unix_ms=?2",
            params![now_unix_ms as i64,floor as i64],
        ).map_err(internal)?;
        if rows != 1 {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "equivalence execution clock floor changed during resolution",
            ));
        }
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WithdrawalIntent {
    pub dossier_id: ArtifactId,
    pub expected_revision: u64,
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub fingerprint: String,
    pub reason: String,
    pub completed_revision: Option<u64>,
}
fn read_intent(
    db: &Connection,
    actor: &SessionId,
    request: &RequestId,
) -> PResult<Option<WithdrawalIntent>> {
    type Row = (String, u64, String, String, Option<u64>);
    let row:Option<Row>=db.query_row("SELECT CASE WHEN typeof(dossier)='text' AND length(dossier)=64 THEN dossier ELSE NULL END,CASE WHEN typeof(expected_revision)='integer' AND expected_revision BETWEEN 1 AND 9223372036854775806 THEN expected_revision ELSE NULL END,CASE WHEN typeof(fingerprint)='text' AND length(fingerprint)=64 THEN fingerprint ELSE NULL END,CASE WHEN typeof(reason)='text' AND length(CAST(reason AS BLOB)) BETWEEN 1 AND 4096 THEN reason ELSE NULL END,completed_revision FROM main.inference_equivalence_withdrawals WHERE actor=?1 AND request=?2",params![actor.as_str(),request.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(internal)?;
    row.map(|(id, expected, fingerprint, reason, completed)| {
        let id = ArtifactId::new(id).map_err(internal)?;
        validate_sha256(id.as_str())?;
        validate_sha256(&fingerprint)?;
        validate_inference_equivalence_note(&reason)?;
        if completed.is_some_and(|v| Some(v) != expected.checked_add(1)) {
            return Err(invalid("equivalence withdrawal completed revision differs"));
        }
        identity_claim(db, actor, request, &fingerprint)?;
        Ok(WithdrawalIntent {
            dossier_id: id,
            expected_revision: expected,
            actor_session: actor.clone(),
            request_id: request.clone(),
            fingerprint,
            reason,
            completed_revision: completed,
        })
    })
    .transpose()
}
pub(super) fn pending_withdrawal(
    db: &Connection,
    id: &ArtifactId,
) -> PResult<Option<WithdrawalIntent>> {
    if !layout(db)? {
        return Ok(None);
    }
    capacity(db)?;
    history_requires_reader(db)?;
    let rows: Vec<(String, String)> = {
        let mut q=db.prepare("SELECT CASE WHEN typeof(actor)='text' AND length(CAST(actor AS BLOB)) BETWEEN 1 AND 128 THEN actor ELSE NULL END,CASE WHEN typeof(request)='text' AND length(CAST(request AS BLOB)) BETWEEN 1 AND 128 THEN request ELSE NULL END FROM main.inference_equivalence_withdrawals WHERE dossier=?1 AND completed_revision IS NULL LIMIT 2").map_err(internal)?;
        let rows = q
            .query_map([id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(internal)?
            .collect::<std::result::Result<_, _>>()
            .map_err(internal)?;
        rows
    };
    if rows.len() > 1 {
        return Err(invalid("multiple pending equivalence withdrawals"));
    }
    rows.into_iter()
        .next()
        .map(|(actor, request)| {
            let actor = SessionId::new(actor).map_err(internal)?;
            let request = RequestId::new(request).map_err(internal)?;
            let value = read_intent(db, &actor, &request)?
                .ok_or_else(|| invalid("pending withdrawal disappeared"))?;
            if stored_revision(db, id)? != value.expected_revision {
                return Err(invalid(
                    "pending withdrawal no longer binds current revision",
                ));
            }
            let exists: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM main.requests WHERE session=?1 AND request=?2)",
                    params![actor.as_str(), request.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if exists {
                return Err(invalid(
                    "pending withdrawal already has a final request receipt",
                ));
            }
            Ok(value)
        })
        .transpose()
}
pub(super) fn status(db: &Connection, id: &ArtifactId) -> PResult<InferenceEquivalenceStatus> {
    let value = report(db, id)?;
    verify_evidence(db, &value)?;
    let status = InferenceEquivalenceStatus {
        report: value,
        withdrawal_pending: pending_withdrawal(db, id)?.is_some(),
        clock_floor_unix_ms: clock_floor(db)?,
        report_only: true,
    };
    status.validate()?;
    Ok(status)
}
pub(super) fn list(
    db: &Connection,
    after: Option<&ArtifactId>,
    limit: u16,
) -> PResult<InferenceEquivalencePage> {
    validate_inference_equivalence_page_limit(limit)?;
    if let Some(id) = after {
        validate_sha256(id.as_str())?;
    }
    if !layout(db)? {
        return Ok(InferenceEquivalencePage {
            entries: vec![],
            next_after: None,
            report_only: true,
        });
    }
    capacity(db)?;
    history_requires_reader(db)?;
    let ids: Vec<String> = {
        let mut q=db.prepare("SELECT CASE WHEN typeof(id)='text' AND length(id)=64 THEN id ELSE NULL END FROM main.inference_equivalence_records WHERE id>?1 ORDER BY id LIMIT ?2").map_err(internal)?;
        let rows = q
            .query_map(
                params![
                    after.map(ArtifactId::as_str).unwrap_or(""),
                    u32::from(limit) + 1
                ],
                |r| r.get(0),
            )
            .map_err(internal)?
            .collect::<std::result::Result<_, _>>()
            .map_err(internal)?;
        rows
    };
    let mut page = InferenceEquivalencePage {
        entries: Vec::new(),
        next_after: None,
        report_only: true,
    };
    let mut more = false;
    for id in ids {
        if page.entries.len() == usize::from(limit) {
            more = true;
            break;
        }
        let id = ArtifactId::new(id).map_err(internal)?;
        page.entries.push(status(db, &id)?);
        // Account for a continuation cursor before admitting the next row.
        page.next_after = Some(id);
        if encode(&page)?.len() > 512 * 1024 {
            page.entries.pop();
            if page.entries.is_empty() {
                return Err(invalid(
                    "single equivalence status exceeds page byte admission",
                ));
            }
            more = true;
            break;
        }
    }
    page.next_after = if more {
        page.entries.last().map(|v| v.report.dossier_id.clone())
    } else {
        None
    };
    page.validate()?;
    Ok(page)
}

pub(super) fn import_pending(
    tx: &Transaction<'_>,
    manifest: &InferenceEquivalenceManifest,
    docs: &[(InferenceEquivalenceEvidenceKind, Vec<u8>)],
) -> PResult<InferenceEquivalenceReport> {
    manifest.validate()?;
    if docs.len() != 8 {
        return Err(invalid("equivalence requires eight input documents"));
    }
    let mut checked = BTreeMap::new();
    let mut total = 0u64;
    for (kind, bytes) in docs {
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("equivalence byte total overflow"))?;
        if bytes.is_empty()
            || bytes.len() as u64 > INFERENCE_EQUIVALENCE_EVIDENCE_LIMIT
            || total > INFERENCE_EQUIVALENCE_EVIDENCE_TOTAL_LIMIT
        {
            return Err(invalid(
                "equivalence input document byte admission exceeded",
            ));
        }
        if checked.insert(*kind, bytes.clone()).is_some() {
            return Err(invalid("duplicate equivalence input evidence role"));
        }
    }
    validate_evidence(manifest, &checked)?;
    initialize(tx)?;
    capacity(tx)?;
    let id = manifest.id()?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM main.inference_equivalence_records WHERE id=?1)",
            [id.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if exists {
        let value = report(tx, &id)?;
        verify_evidence(tx, &value)?;
        return Ok(value);
    }
    let (count,used):(u64,u64)=tx.query_row("SELECT (SELECT COUNT(*) FROM main.inference_equivalence_records),(SELECT COALESCE(SUM(length(data)),0) FROM main.inference_equivalence_evidence)",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
    if count >= MAX_DOSSIERS
        || used
            .checked_add(total)
            .is_none_or(|v| v > MAX_EVIDENCE_BYTES)
    {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "equivalence evidence store is full",
        ));
    }
    let value = InferenceEquivalenceReport {
        dossier_id: id.clone(),
        revision: 1,
        state: InferenceEquivalenceState::Pending,
        manifest: manifest.clone(),
        decision: None,
        report_only: true,
    };
    tx.execute(
        "INSERT INTO main.inference_equivalence_records VALUES(?1,1,?2)",
        params![id.as_str(), bounded_report(&value)?],
    )
    .map_err(internal)?;
    for item in &manifest.evidence {
        tx.execute(
            "INSERT INTO main.inference_equivalence_evidence VALUES(?1,?2,?3,?4)",
            params![
                id.as_str(),
                encode(&item.kind)?,
                item.identity.sha256,
                checked[&item.kind]
            ],
        )
        .map_err(internal)?;
    }
    Ok(value)
}
fn reserve_decision(tx: &Transaction<'_>) -> PResult<()> {
    let count: u64 = tx
        .query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_decisions LIMIT 4097)",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if count >= MAX_DECISIONS {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "equivalence decision history is full",
        ));
    }
    Ok(())
}
fn write_decision(
    tx: &Transaction<'_>,
    value: &InferenceEquivalenceReport,
    expected: u64,
) -> PResult<()> {
    reserve_decision(tx)?;
    let raw = bounded_report(value)?;
    let decision = value
        .decision
        .as_ref()
        .ok_or_else(|| invalid("decision absent"))?;
    crate::inference_equivalence_reader::arm(tx).map_err(internal)?;
    let rows=tx.execute("UPDATE main.inference_equivalence_records SET revision=?1,report=?2 WHERE id=?3 AND revision=?4",params![revision(value.revision)?,raw,value.dossier_id.as_str(),revision(expected)?]).map_err(internal)?;
    if rows != 1 {
        return Err(changed());
    }
    tx.execute(
        "INSERT INTO main.inference_equivalence_decisions VALUES(?1,?2,?3,?4,?5)",
        params![
            value.dossier_id.as_str(),
            revision(value.revision)?,
            decision.actor_session.as_str(),
            decision.request_id.as_str(),
            digest(raw.as_bytes())
        ],
    )
    .map_err(internal)?;
    Ok(())
}
pub(super) fn accept(
    tx: &Transaction<'_>,
    id: &ArtifactId,
    expected: u64,
    actor: &SessionId,
    request: &RequestId,
    operator: &str,
    now: u64,
) -> PResult<InferenceEquivalenceReport> {
    validate(tx)?;
    let mut value = report(tx, id)?;
    if value.revision != expected {
        return Err(changed());
    }
    if pending_withdrawal(tx, id)?.is_some() {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "equivalence withdrawal is pending",
        ));
    }
    let floor = clock_floor(tx)?.unwrap_or(0);
    if now == 0 || now > i64::MAX as u64 || now < floor {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "equivalence clock is unavailable or predates its retained floor",
        ));
    }
    let assessment = verify_evidence(tx, &value)?;
    assessment.require_acceptance(now)?;
    let next = expected
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| invalid("equivalence revision overflow"))?;
    value.revision = next;
    value.state = InferenceEquivalenceState::Accepted;
    value.decision = Some(InferenceEquivalenceDecision {
        revision: next,
        state: value.state,
        request_id: request.clone(),
        actor_session: actor.clone(),
        operator: Some(operator.to_owned()),
        note: String::new(),
        recorded_at_unix_ms: Some(now),
        review_identity: Some(assessment.review_identity().clone()),
    });
    write_decision(tx, &value, expected)?;
    tx.execute("UPDATE main.inference_equivalence_schema SET clock_floor_unix_ms=?1 WHERE singleton=1 AND clock_floor_unix_ms<=?1",[now as i64]).map_err(internal)?;
    Ok(value)
}
pub(super) fn begin_withdrawal(
    tx: &Transaction<'_>,
    id: &ArtifactId,
    expected: u64,
    actor: &SessionId,
    request: &Request,
    fingerprint: &str,
    reason: &str,
) -> PResult<WithdrawalIntent> {
    validate_sha256(id.as_str())?;
    validate_sha256(fingerprint)?;
    validate_inference_equivalence_note(reason)?;
    revision(expected)?;
    if expected == i64::MAX as u64 {
        return Err(invalid("equivalence withdrawal revision overflow"));
    }
    request.validate()?;
    if request.session.as_ref() != Some(actor)
        || digest(&request_fingerprint(request).map_err(internal)?) != fingerprint
        || !matches!(&request.command,Command::WithdrawInferenceEquivalence {dossier_id,expected_record_revision,reason:declared,..} if dossier_id==id && *expected_record_revision==expected && declared==reason)
    {
        return Err(conflict("withdrawal intent differs from original request"));
    }
    if !layout(tx)? {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "inference equivalence dossier not found",
        ));
    }
    capacity(tx)?;
    history_requires_reader(tx)?;
    if let Some(intent) = read_intent(tx, actor, &request.request_id)? {
        if intent.dossier_id != *id
            || intent.expected_revision != expected
            || intent.fingerprint != fingerprint
            || intent.reason != reason
        {
            return Err(conflict(
                "withdrawal request identity differs from retained intent",
            ));
        }
        return Ok(intent);
    }
    if stored_revision(tx, id)? != expected {
        return Err(changed());
    }
    if pending_withdrawal(tx, id)?.is_some() {
        return Err(conflict("another original withdrawal is already pending"));
    }
    let receipt_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM main.requests WHERE session=?1 AND request=?2)",
            params![actor.as_str(), request.request_id.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if receipt_exists {
        return Err(conflict(
            "withdrawal request already has another durable receipt",
        ));
    }
    let count: u64 = tx
        .query_row(
            "SELECT COUNT(*) FROM main.inference_equivalence_withdrawals",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if count >= MAX_INTENTS {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "equivalence withdrawal history is full",
        ));
    }
    claim_request_identity(tx, actor.as_str(), request, fingerprint)?;
    crate::inference_equivalence_reader::arm(tx).map_err(internal)?;
    tx.execute(
        "INSERT INTO main.inference_equivalence_withdrawals VALUES(?1,?2,?3,?4,?5,?6,NULL)",
        params![
            actor.as_str(),
            request.request_id.as_str(),
            id.as_str(),
            expected as i64,
            fingerprint,
            reason
        ],
    )
    .map_err(internal)?;
    Ok(WithdrawalIntent {
        dossier_id: id.clone(),
        expected_revision: expected,
        actor_session: actor.clone(),
        request_id: request.request_id.clone(),
        fingerprint: fingerprint.to_owned(),
        reason: reason.to_owned(),
        completed_revision: None,
    })
}
pub(super) fn finish_withdrawal(
    tx: &Transaction<'_>,
    intent: &WithdrawalIntent,
    now: Option<u64>,
) -> PResult<InferenceEquivalenceReport> {
    if !layout(tx)? {
        return Err(invalid("withdrawal registry disappeared"));
    }
    capacity(tx)?;
    require_reader(tx)?;
    let stored = read_intent(tx, &intent.actor_session, &intent.request_id)?
        .ok_or_else(|| invalid("original withdrawal intent is absent"))?;
    if &stored != intent {
        return Err(conflict(
            "withdrawal completion differs from original intent",
        ));
    }
    if let Some(completed) = stored.completed_revision {
        let (_, receipt) = saved_receipt(tx, &stored.actor_session, &stored.request_id)?;
        if receipt.input_sha256.is_some()
            || receipt.report.dossier_id != stored.dossier_id
            || receipt.report.revision != completed
        {
            return Err(invalid("completed withdrawal receipt differs"));
        }
        decision_link(tx, &receipt.report)?;
        return Ok(receipt.report);
    }
    let mut value = report(tx, &stored.dossier_id)?;
    if value.revision != stored.expected_revision {
        return Err(changed());
    }
    let floor = clock_floor(tx)?.unwrap_or(0);
    let observed = now.filter(|v| *v > 0 && *v <= i64::MAX as u64 && *v >= floor);
    let next = stored
        .expected_revision
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| invalid("equivalence withdrawal revision overflow"))?;
    value.revision = next;
    value.state = InferenceEquivalenceState::Withdrawn;
    value.decision = Some(InferenceEquivalenceDecision {
        revision: next,
        state: value.state,
        request_id: stored.request_id.clone(),
        actor_session: stored.actor_session.clone(),
        operator: None,
        note: stored.reason.clone(),
        recorded_at_unix_ms: observed,
        review_identity: None,
    });
    write_decision(tx, &value, stored.expected_revision)?;
    let rows=tx.execute("UPDATE main.inference_equivalence_withdrawals SET completed_revision=?1 WHERE actor=?2 AND request=?3 AND completed_revision IS NULL AND expected_revision=?4 AND fingerprint=?5",params![next as i64,stored.actor_session.as_str(),stored.request_id.as_str(),stored.expected_revision as i64,stored.fingerprint]).map_err(internal)?;
    if rows != 1 {
        return Err(invalid(
            "withdrawal intent completion compare-and-swap changed",
        ));
    }
    if let Some(now) = observed {
        tx.execute("UPDATE main.inference_equivalence_schema SET clock_floor_unix_ms=?1 WHERE singleton=1 AND clock_floor_unix_ms<=?1",[now as i64]).map_err(internal)?;
    }
    Ok(value)
}

fn scan_receipts(
    db: &Connection,
    mut visit: impl FnMut(&SessionId, &RequestId, &str, &InferenceEquivalenceReceipt) -> PResult<()>,
) -> PResult<()> {
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name='requests')",[],|r|r.get(0)).map_err(internal)?;
    if !exists {
        return Ok(());
    }
    let predicate="CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='inference_equivalence_saved' ELSE instr(response,'inference_equivalence_saved')>0 END";
    let count: u64 = db
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM (SELECT 1 FROM main.requests WHERE {predicate} LIMIT 8193)"
            ),
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if count > MAX_RECEIPTS {
        return Err(invalid("equivalence receipt count exceeds admission"));
    }
    let bytes:u64=db.query_row(&format!("SELECT COALESCE(SUM(length(CAST(response AS BLOB))),0) FROM main.requests WHERE {predicate}"),[],|r|r.get(0)).map_err(internal)?;
    if bytes > MAX_RECEIPT_BYTES {
        return Err(invalid("equivalence receipt bytes exceed admission"));
    }
    let mut query=db.prepare(&format!("SELECT CASE WHEN typeof(session)='text' AND length(CAST(session AS BLOB)) BETWEEN 1 AND 128 THEN session ELSE NULL END,CASE WHEN typeof(request)='text' AND length(CAST(request AS BLOB)) BETWEEN 1 AND 128 THEN request ELSE NULL END,CASE WHEN typeof(fingerprint)='text' AND length(fingerprint)=64 THEN fingerprint ELSE NULL END,CASE WHEN typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND {RECEIPT_LIMIT} THEN response ELSE NULL END FROM main.requests WHERE {predicate} ORDER BY session,request")).map_err(internal)?;
    let mut rows = query.query([]).map_err(internal)?;
    while let Some(row) = rows.next().map_err(internal)? {
        let actor = SessionId::new(row.get::<_, String>(0).map_err(internal)?).map_err(internal)?;
        let request =
            RequestId::new(row.get::<_, String>(1).map_err(internal)?).map_err(internal)?;
        let fingerprint: String = row.get(2).map_err(internal)?;
        let raw: String = row.get(3).map_err(internal)?;
        identity_claim(db, &actor, &request, &fingerprint)?;
        let ResponseBody::InferenceEquivalenceSaved(receipt) = decode(&raw)? else {
            return Err(invalid("equivalence receipt namespace differs"));
        };
        receipt.validate()?;
        if receipt.request_id != request
            || (receipt.input_sha256.is_none()
                && receipt
                    .report
                    .decision
                    .as_ref()
                    .is_none_or(|d| d.actor_session != actor))
        {
            return Err(invalid("equivalence saved receipt actor/request differs"));
        }
        visit(&actor, &request, &fingerprint, &receipt)?;
    }
    Ok(())
}

pub(super) fn validate(db: &Connection) -> PResult<()> {
    let exists = layout(db)?;
    if !exists {
        return scan_receipts(db, |_, _, _, _| {
            Err(invalid("equivalence history exists without its registry"))
        });
    }
    capacity(db)?;
    history_requires_reader(db)?;
    let ids: Vec<String> = {
        let mut q=db.prepare("SELECT CASE WHEN typeof(id)='text' AND length(id)=64 THEN id ELSE NULL END FROM main.inference_equivalence_records ORDER BY id LIMIT 33").map_err(internal)?;
        let rows = q
            .query_map([], |r| r.get(0))
            .map_err(internal)?
            .collect::<std::result::Result<_, _>>()
            .map_err(internal)?;
        rows
    };
    let mut current = BTreeMap::new();
    for raw in ids {
        let id = ArtifactId::new(raw).map_err(internal)?;
        let value = report(db, &id)?;
        verify_evidence(db, &value)?;
        current.insert(id, value);
    }
    let bad_evidence:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM main.inference_equivalence_evidence e LEFT JOIN main.inference_equivalence_records r ON e.dossier=r.id WHERE r.id IS NULL OR typeof(e.kind)!='text' OR length(CAST(e.kind AS BLOB))>128 OR typeof(e.sha256)!='text' OR length(e.sha256)!=64 OR typeof(e.data)!='blob' OR length(e.data) NOT BETWEEN 1 AND 4194304)",[],|r|r.get(0)).map_err(internal)?;
    if bad_evidence {
        return Err(invalid("equivalence evidence storage membership differs"));
    }
    let floor = clock_floor(db)?.unwrap_or(0);
    let mut decisions = BTreeSet::new();
    {
        let mut q=db.prepare("SELECT CASE WHEN typeof(dossier)='text' AND length(dossier)=64 THEN dossier ELSE NULL END,CASE WHEN typeof(revision)='integer' AND revision BETWEEN 2 AND 9223372036854775807 THEN revision ELSE NULL END,CASE WHEN typeof(actor)='text' AND length(CAST(actor AS BLOB)) BETWEEN 1 AND 128 THEN actor ELSE NULL END,CASE WHEN typeof(request)='text' AND length(CAST(request AS BLOB)) BETWEEN 1 AND 128 THEN request ELSE NULL END FROM main.inference_equivalence_decisions ORDER BY dossier,revision").map_err(internal)?;
        let mut rows = q.query([]).map_err(internal)?;
        while let Some(row) = rows.next().map_err(internal)? {
            let id =
                ArtifactId::new(row.get::<_, String>(0).map_err(internal)?).map_err(internal)?;
            let reviewed: u64 = row.get(1).map_err(internal)?;
            let actor =
                SessionId::new(row.get::<_, String>(2).map_err(internal)?).map_err(internal)?;
            let request =
                RequestId::new(row.get::<_, String>(3).map_err(internal)?).map_err(internal)?;
            let (_, receipt) = saved_receipt(db, &actor, &request)?;
            if receipt.input_sha256.is_some()
                || receipt.report.dossier_id != id
                || receipt.report.revision != reviewed
            {
                return Err(invalid(
                    "equivalence decision ledger differs from saved receipt",
                ));
            }
            let latest = current
                .get(&id)
                .ok_or_else(|| invalid("historical equivalence dossier was removed"))?;
            if reviewed > latest.revision
                || encode(&receipt.report.manifest)? != encode(&latest.manifest)?
            {
                return Err(invalid(
                    "historical equivalence decision is not in the current immutable dossier",
                ));
            }
            decision_link(db, &receipt.report)?;
            if receipt
                .report
                .decision
                .as_ref()
                .and_then(|d| d.recorded_at_unix_ms)
                .is_some_and(|at| at > floor)
            {
                return Err(invalid(
                    "equivalence clock floor was lowered below a retained decision",
                ));
            }
            decisions.insert((id, reviewed));
        }
    }
    for (id, value) in &current {
        let revisions: Vec<_> = decisions
            .iter()
            .filter(|(key, _)| key == id)
            .map(|(_, v)| *v)
            .collect();
        if value.revision.checked_sub(1) != Some(revisions.len() as u64)
            || revisions
                .iter()
                .enumerate()
                .any(|(i, v)| *v != i as u64 + 2)
        {
            return Err(invalid(
                "equivalence decision history was removed or reordered",
            ));
        }
    }
    let mut completed_intents = BTreeSet::new();
    {
        let mut q=db.prepare("SELECT CASE WHEN typeof(actor)='text' AND length(CAST(actor AS BLOB)) BETWEEN 1 AND 128 THEN actor ELSE NULL END,CASE WHEN typeof(request)='text' AND length(CAST(request AS BLOB)) BETWEEN 1 AND 128 THEN request ELSE NULL END FROM main.inference_equivalence_withdrawals ORDER BY actor,request").map_err(internal)?;
        let mut rows = q.query([]).map_err(internal)?;
        while let Some(row) = rows.next().map_err(internal)? {
            let actor =
                SessionId::new(row.get::<_, String>(0).map_err(internal)?).map_err(internal)?;
            let request =
                RequestId::new(row.get::<_, String>(1).map_err(internal)?).map_err(internal)?;
            let intent = read_intent(db, &actor, &request)?
                .ok_or_else(|| invalid("withdrawal intent disappeared"))?;
            if let Some(rev) = intent.completed_revision {
                let (fingerprint, receipt) = saved_receipt(db, &actor, &request)?;
                let decision = receipt
                    .report
                    .decision
                    .as_ref()
                    .ok_or_else(|| invalid("withdrawal decision absent"))?;
                if fingerprint != intent.fingerprint
                    || receipt.input_sha256.is_some()
                    || receipt.report.dossier_id != intent.dossier_id
                    || receipt.report.revision != rev
                    || receipt.report.state != InferenceEquivalenceState::Withdrawn
                    || decision.note != intent.reason
                    || decision.actor_session != actor
                    || decision.request_id != request
                    || !decisions.contains(&(intent.dossier_id.clone(), rev))
                {
                    return Err(invalid(
                        "completed withdrawal intent and original decision differ",
                    ));
                }
                completed_intents.insert((intent.dossier_id, rev));
            } else {
                pending_withdrawal(db, &intent.dossier_id)?
                    .ok_or_else(|| invalid("pending withdrawal disappeared"))?;
            }
        }
    }
    scan_receipts(db, |_, _, _, receipt| {
        let latest = current
            .get(&receipt.report.dossier_id)
            .ok_or_else(|| invalid("saved equivalence receipt lost its dossier"))?;
        if receipt.report.revision > latest.revision
            || encode(&receipt.report.manifest)? != encode(&latest.manifest)?
            || (receipt.report.revision == latest.revision
                && bounded_report(&receipt.report)? != bounded_report(latest)?)
        {
            return Err(invalid(
                "saved equivalence receipt differs from retained history",
            ));
        }
        if receipt.report.decision.is_some() {
            decision_link(db, &receipt.report)?;
        }
        if receipt.report.state == InferenceEquivalenceState::Withdrawn
            && !completed_intents
                .contains(&(receipt.report.dossier_id.clone(), receipt.report.revision))
        {
            return Err(invalid(
                "withdrawal receipt lost its original durable intent",
            ));
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
#[path = "inference_equivalence_resolver_store_tests.rs"]
mod resolver_store_tests;
