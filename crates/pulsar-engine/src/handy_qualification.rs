//! Host-local evidence and explicit operator review. No actuation or automatic acceptance.
use super::*;
use std::{
    io::Read,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_DOSSIERS: u64 = 32;
const MAX_EVIDENCE_BYTES: u64 = 256 * 1024 * 1024;
const CREATE_SCHEMA: &str = "CREATE TABLE handy_qualification_schema(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL CHECK(version=1))";
const CREATE_RECORDS: &str = "CREATE TABLE handy_qualifications(id TEXT PRIMARY KEY NOT NULL,revision INTEGER NOT NULL,report TEXT NOT NULL)";
const CREATE_EVIDENCE: &str = "CREATE TABLE handy_qualification_evidence(dossier TEXT NOT NULL REFERENCES handy_qualifications(id),kind TEXT NOT NULL,sha256 TEXT NOT NULL,data BLOB NOT NULL,PRIMARY KEY(dossier,kind))";
fn invalid(s: &str) -> ProtocolError {
    ProtocolError::invalid(s)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn manifest_id(manifest: &HandyQualificationManifest) -> PResult<ArtifactId> {
    manifest.identity()
}

#[cfg(target_os = "linux")]
pub(super) fn control_image() -> PResult<ArtifactIdentity> {
    static IMAGE: OnceLock<std::result::Result<ArtifactIdentity, String>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            let result = (|| -> Result<ArtifactIdentity> {
                // Kernel-owned magic link resolves the running image even after pathname replacement.
                let mut file = fs::File::open("/proc/self/exe")?;
                let before = file.metadata()?;
                anyhow::ensure!(
                    before.is_file() && before.len() > 0 && before.len() <= 512 * 1024 * 1024,
                    "unsupported loaded control image size"
                );
                let mut hash = Sha256::new();
                let mut bytes = 0u64;
                let mut chunk = [0u8; 64 * 1024];
                loop {
                    let n = file.read(&mut chunk)?;
                    if n == 0 {
                        break;
                    }
                    bytes += n as u64;
                    anyhow::ensure!(bytes <= 512 * 1024 * 1024, "control image exceeds bound");
                    hash.update(&chunk[..n]);
                }
                anyhow::ensure!(
                    bytes == before.len() && file.metadata()?.len() == bytes,
                    "control image changed"
                );
                let sha256 = format!("{:x}", hash.finalize());
                Ok(ArtifactIdentity {
                    artifact_id: ArtifactId::new(sha256.clone())?,
                    sha256,
                    byte_len: bytes,
                })
            })();
            result.map_err(|_| "loaded control image identity unavailable".to_string())
        })
        .clone()
        .map_err(|m| ProtocolError::new(ErrorCode::Unavailable, m))
}
#[cfg(not(target_os = "linux"))]
pub(super) fn control_image() -> PResult<ArtifactIdentity> {
    Err(ProtocolError::unsupported(
        "qualification needs a verified loaded control image on this platform",
    ))
}
fn context() -> PResult<HandyQualificationContext> {
    context_for_transport(HandyQualificationTransport::Hssp)
}
pub(super) fn context_for_transport(
    transport: HandyQualificationTransport,
) -> PResult<HandyQualificationContext> {
    #[cfg(target_os = "linux")]
    let platform = {
        let mut raw = String::new();
        fs::File::open("/proc/sys/kernel/osrelease")
            .map_err(internal)?
            .take(257)
            .read_to_string(&mut raw)
            .map_err(internal)?;
        if raw.len() > 256 {
            return Err(invalid("kernel release exceeds bound"));
        }
        format!("linux/{}/{}", std::env::consts::ARCH, raw.trim())
    };
    #[cfg(not(target_os = "linux"))]
    let platform = std::env::consts::OS.to_string();
    let result = HandyQualificationContext {
        schema_version: 1,
        control_image: control_image()?,
        platform,
        transport: ArtifactId::new(transport.transport_id()).map_err(internal)?,
    };
    result.validate()?;
    Ok(result)
}
fn layout(db: &Connection) -> Result<bool> {
    let mut found = 0;
    for (table, sql) in [
        ("handy_qualification_schema", CREATE_SCHEMA),
        ("handy_qualifications", CREATE_RECORDS),
        ("handy_qualification_evidence", CREATE_EVIDENCE),
    ] {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [table],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((kind, actual)) = actual {
            anyhow::ensure!(
                kind == "table" && actual == sql,
                "qualification layout differs"
            );
            found += 1;
        }
    }
    anyhow::ensure!(found == 0 || found == 3, "incomplete qualification layout");
    if found == 0 {
        return Ok(false);
    }
    let versions: Vec<(i64, i64)> = {
        let mut q =
            db.prepare("SELECT singleton,version FROM handy_qualification_schema LIMIT 2")?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    anyhow::ensure!(
        versions == [(1, 1)],
        "qualification metadata requires explicit migration"
    );
    let triggers:u64=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('handy_qualification_schema','handy_qualifications','handy_qualification_evidence')",[],|r|r.get(0))?;
    anyhow::ensure!(triggers == 0, "unexpected qualification metadata trigger");
    Ok(true)
}
fn capacity(db: &Connection) -> PResult<(u64, u64)> {
    let (count,bytes):(u64,u64)=db.query_row("SELECT (SELECT count(*) FROM handy_qualifications),COALESCE(sum(length(data)),0) FROM handy_qualification_evidence",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
    if count > MAX_DOSSIERS || bytes > MAX_EVIDENCE_BYTES {
        return Err(invalid("qualification evidence capacity exceeded"));
    }
    Ok((count, bytes))
}
fn initialize(tx: &Transaction<'_>) -> PResult<()> {
    if !layout(tx).map_err(internal)? {
        for sql in [CREATE_SCHEMA, CREATE_RECORDS, CREATE_EVIDENCE] {
            tx.execute_batch(sql).map_err(internal)?;
        }
        tx.execute("INSERT INTO handy_qualification_schema VALUES(1,1)", [])
            .map_err(internal)?;
    }
    Ok(())
}
fn report(db: &Connection, id: &ArtifactId) -> PResult<HandyQualificationReport> {
    if !layout(db).map_err(internal)? {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "qualification dossier not found",
        ));
    }
    capacity(db)?;
    let row:Option<(u64,String)>=db.query_row("SELECT revision,CASE WHEN length(CAST(report AS BLOB))<=131072 THEN report ELSE NULL END FROM handy_qualifications WHERE id=?1",[id.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
    let (revision, raw) = row.ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, "qualification dossier not found")
    })?;
    let value: HandyQualificationReport = decode(&raw)?;
    value.validate()?;
    if value.dossier_id != *id || value.revision != revision || manifest_id(&value.manifest)? != *id
    {
        return Err(invalid("qualification identity or revision mismatch"));
    }
    if let Some(decision) = &value.decision {
        let raw: String = db
            .query_row(
                "SELECT response FROM requests WHERE session=?1 AND request=?2",
                params![
                    decision.actor_session.as_str(),
                    decision.request_id.as_str()
                ],
                |r| r.get(0),
            )
            .map_err(internal)?;
        let ResponseBody::HandyQualificationSaved(receipt) = decode(&raw)? else {
            return Err(invalid("qualification decision receipt missing"));
        };
        receipt.validate()?;
        if receipt.request_id != decision.request_id
            || receipt.input_sha256.is_some()
            || encode(&receipt.report)? != encode(&value)?
        {
            return Err(invalid("qualification decision and receipt disagree"));
        }
    }
    Ok(value)
}
fn verify_evidence(db: &Connection, value: &HandyQualificationReport) -> PResult<()> {
    let count: u64 = db
        .query_row(
            "SELECT count(*) FROM handy_qualification_evidence WHERE dossier=?1",
            [value.dossier_id.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if count != value.manifest.subject.required_evidence_kinds().len() as u64 {
        return Err(invalid("qualification evidence membership differs"));
    }
    for item in &value.manifest.evidence {
        let (hash,bytes):(String,Vec<u8>)=db.query_row("SELECT sha256,CASE WHEN length(data) BETWEEN 1 AND 4194304 THEN data ELSE NULL END FROM handy_qualification_evidence WHERE dossier=?1 AND kind=?2",params![value.dossier_id.as_str(),encode(&item.kind)?],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
        if hash != item.identity.sha256
            || bytes.len() as u64 != item.identity.byte_len
            || digest(&bytes) != hash
        {
            return Err(invalid("retained qualification evidence changed"));
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
        let mut q=db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM handy_qualifications LIMIT 33")?;
        let rows = q
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    for id in ids {
        let r = report(db, &ArtifactId::new(id)?)?;
        let count: u64 = db.query_row(
            "SELECT count(*) FROM handy_qualification_evidence WHERE dossier=?1",
            [r.dossier_id.as_str()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(
            count == r.manifest.subject.required_evidence_kinds().len() as u64,
            "qualification evidence membership differs"
        );
    }
    let bad:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM handy_qualification_evidence e LEFT JOIN handy_qualifications q ON q.id=e.dossier WHERE q.id IS NULL OR typeof(data)!='blob' OR length(data) NOT BETWEEN 1 AND 4194304 OR length(sha256)!=64)",[],|r|r.get(0))?;
    anyhow::ensure!(!bad, "invalid qualification evidence storage");
    Ok(())
}
pub(super) fn is_command(c: &Command) -> bool {
    matches!(
        c,
        Command::HandyQualificationContext { .. }
            | Command::ImportHandyQualification { .. }
            | Command::HandyQualificationStatus { .. }
            | Command::AcceptHandyQualification { .. }
            | Command::WithdrawHandyQualification { .. }
    )
}
fn check_binding(
    db: &Connection,
    subject: &HandyQualificationSubject,
) -> PResult<HandyRegistration> {
    if subject.context != context_for_transport(subject.transport()?)? {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "qualification control image, platform or transport differs",
        ));
    }
    let registration =
        handy_registrations::current(db, &subject.device_id, subject.registration_revision)?;
    if registration.disabled || registration.binding_sha256 != subject.registration_binding_sha256 {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "qualification registration binding differs",
        ));
    }
    Ok(registration)
}
fn native_timing(
    value: &HandyQualificationReport,
) -> PResult<crate::playback_runtime::QualifiedCommandTiming> {
    if value.state != HandyQualificationState::Accepted
        || value.manifest.subject.origin != HandyEvidenceOrigin::Physical
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "physical evidence has no current acceptance",
        ));
    }
    let s = &value.manifest.subject;
    if !s.transport()?.is_hssp() || s.hsp.is_some() || !matches!(s.schema_version, 1..=6) {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "HSP qualification cannot construct HSSP command timing",
        ));
    }
    let configuration = DeviceConfiguration {
        device: s.device_id.clone(),
        firmware: ArtifactId::new(digest(&serde_json::to_vec(&s.identity).map_err(internal)?))
            .map_err(internal)?,
        driver: s.context.control_image.artifact_id.clone(),
        transport: s.context.transport.clone(),
        profile: ArtifactId::new(digest(
            &if s.continuous_timing.is_some() {
                serde_json::to_vec(&(
                    &s.profile,
                    &s.operative_settings,
                    &s.server_clock,
                    &s.device_clock,
                    &s.recovery,
                    &s.continuous_timing,
                ))
            } else if s.recovery.is_some() {
                serde_json::to_vec(&(
                    &s.profile,
                    &s.operative_settings,
                    &s.server_clock,
                    &s.device_clock,
                    &s.recovery,
                ))
            } else if s.device_clock.is_some() {
                serde_json::to_vec(&(
                    &s.profile,
                    &s.operative_settings,
                    &s.server_clock,
                    &s.device_clock,
                ))
            } else if s.server_clock.is_some() {
                serde_json::to_vec(&(&s.profile, &s.operative_settings, &s.server_clock))
            } else {
                serde_json::to_vec(&(&s.profile, &s.operative_settings))
            }
            .map_err(internal)?,
        ))
        .map_err(internal)?,
    };
    let qualified = DeviceQualification::new(
        configuration,
        value.dossier_id.clone(),
        QualificationAuthority::LocalHumanAcceptance,
        s.profile.stopping_bound_ns,
    )
    .map_err(|e| invalid(&e.to_string()))?;
    crate::playback_runtime::QualifiedCommandTiming::new(
        qualified,
        s.profile.maximum_command_age_ns,
        s.profile.maximum_clock_uncertainty_ns,
        s.profile
            .drift_ppm
            .checked_add(s.device_clock.as_ref().map_or(0, |c| c.drift_ppm))
            .ok_or_else(|| invalid("combined drift overflow"))?,
    )
    .map_err(|_| invalid("accepted timing declarations are invalid"))
}
pub(super) fn accepted_record(
    db: &Connection,
    binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
    identity: &HandyIdentity,
    hosted: &crate::handy_hosting_wire::VerifiedHosted,
) -> PResult<HandyQualificationReport> {
    binding.validate()?;
    let value = report(db, &binding.dossier_id)?;
    if value.revision != binding.revision {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "qualification decision changed",
        ));
    }
    if value.state != HandyQualificationState::Accepted {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "qualification has no current acceptance",
        ));
    }
    if !value.manifest.subject.transport()?.is_hssp()
        || value.manifest.subject.hsp.is_some()
        || value.manifest.subject.origin != HandyEvidenceOrigin::Physical
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "hosted HSSP playback requires HSSP physical qualification",
        ));
    }
    verify_evidence(db, &value)?;
    if check_binding(db, &value.manifest.subject)? != *registration
        || value.manifest.subject.identity != *identity
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "accepted qualification does not match current device",
        ));
    }
    if value.manifest.subject.operative_settings.is_none() {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "qualification lacks explicitly reviewed operative settings",
        ));
    }
    hosted.check_profile(&value.manifest.subject.profile)?;
    Ok(value)
}

