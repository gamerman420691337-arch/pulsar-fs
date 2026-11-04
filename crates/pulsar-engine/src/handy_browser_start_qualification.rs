//! Independent browser/source evidence and operator review. No network or actuation.
use super::*;
use crate::authority::browser_media::SourceMediaPin;
use std::{
    io::Read,
    sync::Weak,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_DOSSIERS: u64 = 32;
const MAX_EVIDENCE_BYTES: u64 = 128 * 1024 * 1024;
const PIN_MEMORY: u64 = 128 * 1024;
const CREATE_SCHEMA: &str = "CREATE TABLE handy_browser_start_qualification_schema(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL CHECK(version=1))";
const CREATE_RECORDS: &str = "CREATE TABLE handy_browser_start_qualifications(id TEXT PRIMARY KEY NOT NULL,revision INTEGER NOT NULL,report TEXT NOT NULL)";
const CREATE_EVIDENCE: &str = "CREATE TABLE handy_browser_start_qualification_evidence(dossier TEXT NOT NULL REFERENCES handy_browser_start_qualifications(id),kind TEXT NOT NULL,sha256 TEXT NOT NULL,data BLOB NOT NULL,PRIMARY KEY(dossier,kind))";

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(message)
}
fn changed() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "browser-start qualification context changed",
    )
}
fn unavailable() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "browser-start qualification is not current",
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
    binding: HandyBrowserStartQualificationBinding,
    subject: HandyBrowserStartSubject,
    script: HandyStrokeScript,
    max_snapshot_bytes: u64,
    live: AtomicBool,
    _memory: Reservation,
}
/// Native current acceptance plus an actual prepared source lifetime. A report
/// cannot construct this type and a later acceptance cannot revive an old pin.
#[derive(Clone)]
pub(super) struct AcceptedBrowserStart {
    data: Arc<PinData>,
    source: SourceMediaPin,
    shutdown: Arc<AtomicBool>,
}
impl AcceptedBrowserStart {
    pub(super) fn current(&self) -> bool {
        self.data.live.load(Ordering::Acquire)
            && !self.shutdown.load(Ordering::Acquire)
            && self.source.current()
    }
    pub(super) fn binding(&self) -> &HandyBrowserStartQualificationBinding {
        &self.data.binding
    }
    pub(super) fn subject(&self) -> &HandyBrowserStartSubject {
        &self.data.subject
    }
    pub(super) fn script_quantization(&self) -> &HandyStrokeQuantization {
        &self.data.script.quantization
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
    let mut found = 0;
    for (table, expected) in [
        ("handy_browser_start_qualification_schema", CREATE_SCHEMA),
        ("handy_browser_start_qualifications", CREATE_RECORDS),
        (
            "handy_browser_start_qualification_evidence",
            CREATE_EVIDENCE,
        ),
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
                "browser-start qualification layout differs"
            );
            found += 1;
        }
    }
    anyhow::ensure!(
        found == 0 || found == 3,
        "incomplete browser-start qualification layout"
    );
    if found == 0 {
        return Ok(false);
    }
    let rows: Vec<(i64, i64)> = {
        let mut query = db.prepare(
            "SELECT singleton,version FROM handy_browser_start_qualification_schema LIMIT 2",
        )?;
        let rows = query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    anyhow::ensure!(
        rows == [(1, 1)],
        "browser-start qualification schema requires explicit migration"
    );
    let triggers: u64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('handy_browser_start_qualification_schema','handy_browser_start_qualifications','handy_browser_start_qualification_evidence')", [], |row| row.get(0))?;
    anyhow::ensure!(
        triggers == 0,
        "unexpected browser-start qualification trigger"
    );
    Ok(true)
}
fn capacity(db: &Connection) -> PResult<(u64, u64)> {
    let result: (u64, u64) = db.query_row("SELECT (SELECT count(*) FROM handy_browser_start_qualifications),COALESCE(sum(length(data)),0) FROM handy_browser_start_qualification_evidence", [], |row| Ok((row.get(0)?, row.get(1)?))).map_err(internal)?;
    if result.0 > MAX_DOSSIERS || result.1 > MAX_EVIDENCE_BYTES {
        return Err(invalid("browser-start qualification capacity exceeded"));
    }
    Ok(result)
}
fn initialize(tx: &Transaction<'_>) -> PResult<()> {
    if !layout(tx).map_err(internal)? {
        for sql in [CREATE_SCHEMA, CREATE_RECORDS, CREATE_EVIDENCE] {
            tx.execute_batch(sql).map_err(internal)?;
        }
        tx.execute(
            "INSERT INTO handy_browser_start_qualification_schema VALUES(1,1)",
            [],
        )
        .map_err(internal)?;
    }
    Ok(())
}
fn report(db: &Connection, id: &ArtifactId) -> PResult<HandyBrowserStartQualificationReport> {
    if !layout(db).map_err(internal)? {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "browser-start dossier not found",
        ));
    }
    capacity(db)?;
    let row: Option<(u64, String)> = db.query_row("SELECT revision,CASE WHEN length(CAST(report AS BLOB))<=131072 THEN report ELSE NULL END FROM handy_browser_start_qualifications WHERE id=?1", [id.as_str()], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(internal)?;
    let (revision, raw) = row.ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, "browser-start dossier not found")
    })?;
    let value: HandyBrowserStartQualificationReport = decode(&raw)?;
    value.validate()?;
    if value.dossier_id != *id || value.revision != revision || value.manifest.identity()? != *id {
        return Err(invalid(
            "browser-start dossier identity or revision differs",
        ));
    }
    if let Some(decision) = &value.decision {
        let raw: String = db.query_row("SELECT CASE WHEN length(CAST(response AS BLOB))<=262144 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2", params![decision.actor_session.as_str(), decision.request_id.as_str()], |row| row.get(0)).map_err(internal)?;
        let ResponseBody::HandyBrowserStartQualificationSaved(receipt) = decode(&raw)? else {
            return Err(invalid("browser-start review receipt missing"));
        };
        receipt.validate()?;
        if receipt.request_id != decision.request_id
            || receipt.input_sha256.is_some()
            || encode(&receipt.report)? != encode(&value)?
        {
            return Err(invalid("browser-start review and receipt disagree"));
        }
    }
    Ok(value)
}
fn verify_evidence(db: &Connection, report: &HandyBrowserStartQualificationReport) -> PResult<()> {
    let count: u64 = db
        .query_row(
            "SELECT count(*) FROM handy_browser_start_qualification_evidence WHERE dossier=?1",
            [report.dossier_id.as_str()],
            |row| row.get(0),
        )
        .map_err(internal)?;
    if count != 4 {
        return Err(invalid("browser-start evidence membership differs"));
    }
    for evidence in &report.manifest.evidence {
        let (hash, bytes): (String, Vec<u8>) = db.query_row("SELECT sha256,CASE WHEN length(data) BETWEEN 1 AND 4194304 THEN data ELSE NULL END FROM handy_browser_start_qualification_evidence WHERE dossier=?1 AND kind=?2", params![report.dossier_id.as_str(), encode(&evidence.kind)?], |row| Ok((row.get(0)?, row.get(1)?))).map_err(internal)?;
        if hash != evidence.identity.sha256
            || bytes.len() as u64 != evidence.identity.byte_len
            || digest(&bytes) != hash
        {
            return Err(invalid("retained browser-start evidence changed"));
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
        let mut query = db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM handy_browser_start_qualifications LIMIT 33")?;
        let rows = query
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    anyhow::ensure!(
        ids.len() <= MAX_DOSSIERS as usize,
        "browser-start dossier capacity exceeded"
    );
    for id in ids {
        verify_evidence(db, &report(db, &ArtifactId::new(id)?)?)?;
    }
    let bad: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM handy_browser_start_qualification_evidence e LEFT JOIN handy_browser_start_qualifications q ON q.id=e.dossier WHERE q.id IS NULL OR typeof(data)!='blob' OR length(data) NOT BETWEEN 1 AND 4194304 OR length(sha256)!=64)", [], |row| row.get(0))?;
    anyhow::ensure!(!bad, "invalid browser-start evidence storage");
    Ok(())
}
pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::ImportHandyBrowserStartQualification { .. }
            | Command::HandyBrowserStartQualificationStatus { .. }
            | Command::AcceptHandyBrowserStartQualification { .. }
            | Command::WithdrawHandyBrowserStartQualification { .. }
    )
}

