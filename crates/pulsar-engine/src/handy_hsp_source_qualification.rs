//! Independent HSP source/presentation evidence and operator review. No network or actuation.
use super::*;
use crate::authority::browser_media::{NativeBrowserMediaSuccessor, SourceMediaPin};
use std::{
    io::Read,
    sync::Weak,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_DOSSIERS: u64 = 32;
const MAX_EVIDENCE_BYTES: u64 = 128 * 1024 * 1024;
const PIN_MEMORY: u64 = 128 * 1024;
const CREATE_SCHEMA: &str = "CREATE TABLE handy_hsp_source_qualification_schema(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL CHECK(version=1))";
const CREATE_RECORDS: &str = "CREATE TABLE handy_hsp_source_qualifications(id TEXT PRIMARY KEY NOT NULL,revision INTEGER NOT NULL,report TEXT NOT NULL)";
const CREATE_EVIDENCE: &str = "CREATE TABLE handy_hsp_source_qualification_evidence(dossier TEXT NOT NULL REFERENCES handy_hsp_source_qualifications(id),kind TEXT NOT NULL,sha256 TEXT NOT NULL,data BLOB NOT NULL,PRIMARY KEY(dossier,kind))";

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(message)
}
fn changed() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "HSP source qualification context changed",
    )
}
fn unavailable() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "HSP source qualification is not current",
    )
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn owner_matches(expected: &str, actual: &str) -> bool {
    let difference = expected
        .bytes()
        .zip(actual.bytes())
        .fold(0u8, |sum, (a, b)| sum | (a ^ b));
    expected.len() == actual.len() && difference == 0
}