pub(super) fn accepted_timing(
    db: &Connection,
    binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
    observation: &HandySetupDeviceObservation,
    hosted: &crate::handy_hosting_wire::VerifiedHosted,
) -> PResult<crate::playback_runtime::QualifiedCommandTiming> {
    observation.validate()?;
    let value = accepted_record(db, binding, registration, &observation.identity, hosted)?;
    if value.manifest.subject.operative_settings.as_ref() != Some(&observation.operative_settings) {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "observed Handy settings differ from accepted qualification",
        ));
    }
    native_timing(&value)
}

/// Current recovery eligibility, without a loaded-program prerequisite. This
/// still supplies no physical stop proof: the caller must drain native dispatch,
/// collect fresh samples, and recheck this record and its exact subject before
/// committing a resolution. Historical records never bypass these checks.
pub(super) fn accepted_recovery_record(
    db: &Connection,
    binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
) -> PResult<HandyQualificationReport> {
    binding.validate()?;
    let value = report(db, &binding.dossier_id)?;
    if value.revision != binding.revision {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "qualification decision changed",
        ));
    }
    let subject = &value.manifest.subject;
    if value.state != HandyQualificationState::Accepted
        || subject.origin != HandyEvidenceOrigin::Physical
        || !matches!(subject.schema_version, 5 | 6)
        || !subject.transport()?.is_hssp()
        || subject.recovery.is_none()
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "reconciliation requires current physical acceptance of an explicit recovery contract",
        ));
    }
    verify_evidence(db, &value)?;
    if check_binding(db, subject)? != *registration {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "recovery qualification does not match current registration",
        ));
    }
    Ok(value)
}