/// Resolve the exact existing device acceptance. Import deliberately requires an
/// already accepted v6 dependency, so accepting hardware later cannot silently
/// change the revision named by a pending browser/source dossier.
fn hardware(
    db: &Connection,
    binding: &HandyQualificationBinding,
) -> PResult<HandyQualificationReport> {
    binding.validate()?;
    let raw: String = db.query_row("SELECT CASE WHEN length(CAST(report AS BLOB))<=131072 THEN report ELSE NULL END FROM handy_qualifications WHERE id=?1 AND revision=?2", params![binding.dossier_id.as_str(), binding.revision], |row| row.get(0)).map_err(|_| unavailable())?;
    let hint: HandyQualificationReport = decode(&raw)?;
    hint.validate()?;
    let subject = &hint.manifest.subject;
    let registration =
        handy_registrations::current(db, &subject.device_id, subject.registration_revision)?;
    let accepted = handy_qualification::accepted_recovery_record(db, binding, &registration)?;
    if accepted.manifest.subject.schema_version != 6
        || accepted.manifest.subject.continuous_timing.is_none()
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "browser start requires an accepted device-v6 timing contract",
        ));
    }
    Ok(accepted)
}
fn script(
    db: &Connection,
    subject: &HandyBrowserStartSubject,
    verify_bytes: bool,
) -> PResult<HandyStrokeScript> {
    let raw: Option<String> = db.query_row("SELECT CASE WHEN length(CAST(r.response AS BLOB))<=131072 THEN r.response ELSE NULL END FROM requests r JOIN exports e ON e.session=r.session AND e.request=r.request WHERE r.session=?1 AND r.request=?2 AND e.project=?3 AND e.state='complete' AND e.sha256=?4 AND e.axis='stroke'", params![subject.script_actor_session.as_str(), subject.script_request_id.as_str(), subject.project_id.as_str(), subject.script.sha256], |row| row.get(0)).optional().map_err(internal)?;
    let ResponseBody::HandyStrokeScript(script) = decode(&raw.ok_or_else(changed)?)? else {
        return Err(changed());
    };
    script.validate()?;
    if script.request_id != subject.script_request_id
        || script.project_id != subject.project_id
        || script.revision != subject.revision
        || script.master_sha256 != subject.master_sha256
        || script.identity != subject.script
    {
        return Err(changed());
    }
    if verify_bytes {
        let mut bytes = Vec::new();
        artifacts::open_regular(&script.path)
            .map_err(|_| unavailable())?
            .take(HANDY_STROKE_SCRIPT_MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() as u64 != subject.script.byte_len || digest(&bytes) != subject.script.sha256
        {
            return Err(changed());
        }
    }
    Ok(*script)
}
fn subject_current(
    db: &Connection,
    subject: &HandyBrowserStartSubject,
    max_snapshot_bytes: u64,
    verify_script_bytes: bool,
) -> PResult<HandyStrokeScript> {
    subject.validate()?;
    let device = hardware(db, &subject.device_qualification)?;
    if subject.context != device.manifest.subject.context
        || subject.timing.maximum_alignment_error_ns
            > device.manifest.subject.profile.maximum_clock_uncertainty_ns
    {
        return Err(changed());
    }
    let (revision, program): (u64, String) = db.query_row(
        "SELECT revision,CASE WHEN length(program)<=71 THEN program ELSE NULL END FROM projects WHERE id=?1",
        [subject.project_id.as_str()], |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(internal)?;
    if revision != subject.revision.value()
        || program != format!("motion:{}", subject.master_sha256)
        || source_placements::active(
            db,
            &subject.project_id,
            &subject.selection.placement.placement_id,
        )? != subject.selection.placement
    {
        return Err(changed());
    }
    let descriptor: String = db.query_row(
        "SELECT CASE WHEN length(CAST(descriptor AS BLOB))<=131072 THEN descriptor ELSE NULL END FROM motion_objects WHERE sha256=?1",
        [&subject.master_sha256], |row| row.get(0),
    ).map_err(internal)?;
    let descriptor: crate::motion_artifacts::StoredProgram = decode(&descriptor)?;
    descriptor.validate()?;
    if descriptor.sha256 != subject.master_sha256 {
        return Err(changed());
    }
    source_kinds::require_media(db, &subject.project_id, &subject.selection.source_version)?;
    let (hash, bytes, evicted): (String, u64, bool) = db.query_row(
        "SELECT CASE WHEN length(sha256)=64 THEN sha256 ELSE NULL END,bytes,evicted FROM sources WHERE project=?1 AND version=?2",
        params![subject.project_id.as_str(), subject.selection.source_version.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).map_err(internal)?;
    if evicted
        || hash != subject.selection.source.sha256
        || hash != subject.selection.source.artifact_id.as_str()
        || bytes != subject.selection.source.byte_len
        || bytes > max_snapshot_bytes
    {
        return Err(changed());
    }
    script(db, subject, verify_script_bytes)
}
/// Shared with the independent play owner. Call while holding the project DB,
/// before native timing/controller locks. It never creates or revives a pin.
pub(super) fn revalidate_accepted_browser_start(
    db: &Connection,
    pool: &ResourcePool,
    pin: &AcceptedBrowserStart,
) -> PResult<()> {
    let checked = (|| {
        if !pin.current() {
            return Err(unavailable());
        }
        let _memory = pool.reserve_memory(32 * 1024 * 1024)?;
        let value = report(db, &pin.data.binding.dossier_id)?;
        if value.revision != pin.data.binding.revision
            || value.state != HandyQualificationState::Accepted
            || value.manifest.subject != pin.data.subject
        {
            return Err(changed());
        }
        verify_evidence(db, &value)?;
        let script = subject_current(db, &pin.data.subject, pin.data.max_snapshot_bytes, true)?;
        if encode(&script)? != encode(&pin.data.script)? || !pin.current() {
            return Err(changed());
        }
        Ok(())
    })();
    if checked.is_err() {
        pin.data.live.store(false, Ordering::Release);
    }
    checked
}
impl Engine {
    fn browser_start_subject_current(
        &self,
        db: &Connection,
        subject: &HandyBrowserStartSubject,
        verify_script_bytes: bool,
    ) -> PResult<HandyStrokeScript> {
        if self.is_shutting_down() {
            return Err(unavailable());
        }
        subject_current(
            db,
            subject,
            self.config.max_snapshot_bytes,
            verify_script_bytes,
        )
    }

    /// Call under the project DB guard, before timing/Handy/runtime locks. This
    /// native eligibility pin is only one input to final play authority; callers
    /// must also hold the exact live timing/hosted/controller/clock capabilities.
    pub(super) fn capture_handy_browser_start_locked(
        &self,
        db: &Connection,
        request: &Request,
        binding: &HandyBrowserStartQualificationBinding,
        media: &BrowserMediaBinding,
        hosting: &HandyHostingReport,
        device_qualification: &HandyQualificationBinding,
    ) -> PResult<AcceptedBrowserStart> {
        request.validate()?;
        binding.validate()?;
        let actor = request
            .session
            .as_ref()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing is required"))?;
        self.authenticate(db, actor, request.auth_token.as_deref())?;
        require_grant(db, actor, project(request)?, Scope::Read)?;
        require_grant(db, actor, project(request)?, Scope::Export)?;
        let _memory = self.pool.reserve_memory(32 * 1024 * 1024)?;
        let report = report(db, &binding.dossier_id)?;
        if report.revision != binding.revision
            || report.state != HandyQualificationState::Accepted
            || report.manifest.subject.origin != HandyEvidenceOrigin::Physical
        {
            return Err(unavailable());
        }
        verify_evidence(db, &report)?;
        let subject = report.manifest.subject;
        hosting.validate()?;
        if request.project.as_ref() != Some(&subject.project_id)
            || request.expected_revision != Some(subject.revision)
            || media.project_id != subject.project_id
            || media.revision != subject.revision
            || media.selection != subject.selection
            || media.clock != subject.clock
            || media.engine_epoch.as_str() != self.engine_epoch
            || subject.device_qualification != *device_qualification
            || subject.script_actor_session != *actor
            || hosting.project_id != subject.project_id
            || hosting.revision != subject.revision
            || hosting.master_sha256 != subject.master_sha256
            || hosting.script != subject.script
            || hosting.preparation_request_id != subject.script_request_id
            || hosting.engine_epoch.as_str() != self.engine_epoch
            || hosting.retired
            || hosting.outcome != HandyHostingOutcome::Verified
        {
            return Err(changed());
        }
        let prepared = self.browser_start_subject_current(db, &subject, true)?;
        let (current_hosting, native_hosted) = self.capture_handy_hosted(
            db,
            actor,
            &subject.project_id,
            subject.revision,
            &hosting.request_id,
        )?;
        if current_hosting.preparation_request_id != subject.script_request_id
            || !native_hosted.matches(hosting)
        {
            return Err(changed());
        }
        let source = self.pin_browser_media_locked(db, request, media)?;
        let mut registry = self
            .handy_browser_start_qualifications
            .lock()
            .map_err(|poisoned| {
                poisoned.into_inner().cancel_all();
                unavailable()
            })?;
        if registry.fence_capacity_exhausted {
            return Err(unavailable());
        }
        if registry
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
            pin.live.load(Ordering::Acquire) && pin.binding == *binding && pin.subject == subject
        }) {
            if encode(&existing.script)? != encode(&prepared)? {
                return Err(changed());
            }
            existing
        } else {
            registry.invalidate(&binding.dossier_id);
            registry.pins.retain(|_, pin| pin.strong_count() > 0);
            if registry.pins.len() >= MAX_DOSSIERS as usize {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "browser-start native pin capacity exceeded",
                ));
            }
            let data = Arc::new(PinData {
                binding: binding.clone(),
                subject,
                script: prepared,
                max_snapshot_bytes: self.config.max_snapshot_bytes,
                live: AtomicBool::new(true),
                _memory: self.pool.reserve_memory(PIN_MEMORY)?,
            });
            registry
                .pins
                .insert(binding.dossier_id.clone(), Arc::downgrade(&data));
            data
        };
        Ok(AcceptedBrowserStart {
            data,
            source,
            shutdown: self.shutdown_requested.clone(),
        })
    }

    /// Public project/device/qualification mutations call this while retaining
    /// DB ownership. No source/script file or network read is performed here.
    pub(super) fn retire_changed_handy_browser_start_qualifications(&self, db: &Connection) {
        let pins: Vec<_> = self
            .handy_browser_start_qualifications
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
                if value.revision != pin.binding.revision
                    || value.state != HandyQualificationState::Accepted
                    || value.manifest.subject != pin.subject
                {
                    return Err(changed());
                }
                verify_evidence(db, &value)?;
                self.browser_start_subject_current(db, &pin.subject, false)
                    .map(|_| ())
            });
            if eligible.is_err() {
                pin.live.store(false, Ordering::Release);
            }
        }
    }

    pub(super) fn handy_browser_start_qualification_control(
        &self,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        let proof = match &request.command {
            Command::ImportHandyBrowserStartQualification { owner_proof, .. }
            | Command::HandyBrowserStartQualificationStatus { owner_proof, .. }
            | Command::AcceptHandyBrowserStartQualification { owner_proof, .. }
            | Command::WithdrawHandyBrowserStartQualification { owner_proof, .. } => owner_proof,
            _ => {
                return Err(ProtocolError::unsupported(
                    "browser-start qualification operation",
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
        if let Command::HandyBrowserStartQualificationStatus { dossier_id, .. } = &request.command {
            let value = report(&db, dossier_id)?;
            if request.project.as_ref() != Some(&value.manifest.subject.project_id) {
                return Err(changed());
            }
            let _memory = self.pool.reserve_memory(8 * 1024 * 1024)?;
            verify_evidence(&db, &value)?;
            return Ok(ResponseBody::HandyBrowserStartQualification(Box::new(
                value,
            )));
        }
        if let Some(body) = replay(&db, actor.as_str(), request, fingerprint)? {
            let ResponseBody::HandyBrowserStartQualificationSaved(receipt) = &body else {
                return Err(invalid("browser-start replay type differs"));
            };
            receipt.validate()?;
            if receipt.request_id != request.request_id
                || request.project.as_ref() != Some(&receipt.report.manifest.subject.project_id)
            {
                return Err(invalid("browser-start replay identity differs"));
            }
            let matched = match &request.command {
                Command::ImportHandyBrowserStartQualification {
                    manifest_sha256, ..
                } => receipt.input_sha256.as_ref() == Some(manifest_sha256),
                Command::AcceptHandyBrowserStartQualification {
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
                Command::WithdrawHandyBrowserStartQualification {
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
                return Err(invalid(
                    "browser-start replay does not match original intent",
                ));
            }
            return Ok(body);
        }
        if let Command::ImportHandyBrowserStartQualification {
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
                .take(HANDY_BROWSER_START_MANIFEST_LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(internal)?;
            if bytes.len() as u64 > HANDY_BROWSER_START_MANIFEST_LIMIT
                || digest(&bytes) != *manifest_sha256
            {
                return Err(invalid(
                    "browser-start manifest input changed or exceeds bound",
                ));
            }
            let input: HandyBrowserStartQualificationInput = serde_json::from_slice(&bytes)
                .map_err(|_| invalid("invalid browser-start input document"))?;
            input.validate()?;
            if request.project.as_ref() != Some(&input.subject.project_id)
                || request.expected_revision != Some(input.subject.revision)
            {
                return Err(changed());
            }
            self.browser_start_subject_current(&db, &input.subject, true)?;
            let mut retained = Vec::new();
            let mut payloads = Vec::new();
            let mut size = 0u64;
            for item in input.evidence {
                let mut bytes = Vec::new();
                artifacts::open_regular(&item.path)
                    .map_err(internal)?
                    .take(HANDY_BROWSER_START_EVIDENCE_LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(internal)?;
                if bytes.len() as u64 != item.byte_len || digest(&bytes) != item.sha256 {
                    return Err(invalid("browser-start evidence input changed"));
                }
                size = size
                    .checked_add(item.byte_len)
                    .ok_or_else(|| invalid("browser-start evidence size overflow"))?;
                retained.push(HandyBrowserStartQualificationEvidence {
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
            let manifest = HandyBrowserStartQualificationManifest {
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
                    "SELECT EXISTS(SELECT 1 FROM handy_browser_start_qualifications WHERE id=?1)",
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
                        "browser-start evidence store is full",
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
                        "insufficient browser-start evidence storage",
                    ));
                }
                let value = HandyBrowserStartQualificationReport {
                    dossier_id: id.clone(),
                    revision: 1,
                    state: HandyQualificationState::Pending,
                    manifest,
                    decision: None,
                };
                value.validate()?;
                tx.execute(
                    "INSERT INTO handy_browser_start_qualifications VALUES(?1,1,?2)",
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
                    tx.execute("INSERT INTO handy_browser_start_qualification_evidence VALUES(?1,?2,?3,?4)", params![id.as_str(), encode(&kind)?, item.identity.sha256, bytes]).map_err(internal)?;
                }
                value
            };
            let body = ResponseBody::HandyBrowserStartQualificationSaved(Box::new(
                HandyBrowserStartQualificationReceipt {
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
            Command::AcceptHandyBrowserStartQualification {
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
            Command::WithdrawHandyBrowserStartQualification {
                dossier_id,
                expected_record_revision,
                reason,
                ..
            } => (dossier_id, *expected_record_revision, None, reason.clone()),
            _ => {
                return Err(ProtocolError::unsupported(
                    "browser-start qualification operation",
                ))
            }
        };
        if operator.is_none() {
            let mut registry =
                self.handy_browser_start_qualifications
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
                "browser-start review revision changed",
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
            self.browser_start_subject_current(&db, &value.manifest.subject, true)?;
        } else {
            // Evidence corruption must not prevent withdrawal. Invalidate native
            // users before attempting the durable update, including failed writes.
            self.handy_browser_start_qualifications
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
            .ok_or_else(|| invalid("browser-start review revision overflow"))?;
        value.state = if accepted {
            HandyQualificationState::Accepted
        } else {
            HandyQualificationState::Withdrawn
        };
        value.decision = Some(HandyBrowserStartQualificationDecision {
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
        let rows = tx.execute("UPDATE handy_browser_start_qualifications SET revision=?1,report=?2 WHERE id=?3 AND revision=?4", params![value.revision, encode(&value)?, id.as_str(), expected]).map_err(internal)?;
        if rows != 1 {
            return Err(invalid("browser-start review compare-and-swap changed"));
        }
        let body = ResponseBody::HandyBrowserStartQualificationSaved(Box::new(
            HandyBrowserStartQualificationReceipt {
                input_sha256: None,
                request_id: request.request_id.clone(),
                report: value,
            },
        ));
        remember(&tx, actor.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        self.handy_browser_start_qualifications
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
