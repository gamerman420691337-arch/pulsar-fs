//! Historical captured CPU live admission, never a lease or a worker factory.
//! Original request credentials are deliberately not retained. The opaque
//! request fingerprint is checked against both existing request namespaces.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TABLE: &str = "live_cpu_admissions";
const CREATE: &str = "CREATE TABLE live_cpu_admissions(epoch TEXT PRIMARY KEY NOT NULL,owner TEXT NOT NULL REFERENCES sessions(id),request TEXT NOT NULL,project TEXT NOT NULL REFERENCES projects(id),revision INTEGER NOT NULL CHECK(revision>=0),attempt TEXT UNIQUE NOT NULL,origin BLOB NOT NULL,UNIQUE(owner,request),FOREIGN KEY(owner,request) REFERENCES requests(session,request) DEFERRABLE INITIALLY DEFERRED)";
const MAX_ORIGIN: usize = 2 * MAX_CONTROL_BYTES;
const MAX_ROWS: u64 = 4096;
const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Origin {
    schema_version: u16,
    /// Exact original kind and scope, with authentication removed before write.
    request: Request,
    /// This digest includes the original credential; it cannot be recomputed
    /// from `request` and it must never be treated as a reusable credential.
    fingerprint: String,
    response_sha256: String,
    manifest: LiveCpuInferenceManifest,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn same<T: Serialize>(a: &T, b: &T) -> Result<bool> {
    Ok(serde_json::to_vec(a)? == serde_json::to_vec(b)?)
}
fn cpu(report: &LiveCaptureReport) -> Option<&LiveCpuInferenceManifest> {
    report.inference.as_ref()?.cpu_manifest.as_deref()
}
fn capture(body: &ResponseBody) -> Option<&LiveCaptureReport> {
    match body {
        ResponseBody::LiveCapture(report) => Some(report),
        ResponseBody::BrowserAnalysisSuccessor(report) => Some(&report.successor),
        _ => None,
    }
}
fn admission(command: &Command) -> bool {
    matches!(
        command,
        Command::StartLiveCapture { .. }
            | Command::ContinueLiveCapture { .. }
            | Command::ContinueLiveCaptureAt { .. }
            | Command::PrepareBrowserAnalysisSuccessor { .. }
    )
}
fn origin_bytes(origin: &Origin) -> Result<Vec<u8>> {
    ensure!(
        origin.schema_version == 1
            && origin.request.auth_token.is_none()
            && is_digest(&origin.fingerprint)
            && is_digest(&origin.response_sha256)
            && admission(&origin.request.command),
        "captured CPU live origin kind or scalar differs"
    );
    origin.request.validate()?;
    origin.manifest.validate()?;
    let bytes = serde_json::to_vec(origin)?;
    ensure!(
        bytes.len() <= MAX_ORIGIN,
        "captured CPU live origin exceeds bound"
    );
    Ok(bytes)
}

fn layout(db: &Connection) -> Result<bool> {
    crate::live_cpu_reader::version(db)?;
    let raw: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    let temporary: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE (lower(name) GLOB 'live_cpu_*' OR lower(tbl_name) GLOB 'live_cpu_*') AND lower(name)!='live_cpu_reader' AND lower(tbl_name)!='live_cpu_reader')", [], |r| r.get(0))?;
    ensure!(
        !temporary,
        "temporary captured CPU live storage is forbidden"
    );
    let objects: u64 = db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE (lower(name) GLOB 'live_cpu_*' OR lower(tbl_name) GLOB 'live_cpu_*') AND lower(name)!='live_cpu_reader' AND lower(tbl_name)!='live_cpu_reader' AND sql IS NOT NULL LIMIT 2)", [], |r| r.get(0))?;
    let exact: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)",params![TABLE, CREATE.len() as i64, CREATE], |r|r.get(0))?;
    ensure!(
        (objects == 0 && raw != 38) || (objects == 1 && exact && raw == 38),
        "captured CPU live history requires exact layout and reader38"
    );
    Ok(objects == 1)
}
fn capacity(db: &Connection) -> Result<()> {
    let count: u64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.live_cpu_admissions LIMIT 4097)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count <= MAX_ROWS,
        "captured CPU live admission row capacity exceeded"
    );
    let invalid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.live_cpu_admissions WHERE typeof(epoch)!='text' OR length(CAST(epoch AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(owner)!='text' OR length(CAST(owner AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(request)!='text' OR length(CAST(request AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(project)!='text' OR length(CAST(project AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(revision)!='integer' OR revision<0 OR typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(origin)!='blob' OR length(origin) NOT BETWEEN 1 AND 2097152)", [], |r|r.get(0))?;
    ensure!(
        !invalid,
        "captured CPU live admission scalar or byte bound differs"
    );
    let bytes: u64 = db.query_row(
        "SELECT COALESCE(SUM(length(origin)),0) FROM main.live_cpu_admissions",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        bytes <= MAX_BYTES,
        "captured CPU live admission byte capacity exceeded"
    );
    Ok(())
}

/// Arm the outer barrier for either capture admission or other metadata whose
/// captured-image meaning preceding readers do not understand. An empty table
/// is deliberate: a qualification subject is not a live capture origin.
fn arm_metadata_reader(tx: &Transaction<'_>) -> Result<()> {
    if !layout(tx)? {
        crate::live_cpu_reader::arm(tx)?;
        tx.execute_batch(CREATE)?;
    }
    capacity(tx)
}

/// Call in the existing qualification transaction before storing a subject with
/// a captured CPU image. This changes only reader metadata, never acceptance.
pub(crate) fn arm_hsp_source_subject(
    tx: &Transaction<'_>,
    subject: &HandyHspSourceSubject,
) -> Result<()> {
    if subject.cpu_runtime_image.is_some() {
        subject.validate()?;
        arm_metadata_reader(tx)?;
    }
    Ok(())
}

/// Historical read requirement only; no original capture is required or made.
pub(crate) fn require_hsp_source_reader(
    db: &Connection,
    subject: &HandyHspSourceSubject,
) -> Result<()> {
    if subject.cpu_runtime_image.is_some() {
        subject.validate()?;
        ensure!(layout(db)?, "HSP captured CPU image requires reader38");
        capacity(db)?;
    }
    Ok(())
}

/// Only absent inference resolved through the exact original selected pack may
/// differ. Explicit inference, selected revision, paths and all other options
/// remain byte-identical typed declarations.
fn request_options(original: &LiveCaptureOptions, resolved: &LiveCaptureOptions) -> Result<()> {
    let mut expected = original.clone();
    if expected.inference.is_none() && expected.model_pack.is_some() {
        ensure!(
            resolved.inference.is_some(),
            "captured CPU pack inference absent"
        );
        expected.inference = resolved.inference.clone();
    }
    ensure!(
        same(&expected, resolved)?,
        "captured CPU live original options differ"
    );
    Ok(())
}
fn initial_report(origin: &Origin, body: &ResponseBody) -> Result<LiveCaptureReport> {
    origin_bytes(origin)?;
    let report = capture(body).context("captured CPU live initial response kind differs")?;
    report.validate()?;
    let request = &origin.request;
    ensure!(
        request.session.is_some()
            && request.project.as_ref() == Some(&report.identity.project)
            && request.expected_revision == Some(report.identity.revision)
            && report.active
            && report.received_frames == 0
            && report.last_frame.is_none()
            && report.inference.as_ref().is_some_and(|i| i.state
                == LiveCaptureInferenceState::Starting
                && i.completed == 0
                && i.accepted == 0
                && i.stale == 0
                && i.latest.is_none())
            && cpu(report).is_some_and(|m| same(m, &origin.manifest).unwrap_or(false)),
        "captured CPU live initial report scope or execution differs"
    );
    match (&request.command, body) {
        (
            Command::StartLiveCapture {
                engine_epoch,
                options,
            },
            ResponseBody::LiveCapture(_),
        ) => {
            ensure!(
                report.engine_epoch == *engine_epoch && report.continuation.is_none(),
                "captured CPU live start origin differs"
            );
            request_options(options, &report.options)?;
        }
        (
            Command::ContinueLiveCapture {
                engine_epoch,
                predecessor,
                options,
            },
            ResponseBody::LiveCapture(_),
        ) => {
            ensure!(
                report.engine_epoch == *engine_epoch
                    && report
                        .continuation
                        .as_ref()
                        .is_some_and(|c| c.predecessor.epoch == *predecessor),
                "captured CPU live continuation origin differs"
            );
            request_options(options, &report.options)?;
        }
        (
            Command::ContinueLiveCaptureAt {
                engine_epoch,
                predecessor,
                source_anchor,
                lease_ms,
            },
            ResponseBody::LiveCapture(_),
        ) => {
            ensure!(
                report.engine_epoch == *engine_epoch
                    && report
                        .continuation
                        .as_ref()
                        .is_some_and(|c| c.predecessor.epoch == *predecessor)
                    && report.options.source_anchor == *source_anchor
                    && report.options.lease_ms == *lease_ms,
                "captured CPU live derived continuation origin differs"
            );
        }
        (
            Command::PrepareBrowserAnalysisSuccessor {
                engine_epoch,
                predecessor,
                options,
            },
            ResponseBody::BrowserAnalysisSuccessor(successor),
        ) => {
            successor.validate()?;
            ensure!(
                report.engine_epoch == *engine_epoch
                    && successor.original_request_id == request.request_id
                    && successor.predecessor.epoch == *predecessor
                    && successor.phase == BrowserAnalysisSuccessorPhase::Preparing
                    && successor.transfer_request_id.is_none()
                    && report.options.browser_media.as_ref() == Some(&options.browser_media)
                    && same(
                        &report.options.browser_analysis,
                        &Some(options.browser_analysis.clone())
                    )?
                    && report.options.lease_ms == options.lease_ms,
                "captured CPU live browser successor origin differs"
            );
        }
        _ => anyhow::bail!("captured CPU live original request/response kind differs"),
    }
    Ok(report.clone())
}

/// Loads only bounded original metadata. No host paths are opened and no
/// permission, process, provider or native timing object can be constructed.
fn load_base(db: &Connection, epoch: &AttemptId) -> Result<Option<(Origin, LiveCaptureReport)>> {
    if !layout(db)? {
        return Ok(None);
    }
    capacity(db)?;
    let row: Option<(String, String, String, i64, String, Vec<u8>)> = db.query_row(
        "SELECT owner,request,project,revision,attempt,origin FROM main.live_cpu_admissions WHERE epoch=?1",
        [epoch.as_str()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((owner, request, project, revision, attempt, bytes)) = row else {
        return Ok(None);
    };
    let origin: Origin = serde_json::from_slice(&bytes)?;
    ensure!(
        origin_bytes(&origin)? == bytes,
        "captured CPU live origin is noncanonical"
    );
    let stored: Option<(String, String)> = db.query_row("SELECT fingerprint,response FROM main.requests WHERE session=?1 AND request=?2 AND typeof(fingerprint)='text' AND length(CAST(fingerprint AS BLOB))=64 AND typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND 1048576", params![owner,request], |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let (fingerprint, response) =
        stored.context("captured CPU live initial receipt missing or oversized")?;
    ensure!(
        fingerprint == origin.fingerprint && digest(response.as_bytes()) == origin.response_sha256,
        "captured CPU live initial receipt identity differs"
    );
    let body: ResponseBody = serde_json::from_str(&response)?;
    ensure!(
        serde_json::to_string(&body)? == response,
        "captured CPU live initial receipt is noncanonical"
    );
    let report = initial_report(&origin, &body)?;
    ensure!(
        report.identity.epoch == *epoch
            && origin.request.session.as_ref().map(SessionId::as_str) == Some(owner.as_str())
            && origin.request.request_id.as_str() == request
            && report.identity.project.as_str() == project
            && i64::try_from(report.identity.revision.value())? == revision
            && origin.manifest.stream.attempt_id.as_str() == attempt,
        "captured CPU live indexed membership differs"
    );
    let member: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.projects p JOIN main.sessions s ON s.id=?1 JOIN main.request_identities i ON i.session=s.id AND i.request=?2 AND i.fingerprint=?3 WHERE p.id=?4 AND typeof(p.revision)='integer' AND p.revision>=?5)", params![owner,request,fingerprint,project,revision], |r|r.get(0))?;
    ensure!(
        member,
        "captured CPU live actor/request/project membership differs"
    );
    Ok(Some((origin, report)))
}
fn predecessor(db: &Connection, origin: &Origin, report: &LiveCaptureReport) -> Result<()> {
    let epoch = match &origin.request.command {
        Command::ContinueLiveCapture { predecessor, .. }
        | Command::ContinueLiveCaptureAt { predecessor, .. }
        | Command::PrepareBrowserAnalysisSuccessor { predecessor, .. } => predecessor,
        Command::StartLiveCapture { .. } => return Ok(()),
        _ => anyhow::bail!("captured CPU live original kind differs"),
    };
    let (prior_origin, prior) =
        load_base(db, epoch)?.context("captured CPU live predecessor admission absent")?;
    ensure!(
        epoch != &report.identity.epoch
            && prior.engine_epoch == report.engine_epoch
            && prior_origin.request.session == origin.request.session
            && prior.identity.project == report.identity.project
            && prior.identity.revision == report.identity.revision
            && prior.identity.source == report.identity.source,
        "captured CPU live predecessor owner/boot/scope differs"
    );
    if let Some(continuation) = &report.continuation {
        ensure!(
            continuation.predecessor == prior.identity
                && continuation.predecessor_clock == prior.clock,
            "captured CPU live predecessor clock differs"
        );
    }
    match &origin.request.command {
        Command::ContinueLiveCaptureAt {
            source_anchor,
            lease_ms,
            ..
        } => {
            let expected = prior.continuation_options(*source_anchor, *lease_ms)?;
            request_options(&expected, &report.options)?;
        }
        Command::ContinueLiveCapture { .. } => {
            ensure!(
                same(&prior.options.inference, &report.options.inference)?
                    && prior.options.model_pack == report.options.model_pack
                    && same(
                        &prior.options.encoded_source,
                        &report.options.encoded_source
                    )?
                    && prior.options.policy == report.options.policy
                    && prior.options.source_time_basis == report.options.source_time_basis
                    && prior.options.browser_media == report.options.browser_media,
                "captured CPU live continuation changed source/profile/policy"
            );
        }
        Command::PrepareBrowserAnalysisSuccessor { options, .. } => {
            let mut expected = prior.options.clone();
            expected.browser_media = Some(options.browser_media.clone());
            expected.browser_analysis = Some(options.browser_analysis.clone());
            expected.lease_ms = options.lease_ms;
            expected.continuation_mode = LiveContinuationMode::Immediate;
            ensure!(
                same(&expected, &report.options)?,
                "captured CPU live successor changed derived options"
            );
        }
        _ => unreachable!("predecessor kind selected"),
    }
    Ok(())
}
fn require_capture(db: &Connection, actor: &SessionId, report: &LiveCaptureReport) -> Result<()> {
    report.validate()?;
    let original = load_base(db, &report.identity.epoch)?;
    let Some((origin, initial)) = original else {
        ensure!(
            cpu(report).is_none(),
            "captured CPU live report lacks original admission"
        );
        return Ok(());
    };
    report.validate()?;
    ensure!(
        origin.request.session.as_ref() == Some(actor)
            && initial.engine_epoch == report.engine_epoch
            && initial.identity == report.identity
            && initial.clock == report.clock
            && same(&initial.options, &report.options)?
            && cpu(report).is_some_and(|m| same(m, &origin.manifest).unwrap_or(false)),
        "captured CPU live report differs from original admission"
    );
    predecessor(db, &origin, &initial)
}

/// Original actor and capture membership for an Engine-owned recording page.
/// In particular, the lease in the page's manifest must equal original admission.
pub(crate) fn validate_recording_page(
    db: &Connection,
    actor: &SessionId,
    page: &LiveRecordingPage,
) -> Result<()> {
    page.validate()?;
    let original = load_base(db, &page.identity.epoch)?;
    let Some((origin, report)) = original else {
        ensure!(
            page.cpu_manifest.is_none(),
            "captured CPU live recording lacks original admission"
        );
        return Ok(());
    };
    page.validate()?;
    ensure!(
        origin.request.session.as_ref() == Some(actor)
            && page.engine_epoch == report.engine_epoch
            && page.identity == report.identity
            && page.clock == report.clock
            && page.inference_attempt == origin.manifest.stream.attempt_id
            && same(&page.profile, &origin.manifest.stream.profile)?
            && page.model_pack == report.options.model_pack
            && page.browser_media == report.options.browser_media
            && page.source_time_basis == report.options.source_time_basis
            && page
                .cpu_manifest
                .as_deref()
                .is_some_and(|m| same(m, &origin.manifest).unwrap_or(false)),
        "captured CPU live recording differs from original admission"
    );
    predecessor(db, &origin, &report)
}

/// Call from the common receipt writer before INSERT into requests, with the
/// caller's original (not pack-resolved) request. The entire outer transaction
/// must roll back on error. The deferred request FK resolves on that commit.
pub(crate) fn remember(
    tx: &Transaction<'_>,
    session: &str,
    request: &Request,
    fingerprint: &str,
    body: &ResponseBody,
) -> Result<()> {
    match body {
        ResponseBody::HandyHspSourceQualificationSaved(receipt) => {
            receipt.validate()?;
            return arm_hsp_source_subject(tx, &receipt.report.manifest.subject);
        }
        ResponseBody::HandyHspSourceQualification(report) => {
            report.validate()?;
            return arm_hsp_source_subject(tx, &report.manifest.subject);
        }
        _ => {}
    }
    let actor = SessionId::new(session)?;
    if let ResponseBody::LiveRecordingPage(page) = body {
        return validate_recording_page(tx, &actor, page);
    }
    let Some(report) = capture(body) else {
        return Ok(());
    };
    if !admission(&request.command) || cpu(report).is_none() {
        return require_capture(tx, &actor, report);
    }
    let mut clean = request.clone();
    clean.auth_token = None;
    ensure!(
        clean.session.as_ref() == Some(&actor),
        "captured CPU live original actor differs"
    );
    let body_bytes = serde_json::to_vec(body)?;
    ensure!(
        body_bytes.len() <= MAX_CONTROL_BYTES,
        "captured CPU live response exceeds bound"
    );
    let origin = Origin {
        schema_version: 1,
        request: clean,
        fingerprint: fingerprint.into(),
        response_sha256: digest(&body_bytes),
        manifest: cpu(report).expect("checked").clone(),
    };
    let initial = initial_report(&origin, body)?;
    predecessor(tx, &origin, &initial)?;
    let bytes = origin_bytes(&origin)?;
    let current: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM main.projects p JOIN main.sessions s ON s.id=?1 JOIN main.request_identities i ON i.session=s.id AND i.request=?2 AND i.fingerprint=?3 WHERE p.id=?4 AND typeof(p.revision)='integer' AND p.revision=?5)", params![session,request.request_id.as_str(),fingerprint,report.identity.project.as_str(),i64::try_from(report.identity.revision.value())?], |r|r.get(0))?;
    ensure!(
        current,
        "captured CPU live original actor/request/current revision differs"
    );
    arm_metadata_reader(tx)?;
    ensure!(
        load_base(tx, &report.identity.epoch)?.is_none(),
        "captured CPU live admission already exists"
    );
    tx.execute("INSERT INTO main.live_cpu_admissions(epoch,owner,request,project,revision,attempt,origin) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![report.identity.epoch.as_str(),session,request.request_id.as_str(),report.identity.project.as_str(),i64::try_from(report.identity.revision.value())?,origin.manifest.stream.attempt_id.as_str(),bytes])?;
    capacity(tx)?;
    Ok(())
}

/// Read-only startup inspection. This never repairs records or populates the
/// live registry. Old boot receipts remain history and are retired by replay.
pub(crate) fn validate(db: &Connection) -> Result<()> {
    let present = layout(db)?;
    if present {
        capacity(db)?;
        let mut query =
            db.prepare("SELECT epoch FROM main.live_cpu_admissions ORDER BY epoch COLLATE BINARY")?;
        let mut rows = query.query([])?;
        while let Some(row) = rows.next()? {
            let epoch = AttemptId::new(row.get::<_, String>(0)?)?;
            let (origin, report) =
                load_base(db, &epoch)?.context("captured CPU live admission vanished")?;
            predecessor(db, &origin, &report)?;
        }
    }
    let requests: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name='requests')",
        [],
        |r| r.get(0),
    )?;
    if !requests {
        ensure!(!present, "captured CPU live request namespace absent");
        return Ok(());
    }
    // Inspect response kinds using SQLite JSON parsing, including escaped keys
    // and whitespace. Bound each selected scalar before transferring bytes into
    // Rust. Streaming old host reports imposes no new global host-history cap.
    const FILTER: &str = "CASE WHEN json_valid(response) THEN json_extract(response,'$.kind') IN ('live_capture','browser_analysis_successor','live_recording_page','handy_hsp_source_qualification','handy_hsp_source_qualification_saved') ELSE instr(response,'cpu_manifest')>0 OR instr(response,'cpu_runtime_image')>0 END";
    let invalid: bool = db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM main.requests WHERE ({FILTER}) AND (typeof(session)!='text' OR length(CAST(session AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(response)!='text' OR length(CAST(response AS BLOB)) NOT BETWEEN 1 AND 1048576))"), [], |r|r.get(0))?;
    ensure!(
        !invalid,
        "captured CPU live retained response exceeds scalar bound"
    );
    let mut query = db.prepare(&format!(
        "SELECT session,response FROM main.requests WHERE {FILTER}"
    ))?;
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        let actor = SessionId::new(row.get::<_, String>(0)?)?;
        let text: String = row.get(1)?;
        let body: ResponseBody = serde_json::from_str(&text)?;
        if let Some(report) = capture(&body) {
            require_capture(db, &actor, report)?;
        } else if let ResponseBody::LiveRecordingPage(page) = &body {
            validate_recording_page(db, &actor, page)?;
        } else if let ResponseBody::HandyHspSourceQualificationSaved(receipt) = &body {
            receipt.validate()?;
            require_hsp_source_reader(db, &receipt.report.manifest.subject)?;
        } else if let ResponseBody::HandyHspSourceQualification(report) = &body {
            report.validate()?;
            require_hsp_source_reader(db, &report.manifest.subject)?;
        } else {
            anyhow::bail!("captured CPU live retained response kind differs");
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "live_cpu_history_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "live_cpu_hsp_history_tests.rs"]
mod hsp_history_tests;