struct PinData {
    binding: HandyHspSourceQualificationBinding,
    subject: HandyHspSourceSubject,
    max_snapshot_bytes: u64,
    live: AtomicBool,
    _memory: Reservation,
}
/// Native current acceptance plus an actual prepared source lifetime. A report
/// cannot construct this type and a later acceptance cannot revive an old pin.
#[derive(Clone)]
pub(super) struct AcceptedHspSource {
    data: Arc<PinData>,
    source: SourceMediaPin,
    shutdown: Arc<AtomicBool>,
}
impl AcceptedHspSource {
    pub(super) fn current(&self) -> bool {
        self.data.live.load(Ordering::Acquire)
            && !self.shutdown.load(Ordering::Acquire)
            && self.source.current()
    }
    pub(super) fn binding(&self) -> &HandyHspSourceQualificationBinding {
        &self.data.binding
    }
    pub(super) fn subject(&self) -> &HandyHspSourceSubject {
        &self.data.subject
    }
}
#[derive(Default)]
pub(super) struct Registry {
    pins: HashMap<ArtifactId, Weak<PinData>>,
    /// A failed withdrawal is conservative in this boot. Only an explicitly
    /// newer review revision can clear its local admission fence.
    fenced: HashMap<ArtifactId, u64>,
    fence_capacity_exhausted: bool,
}
impl Registry {
    fn invalidate(&mut self, id: &ArtifactId) {
        if let Some(pin) = self.pins.remove(id).and_then(|pin| pin.upgrade()) {
            pin.live.store(false, Ordering::Release);
        }
    }
    fn fence(&mut self, id: &ArtifactId, revision: u64) {
        self.invalidate(id);
        // Normal immutable storage contains at most 32 dossier identities. If
        // external database replacement violates that invariant in this boot,
        // retain every prior fence and close admission instead of growing state
        // or evicting a fence which would revive an earlier acceptance.
        if !self.fenced.contains_key(id) && self.fenced.len() >= MAX_DOSSIERS as usize {
            self.cancel_all();
            self.fence_capacity_exhausted = true;
            return;
        }
        self.fenced
            .entry(id.clone())
            .and_modify(|old| *old = (*old).max(revision))
            .or_insert(revision);
    }
    pub(super) fn cancel_all(&mut self) {
        for pin in self.pins.values().filter_map(Weak::upgrade) {
            pin.live.store(false, Ordering::Release);
        }
        self.pins.clear();
    }
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

fn layout(db: &Connection) -> Result<bool> {
    let temporary:u64=db.query_row("SELECT count(*) FROM sqlite_temp_master WHERE lower(name) GLOB 'handy_hsp_source_*' OR lower(tbl_name) GLOB 'handy_hsp_source_*'",[],|r|r.get(0))?;
    anyhow::ensure!(
        temporary == 0,
        "temporary HSP source evidence objects are forbidden"
    );
    let mut found = 0;
    for (table, expected) in [
        ("handy_hsp_source_qualification_schema", CREATE_SCHEMA),
        ("handy_hsp_source_qualifications", CREATE_RECORDS),
        ("handy_hsp_source_qualification_evidence", CREATE_EVIDENCE),
    ] {
        let value: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [table],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((kind, sql)) = value {
            anyhow::ensure!(
                kind == "table" && sql == expected,
                "HSP source qualification layout differs"
            );
            found += 1;
        }
    }
    anyhow::ensure!(
        found == 0 || found == 3,
        "incomplete HSP source qualification layout"
    );
    let objects:u64=db.query_row("SELECT count(*) FROM sqlite_master WHERE (lower(name) GLOB 'handy_hsp_source_*' OR lower(tbl_name) GLOB 'handy_hsp_source_*') AND sql IS NOT NULL",[],|r|r.get(0))?;
    anyhow::ensure!(objects == found, "unexpected HSP source evidence objects");
    if found == 0 {
        return Ok(false);
    }
    let rows: Vec<(i64, i64)> = {
        let mut query = db.prepare(
            "SELECT singleton,version FROM handy_hsp_source_qualification_schema LIMIT 2",
        )?;
        let rows = query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    anyhow::ensure!(
        rows == [(1, 1)],
        "HSP source qualification schema requires explicit migration"
    );
    let triggers: u64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('handy_hsp_source_qualification_schema','handy_hsp_source_qualifications','handy_hsp_source_qualification_evidence')", [], |row| row.get(0))?;
    anyhow::ensure!(triggers == 0, "unexpected HSP source qualification trigger");
    Ok(true)
}
fn capacity(db: &Connection) -> PResult<(u64, u64)> {
    let result: (u64, u64) = db.query_row("SELECT (SELECT count(*) FROM handy_hsp_source_qualifications),COALESCE(sum(length(data)),0) FROM handy_hsp_source_qualification_evidence", [], |row| Ok((row.get(0)?, row.get(1)?))).map_err(internal)?;
    if result.0 > MAX_DOSSIERS || result.1 > MAX_EVIDENCE_BYTES {
        return Err(invalid("HSP source qualification capacity exceeded"));
    }
    Ok(result)
}
fn initialize(tx: &Transaction<'_>) -> PResult<()> {
    if !layout(tx).map_err(internal)? {
        for sql in [CREATE_SCHEMA, CREATE_RECORDS, CREATE_EVIDENCE] {
            tx.execute_batch(sql).map_err(internal)?;
        }
        tx.execute(
            "INSERT INTO handy_hsp_source_qualification_schema VALUES(1,1)",
            [],
        )
        .map_err(internal)?;
    }
    Ok(())
}
fn report(db: &Connection, id: &ArtifactId) -> PResult<HandyHspSourceQualificationReport> {
    if !layout(db).map_err(internal)? {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "HSP source dossier not found",
        ));
    }
    capacity(db)?;
    let row: Option<(u64, String)> = db.query_row("SELECT revision,CASE WHEN length(CAST(report AS BLOB))<=131072 THEN report ELSE NULL END FROM handy_hsp_source_qualifications WHERE id=?1", [id.as_str()], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(internal)?;
    let (revision, raw) =
        row.ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "HSP source dossier not found"))?;
    let value: HandyHspSourceQualificationReport = decode(&raw)?;
    value.validate()?;
    crate::live_cpu_history::require_hsp_source_reader(db, &value.manifest.subject)
        .map_err(internal)?;
    if value.dossier_id != *id || value.revision != revision || value.manifest.identity()? != *id {
        return Err(invalid("HSP source dossier identity or revision differs"));
    }
    if let Some(decision) = &value.decision {
        let raw: String = db.query_row("SELECT CASE WHEN length(CAST(response AS BLOB))<=262144 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2", params![decision.actor_session.as_str(), decision.request_id.as_str()], |row| row.get(0)).map_err(internal)?;
        let ResponseBody::HandyHspSourceQualificationSaved(receipt) = decode(&raw)? else {
            return Err(invalid("HSP source review receipt missing"));
        };
        receipt.validate()?;
        if receipt.request_id != decision.request_id
            || receipt.input_sha256.is_some()
            || encode(&receipt.report)? != encode(&value)?
        {
            return Err(invalid("HSP source review and receipt disagree"));
        }
    }
    Ok(value)
}
fn accepted_revision(
    db: &Connection,
    value: &HandyHspSourceQualificationReport,
    binding: &HandyHspSourceQualificationBinding,
) -> PResult<bool> {
    if value.dossier_id != binding.dossier_id {
        return Ok(false);
    }
    if value.revision == binding.revision
        && value.state == HandyQualificationState::Accepted
        && value.manifest.subject.origin == HandyEvidenceOrigin::Physical
    {
        return Ok(true);
    }
    #[cfg(test)]
    {
        if value.revision == 1
            && binding.revision == 2
            && value.state == HandyQualificationState::Pending
            && value.manifest.subject.origin == HandyEvidenceOrigin::Synthetic
        {
            let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE name='pulsar_test_hsp_source_acceptance')",[],|r|r.get(0)).map_err(internal)?;
            if exists {
                return db.query_row("SELECT EXISTS(SELECT 1 FROM temp.pulsar_test_hsp_source_acceptance WHERE dossier=?1 AND revision=?2 AND subject_sha256=?3)",params![binding.dossier_id.as_str(),binding.revision,value.manifest.subject.identity_sha256()?],|r|r.get(0)).map_err(internal);
            }
        }
    }
    #[cfg(not(test))]
    let _ = db;
    Ok(false)
}
/// Original dossier contents for startup history comparison only. This neither
/// checks current acceptance nor creates a native source pin; withdrawn or
/// synthetic historical evidence cannot become authority through this read.
pub(super) fn retained_hsp_source_subject(
    db: &Connection,
    id: &ArtifactId,
) -> PResult<HandyHspSourceSubject> {
    Ok(report(db, id)?.manifest.subject)
}

