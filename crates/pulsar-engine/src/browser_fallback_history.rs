//! Exact original browser fallback admission index. Retained bytes are inert:
//! this store never creates source pins, execution authority or accepted records.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TABLE: &str = "browser_fallback_admissions";
const CREATE: &str = "CREATE TABLE browser_fallback_admissions(epoch TEXT PRIMARY KEY NOT NULL,owner TEXT NOT NULL REFERENCES sessions(id),request TEXT NOT NULL,project TEXT NOT NULL REFERENCES projects(id),revision INTEGER NOT NULL CHECK(revision>=0),attempt TEXT UNIQUE NOT NULL,fingerprint TEXT NOT NULL,response_sha256 TEXT NOT NULL,UNIQUE(owner,request),FOREIGN KEY(owner,request) REFERENCES requests(session,request) DEFERRABLE INITIALLY DEFERRED)";
const MAX_ROWS: u64 = 4096;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const FILTER: &str = "CASE WHEN typeof(response)!='text' OR length(CAST(response AS BLOB)) NOT BETWEEN 1 AND 1048576 THEN 1 WHEN NOT json_valid(response) THEN instr(response,'inference_fallback')>0 ELSE json_extract(response,'$.kind') IN ('live_capture','browser_analysis_successor') AND EXISTS(SELECT 1 FROM json_tree(response) j WHERE j.key='inference_fallback') END";
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn same<T: Serialize>(a: &T, b: &T) -> Result<bool> {
    Ok(serde_json::to_vec(a)? == serde_json::to_vec(b)?)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn capture(body: &ResponseBody) -> Option<&LiveCaptureReport> {
    match body {
        ResponseBody::LiveCapture(c) => Some(c),
        ResponseBody::BrowserAnalysisSuccessor(c) => Some(&c.successor),
        _ => None,
    }
}
fn exists(db: &Connection, table: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )?)
}
fn layout(db: &Connection) -> Result<bool> {
    crate::browser_fallback_reader::version(db)?;
    let shadow: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE)", [TABLE], |r| r.get(0))?;
    ensure!(
        !shadow,
        "temporary browser fallback admission index is forbidden"
    );
    let count: u64 = db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE (name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE) AND sql IS NOT NULL LIMIT 2)", [TABLE], |r| r.get(0))?;
    if count == 0 {
        return Ok(false);
    }
    let exact: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND sql=?2)", params![TABLE, CREATE], |r| r.get(0))?;
    let version = crate::portable_browser_history_reader::version(db)?;
    ensure!(
        count == 1 && exact && version == crate::browser_fallback_reader::MAX_STORAGE_VERSION,
        "browser fallback admission layout or reader barrier differs"
    );
    Ok(true)
}
fn capacity(db: &Connection) -> Result<()> {
    let count: u64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.browser_fallback_admissions LIMIT 4097)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count <= MAX_ROWS,
        "browser fallback admission capacity exceeded"
    );
    let invalid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.browser_fallback_admissions WHERE typeof(epoch)!='text' OR length(CAST(epoch AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(owner)!='text' OR length(CAST(owner AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(request)!='text' OR length(CAST(request AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(project)!='text' OR length(CAST(project AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(revision)!='integer' OR revision<0 OR typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(fingerprint)!='text' OR length(CAST(fingerprint AS BLOB))!=64 OR typeof(response_sha256)!='text' OR length(CAST(response_sha256 AS BLOB))!=64)", [], |r| r.get(0))?;
    ensure!(!invalid, "browser fallback admission scalar differs");
    Ok(())
}
fn response_budget(db: &Connection, additional: usize) -> Result<()> {
    if !exists(db, "requests")? {
        ensure!(additional == 0, "request storage absent");
        return Ok(());
    }
    let invalid: bool = db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM main.requests WHERE ({FILTER}) AND (typeof(response)!='text' OR length(CAST(response AS BLOB)) NOT BETWEEN 1 AND 1048576 OR typeof(session)!='text' OR length(CAST(session AS BLOB)) NOT BETWEEN 1 AND 128))"), [], |r| r.get(0))?;
    ensure!(
        !invalid,
        "browser fallback receipt exceeds scalar or byte admission"
    );
    let (count, bytes): (u64,u64) = db.query_row(&format!("SELECT COUNT(*),COALESCE(SUM(length(CAST(response AS BLOB))),0) FROM (SELECT session,response FROM main.requests WHERE {FILTER} LIMIT 16385)"), [], |r| Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        count + u64::from(additional != 0) <= 16384
            && bytes
                .checked_add(additional as u64)
                .is_some_and(|n| n <= MAX_BYTES),
        "browser fallback receipt capacity exceeded"
    );
    Ok(())
}
fn load_indexed(
    db: &Connection,
    epoch: &AttemptId,
) -> Result<Option<(SessionId, LiveCaptureReport)>> {
    let row: Option<(String,String,String,i64,String,String,String)> = db.query_row(
        "SELECT owner,request,project,revision,attempt,fingerprint,response_sha256 FROM main.browser_fallback_admissions WHERE epoch=?1", [epoch.as_str()],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    let Some((owner, request, project, revision, attempt, fingerprint, expected)) = row else {
        return Ok(None);
    };
    ensure!(
        digest(&fingerprint) && digest(&expected),
        "browser fallback admission digest differs"
    );
    let response: Option<String> = db.query_row("SELECT response FROM main.requests WHERE session=?1 AND request=?2 AND fingerprint=?3 AND typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND 1048576",
        params![owner,request,fingerprint], |r| r.get(0)).optional()?;
    let response = response.context("browser fallback original receipt absent or oversized")?;
    ensure!(
        hash(response.as_bytes()) == expected,
        "browser fallback original receipt identity differs"
    );
    let body: ResponseBody = serde_json::from_str(&response)?;
    ensure!(
        serde_json::to_string(&body)? == response,
        "browser fallback original receipt is not canonical"
    );
    let report = capture(&body).context("browser fallback original receipt kind differs")?;
    report.validate()?;
    let info = report
        .inference
        .as_ref()
        .context("browser fallback original inference absent")?;
    ensure!(
        report.active
            && report.received_frames == 0
            && info.state == LiveCaptureInferenceState::Starting
            && info.completed == 0
            && info.accepted == 0
            && info.fallback.is_some()
            && info.fallback_transition.is_none()
            && report.identity.epoch == *epoch
            && report.identity.project.as_str() == project
            && i64::try_from(report.identity.revision.value())? == revision
            && info.attempt_id.as_str() == attempt,
        "browser fallback original scope differs"
    );
    if let ResponseBody::BrowserAnalysisSuccessor(successor) = &body {
        successor.validate()?;
        ensure!(
            successor.original_request_id.as_str() == request,
            "browser fallback successor request differs"
        );
    }
    let member: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.projects p JOIN main.sessions s ON s.id=?1 JOIN main.request_identities i ON i.session=s.id AND i.request=?2 AND i.fingerprint=?3 WHERE p.id=?4 AND typeof(p.revision)='integer' AND p.revision>=?5)",
        params![owner,request,fingerprint,project,revision], |r| r.get(0))?;
    ensure!(
        member,
        "browser fallback original actor/request/project membership differs"
    );
    Ok(Some((SessionId::new(owner)?, report.clone())))
}
fn load(db: &Connection, epoch: &AttemptId) -> Result<Option<(SessionId, LiveCaptureReport)>> {
    if !layout(db)? {
        let version = crate::portable_browser_history_reader::version(db)?;
        if version == crate::browser_fallback_reader::MAX_STORAGE_VERSION && exists(db, "requests")?
        {
            let required: bool = db.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM main.requests WHERE {FILTER})"),
                [],
                |r| r.get(0),
            )?;
            ensure!(!required, "browser fallback admission index stripped");
        }
        return Ok(None);
    }
    capacity(db)?;
    load_indexed(db, epoch)
}
/// Exact canonical original acknowledgement for inert, project-scoped archives.
/// Caller holds the SQL snapshot and separately checks disclosure approval.
pub(crate) fn archival_receipt(
    db: &Connection,
    epoch: &AttemptId,
) -> Result<(SessionId, RequestId, String, String)> {
    let (owner, _) = load(db, epoch)?.context("original browser admission absent")?;
    let (request,fingerprint,response):(String,String,String)=db.query_row("SELECT i.request,i.fingerprint,r.response FROM main.browser_fallback_admissions i JOIN main.requests r ON r.session=i.owner AND r.request=i.request WHERE i.epoch=?1 AND i.owner=?2 AND typeof(r.response)='text' AND length(CAST(r.response AS BLOB)) BETWEEN 1 AND 1048576",params![epoch.as_str(),owner.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    Ok((owner, RequestId::new(request)?, fingerprint, response))
}

fn require_capture(db: &Connection, actor: &SessionId, report: &LiveCaptureReport) -> Result<()> {
    require_original(actor, report, load(db, &report.identity.epoch)?)
}
fn require_original(
    actor: &SessionId,
    report: &LiveCaptureReport,
    original: Option<(SessionId, LiveCaptureReport)>,
) -> Result<()> {
    report.validate()?;
    let Some((owner, original)) = original else {
        ensure!(
            report.options.inference_fallback.is_none(),
            "browser fallback original admission absent"
        );
        return Ok(());
    };
    ensure!(
        owner == *actor
            && original.engine_epoch == report.engine_epoch
            && original.identity == report.identity
            && original.clock == report.clock
            && same(&original.options, &report.options)?
            && same(
                &original.inference.as_ref().unwrap().fallback,
                &report
                    .inference
                    .as_ref()
                    .context("fallback inference stripped")?
                    .fallback
            )?,
        "browser fallback capture differs from original admission"
    );
    Ok(())
}
pub(crate) fn validate_recording_page(
    db: &Connection,
    actor: &SessionId,
    page: &LiveRecordingPage,
) -> Result<()> {
    page.validate()?;
    let Some((owner, original)) = load(db, &page.identity.epoch)? else {
        ensure!(
            page.fallback.is_none(),
            "recorded fallback lacks original admission"
        );
        return Ok(());
    };
    let envelope = page
        .fallback
        .as_ref()
        .context("recorded fallback provenance stripped")?;
    ensure!(
        owner == *actor
            && original.engine_epoch == page.engine_epoch
            && original.identity == page.identity
            && original.clock == page.clock
            && original.inference.as_ref().unwrap().attempt_id == page.inference_attempt
            && same(&*envelope.options, &original.options)?
            && same(
                &envelope.admission,
                original
                    .inference
                    .as_ref()
                    .unwrap()
                    .fallback
                    .as_ref()
                    .unwrap()
            )?,
        "recorded fallback differs from original admission"
    );
    Ok(())
}
pub(crate) fn remember(
    tx: &Transaction<'_>,
    session: &str,
    request: &Request,
    fingerprint: &str,
    body: &ResponseBody,
) -> Result<()> {
    let actor = SessionId::new(session)?;
    if let ResponseBody::LiveRecordingPage(page) = body {
        return validate_recording_page(tx, &actor, page);
    }
    let Some(report) = capture(body) else {
        return Ok(());
    };
    if let ResponseBody::BrowserAnalysisSuccessor(successor) = body {
        successor.validate()?;
    }
    if report.options.inference_fallback.is_some() {
        let bytes = serde_json::to_vec(body)?;
        ensure!(
            bytes.len() <= MAX_CONTROL_BYTES,
            "browser fallback receipt exceeds admission"
        );
        response_budget(tx, bytes.len())?;
    }
    let original = matches!(
        request.command,
        Command::StartLiveCapture { .. }
            | Command::ContinueLiveCapture { .. }
            | Command::ContinueLiveCaptureAt { .. }
            | Command::PrepareBrowserAnalysisSuccessor { .. }
    );
    if !original || report.options.inference_fallback.is_none() {
        return require_capture(tx, &actor, report);
    }
    report.validate()?;
    let info = report
        .inference
        .as_ref()
        .context("fallback admission inference absent")?;
    ensure!(
        report.active
            && report.received_frames == 0
            && info.state == LiveCaptureInferenceState::Starting
            && info.completed == 0
            && info.accepted == 0
            && info.fallback_transition.is_none()
            && request.session.as_ref() == Some(&actor)
            && request.project.as_ref() == Some(&report.identity.project)
            && request.expected_revision == Some(report.identity.revision)
            && digest(fingerprint),
        "browser fallback requires original initial admission"
    );
    crate::browser_fallback_reader::arm(tx)?;
    if !layout(tx)? {
        tx.execute_batch(CREATE)?;
    }
    capacity(tx)?;
    let count: u64 = tx.query_row(
        "SELECT COUNT(*) FROM main.browser_fallback_admissions",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count < MAX_ROWS,
        "browser fallback admission capacity exhausted"
    );
    let bytes = serde_json::to_vec(body)?;
    ensure!(
        bytes.len() <= MAX_CONTROL_BYTES,
        "browser fallback receipt exceeds admission"
    );
    tx.execute("INSERT INTO main.browser_fallback_admissions(epoch,owner,request,project,revision,attempt,fingerprint,response_sha256) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![report.identity.epoch.as_str(),session,request.request_id.as_str(),report.identity.project.as_str(),
            i64::try_from(report.identity.revision.value())?,info.attempt_id.as_str(),fingerprint,hash(&bytes)])?;
    Ok(())
}
pub(crate) fn validate(db: &Connection) -> Result<()> {
    let present = layout(db)?;
    if !present {
        let version = crate::portable_browser_history_reader::version(db)?;
        if version != crate::browser_fallback_reader::MAX_STORAGE_VERSION {
            return Ok(());
        }
    }
    if present {
        capacity(db)?;
        let mut stmt =
            db.prepare("SELECT epoch FROM main.browser_fallback_admissions ORDER BY epoch")?;
        let epochs = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for epoch in epochs {
            load_indexed(db, &AttemptId::new(epoch?)?)?
                .context("fallback index row disappeared")?;
        }
    }
    response_budget(db, 0)?;
    if !exists(db, "requests")? {
        return Ok(());
    }
    let mut stmt = db.prepare(&format!(
        "SELECT session,response FROM main.requests WHERE {FILTER}"
    ))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let actor = SessionId::new(row.get::<_, String>(0)?)?;
        let raw: String = row.get(1)?;
        let body: ResponseBody = serde_json::from_str(&raw)?;
        ensure!(
            serde_json::to_string(&body)? == raw,
            "browser fallback receipt is not canonical"
        );
        let report = capture(&body).context("browser fallback receipt kind differs")?;
        if let ResponseBody::BrowserAnalysisSuccessor(successor) = &body {
            successor.validate()?;
        }
        ensure!(present, "browser fallback admission index stripped");
        require_original(&actor, report, load_indexed(db, &report.identity.epoch)?)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "browser_fallback_history_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn authored_capture_for_archive_test() -> LiveCaptureReport {
    tests::report()
}