/// Select the explicit recovery transport without widening the hosted-only
/// helper. A returned report still requires native drain and fresh evidence.
pub(super) fn accepted_recovery_record_for_selected_transport(
    db: &Connection,
    binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
) -> PResult<HandyQualificationReport> {
    binding.validate()?;
    match report(db, &binding.dossier_id)?
        .manifest
        .subject
        .transport()?
    {
        HandyQualificationTransport::Hssp => accepted_recovery_record(db, binding, registration),
        HandyQualificationTransport::Hsp => accepted_hsp_record(db, binding, registration),
    }
}

/// Current HSP recovery eligibility. This checks a reviewed physical v7 record
/// against current retained evidence, loaded control image and registration. It
/// never constructs HSSP timing, restores a stream owner, or proves stopping.
pub(super) fn accepted_hsp_record(
    db: &Connection,
    binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
) -> PResult<HandyQualificationReport> {
    binding.validate()?;
    let value = report(db, &binding.dossier_id)?;
    #[cfg(test)]
    let authored = authored_hsp_device_permit_current(db, &value, binding)?;
    let revision_matches = value.revision == binding.revision;
    #[cfg(test)]
    let revision_matches = revision_matches || authored;
    if !revision_matches {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "qualification decision changed",
        ));
    }
    let subject = &value.manifest.subject;
    let accepted = value.state == HandyQualificationState::Accepted
        && subject.origin == HandyEvidenceOrigin::Physical;
    #[cfg(test)]
    let accepted = accepted || authored;
    if !accepted
        || subject.schema_version != 7
        || subject.transport()? != HandyQualificationTransport::Hsp
        || subject.hsp.is_none()
        || subject
            .recovery
            .as_ref()
            .is_none_or(|r| r.transport != HandyQualificationTransport::Hsp)
    {
        return Err(ProtocolError::new(ErrorCode::Forbidden,
            "HSP requires current physical acceptance of an explicit v7 streaming and recovery contract"));
    }
    verify_evidence(db, &value)?;
    if check_binding(db, subject)? != *registration {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "HSP qualification does not match current registration",
        ));
    }
    #[cfg(test)]
    if !(value.state == HandyQualificationState::Accepted
        && subject.origin == HandyEvidenceOrigin::Physical)
        && !authored_hsp_device_permit_current(db, &value, binding)?
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "authored HSP device fixture scope expired",
        ));
    }
    Ok(value)
}