/// Metadata hint only: callers still capture the actual source lifetime and
/// revalidate the complete expected subject before any native admission.
pub(super) fn accepted_hsp_source_subject(
    db: &Connection,
    binding: &HandyHspSourceQualificationBinding,
) -> PResult<HandyHspSourceSubject> {
    binding.validate()?;
    let value = report(db, &binding.dossier_id)?;
    if !accepted_revision(db, &value, binding)? {
        return Err(unavailable());
    }
    verify_evidence(db, &value)?;
    Ok(value.manifest.subject)
}
/// Authored test permit only. It neither changes a persisted report nor labels
/// synthetic evidence Physical. No corresponding admission branch exists in a
/// production build, and this TEMP state cannot survive connection/process loss.
#[cfg(test)]
pub(super) fn install_authored_hsp_source_acceptance_for_test(
    db: &Connection,
    id: &ArtifactId,
) -> PResult<HandyHspSourceQualificationBinding> {
    let value = report(db, id)?;
    if value.revision != 1
        || value.state != HandyQualificationState::Pending
        || value.manifest.subject.origin != HandyEvidenceOrigin::Synthetic
    {
        return Err(changed());
    }
    verify_evidence(db, &value)?;
    hardware(db, &value.manifest.subject.device_qualification)?;
    db.execute_batch("CREATE TEMP TABLE IF NOT EXISTS pulsar_test_hsp_source_acceptance(dossier TEXT PRIMARY KEY,revision INTEGER NOT NULL,subject_sha256 TEXT NOT NULL)").map_err(internal)?;
    let count: u64 = db
        .query_row(
            "SELECT count(*) FROM temp.pulsar_test_hsp_source_acceptance",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if count >= MAX_DOSSIERS {
        return Err(unavailable());
    }
    db.execute(
        "INSERT INTO temp.pulsar_test_hsp_source_acceptance VALUES(?1,2,?2)",
        params![id.as_str(), value.manifest.subject.identity_sha256()?],
    )
    .map_err(internal)?;
    Ok(HandyHspSourceQualificationBinding {
        dossier_id: id.clone(),
        revision: 2,
    })
}

fn verify_evidence(db: &Connection, report: &HandyHspSourceQualificationReport) -> PResult<()> {
    let count: u64 = db
        .query_row(
            "SELECT count(*) FROM handy_hsp_source_qualification_evidence WHERE dossier=?1",
            [report.dossier_id.as_str()],
            |row| row.get(0),
        )
        .map_err(internal)?;
    if count != report.manifest.subject.required_evidence_kinds().len() as u64 {
        return Err(invalid("HSP source evidence membership differs"));
    }
    for evidence in &report.manifest.evidence {
        let (hash, bytes): (String, Vec<u8>) = db.query_row("SELECT sha256,CASE WHEN length(data) BETWEEN 1 AND 4194304 THEN data ELSE NULL END FROM handy_hsp_source_qualification_evidence WHERE dossier=?1 AND kind=?2", params![report.dossier_id.as_str(), encode(&evidence.kind)?], |row| Ok((row.get(0)?, row.get(1)?))).map_err(internal)?;
        if hash != evidence.identity.sha256
            || bytes.len() as u64 != evidence.identity.byte_len
            || digest(&bytes) != hash
        {
            return Err(invalid("retained HSP source evidence changed"));
        }
    }
    Ok(())
}
pub(super) fn validate(db: &Connection) -> Result<()> {
    if !layout(db)? {
        return Ok(());
    }
    capacity(db)?;
    let ids: Vec<String> = {
        let mut query = db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM handy_hsp_source_qualifications LIMIT 33")?;
        let rows = query
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    anyhow::ensure!(
        ids.len() <= MAX_DOSSIERS as usize,
        "HSP source dossier capacity exceeded"
    );
    for id in ids {
        verify_evidence(db, &report(db, &ArtifactId::new(id)?)?)?;
    }
    let bad: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM handy_hsp_source_qualification_evidence e LEFT JOIN handy_hsp_source_qualifications q ON q.id=e.dossier WHERE q.id IS NULL OR typeof(data)!='blob' OR length(data) NOT BETWEEN 1 AND 4194304 OR length(sha256)!=64)", [], |row| row.get(0))?;
    anyhow::ensure!(!bad, "invalid HSP source evidence storage");
    Ok(())
}
pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::ImportHandyHspSourceQualification { .. }
            | Command::HandyHspSourceQualificationStatus { .. }
            | Command::AcceptHandyHspSourceQualification { .. }
            | Command::WithdrawHandyHspSourceQualification { .. }
    )
}