#[cfg(test)]
fn authored_hsp_device_permit_current(
    db: &Connection,
    value: &HandyQualificationReport,
    binding: &HandyQualificationBinding,
) -> PResult<bool> {
    if value.dossier_id != binding.dossier_id
        || value.revision != 1
        || binding.revision != 2
        || value.state != HandyQualificationState::Pending
        || value.manifest.subject.origin != HandyEvidenceOrigin::Synthetic
    {
        return Ok(false);
    }
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE name='pulsar_test_hsp_device_acceptance')",
        [], |r| r.get(0),
    ).map_err(internal)?;
    if !exists {
        return Ok(false);
    }
    let now = crate::handy_clock::boottime_ns()?;
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.pulsar_test_hsp_device_acceptance WHERE dossier=?1 AND revision=?2 AND binding_revision=?3 AND subject_sha256=?4 AND expires_at_ns>?5)",
        params![binding.dossier_id.as_str(), value.revision, binding.revision, value.manifest.subject.sha256()?, now],
        |r| r.get(0),
    ).map_err(internal)
}

/// Explicit authored fixture scope only. No physical origin, decision, durable
/// acceptance, production lookup or restart-restored authority is created.
/// The virtual test binding is revision 2; the retained Pending report stays 1.
#[cfg(test)]
pub(super) fn install_authored_hsp_device_acceptance_for_test(
    db: &Connection,
    id: &ArtifactId,
    expires_at_boottime_ns: u64,
) -> PResult<HandyQualificationBinding> {
    let value = report(db, id)?;
    let subject = &value.manifest.subject;
    if value.revision != 1
        || value.state != HandyQualificationState::Pending
        || subject.origin != HandyEvidenceOrigin::Synthetic
        || subject.schema_version != 7
        || subject.transport()? != HandyQualificationTransport::Hsp
        || expires_at_boottime_ns <= crate::handy_clock::boottime_ns()?
        || expires_at_boottime_ns > i64::MAX as u64
    {
        return Err(invalid("invalid authored HSP device fixture scope"));
    }
    verify_evidence(db, &value)?;
    check_binding(db, subject)?;
    db.execute_batch("CREATE TEMP TABLE IF NOT EXISTS pulsar_test_hsp_device_acceptance(dossier TEXT PRIMARY KEY,revision INTEGER NOT NULL,binding_revision INTEGER NOT NULL,subject_sha256 TEXT NOT NULL,expires_at_ns INTEGER NOT NULL)").map_err(internal)?;
    let count: u64 = db
        .query_row(
            "SELECT count(*) FROM temp.pulsar_test_hsp_device_acceptance",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if count >= MAX_DOSSIERS {
        return Err(invalid("authored HSP device fixture capacity exceeded"));
    }
    db.execute(
        "INSERT INTO temp.pulsar_test_hsp_device_acceptance VALUES(?1,?2,2,?3,?4)",
        params![
            id.as_str(),
            value.revision,
            subject.sha256()?,
            expires_at_boottime_ns
        ],
    )
    .map_err(internal)?;
    Ok(HandyQualificationBinding {
        dossier_id: id.clone(),
        revision: 2,
    })
}

/// Distinct HSP evidence bundle. This is not a live stream authority and cannot
/// be passed to the hosted HSSP dispatcher as QualifiedCommandTiming.
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct QualifiedHspBundle {
    pub report: HandyQualificationReport,
    pub policy: HspPolicy,
    pub subject_sha256: String,
    pub profile_sha256: String,
}

/// Exact current device decision resolved for one selected finite policy. This
/// carries no source lifetime, session ownership or permission to issue Stop.
#[derive(Clone, Debug)]
pub(super) struct QualifiedHspFiniteDrainPolicy {
    pub policy: pulsar_core::HspFiniteDrainPolicy,
    pub policy_sha256: String,
    pub maximum_stop_dispatch_delay_ns: u64,
    pub stopping_bound_ns: u64,
    _accepted: (),
}

pub(super) fn accepted_hsp_finite_drain_policy(
    db: &Connection,
    binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
    selected_policy_sha256: &str,
) -> PResult<QualifiedHspFiniteDrainPolicy> {
    validate_sha256(selected_policy_sha256)?;
    let report = accepted_hsp_record(db, binding, registration)?;
    let subject = &report.manifest.subject;
    let policy_sha256 = subject.hsp_finite_drain_policy_sha256()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::Forbidden,
            "accepted HSP dossier has no finite drain policy",
        )
    })?;
    if selected_policy_sha256 != policy_sha256 {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "selected HSP finite drain policy differs from current device acceptance",
        ));
    }
    let finite = subject
        .hsp
        .as_ref()
        .and_then(|hsp| hsp.finite_drain.as_ref())
        .ok_or_else(|| invalid("HSP finite drain declaration missing"))?;
    let stopping_bound_ns = subject
        .profile
        .stopping_bound_ns
        .ok_or_else(|| invalid("HSP stopping bound missing"))?;
    let stop_budget_ns = finite.stop_budget_ns(Some(stopping_bound_ns))?;
    Ok(QualifiedHspFiniteDrainPolicy {
        policy: pulsar_core::HspFiniteDrainPolicy {
            maximum_duration_ns: finite.maximum_drain_duration_ns,
            maximum_terminal_overrun_ns: finite.maximum_terminal_overrun_ns,
            stop_budget_ns,
        },
        policy_sha256,
        maximum_stop_dispatch_delay_ns: finite.maximum_stop_dispatch_delay_ns,
        stopping_bound_ns,
        _accepted: (),
    })
}
/// The caller must supply its just-acquired native observation and independently
/// bound its age. A deserialized observation or receipt timestamp cannot prove
/// freshness. Current decision and exact settings are checked on every call.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn accepted_hsp_bundle(
    db: &Connection,
    binding: &HandyQualificationBinding,
    registration: &HandyRegistration,
    observation: &HandySetupDeviceObservation,
) -> PResult<QualifiedHspBundle> {
    observation.validate()?;
    let value = accepted_hsp_record(db, binding, registration)?;
    let subject = &value.manifest.subject;
    if observation.mode.mode != 4
        || observation.remote_hsp.is_none()
        || observation.identity != subject.identity
        || subject.operative_settings.as_ref() != Some(&observation.operative_settings)
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "fresh HSP mode, identity or operative settings differ from accepted qualification",
        ));
    }
    let policy = subject
        .hsp
        .as_ref()
        .ok_or_else(|| invalid("HSP qualification profile missing"))?
        .policy
        .to_policy()?;
    let subject_sha256 = subject.sha256()?;
    let profile_sha256 = subject.hsp_profile_sha256()?;
    Ok(QualifiedHspBundle {
        report: value,
        policy,
        subject_sha256,
        profile_sha256,
    })
}

/// Construct only HSP-native clock contracts from the exact already accepted
/// bundle. Hosted HSSP helpers remain transport-specific and unchanged.
pub(super) fn hsp_clock_contracts(
    bundle: &QualifiedHspBundle,
) -> PResult<(
    crate::handy_server_clock::QualifiedServerClock,
    crate::handy_playback_clock::QualifiedDeviceClock,
)> {
    let subject = &bundle.report.manifest.subject;
    if subject.transport()? != HandyQualificationTransport::Hsp || subject.schema_version != 7 {
        return Err(invalid("HSP native clocks require the exact v7 bundle"));
    }
    Ok((
        crate::handy_server_clock::QualifiedServerClock {
            contract: subject
                .server_clock
                .clone()
                .ok_or_else(|| invalid("HSP server clock contract missing"))?,
            max_age_ns: subject.profile.maximum_command_age_ns,
            max_uncertainty_ns: subject.profile.maximum_clock_uncertainty_ns,
            drift_ppm: subject.profile.drift_ppm,
        },
        crate::handy_playback_clock::QualifiedDeviceClock {
            contract: subject
                .device_clock
                .clone()
                .ok_or_else(|| invalid("HSP device clock contract missing"))?,
            maximum_total_error_ns: subject.profile.maximum_clock_uncertainty_ns,
        },
    ))
}

/// Inspect the immutable physical subject of an earlier effect, including a
/// withdrawn dossier. This grants no current acceptance and must not be used to
/// construct timing or dispatch authority. Recovery compares it with its new
/// accepted subject so a rebound credential route cannot stand in for hardware
/// identity or changed operative settings.
pub(super) fn historical_recovery_subject(
    db: &Connection,
    binding: &HandyQualificationBinding,
) -> PResult<HandyQualificationSubject> {
    binding.validate()?;
    let value = report(db, &binding.dossier_id)?;
    if value.revision < binding.revision
        || value.manifest.subject.origin != HandyEvidenceOrigin::Physical
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "original physical qualification subject is unavailable",
        ));
    }
    verify_evidence(db, &value)?;
    Ok(value.manifest.subject)
}