/// Import/acceptance require the exact already accepted physical schema7 HSP
/// decision. Hosted controller/script acceptance is never substituted here.
fn hardware(
    db: &Connection,
    binding: &HandyQualificationBinding,
) -> PResult<HandyQualificationReport> {
    binding.validate()?;
    #[cfg(not(test))]
    let query = "SELECT CASE WHEN length(CAST(report AS BLOB))<=131072 THEN report ELSE NULL END FROM handy_qualifications WHERE id=?1 AND revision=?2";
    // The native resolver below is the sole fixture-authority gate. The test
    // hint lookup may see Pending revision 1 for an explicit virtual binding 2.
    #[cfg(test)]
    let query = "SELECT CASE WHEN length(CAST(report AS BLOB))<=131072 THEN report ELSE NULL END FROM handy_qualifications WHERE id=?1 AND (revision=?2 OR (revision=1 AND ?2=2))";
    let raw: String = db
        .query_row(
            query,
            params![binding.dossier_id.as_str(), binding.revision],
            |r| r.get(0),
        )
        .map_err(|_| unavailable())?;
    let hint: HandyQualificationReport = decode(&raw)?;
    hint.validate()?;
    let subject = &hint.manifest.subject;
    let registration =
        handy_registrations::current(db, &subject.device_id, subject.registration_revision)?;
    handy_qualification::accepted_hsp_record(db, binding, &registration)
}
fn subject_current(
    db: &Connection,
    subject: &HandyHspSourceSubject,
    max_snapshot_bytes: u64,
) -> PResult<()> {
    subject.validate()?;
    let device = hardware(db, &subject.device_qualification)?;
    if let Some(selected) = &subject.finite_drain_policy_sha256 {
        if device
            .manifest
            .subject
            .hsp_finite_drain_policy_sha256()?
            .as_ref()
            != Some(selected)
        {
            return Err(changed());
        }
    }
    let hsp = device.manifest.subject.hsp.as_ref().ok_or_else(changed)?;
    let policy = hsp.policy.to_policy()?;
    let lead_min = subject
        .lookahead
        .minimum_lead_us
        .checked_mul(1000)
        .ok_or_else(changed)?;
    let lead_max = subject
        .lookahead
        .maximum_lead_us
        .checked_mul(1000)
        .ok_or_else(changed)?;
    let age = subject
        .maximum_capture_to_engine_receipt_ns
        .checked_add(subject.maximum_analysis_age_ns)
        .ok_or_else(changed)?;
    if subject.context != device.manifest.subject.context
        || subject.timing.maximum_alignment_error_ns
            > device.manifest.subject.profile.maximum_clock_uncertainty_ns
        || lead_min < u64::from(policy.min_lead_ms) * 1_000_000
        || lead_max > u64::from(policy.max_future_ms) * 1_000_000
        || age > policy.max_sample_age_ns
    {
        return Err(changed());
    }
    let revision: u64 = db
        .query_row(
            "SELECT revision FROM projects WHERE id=?1",
            [subject.project_id.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if revision != subject.revision.value()
        || source_placements::active(
            db,
            &subject.project_id,
            &subject.selection.placement.placement_id,
        )? != subject.selection.placement
    {
        return Err(changed());
    }
    source_kinds::require_media(db, &subject.project_id, &subject.selection.source_version)?;
    let (hash,bytes,evicted):(String,u64,bool)=db.query_row("SELECT CASE WHEN length(sha256)=64 THEN sha256 ELSE NULL END,bytes,evicted FROM sources WHERE project=?1 AND version=?2",params![subject.project_id.as_str(),subject.selection.source_version.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(internal)?;
    if evicted
        || hash != subject.selection.source.sha256
        || hash != subject.selection.source.artifact_id.as_str()
        || bytes != subject.selection.source.byte_len
        || bytes > max_snapshot_bytes
    {
        return Err(changed());
    }
    Ok(())
}
/// Shared with the HSP source owner. Call while holding the project DB,
/// before native timing/controller locks. It never creates or revives a pin.
pub(super) fn revalidate_accepted_hsp_source(
    db: &Connection,
    pool: &ResourcePool,
    pin: &AcceptedHspSource,
) -> PResult<()> {
    // Source expiry is local to this wrapper. An older source lease may expire
    // after the same accepted subject has transferred to another native lease.
    // Only failure of the common accepted record/evidence invalidates all pins.
    if !pin.current() {
        return Err(unavailable());
    }
    let _memory = pool.reserve_memory(32 * 1024 * 1024)?;
    let checked = (|| {
        let value = report(db, &pin.data.binding.dossier_id)?;
        if !accepted_revision(db, &value, &pin.data.binding)?
            || !value.manifest.subject.exact_eq(&pin.data.subject)
        {
            return Err(changed());
        }
        verify_evidence(db, &value)?;
        subject_current(db, &pin.data.subject, pin.data.max_snapshot_bytes)?;
        Ok(())
    })();
    if checked.is_err() {
        pin.data.live.store(false, Ordering::Release);
    }
    checked?;
    if !pin.current() {
        return Err(unavailable());
    }
    Ok(())
}

/// Call under the project DB guard, before controller locks. Explicit selection
/// must match both current accepted dossiers and the original native source pin.
/// No selection retains conservative Stop even when a dossier supports drain.
pub(super) fn accepted_hsp_finite_drain_policy(
    db: &Connection,
    pool: &ResourcePool,
    pin: &AcceptedHspSource,
    device_binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
    selected_policy_sha256: Option<&str>,
) -> PResult<Option<handy_qualification::QualifiedHspFiniteDrainPolicy>> {
    revalidate_accepted_hsp_source(db, pool, pin)?;
    if pin.subject().device_qualification != *device_binding {
        return Err(changed());
    }
    let Some(selected) = selected_policy_sha256 else {
        return Ok(None);
    };
    validate_sha256(selected)?;
    if pin.subject().finite_drain_policy_sha256.as_deref() != Some(selected) {
        return Err(changed());
    }
    let policy = handy_qualification::accepted_hsp_finite_drain_policy(
        db,
        device_binding,
        registration,
        selected,
    )?;
    if !pin.current() {
        return Err(unavailable());
    }
    Ok(Some(policy))
}
impl Engine {
    fn hsp_source_subject_current(
        &self,
        db: &Connection,
        subject: &HandyHspSourceSubject,
    ) -> PResult<()> {
        if self.is_shutting_down() {
            return Err(unavailable());
        }
        subject_current(db, subject, self.config.max_snapshot_bytes)
    }
    /// The caller retains the project DB guard through this operation. The
    /// expected subject must come from its actual admitted source/profile/options.
    /// This pin remains independent of device preparation and final source clocks.
    pub(super) fn capture_handy_hsp_source_locked(
        &self,
        db: &Connection,
        request: &Request,
        binding: &HandyHspSourceQualificationBinding,
        media: &BrowserMediaBinding,
        expected: &HandyHspSourceSubject,
    ) -> PResult<AcceptedHspSource> {
        request.validate()?;
        binding.validate()?;
        expected.validate()?;
        media.validate()?;
        let actor = request
            .session
            .as_ref()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing is required"))?;
        self.authenticate(db, actor, request.auth_token.as_deref())?;
        require_grant(db, actor, project(request)?, Scope::Read)?;
        require_grant(db, actor, project(request)?, Scope::Export)?;
        let _memory = self.pool.reserve_memory(32 * 1024 * 1024)?;
        let report = report(db, &binding.dossier_id)?;
        if !accepted_revision(db, &report, binding)? {
            return Err(unavailable());
        }
        verify_evidence(db, &report)?;
        let subject = report.manifest.subject;
        if !subject.exact_eq(expected)
            || request.project.as_ref() != Some(&subject.project_id)
            || request.expected_revision != Some(subject.revision)
            || media.project_id != subject.project_id
            || media.revision != subject.revision
            || media.selection != subject.selection
            || media.clock != subject.clock
            || media.engine_epoch.as_str() != self.engine_epoch
        {
            return Err(changed());
        }
        self.hsp_source_subject_current(db, &subject)?;
        let source = self.pin_browser_media_locked(db, request, media)?;
        let mut registry = self
            .handy_hsp_source_qualifications
            .lock()
            .map_err(|poisoned| {
                poisoned.into_inner().cancel_all();
                unavailable()
            })?;
        if registry.fence_capacity_exhausted
            || registry
                .fenced
                .get(&binding.dossier_id)
                .is_some_and(|revision| *revision >= binding.revision)
        {
            return Err(unavailable());
        }
        let existing = registry
            .pins
            .get(&binding.dossier_id)
            .and_then(Weak::upgrade);
        let data = if let Some(existing) = existing.filter(|pin| {
            pin.live.load(Ordering::Acquire)
                && pin.binding == *binding
                && pin.subject.exact_eq(&subject)
        }) {
            existing
        } else {
            registry.invalidate(&binding.dossier_id);
            registry.pins.retain(|_, pin| pin.strong_count() > 0);
            if registry.pins.len() >= MAX_DOSSIERS as usize {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "HSP source native pin capacity exceeded",
                ));
            }
            let data = Arc::new(PinData {
                binding: binding.clone(),
                subject,
                max_snapshot_bytes: self.config.max_snapshot_bytes,
                live: AtomicBool::new(true),
                _memory: self.pool.reserve_memory(PIN_MEMORY)?,
            });
            registry
                .pins
                .insert(binding.dossier_id.clone(), Arc::downgrade(&data));
            data
        };
        Ok(AcceptedHspSource {
            data,
            source,
            shutdown: self.shutdown_requested.clone(),
        })
    }

    /// Resolve the unchanged current acceptance onto the exact native media
    /// successor. Call under DB, before capture/timing guards. This does not
    /// approve new evidence or activate a source/capture/controller.
    pub(super) fn capture_handy_hsp_source_successor_locked(
        &self,
        db: &Connection,
        request: &Request,
        predecessor: &AcceptedHspSource,
        media: &NativeBrowserMediaSuccessor,
        expected: &HandyHspSourceSubject,
    ) -> PResult<AcceptedHspSource> {
        if !media.current()
            || !media.matches(&predecessor.source, media.successor())
            || !predecessor.subject().exact_eq(expected)
        {
            return Err(changed());
        }
        revalidate_accepted_hsp_source(db, &self.pool, predecessor)?;
        let successor = self.capture_handy_hsp_source_locked(
            db,
            request,
            predecessor.binding(),
            media.successor().binding(),
            expected,
        )?;
        if !media.current()
            || !predecessor.current()
            || !successor.current()
            || !media.matches(&predecessor.source, &successor.source)
            || !Arc::ptr_eq(&predecessor.data, &successor.data)
        {
            return Err(changed());
        }
        Ok(successor)
    }

    /// Public project/device/qualification mutations call this while retaining
    /// DB ownership. No source file or network read is performed here.
    pub(super) fn retire_changed_handy_hsp_source_qualifications(&self, db: &Connection) {
        let pins: Vec<_> = self
            .handy_hsp_source_qualifications
            .lock()
            .unwrap_or_else(|e| {
                let mut registry = e.into_inner();
                registry.cancel_all();
                registry
            })
            .pins
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        let memory = self.pool.reserve_memory(32 * 1024 * 1024);
        for pin in pins {
            if memory.is_err() {
                pin.live.store(false, Ordering::Release);
                continue;
            }
            let eligible = report(db, &pin.binding.dossier_id).and_then(|value| {
                if !accepted_revision(db, &value, &pin.binding)?
                    || !value.manifest.subject.exact_eq(&pin.subject)
                {
                    return Err(changed());
                }
                verify_evidence(db, &value)?;
                self.hsp_source_subject_current(db, &pin.subject)
                    .map(|_| ())
            });
            if eligible.is_err() {
                pin.live.store(false, Ordering::Release);
            }
        }
    }

    pub(super) fn handy_hsp_source_qualification_control(
        &self,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        let proof = match &request.command {
            Command::ImportHandyHspSourceQualification { owner_proof, .. }
            | Command::HandyHspSourceQualificationStatus { owner_proof, .. }
            | Command::AcceptHandyHspSourceQualification { owner_proof, .. }
            | Command::WithdrawHandyHspSourceQualification { owner_proof, .. } => owner_proof,
            _ => {
                return Err(ProtocolError::unsupported(
                    "HSP source qualification operation",
                ))
            }
        };
        if !owner_matches(&self.handy_owner_token, proof) {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "Handy owner capability required",
            ));
        }
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        require_grant(&db, actor, project(request)?, Scope::Read)?;
        require_grant(&db, actor, project(request)?, Scope::Export)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        if let Command::HandyHspSourceQualificationStatus { dossier_id, .. } = &request.command {
            let value = report(&db, dossier_id)?;
            if request.project.as_ref() != Some(&value.manifest.subject.project_id) {
                return Err(changed());
            }
            let _memory = self.pool.reserve_memory(8 * 1024 * 1024)?;
            verify_evidence(&db, &value)?;
            return Ok(ResponseBody::HandyHspSourceQualification(Box::new(value)));
        }
        if let Some(body) = replay(&db, actor.as_str(), request, fingerprint)? {
            let ResponseBody::HandyHspSourceQualificationSaved(receipt) = &body else {
                return Err(invalid("HSP source replay type differs"));
            };
            receipt.validate()?;
            if receipt.request_id != request.request_id
                || request.project.as_ref() != Some(&receipt.report.manifest.subject.project_id)
            {
                return Err(invalid("HSP source replay identity differs"));
            }
            let matched = match &request.command {
                Command::ImportHandyHspSourceQualification {
                    manifest_sha256, ..
                } => receipt.input_sha256.as_ref() == Some(manifest_sha256),
                Command::AcceptHandyHspSourceQualification {
                    dossier_id,
                    expected_record_revision,
                    operator,
                    ..
                } => {
                    receipt.input_sha256.is_none()
                        && receipt.report.dossier_id == *dossier_id
                        && Some(receipt.report.revision) == expected_record_revision.checked_add(1)
                        && receipt.report.state == HandyQualificationState::Accepted
                        && receipt.report.decision.as_ref().is_some_and(|d| {
                            d.request_id == request.request_id
                                && d.actor_session == *actor
                                && d.operator.as_ref() == Some(operator)
                                && d.note.is_empty()
                        })
                }
                Command::WithdrawHandyHspSourceQualification {
                    dossier_id,
                    expected_record_revision,
                    reason,
                    ..
                } => {
                    receipt.input_sha256.is_none()
                        && receipt.report.dossier_id == *dossier_id
                        && Some(receipt.report.revision) == expected_record_revision.checked_add(1)
                        && receipt.report.state == HandyQualificationState::Withdrawn
                        && receipt.report.decision.as_ref().is_some_and(|d| {
                            d.request_id == request.request_id
                                && d.actor_session == *actor
                                && d.operator.is_none()
                                && d.note == *reason
                        })
                }
                _ => false,
            };
            if !matched {
                return Err(invalid("HSP source replay does not match original intent"));
            }
            return Ok(body);
        }
        if let Command::ImportHandyHspSourceQualification {
            path,
            manifest_sha256,
            ..
        } = &request.command
        {
            check_revision(&db, request)?;
            let _memory = self.pool.reserve_memory(64 * 1024 * 1024)?;
            let mut bytes = Vec::new();
            artifacts::open_regular(path)
                .map_err(internal)?
                .take(HANDY_HSP_SOURCE_MANIFEST_LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(internal)?;
            if bytes.len() as u64 > HANDY_HSP_SOURCE_MANIFEST_LIMIT
                || digest(&bytes) != *manifest_sha256
            {
                return Err(invalid(
                    "HSP source manifest input changed or exceeds bound",
                ));
            }
            let input: HandyHspSourceQualificationInput = serde_json::from_slice(&bytes)
                .map_err(|_| invalid("invalid HSP source input document"))?;
            input.validate()?;
            if request.project.as_ref() != Some(&input.subject.project_id)
                || request.expected_revision != Some(input.subject.revision)
            {
                return Err(changed());
            }
            self.hsp_source_subject_current(&db, &input.subject)?;
            let mut retained = Vec::new();
            let mut payloads = Vec::new();
            let mut size = 0u64;
            for item in input.evidence {
                let mut bytes = Vec::new();
                artifacts::open_regular(&item.path)
                    .map_err(internal)?
                    .take(HANDY_HSP_SOURCE_EVIDENCE_LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(internal)?;
                if bytes.len() as u64 != item.byte_len || digest(&bytes) != item.sha256 {
                    return Err(invalid("HSP source evidence input changed"));
                }
                size = size
                    .checked_add(item.byte_len)
                    .ok_or_else(|| invalid("HSP source evidence size overflow"))?;
                retained.push(HandyHspSourceQualificationEvidence {
                    kind: item.kind,
                    identity: ArtifactIdentity {
                        artifact_id: ArtifactId::new(item.sha256.clone()).map_err(internal)?,
                        sha256: item.sha256,
                        byte_len: item.byte_len,
                    },
                });
                payloads.push((item.kind, bytes));
            }
            retained.sort_by_key(|entry| entry.kind);
            let manifest = HandyHspSourceQualificationManifest {
                subject: input.subject,
                evidence: retained,
            };
            manifest.validate()?;
            let id = manifest.identity()?;
            let tx = db.transaction().map_err(internal)?;
            initialize(&tx)?;
            let (count, used) = capacity(&tx)?;
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM handy_hsp_source_qualifications WHERE id=?1)",
                    [id.as_str()],
                    |row| row.get(0),
                )
                .map_err(internal)?;
            let value = if exists {
                let value = report(&tx, &id)?;
                verify_evidence(&tx, &value)?;
                value
            } else {
                if count >= MAX_DOSSIERS
                    || used
                        .checked_add(size)
                        .is_none_or(|n| n > MAX_EVIDENCE_BYTES)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "HSP source evidence store is full",
                    ));
                }
                if fs2::available_space(&self.config.state_dir).map_err(internal)?
                    < size
                        .saturating_mul(3)
                        .saturating_add(self.pool.reserved_storage())
                        .saturating_add(16 * 1024 * 1024)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "insufficient HSP source evidence storage",
                    ));
                }
                let value = HandyHspSourceQualificationReport {
                    dossier_id: id.clone(),
                    revision: 1,
                    state: HandyQualificationState::Pending,
                    manifest,
                    decision: None,
                };
                value.validate()?;
                crate::live_cpu_history::arm_hsp_source_subject(&tx, &value.manifest.subject)
                    .map_err(internal)?;
                tx.execute(
                    "INSERT INTO handy_hsp_source_qualifications VALUES(?1,1,?2)",
                    params![id.as_str(), encode(&value)?],
                )
                .map_err(internal)?;
                for (kind, bytes) in payloads {
                    let item = value
                        .manifest
                        .evidence
                        .iter()
                        .find(|item| item.kind == kind)
                        .expect("validated role");
                    tx.execute(
                        "INSERT INTO handy_hsp_source_qualification_evidence VALUES(?1,?2,?3,?4)",
                        params![id.as_str(), encode(&kind)?, item.identity.sha256, bytes],
                    )
                    .map_err(internal)?;
                }
                value
            };
            let body = ResponseBody::HandyHspSourceQualificationSaved(Box::new(
                HandyHspSourceQualificationReceipt {
                    input_sha256: Some(manifest_sha256.clone()),
                    request_id: request.request_id.clone(),
                    report: value,
                },
            ));
            remember(&tx, actor.as_str(), request, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            return Ok(body);
        }
        let (id, expected, operator, note) = match &request.command {
            Command::AcceptHandyHspSourceQualification {
                dossier_id,
                expected_record_revision,
                operator,
                ..
            } => {
                check_revision(&db, request)?;
                (
                    dossier_id,
                    *expected_record_revision,
                    Some(operator.clone()),
                    String::new(),
                )
            }
            Command::WithdrawHandyHspSourceQualification {
                dossier_id,
                expected_record_revision,
                reason,
                ..
            } => (dossier_id, *expected_record_revision, None, reason.clone()),
            _ => {
                return Err(ProtocolError::unsupported(
                    "HSP source qualification operation",
                ))
            }
        };
        if operator.is_none() {
            let mut registry = self
                .handy_hsp_source_qualifications
                .lock()
                .map_err(|poisoned| {
                    poisoned.into_inner().cancel_all();
                    unavailable()
                })?;
            // A corrupt durable report cannot erase the exact already-captured
            // native subject. Retire only that original revision/project; an
            // old withdrawal must not touch a newer accepted revision.
            let exact = registry
                .pins
                .get(id)
                .and_then(Weak::upgrade)
                .is_some_and(|pin| {
                    pin.binding.revision == expected
                        && request.project.as_ref() == Some(&pin.subject.project_id)
                });
            if exact {
                registry.fence(id, expected);
            }
        }
        let mut value = report(&db, id)?;
        if request.project.as_ref() != Some(&value.manifest.subject.project_id) {
            return Err(changed());
        }
        if value.revision != expected {
            return Err(ProtocolError::new(
                ErrorCode::RevisionConflict,
                "HSP source review revision changed",
            ));
        }
        let accepted = operator.is_some();
        let _memory = self.pool.reserve_memory(32 * 1024 * 1024)?;
        if accepted {
            if request.expected_revision != Some(value.manifest.subject.revision) {
                return Err(changed());
            }
            verify_evidence(&db, &value)?;
            if value.manifest.subject.origin != HandyEvidenceOrigin::Physical {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "synthetic browser evidence cannot receive physical acceptance",
                ));
            }
            self.hsp_source_subject_current(&db, &value.manifest.subject)?;
        } else {
            // Evidence corruption must not prevent withdrawal. Invalidate native
            // users before attempting the durable update, including failed writes.
            self.handy_hsp_source_qualifications
                .lock()
                .map_err(|poisoned| {
                    poisoned.into_inner().cancel_all();
                    unavailable()
                })?
                .fence(id, expected);
        }
        value.revision = expected
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(|| invalid("HSP source review revision overflow"))?;
        value.state = if accepted {
            HandyQualificationState::Accepted
        } else {
            HandyQualificationState::Withdrawn
        };
        value.decision = Some(HandyHspSourceQualificationDecision {
            revision: value.revision,
            state: value.state,
            request_id: request.request_id.clone(),
            actor_session: actor.clone(),
            operator,
            note,
            recorded_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(internal)?
                .as_millis()
                .try_into()
                .map_err(internal)?,
        });
        value.validate()?;
        let tx = db.transaction().map_err(internal)?;
        crate::live_cpu_history::arm_hsp_source_subject(&tx, &value.manifest.subject)
            .map_err(internal)?;
        let rows = tx.execute("UPDATE handy_hsp_source_qualifications SET revision=?1,report=?2 WHERE id=?3 AND revision=?4", params![value.revision, encode(&value)?, id.as_str(), expected]).map_err(internal)?;
        if rows != 1 {
            return Err(invalid("HSP source review compare-and-swap changed"));
        }
        let body = ResponseBody::HandyHspSourceQualificationSaved(Box::new(
            HandyHspSourceQualificationReceipt {
                input_sha256: None,
                request_id: request.request_id.clone(),
                report: value,
            },
        ));
        remember(&tx, actor.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        self.handy_hsp_source_qualifications
            .lock()
            .map_err(|poisoned| {
                poisoned.into_inner().cancel_all();
                unavailable()
            })?
            .invalidate(id);
        Ok(body)
    }
}

#[cfg(test)]
mod bounded_registry_tests {
    use super::*;

    #[test]
    fn browser_dossier_withdrawal_fences_never_grow_or_evict_prior_retirement() {
        let mut registry = Registry::default();
        for i in 0..MAX_DOSSIERS {
            registry.fence(&ArtifactId::new(format!("{i:064x}")).unwrap(), i + 2);
        }
        assert_eq!(registry.fenced.len(), MAX_DOSSIERS as usize);
        assert!(!registry.fence_capacity_exhausted);
        let first = ArtifactId::new(format!("{:064x}", 0)).unwrap();
        registry.fence(&first, 1);
        assert_eq!(registry.fenced[&first], 2);
        registry.fence(&first, 99);
        assert_eq!(registry.fenced[&first], 99);
        registry.fence(
            &ArtifactId::new(format!("{:064x}", MAX_DOSSIERS)).unwrap(),
            2,
        );
        assert!(registry.fence_capacity_exhausted);
        assert_eq!(registry.fenced.len(), MAX_DOSSIERS as usize);
        assert_eq!(registry.fenced[&first], 99);
    }
}