impl Engine {
    pub(super) fn accepted_handy_continuous_timing(
        &self,
        db: &Connection,
        binding: &HandyQualificationBinding,
        registration: &HandyRegistration,
        observation: &HandySetupDeviceObservation,
        hosted: &crate::handy_hosting_wire::VerifiedHosted,
    ) -> PResult<crate::handy_continuous_timing::QualifiedContinuousTiming> {
        self.accepted_handy_timing(db, binding, registration, observation, hosted)?;
        let value =
            self.accepted_handy_record(db, binding, registration, &observation.identity, hosted)?;
        let subject = value.manifest.subject;
        if subject.schema_version != 6 || subject.origin != HandyEvidenceOrigin::Physical {
            return Err(ProtocolError::new(ErrorCode::Forbidden,
                "continuous timing requires current physical acceptance of an explicit observation contract"));
        }
        let contract = subject.continuous_timing.ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Forbidden,
                "continuous timing observation contract is absent",
            )
        })?;
        Ok(crate::handy_continuous_timing::QualifiedContinuousTiming {
            contract,
            maximum_total_error_ns: subject.profile.maximum_clock_uncertainty_ns,
            server: crate::handy_server_clock::QualifiedServerClock {
                contract: subject
                    .server_clock
                    .ok_or_else(|| internal("continuous server clock contract missing"))?,
                max_age_ns: subject.profile.maximum_command_age_ns,
                max_uncertainty_ns: subject.profile.maximum_clock_uncertainty_ns,
                drift_ppm: subject.profile.drift_ppm,
            },
        })
    }
    pub(super) fn accepted_handy_device_clock(
        &self,
        db: &Connection,
        binding: &HandyQualificationBinding,
        registration: &HandyRegistration,
        observation: &HandySetupDeviceObservation,
        hosted: &crate::handy_hosting_wire::VerifiedHosted,
    ) -> PResult<Option<crate::handy_playback_clock::QualifiedDeviceClock>> {
        self.accepted_handy_timing(db, binding, registration, observation, hosted)?;
        let value =
            self.accepted_handy_record(db, binding, registration, &observation.identity, hosted)?;
        Ok(value.manifest.subject.device_clock.map(|contract| {
            crate::handy_playback_clock::QualifiedDeviceClock {
                contract,
                maximum_total_error_ns: value.manifest.subject.profile.maximum_clock_uncertainty_ns,
            }
        }))
    }
    pub(super) fn accepted_handy_server_clock(
        &self,
        db: &Connection,
        binding: &HandyQualificationBinding,
        registration: &HandyRegistration,
        observation: &HandySetupDeviceObservation,
        hosted: &crate::handy_hosting_wire::VerifiedHosted,
    ) -> PResult<crate::handy_server_clock::QualifiedServerClock> {
        self.accepted_handy_timing(db, binding, registration, observation, hosted)?;
        let value =
            self.accepted_handy_record(db, binding, registration, &observation.identity, hosted)?;
        let subject = value.manifest.subject;
        let contract = subject.server_clock.ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Forbidden,
                "qualification lacks explicit server-clock origin and error bounds",
            )
        })?;
        Ok(crate::handy_server_clock::QualifiedServerClock {
            contract,
            max_age_ns: subject.profile.maximum_command_age_ns,
            max_uncertainty_ns: subject.profile.maximum_clock_uncertainty_ns,
            drift_ppm: subject.profile.drift_ppm,
        })
    }
    /// Caller supplies freshly observed identity and holds the DB guard. This
    /// checks the current record and hosted discrete sample envelope. Observed
    /// settings are required separately before constructing native timing facts.
    fn accepted_handy_record(
        &self,
        db: &Connection,
        binding: &HandyQualificationBinding,
        registration: &HandyRegistration,
        identity: &HandyIdentity,
        hosted: &crate::handy_hosting_wire::VerifiedHosted,
    ) -> PResult<HandyQualificationReport> {
        accepted_record(db, binding, registration, identity, hosted)
    }
    pub(super) fn accepted_handy_settings(
        &self,
        db: &Connection,
        binding: &HandyQualificationBinding,
        registration: &HandyRegistration,
        identity: &HandyIdentity,
        hosted: &crate::handy_hosting_wire::VerifiedHosted,
    ) -> PResult<HandyOperativeSettings> {
        let value = self.accepted_handy_record(db, binding, registration, identity, hosted)?;
        Ok(value
            .manifest
            .subject
            .operative_settings
            .expect("checked settings subject"))
    }
    pub(super) fn accepted_handy_timing(
        &self,
        db: &Connection,
        binding: &HandyQualificationBinding,
        registration: &HandyRegistration,
        observation: &HandySetupDeviceObservation,
        hosted: &crate::handy_hosting_wire::VerifiedHosted,
    ) -> PResult<crate::playback_runtime::QualifiedCommandTiming> {
        accepted_timing(db, binding, registration, observation, hosted)
    }
    pub(super) fn handy_qualification_control(
        &self,
        r: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_qualification_with(
            r,
            session,
            fingerprint,
            crate::handy_online::inspect_qualification,
        )
    }
    pub(super) fn handy_qualification_with<F>(
        &self,
        r: &Request,
        session: &SessionId,
        fingerprint: &str,
        inspect: F,
    ) -> PResult<ResponseBody>
    where
        F: FnOnce(
            Arc<crate::handy_credentials::Credentials>,
        ) -> PResult<HandySetupDeviceObservation>,
    {
        r.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, session)?;
        check_request_identity(&db, session.as_str(), r, fingerprint)?;
        if let Command::HandyQualificationContext { transport, .. } = &r.command {
            let value = if transport.is_hssp() {
                context()?
            } else {
                context_for_transport(*transport)?
            };
            return Ok(ResponseBody::HandyQualificationContext(Box::new(value)));
        }
        if let Command::HandyQualificationStatus { dossier_id, .. } = &r.command {
            let value = report(&db, dossier_id)?;
            let _memory = self.pool.reserve_memory(16 * 1024 * 1024)?;
            verify_evidence(&db, &value)?;
            return Ok(ResponseBody::HandyQualification(Box::new(value)));
        }
        if let Some(body) = replay(&db, session.as_str(), r, fingerprint)? {
            let ResponseBody::HandyQualificationSaved(receipt) = &body else {
                return Err(invalid("qualification replay type mismatch"));
            };
            receipt.validate()?;
            if receipt.request_id != r.request_id
                || manifest_id(&receipt.report.manifest)? != receipt.report.dossier_id
            {
                return Err(invalid("qualification replay identity differs"));
            }
            let matched = match &r.command {
                Command::ImportHandyQualification {
                    manifest_sha256, ..
                } => receipt.input_sha256.as_ref() == Some(manifest_sha256),
                Command::AcceptHandyQualification {
                    dossier_id,
                    expected_qualification_revision,
                    note,
                    ..
                } => {
                    receipt.input_sha256.is_none()
                        && receipt.report.dossier_id == *dossier_id
                        && receipt.report.revision == expected_qualification_revision + 1
                        && receipt.report.state == HandyQualificationState::Accepted
                        && receipt.report.decision.as_ref().is_some_and(|d| {
                            d.request_id == r.request_id
                                && d.actor_session == *session
                                && d.note == *note
                        })
                }
                Command::WithdrawHandyQualification {
                    dossier_id,
                    expected_qualification_revision,
                    note,
                    ..
                } => {
                    receipt.input_sha256.is_none()
                        && receipt.report.dossier_id == *dossier_id
                        && receipt.report.revision == expected_qualification_revision + 1
                        && receipt.report.state == HandyQualificationState::Withdrawn
                        && receipt.report.decision.as_ref().is_some_and(|d| {
                            d.request_id == r.request_id
                                && d.actor_session == *session
                                && d.note == *note
                        })
                }
                _ => false,
            };
            if !matched {
                return Err(invalid(
                    "qualification replay does not match captured intent",
                ));
            }
            return Ok(body);
        }
        if let Command::ImportHandyQualification {
            manifest_path,
            manifest_sha256,
            ..
        } = &r.command
        {
            let _memory = self.pool.reserve_memory(128 * 1024 * 1024)?;
            let mut raw = Vec::new();
            artifacts::open_regular(manifest_path)
                .map_err(internal)?
                .take(HANDY_QUALIFICATION_MANIFEST_LIMIT + 1)
                .read_to_end(&mut raw)
                .map_err(internal)?;
            if raw.len() as u64 > HANDY_QUALIFICATION_MANIFEST_LIMIT
                || digest(&raw) != *manifest_sha256
            {
                return Err(invalid("qualification input changed or exceeds bound"));
            }
            let input: HandyQualificationInput = serde_json::from_slice(&raw)
                .map_err(|_| invalid("invalid qualification input document"))?;
            input.validate()?;
            check_binding(&db, &input.subject)?;
            let mut evidence = Vec::new();
            let mut payloads = Vec::new();
            let mut size = 0u64;
            for item in input.evidence {
                let mut bytes = Vec::new();
                artifacts::open_regular(&item.path)
                    .map_err(internal)?
                    .take(HANDY_QUALIFICATION_EVIDENCE_LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(internal)?;
                if bytes.len() as u64 != item.byte_len || digest(&bytes) != item.sha256 {
                    return Err(invalid("qualification evidence input changed"));
                }
                size += item.byte_len;
                evidence.push(HandyQualificationEvidence {
                    kind: item.kind,
                    identity: ArtifactIdentity {
                        artifact_id: ArtifactId::new(item.sha256.clone()).map_err(internal)?,
                        sha256: item.sha256,
                        byte_len: item.byte_len,
                    },
                });
                payloads.push((item.kind, bytes));
            }
            evidence.sort_by_key(|e| e.kind);
            let manifest = HandyQualificationManifest {
                subject: input.subject,
                evidence,
            };
            manifest.validate()?;
            let id = manifest_id(&manifest)?;
            let tx = db.transaction().map_err(internal)?;
            initialize(&tx)?;
            let (count, used) = capacity(&tx)?;
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM handy_qualifications WHERE id=?1)",
                    [id.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            let value = if exists {
                let current = report(&tx, &id)?;
                verify_evidence(&tx, &current)?;
                current
            } else {
                if count >= MAX_DOSSIERS
                    || used
                        .checked_add(size)
                        .is_none_or(|n| n > MAX_EVIDENCE_BYTES)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "qualification store capacity exceeded",
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
                        "insufficient qualification storage",
                    ));
                }
                let value = HandyQualificationReport {
                    dossier_id: id.clone(),
                    revision: 1,
                    state: HandyQualificationState::Pending,
                    manifest,
                    decision: None,
                };
                value.validate()?;
                tx.execute(
                    "INSERT INTO handy_qualifications VALUES(?1,1,?2)",
                    params![id.as_str(), encode(&value)?],
                )
                .map_err(internal)?;
                for (kind, bytes) in payloads {
                    let item = value
                        .manifest
                        .evidence
                        .iter()
                        .find(|e| e.kind == kind)
                        .expect("validated role");
                    tx.execute(
                        "INSERT INTO handy_qualification_evidence VALUES(?1,?2,?3,?4)",
                        params![id.as_str(), encode(&kind)?, item.identity.sha256, bytes],
                    )
                    .map_err(internal)?;
                }
                value
            };
            let body = ResponseBody::HandyQualificationSaved(Box::new(HandyQualificationReceipt {
                request_id: r.request_id.clone(),
                input_sha256: Some(manifest_sha256.clone()),
                report: value,
            }));
            remember(&tx, session.as_str(), r, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            return Ok(body);
        }
        let (id, expected, note, accept) = match &r.command {
            Command::AcceptHandyQualification {
                dossier_id,
                expected_qualification_revision,
                note,
                ..
            } => (dossier_id, *expected_qualification_revision, note, true),
            Command::WithdrawHandyQualification {
                dossier_id,
                expected_qualification_revision,
                note,
                ..
            } => (dossier_id, *expected_qualification_revision, note, false),
            _ => return Err(ProtocolError::unsupported("qualification operation")),
        };
        let mut value = report(&db, id)?;
        if value.revision != expected {
            return Err(ProtocolError::new(
                ErrorCode::RevisionConflict,
                "qualification review revision changed",
            ));
        }
        let _memory = self.pool.reserve_memory(16 * 1024 * 1024)?;
        if accept {
            verify_evidence(&db, &value)?;
            if value.manifest.subject.origin != HandyEvidenceOrigin::Physical {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "synthetic evidence cannot receive physical acceptance",
                ));
            }
            let expected_settings = value
                .manifest
                .subject
                .operative_settings
                .as_ref()
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::Forbidden,
                        "qualification lacks explicitly reviewed operative settings",
                    )
                })?;
            let Command::AcceptHandyQualification {
                engine_epoch,
                device_session,
                ..
            } = &r.command
            else {
                unreachable!()
            };
            if engine_epoch.as_str() != self.engine_epoch {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "qualification acceptance belongs to a retired Engine boot",
                ));
            }
            let before = check_binding(&db, &value.manifest.subject)?;
            let (connection, credentials) = self
                .handy_sessions
                .lock()
                .map_err(internal)?
                .capture(session, device_session)?;
            if connection.registration != before
                || connection
                    .inspection
                    .as_ref()
                    .and_then(|s| s.identity.as_ref())
                    != Some(&value.manifest.subject.identity)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "qualification device observation differs",
                ));
            }
            drop(db);
            let observation = inspect(credentials.clone())?;
            observation.validate()?;
            if (value.manifest.subject.transport()? == HandyQualificationTransport::Hsp
                && (observation.mode.mode != 4 || observation.remote_hsp.is_none()))
                || observation.identity != value.manifest.subject.identity
                || observation.operative_settings != *expected_settings
                || credentials.echoed_text(&serde_json::to_string(&observation).map_err(internal)?)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "fresh qualification identity or operative settings differ",
                ));
            }
            db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, session)?;
            check_request_identity(&db, session.as_str(), r, fingerprint)?;
            // A concurrent identical acceptance may have completed during inspection.
            if let Some(body) = replay(&db, session.as_str(), r, fingerprint)? {
                return Ok(body);
            }
            let current = report(&db, id)?;
            if current.revision != expected {
                return Err(ProtocolError::new(
                    ErrorCode::RevisionConflict,
                    "qualification review changed during inspection",
                ));
            }
            verify_evidence(&db, &current)?;
            if check_binding(&db, &current.manifest.subject)? != before {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "qualification registration changed during inspection",
                ));
            }
            let (now, keys) = self
                .handy_sessions
                .lock()
                .map_err(internal)?
                .capture(session, device_session)?;
            if now.registration != connection.registration || !Arc::ptr_eq(&keys, &credentials) {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "qualification connection retired",
                ));
            }
        }
        value.revision = expected
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or_else(|| invalid("qualification revision overflow"))?;
        value.state = if accept {
            HandyQualificationState::Accepted
        } else {
            HandyQualificationState::Withdrawn
        };
        value.decision = Some(HandyQualificationDecision {
            request_id: r.request_id.clone(),
            actor_session: session.clone(),
            note: note.clone(),
            revision: value.revision,
            state: value.state,
            recorded_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(internal)?
                .as_millis()
                .try_into()
                .map_err(internal)?,
        });
        value.validate()?;
        if accept {
            if value.manifest.subject.transport()?.is_hssp() {
                let _checked_facts = native_timing(&value)?;
            } else {
                // Validate the HSP envelope without minting hosted HSSP authority.
                value.manifest.subject.hsp_profile_sha256()?;
            }
        }
        let tx = db.transaction().map_err(internal)?;
        let changed = tx
            .execute(
                "UPDATE handy_qualifications SET revision=?1,report=?2 WHERE id=?3 AND revision=?4",
                params![value.revision, encode(&value)?, id.as_str(), expected],
            )
            .map_err(internal)?;
        if changed != 1 {
            return Err(invalid("qualification CAS changed"));
        }
        let body = ResponseBody::HandyQualificationSaved(Box::new(HandyQualificationReceipt {
            request_id: r.request_id.clone(),
            input_sha256: None,
            report: value,
        }));
        remember(&tx, session.as_str(), r, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        self.retire_handy_preparations(handy_playback_preparation::Retirement::Qualification(id));
        self.retire_changed_handy_browser_start_qualifications(&db);
        self.retire_changed_handy_hsp_source_qualifications(&db);
        Ok(body)
    }
}
