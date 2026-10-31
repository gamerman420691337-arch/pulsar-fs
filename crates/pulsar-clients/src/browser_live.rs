//! Scoped loopback player. Frames and optional stop guards use native Engine authority.
use crate::{motion::ResponseBody, EngineApi, SessionClient};
use anyhow::{bail, ensure, Context, Result};
use pulsar_protocol::*;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

const BODY_LIMIT: usize = LIVE_FRAME_BYTES + 1024;
const PAGE: &str = include_str!("browser_live.html");
#[path = "browser_live_fallback.rs"]
mod fallback;
#[path = "browser_player_http.rs"]
mod player_http;
#[path = "browser_source_media.rs"]
mod source_media;
use crate::browser_player_control::{GuardConfig, Scope};
fn checked_script(program: &MotionProgram) -> Result<serde_json::Value> {
    let track = program
        .track(Axis::Stroke)
        .context("committed Stroke axis absent")?;
    ensure!(
        track.gaps().is_empty(),
        "resolve committed Stroke gaps before downloading a funscript"
    );
    ensure!(
        !track.actions().is_empty(),
        "committed Stroke has no actions"
    );
    let mut actions = Vec::with_capacity(track.actions().len());
    let mut last = None;
    for a in track.actions() {
        let ns = a.time().as_nanos();
        ensure!(ns >= 0, "negative script time refused");
        let ms = ns
            .checked_add(500_000)
            .context("script timestamp overflow")?
            / 1_000_000;
        ensure!(
            last.is_none_or(|old| old < ms),
            "millisecond quantization collision requires explicit resampling"
        );
        last = Some(ms);
        actions
            .push(serde_json::json!({"at":ms,"pos":(a.position().value()*100.0).round() as u32}));
    }
    Ok(serde_json::json!({"version":"1.0","inverted":false,"range":90,"actions":actions}))
}
struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
    scope: Scope,
    received: Instant,
    range: Option<String>,
    if_range: Option<String>,
    browser_media: Option<BrowserMediaBinding>,
    player_permit: Option<crate::browser_player_control::Permit>,
    browser_timing: Option<BrowserTimingBinding>,
    browser_media_expires: Option<Instant>,
}
#[cfg(test)]
fn read_request(stream: &mut TcpStream, authority: &str) -> Result<Request> {
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    let mut bytes = Vec::new();
    stream
        .take((BODY_LIMIT + 8193) as u64)
        .read_to_end(&mut bytes)?;
    parse_request(&bytes, authority, Instant::now())
}
fn parse_request(bytes: &[u8], authority: &str, received: Instant) -> Result<Request> {
    let split = bytes
        .windows(4)
        .position(|b| b == b"\r\n\r\n")
        .context("HTTP header incomplete")?
        + 4;
    ensure!(split <= 8192, "HTTP header limit");
    let header = &bytes[..split];
    let text = std::str::from_utf8(header)?;
    let mut lines = text.split("\r\n");
    let parts: Vec<_> = lines
        .next()
        .context("request line absent")?
        .split(' ')
        .collect();
    ensure!(
        parts.len() == 3 && parts[2] == "HTTP/1.1",
        "HTTP/1.1 request required"
    );
    let mut fields = BTreeMap::new();
    for line in lines.filter(|s| !s.is_empty()) {
        let (name, value) = line.split_once(':').context("malformed HTTP field")?;
        ensure!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "invalid HTTP field"
        );
        ensure!(
            fields
                .insert(name.to_ascii_lowercase(), value.trim())
                .is_none(),
            "duplicate HTTP field"
        );
    }
    ensure!(
        fields.get("host") == Some(&authority),
        "loopback Host mismatch"
    );
    ensure!(
        !fields.contains_key("transfer-encoding"),
        "chunked ingress refused"
    );
    let length = fields
        .get("content-length")
        .map(|v| v.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    ensure!(length <= BODY_LIMIT, "HTTP body limit");
    match parts[0] {
        "GET" | "HEAD" => {
            ensure!(
                length == 0
                    && (parts[0] == "GET" && parts[1] == "/" || parts[1].starts_with("/media/")),
                "unknown media GET/HEAD route"
            );
            if let Some(origin) = fields.get("origin") {
                ensure!(
                    *origin == format!("http://{authority}"),
                    "foreign media Origin refused"
                );
            }
        }
        "POST" => {
            let origin = format!("http://{authority}");
            ensure!(
                fields.get("origin") == Some(&origin.as_str()),
                "same-origin POST required"
            );
            ensure!(
                fields.get("content-type") == Some(&"application/octet-stream"),
                "binary ingress required"
            );
            ensure!(
                matches!(
                    parts[1],
                    "/media/catalog"
                        | "/media/prepare"
                        | "/media/status"
                        | "/media/release"
                        | "/media/continue"
                        | "/media/continue-status"
                        | "/player/start"
                        | "/player/challenge"
                        | "/player/observe"
                        | "/player/timing-probe"
                        | "/player/timing-observe"
                        | "/player/timing-status"
                        | "/player/timing-next-open"
                        | "/player/timing-next-probe"
                        | "/player/timing-next-observe"
                        | "/player/timing-next-status"
                        | "/player/hsp-inspect"
                        | "/player/hsp-prepare"
                        | "/player/hsp-preparation-status"
                        | "/player/hsp-cancel"
                        | "/player/hsp-bind"
                        | "/player/hsp-play"
                        | "/player/hsp-stop"
                        | "/player/hsp-status"
                        | "/player/hsp-continuity-enable"
                        | "/player/hsp-continuity-status"
                        | "/player/handy-inspect"
                        | "/player/handy-start"
                        | "/player/handy-inspect-original"
                        | "/player/handy-status"
                        | "/player/handy-stop"
                        | "/player/stop"
                        | "/start"
                        | "/frame"
                        | "/status"
                        | "/analysis/finish"
                        | "/analysis/finish-status"
                        | "/analysis/prepare"
                        | "/analysis/transfer"
                        | "/analysis/transfer-status"
                        | "/analysis/successor-frame"
                        | "/analysis/cancel-successor"
                        | "/stop"
                        | "/recordings"
                        | "/retire-recordings"
                        | "/recording-history"
                        | "/recording-page"
                        | "/recording-download"
                        | "/prepare-candidate"
                        | "/stage-candidate"
                        | "/candidate-status"
                        | "/retire-candidate"
                        | "/candidate-history"
                        | "/review-candidate"
                        | "/prepare-commit"
                        | "/commit-candidate"
                        | "/inspect-export"
                        | "/prepare-export"
                        | "/download-script"
                        | "/export-status"
                        | "/retire-export"
                        | "/export-history"
                        | "/history-download"
                        | "/prepare-gaps"
                        | "/stage-gaps"
                        | "/gap-status"
                        | "/retire-gap"
                        | "/gap-history"
                ),
                "unknown POST route"
            );
        }
        _ => bail!("method refused"),
    }

    ensure!(bytes.len() - split == length, "HTTP body length differs");
    if parts[1].starts_with("/player/") {
        ensure!(length <= 4096, "player body limit");
    }
    let field = |name: &str| fields.get(name).map(|s| (*s).to_owned());
    let scope = Scope {
        launch: field("x-pulsar-launch").unwrap_or_default(),
        page: field("x-pulsar-page").unwrap_or_default(),
        page_capability: field("x-pulsar-page-capability").unwrap_or_default(),
        player: field("x-pulsar-player"),
        source: field("x-pulsar-source"),
        generation: field("x-pulsar-generation")
            .map(|s| s.parse())
            .transpose()?,
        player_capability: field("x-pulsar-player-capability"),
    };
    Ok(Request {
        method: parts[0].into(),
        path: parts[1].into(),
        body: bytes[split..].to_vec(),
        scope,
        received,
        range: field("range"),
        if_range: field("if-range"),
        browser_media: None,
        player_permit: None,
        browser_timing: None,
        browser_media_expires: None,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    #[serde(default)]
    source_time_basis: LiveSourceTimeBasis,
    media_us: i64,
    #[serde(default)]
    recording_maximum: Option<u16>,
    #[serde(default)]
    analysis: Option<BrowserAnalysisRequest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordingRead {
    epoch: AttemptId,
    #[serde(default)]
    after: u32,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordingIntent {
    epoch: AttemptId,
    request: RequestId,
    playback: AttemptId,
    revision: RevisionId,
}
// This index locates Engine-owned records; it carries neither samples nor authority.
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordingIndex {
    schema: u16,
    project: ProjectId,
    credential_binding: String,
    recordings: Vec<RecordingIntent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retired: Vec<RecordingIntent>,
}
impl RecordingIndex {
    fn validate(&self, project: &ProjectId, binding: &str) -> Result<()> {
        ensure!(
            self.schema == 1 && &self.project == project && self.credential_binding == binding,
            "recording index belongs to another project or credential scope"
        );
        ensure!(
            self.recordings.len() <= 64 && self.retired.len() <= 64,
            "recording index exceeds scope bound"
        );
        let mut epochs = std::collections::BTreeSet::new();
        let mut requests = std::collections::BTreeSet::new();
        for intent in self.recordings.iter().chain(&self.retired) {
            ensure!(
                epochs.insert(intent.epoch.as_str()) && requests.insert(intent.request.as_str()),
                "duplicate recording epoch or request in index"
            );
        }
        ensure!(
            !self.recordings.iter().any(|active| self
                .retired
                .iter()
                .any(|old| old.playback == active.playback)),
            "recording playback split between active index and history"
        );
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetireRecordingGroup {
    epoch: AttemptId,
    recording_request_id: RequestId,
    acknowledge_preserve_records: bool,
}
fn recording_namespace(project: &ProjectId) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "browser-recording-{:x}",
        Sha256::digest(project.as_str().as_bytes())
    )
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewPin {
    candidate: CandidateId,
    revision: RevisionId,
    program_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitApproval {
    candidate_id: CandidateId,
    base_revision: RevisionId,
    program_sha256: String,
    acknowledge: bool,
}
impl CommitApproval {
    fn validate(&self, pin: &ReviewPin) -> Result<()> {
        ensure!(self.acknowledge, "explicit review acknowledgement required");
        ensure!(
            self.candidate_id == pin.candidate
                && self.base_revision == pin.revision
                && self.program_sha256 == pin.program_sha256,
            "commit approval differs from reviewed candidate"
        );
        Ok(())
    }
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitIntent {
    request: RequestId,
    pin: ReviewPin,
    acknowledged_revision: Option<RevisionId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportApproval {
    revision: RevisionId,
    program_sha256: String,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportIntent {
    request: RequestId,
    revision: RevisionId,
    program_sha256: String,
    path: std::path::PathBuf,
    directory: PublicationDirectoryIdentity,
    expected: serde_json::Value,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedExport {
    schema: u32,
    project: ProjectId,
    credential_binding: String,
    intent: ExportIntent,
    acknowledged_revision: Option<RevisionId>,
    output_sha256: Option<String>,
}
impl SavedExport {
    fn validate_metadata(
        &self,
        project: &ProjectId,
        binding: &str,
        root: &std::path::Path,
    ) -> Result<()> {
        ensure!(
            self.schema == 1 && &self.project == project && self.credential_binding == binding,
            "export recovery project, schema or credential binding differs"
        );
        ensure!(
            self.intent.program_sha256.len() == 64
                && self
                    .intent
                    .program_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit()),
            "export recovery master digest differs"
        );
        ensure!(
            self.acknowledged_revision
                .is_none_or(|r| r == self.intent.revision),
            "export recovery acknowledgement revision differs"
        );
        ensure!(
            self.output_sha256
                .as_ref()
                .is_none_or(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
            "export recovery output digest differs"
        );
        let parent = self
            .intent
            .path
            .parent()
            .context("export recovery parent absent")?;
        ensure!(
            self.intent.path.is_absolute()
                && self.intent.path.file_name() == Some(std::ffi::OsStr::new("stroke.funscript"))
                && parent.parent() == Some(root)
                && parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("browser-export-")),
            "export recovery path is outside application-owned output storage"
        );
        Ok(())
    }
    fn validate(
        &self,
        project: &ProjectId,
        binding: &str,
        root: &std::path::Path,
    ) -> Result<std::fs::File> {
        self.validate_metadata(project, binding, root)?;
        open_export_directory(
            self.intent.path.parent().context("export parent absent")?,
            &self.intent.directory,
        )
    }
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetiredExport {
    project: ProjectId,
    credential_binding: String,
    request: RequestId,
    revision: RevisionId,
    program_sha256: String,
    path: std::path::PathBuf,
    directory: PublicationDirectoryIdentity,
    output_sha256: String,
}
impl RetiredExport {
    fn from_saved(saved: &SavedExport) -> Result<Self> {
        ensure!(
            saved.acknowledged_revision == Some(saved.intent.revision),
            "unconfirmed export cannot be retired"
        );
        Ok(Self {
            project: saved.project.clone(),
            credential_binding: saved.credential_binding.clone(),
            request: saved.intent.request.clone(),
            revision: saved.intent.revision,
            program_sha256: saved.intent.program_sha256.clone(),
            path: saved.intent.path.clone(),
            directory: saved.intent.directory,
            output_sha256: saved
                .output_sha256
                .clone()
                .context("download bytes not confirmed; preserve original export")?,
        })
    }
    fn metadata(&self) -> SavedExport {
        SavedExport {
            schema: 1,
            project: self.project.clone(),
            credential_binding: self.credential_binding.clone(),
            intent: ExportIntent {
                request: self.request.clone(),
                revision: self.revision,
                program_sha256: self.program_sha256.clone(),
                path: self.path.clone(),
                directory: self.directory,
                expected: serde_json::Value::Null,
            },
            acknowledged_revision: Some(self.revision),
            output_sha256: Some(self.output_sha256.clone()),
        }
    }
}
#[derive(Clone, serde::Serialize)]
struct ExportCatalog {
    format: u32,
    active: Option<SavedExport>,
    retired: Vec<RetiredExport>,
}
impl Default for ExportCatalog {
    fn default() -> Self {
        Self {
            format: 2,
            active: None,
            retired: vec![],
        }
    }
}
impl<'de> Deserialize<'de> for ExportCatalog {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Current {
            format: u32,
            active: Option<SavedExport>,
            retired: Vec<RetiredExport>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Current(Current),
            Legacy(Option<SavedExport>),
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Current(c) => Self {
                format: c.format,
                active: c.active,
                retired: c.retired,
            },
            Wire::Legacy(active) => Self {
                active,
                ..Self::default()
            },
        })
    }
}
impl ExportCatalog {
    fn validate(&self, project: &ProjectId, binding: &str, root: &std::path::Path) -> Result<()> {
        ensure!(
            self.format == 2 && self.retired.len() <= 64,
            "export catalog format or history bound differs"
        );
        let mut ids = std::collections::BTreeSet::new();
        if let Some(active) = &self.active {
            active.validate_metadata(project, binding, root)?;
            ids.insert(active.intent.request.as_str());
        }
        for retired in &self.retired {
            retired
                .metadata()
                .validate_metadata(project, binding, root)?;
            ensure!(
                ids.insert(retired.request.as_str()),
                "export catalog repeats original request identity"
            );
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetireExport {
    request_id: RequestId,
    revision: RevisionId,
    output_sha256: String,
    acknowledge_preserve_files: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryExportRead {
    request_id: RequestId,
}
fn retirement_receipt(entry: &RetiredExport) -> serde_json::Value {
    serde_json::json!({"retired_request_id":entry.request,"revision":entry.revision,"files_preserved":true,"qualified":false})
}
#[cfg(unix)]
fn read_export_side(directory: &std::fs::File) -> Result<SavedExport> {
    use std::os::unix::fs::MetadataExt;
    let file = std::fs::File::from(rustix::fs::openat(
        directory,
        "intent.json",
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?);
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == rustix::process::geteuid().as_raw()
            && meta.mode() & 0o077 == 0
            && meta.nlink() == 1
            && meta.len() <= 32 * 1024 * 1024,
        "retired export side journal shape differs"
    );
    let mut bytes = vec![];
    file.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 32 * 1024 * 1024,
        "retired export side journal exceeds bound"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
#[cfg(not(unix))]
fn read_export_side(_: &std::fs::File) -> Result<SavedExport> {
    bail!("export history requires pinned Unix storage")
}
#[cfg(unix)]
fn open_export_directory(
    path: &std::path::Path,
    identity: &PublicationDirectoryIdentity,
) -> Result<std::fs::File> {
    use std::os::unix::fs::MetadataExt;
    let directory = std::fs::File::from(rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?);
    let meta = directory.metadata()?;
    ensure!(
        meta.is_dir()
            && meta.uid() == rustix::process::geteuid().as_raw()
            && meta.mode() & 0o077 == 0
            && meta.dev() == identity.device
            && meta.ino() == identity.inode,
        "export output directory privacy or pinned identity differs"
    );
    Ok(directory)
}
#[cfg(not(unix))]
fn open_export_directory(
    _: &std::path::Path,
    _: &PublicationDirectoryIdentity,
) -> Result<std::fs::File> {
    bail!("browser export recovery requires pinned Unix publication storage")
}
#[cfg(unix)]
fn read_export_bytes(directory: &std::fs::File) -> Result<Vec<u8>> {
    use std::os::unix::fs::MetadataExt;
    let file = std::fs::File::from(rustix::fs::openat(
        directory,
        "stroke.funscript",
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?);
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == rustix::process::geteuid().as_raw()
            && meta.nlink() == 1
            && meta.len() <= 32 * 1024 * 1024,
        "export file shape, owner, links or length differ"
    );
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 32 * 1024 * 1024, "export read exceeds limit");
    Ok(bytes)
}
#[cfg(not(unix))]
fn read_export_bytes(_: &std::fs::File) -> Result<Vec<u8>> {
    bail!("browser export recovery requires pinned Unix publication storage")
}
fn export_namespace(project: &ProjectId) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "browser-export-{:x}",
        Sha256::digest(project.as_str().as_bytes())
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidatePrepare {
    epoch: AttemptId,
    range: TimeRange,
    maximum_interpolation_ns: u64,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRecordingCandidate {
    schema: u16,
    project: ProjectId,
    credential_binding: String,
    request: RequestId,
    revision: RevisionId,
    options: LiveRecordingStageOptions,
    attempted: bool,
    candidate: Option<CandidateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit: Option<CommitIntent>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    retired: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    history: Vec<RetiredProposal>,
}
impl SavedRecordingCandidate {
    fn validate(&self, project: &ProjectId, binding: &str) -> Result<()> {
        ensure!(
            self.schema == 1 && &self.project == project && self.credential_binding == binding,
            "recording candidate recovery scope differs"
        );
        self.options.validate()?;
        ensure!(
            self.candidate.is_none() || self.attempted,
            "recording candidate acknowledgement without original attempt"
        );
        let mut requests = std::collections::BTreeSet::new();
        ensure!(
            self.options
                .recordings
                .iter()
                .all(|r| requests.insert(r.recording_request_id.as_str())),
            "recording candidate repeats a recording request"
        );
        if let Some(commit) = &self.commit {
            ensure!(
                self.candidate.as_ref() == Some(&commit.pin.candidate)
                    && commit.pin.revision == self.revision
                    && commit.pin.program_sha256.len() == 64
                    && commit
                        .pin
                        .program_sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit()),
                "saved recording commit pin differs from original candidate"
            );
            ensure!(
                commit
                    .acknowledged_revision
                    .is_none_or(|r| self.revision.checked_next().is_ok_and(|next| next == r)),
                "saved recording commit acknowledgement differs from original revision"
            );
        }
        ensure!(
            self.history.len() <= 64,
            "recording candidate history exceeds bound"
        );
        let mut stages = std::collections::BTreeSet::new();
        let mut commits = std::collections::BTreeSet::new();
        for old in &self.history {
            old.validate()?;
            ensure!(
                stages.insert(old.request.as_str()) && commits.insert(old.commit_request.as_str()),
                "duplicate recording candidate history identity"
            );
            ensure!(
                self.retired || old.request != self.request,
                "active recording candidate already appears in retired history"
            );
        }
        if self.retired {
            let entry = RetiredProposal::from_saved(self)?;
            ensure!(
                self.history.iter().any(|old| old == &entry),
                "retired recording candidate receipt absent"
            );
        }
        Ok(())
    }
    fn into_intent(self) -> CandidateIntent {
        CandidateIntent {
            request: self.request,
            options: self.options,
            proposal: Proposal {
                revision: self.revision,
                candidate: self.candidate,
                reviewed: None,
                commit: self.commit,
            },
        }
    }
}
#[derive(Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetiredProposal {
    request: RequestId,
    candidate: CandidateId,
    base_revision: RevisionId,
    commit_request: RequestId,
    acknowledged_revision: RevisionId,
    program_sha256: String,
}
impl RetiredProposal {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.acknowledged_revision == self.base_revision.checked_next()?
                && self.program_sha256.len() == 64
                && self.program_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "retired recording candidate pin differs"
        );
        Ok(())
    }
    fn from_saved(saved: &SavedRecordingCandidate) -> Result<Self> {
        let commit = saved
            .commit
            .as_ref()
            .context("approved recording commit absent")?;
        let acknowledged = commit
            .acknowledged_revision
            .context("recording commit is not confirmed; retry original commit first")?;
        let entry = Self {
            request: saved.request.clone(),
            candidate: commit.pin.candidate.clone(),
            base_revision: saved.revision,
            commit_request: commit.request.clone(),
            acknowledged_revision: acknowledged,
            program_sha256: commit.pin.program_sha256.clone(),
        };
        entry.validate()?;
        Ok(entry)
    }
    fn from_gap(saved: &SavedGap) -> Result<Self> {
        let commit = saved
            .commit
            .as_ref()
            .context("approved gap commit absent")?;
        let acknowledged = commit
            .acknowledged_revision
            .context("gap commit is not confirmed; retry its original request")?;
        let entry = Self {
            request: saved.request.clone(),
            candidate: commit.pin.candidate.clone(),
            base_revision: saved.revision,
            commit_request: commit.request.clone(),
            acknowledged_revision: acknowledged,
            program_sha256: commit.pin.program_sha256.clone(),
        };
        entry.validate()?;
        Ok(entry)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetireProposal {
    request_id: RequestId,
    candidate_id: CandidateId,
    commit_request_id: RequestId,
    acknowledged_revision: RevisionId,
    acknowledge_preserve_history: bool,
}
impl RetireProposal {
    fn matches(&self, old: &RetiredProposal) -> bool {
        self.request_id == old.request
            && self.candidate_id == old.candidate
            && self.commit_request_id == old.commit_request
            && self.acknowledged_revision == old.acknowledged_revision
    }
}
fn candidate_namespace(project: &ProjectId) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "browser-candidate-{:x}",
        Sha256::digest(project.as_str().as_bytes())
    )
}
struct CandidateIntent {
    request: RequestId,
    options: LiveRecordingStageOptions,
    proposal: Proposal,
}
struct Proposal {
    revision: RevisionId,
    candidate: Option<CandidateId>,
    reviewed: Option<ReviewPin>,
    commit: Option<CommitIntent>,
}
impl std::ops::Deref for CandidateIntent {
    type Target = Proposal;
    fn deref(&self) -> &Proposal {
        &self.proposal
    }
}
impl std::ops::DerefMut for CandidateIntent {
    fn deref_mut(&mut self) -> &mut Proposal {
        &mut self.proposal
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GapApproval {
    revision: RevisionId,
    program_sha256: String,
    gaps: Vec<TimeRange>,
    acknowledge_linear_edge_hold: bool,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedGap {
    schema: u32,
    credential_binding: String,
    project: ProjectId,
    request: RequestId,
    revision: RevisionId,
    program_sha256: String,
    gaps: Vec<TimeRange>,
    program: MotionProgram,
    attempted: bool,
    upload: crate::motion::RetainedEditUpload,
    candidate: Option<CandidateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit: Option<CommitIntent>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    retired: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    history: Vec<RetiredProposal>,
}
impl SavedGap {
    fn validate(&self, project: &ProjectId, binding: &str) -> Result<()> {
        use sha2::{Digest, Sha256};
        ensure!(
            self.schema == 1 && &self.project == project && self.credential_binding == binding,
            "gap recovery schema, project or credential binding differs"
        );
        ensure!(
            self.upload.project == self.project && self.upload.revision == self.revision,
            "gap recovery upload scope differs"
        );
        ensure!(
            !self.gaps.is_empty()
                && self.gaps.len() <= 4096
                && self.gaps.windows(2).all(|g| g[0].end() <= g[1].start()),
            "gap recovery selection bounds/order differ"
        );
        ensure!(
            self.program_sha256.len() == 64
                && self.program_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "gap recovery master digest differs"
        );
        let bytes = serde_json::to_vec(&EditValuesProgram::from_program_values(&self.program)?)?;
        ensure!(
            self.upload.byte_len == bytes.len() as u64
                && self.upload.sha256 == format!("{:x}", Sha256::digest(&bytes)),
            "gap recovery values digest differs"
        );
        ensure!(
            !self.upload.finalization_attempted || self.upload.lease.is_some(),
            "gap recovery finalization lease absent"
        );
        ensure!(
            self.candidate.is_none() || self.upload.finalization_attempted,
            "gap recovery candidate precedes finalization"
        );
        ensure!(
            self.attempted || (self.upload.lease.is_none() && !self.upload.finalization_attempted),
            "gap recovery transfer precedes attempted phase"
        );
        if let Some(lease) = &self.upload.lease {
            lease.validate()?;
            ensure!(
                lease.project_id == self.project
                    && lease.direction == TransferDirection::Upload
                    && lease.sha256 == self.upload.sha256
                    && lease.total_byte_len == self.upload.byte_len
                    && lease.offset == 0
                    && lease.byte_len == self.upload.byte_len,
                "gap recovery lease binding differs"
            );
        }
        if let Some(commit) = &self.commit {
            ensure!(
                self.candidate.as_ref() == Some(&commit.pin.candidate)
                    && commit.pin.revision == self.revision
                    && commit.pin.program_sha256.len() == 64
                    && commit
                        .pin
                        .program_sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit()),
                "saved commit pin differs from original candidate"
            );
            ensure!(
                commit
                    .acknowledged_revision
                    .is_none_or(|r| self.revision.checked_next().is_ok_and(|next| next == r)),
                "saved commit acknowledgement differs from original revision"
            );
        }
        ensure!(
            self.history.len() <= 64,
            "gap proposal history exceeds bound"
        );
        let mut stages = std::collections::BTreeSet::new();
        let mut commits = std::collections::BTreeSet::new();
        for old in &self.history {
            old.validate()?;
            ensure!(
                stages.insert(old.request.as_str()) && commits.insert(old.commit_request.as_str()),
                "duplicate gap history identity"
            );
            ensure!(
                self.retired || old.request != self.request,
                "active gap already appears in retired history"
            );
        }
        if self.retired {
            let entry = RetiredProposal::from_gap(self)?;
            ensure!(
                self.history.iter().any(|old| old == &entry),
                "retired gap receipt absent"
            );
        }
        Ok(())
    }
    fn from_intent(i: &GapIntent, project: &ProjectId, binding: String) -> Self {
        Self {
            schema: 1,
            credential_binding: binding,
            project: project.clone(),
            request: i.request.clone(),
            revision: i.proposal.revision,
            program_sha256: i.program_sha256.clone(),
            gaps: i.gaps.clone(),
            program: i.program.clone(),
            attempted: i.attempted,
            upload: i.upload.clone(),
            candidate: i.proposal.candidate.clone(),
            commit: i.proposal.commit.clone(),
            retired: false,
            history: Vec::new(),
        }
    }
    fn into_intent(self) -> GapIntent {
        GapIntent {
            request: self.request,
            program_sha256: self.program_sha256,
            gaps: self.gaps,
            program: self.program,
            attempted: self.attempted,
            upload: self.upload,
            proposal: Proposal {
                revision: self.revision,
                candidate: self.candidate,
                reviewed: None,
                commit: self.commit,
            },
        }
    }
}
fn gap_namespace(project: &ProjectId) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "browser-gap-{:x}",
        Sha256::digest(project.as_str().as_bytes())
    )
}
struct GapIntent {
    request: RequestId,
    program_sha256: String,
    gaps: Vec<TimeRange>,
    program: MotionProgram,
    attempted: bool,
    upload: crate::motion::RetainedEditUpload,
    proposal: Proposal,
}
/// Explicit software lookahead selection; never timing qualification.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserAnalysisRequest {
    role: BrowserAnalysisRole,
    policy: BrowserAnalysisLookaheadPolicy,
    #[serde(default)]
    continuity: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BrowserAnalysisRole {
    Lookahead,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct BrowserAnalysisFeedBinding {
    pub feed_id: ArtifactId,
    pub role: BrowserAnalysisRole,
    pub generation: u64,
    pub player: HandyPlayerIdentity,
    pub capture_epoch: AttemptId,
    pub policy: BrowserAnalysisLookaheadPolicy,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrowserAnalysisFrameObservation {
    pub feed_id: ArtifactId,
    pub role: BrowserAnalysisRole,
    pub generation: u64,
    pub visible_media_us: i64,
    pub visible_presented_frames: u64,
    pub visible_tick_us: u64,
    pub analysis_tick_us: u64,
    pub presented_frames: u64,
}
type AnalysisFrameIdentity = BrowserAnalysisFrameObservation;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisSubmittedFrame {
    media_us: i64,
    observation: AnalysisFrameIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisFinishRequest {
    feed_id: ArtifactId,
    generation: u64,
    capture_epoch: AttemptId,
    final_observation: Option<AnalysisSubmittedFrame>,
}
struct AnalysisUploadReceipt {
    header: LiveFrameHeader,
    submitted: AnalysisSubmittedFrame,
}
struct AnalysisCloseIntent {
    request_id: RequestId,
    final_frame: Option<LiveFrameHeader>,
    verified_at: Option<Instant>,
}
/// A mode fixed from the first admission report, independent of refreshed status.
/// This binds metadata only; it cannot qualify estimates or create live authority.
#[derive(Default)]
struct AnalysisCpuMode {
    captured: Option<LiveCpuReplyBinding>,
}
impl AnalysisCpuMode {
    fn from_report(report: &LiveCaptureReport) -> Result<Self> {
        Ok(Self {
            captured: report
                .inference
                .as_ref()
                .context("analysis inference absent")?
                .cpu_manifest
                .as_deref()
                .map(LiveCpuInferenceManifest::reply_binding)
                .transpose()?,
        })
    }
    fn validate_report(&self, report: &LiveCaptureReport) -> Result<()> {
        let declared = report
            .inference
            .as_ref()
            .context("analysis inference absent")?
            .cpu_manifest
            .as_deref();
        let matches = match (&self.captured, declared) {
            (None, None) => true,
            (Some(expected), Some(current)) => {
                current.identity_sha256()? == expected.manifest_sha256()
            }
            _ => false,
        };
        ensure!(
            matches,
            "analysis captured CPU mode or original manifest changed"
        );
        Ok(())
    }
    fn estimated_input<'a>(&self, report: &'a LiveCaptureReport) -> Option<&'a LiveInferenceInput> {
        self.validate_report(report).ok()?;
        let reply = &report.inference.as_ref()?.latest.as_ref()?.result;
        match (&self.captured, reply) {
            (None, LiveInferenceReply::Estimated { input, .. }) => Some(input),
            (Some(expected), LiveInferenceReply::CapturedCpuEstimated { input, .. }) => {
                expected.validate_estimated(input, reply).ok()?;
                Some(input)
            }
            _ => None,
        }
    }
}
struct AnalysisFeed {
    binding: BrowserAnalysisFeedBinding,
    media: BrowserMediaBinding,
    media_expires: Option<Instant>,
    browser_timing: BrowserTimingBinding,
    permit: crate::browser_player_control::Permit,
    latest: Option<AnalysisFrameIdentity>,
    measured_lead_us: Option<u64>,
    last_upload: Option<AnalysisUploadReceipt>,
    close: Option<AnalysisCloseIntent>,
    capture_deadline: Option<Instant>,
    cpu_mode: AnalysisCpuMode,
    continuity: bool,
}
struct AnalysisSuccessor {
    request_id: RequestId,
    options: BrowserAnalysisSuccessorOptions,
    predecessor: LiveCaptureReport,
    feed: AnalysisFeed,
    admitted_before: Instant,
    report: Option<BrowserAnalysisSuccessorReport>,
    bulk: Option<LiveBulkClient>,
    transfer_id: Option<RequestId>,
    boundary: Option<LiveFrameHeader>,
    transfer: Option<BrowserAnalysisTransferReport>,
    pair_sent: bool,
}
/// Native response custody passed directly from Preview to the HTTP owner.
/// It is never deserialized from a browser request or minted by a status flag.
pub(super) struct AnalysisCommit {
    pub old_media: BrowserMediaBinding,
    pub new_media: BrowserMediaBinding,
    pub old_timing: BrowserTimingBinding,
    pub new_timing: BrowserTimingBinding,
    pub transfer: BrowserAnalysisTransferReport,
}
/// Exact active local feed/capture metadata for the future qualified stream
/// command. This snapshot neither contains credentials nor creates Engine authority.
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct BrowserAnalysisFeedSnapshot {
    pub binding: BrowserAnalysisFeedBinding,
    pub browser_media: BrowserMediaBinding,
    pub browser_timing: BrowserTimingBinding,
    pub engine_epoch: AttemptId,
    pub capture: LiveIdentity,
    pub inference_attempt_id: Option<AttemptId>,
    pub inference_profile: Option<DeclaredInferenceProfile>,
    pub estimated: bool,
    pub maximum_analysis_age_ns: u64,
    pub latest_observation: Option<BrowserAnalysisFrameObservation>,
    pub measured_lead_us: Option<u64>,
}
impl AnalysisFeed {
    fn declaration(&self) -> BrowserAnalysisCaptureDeclaration {
        BrowserAnalysisCaptureDeclaration {
            feed_id: self.binding.feed_id.clone(),
            player: self.binding.player.clone(),
            timing_binding_id: self.browser_timing.binding_id.clone(),
            policy: self.binding.policy,
        }
    }
    fn new(
        request: &Request,
        selection: BrowserAnalysisRequest,
        capture_epoch: AttemptId,
    ) -> Result<Self> {
        selection.policy.validate()?;
        let BrowserAnalysisRequest {
            role,
            policy,
            continuity,
        } = selection;
        let permit = request
            .player_permit
            .as_ref()
            .context("lookahead requires a native visible-player permit")?;
        ensure!(permit.current(), "lookahead visible player retired");
        let player = permit
            .identity
            .as_ref()
            .context("lookahead visible player absent")?;
        ensure!(
            player.time_basis == LiveSourceTimeBasis::SourceMediaPresentation,
            "lookahead requires a prepared source-media player"
        );
        let media = request
            .browser_media
            .as_ref()
            .context("lookahead requires exact prepared Engine media")?;
        media.validate()?;
        let timing = request
            .browser_timing
            .as_ref()
            .context("lookahead requires current native visible-player timing")?;
        timing.validate()?;
        ensure!(
            timing.player == *player && timing.browser_media == *media,
            "lookahead timing differs from native player/media"
        );
        use sha2::{Digest, Sha256};
        let feed_id = ArtifactId::new(format!(
            "{:x}",
            Sha256::digest(uuid::Uuid::new_v4().as_bytes())
        ))?;
        Ok(Self {
            binding: BrowserAnalysisFeedBinding {
                feed_id,
                role,
                generation: player.generation,
                player: player.clone(),
                capture_epoch,
                policy,
            },
            media: media.clone(),
            media_expires: request.browser_media_expires,
            browser_timing: timing.clone(),
            permit: permit.clone(),
            latest: None,
            measured_lead_us: None,
            last_upload: None,
            close: None,
            capture_deadline: None,
            cpu_mode: AnalysisCpuMode::default(),
            continuity,
        })
    }
    fn validate_scope(&self, request: &Request) -> Result<()> {
        ensure!(
            self.permit.current()
                && request.player_permit.as_ref().is_some_and(
                    |p| p.current() && p.identity.as_ref() == Some(&self.binding.player)
                )
                && request.browser_media.as_ref() == Some(&self.media)
                && request.browser_media_expires == self.media_expires
                && request.browser_timing.as_ref() == Some(&self.browser_timing),
            "lookahead native player/media/timing binding changed"
        );
        Ok(())
    }
    fn validate_finish(&self, request: &Request, finish: &AnalysisFinishRequest) -> Result<()> {
        self.validate_scope(request)?;
        ensure!(
            finish.feed_id == self.binding.feed_id
                && finish.generation == self.binding.generation
                && finish.capture_epoch == self.binding.capture_epoch,
            "input close belongs to another analysis feed"
        );
        ensure!(
            finish.final_observation.as_ref() == self.last_upload.as_ref().map(|u| &u.submitted),
            "input close does not name the final acknowledged browser upload"
        );
        Ok(())
    }
    fn valid_closed_input(&self, report: &LiveCaptureReport, now: Instant) -> bool {
        let Some(intent) = &self.close else {
            return false;
        };
        report.active
            && self.permit.current()
            && self.capture_deadline.is_some_and(|end| now < end)
            && intent.verified_at.is_some_and(|at| {
                now.checked_duration_since(at)
                    .is_some_and(|age| age <= Duration::from_secs(1))
            })
            && report.input_close.as_ref().is_some_and(|closed| {
                closed.original_request_id == intent.request_id
                    && closed.final_frame == intent.final_frame
                    && matches!(
                        closed.phase,
                        LiveCaptureInputPhase::Closing
                            | LiveCaptureInputPhase::Joining
                            | LiveCaptureInputPhase::Sealed
                    )
            })
    }
    fn final_result_matches(&self, report: &LiveCaptureReport) -> bool {
        let Some(upload) = &self.last_upload else {
            return false;
        };
        let Some(closed) = &report.input_close else {
            return false;
        };
        if closed.phase != LiveCaptureInputPhase::Sealed
            || closed.disposition != Some(LiveCaptureInputFinalDisposition::Accepted)
            || closed.final_frame.as_ref() != Some(&upload.header)
        {
            return false;
        }
        let Some(input) = self.cpu_mode.estimated_input(report) else {
            return false;
        };
        input.pair.current_frame == upload.header.frame
            && input.pair.current_time == upload.header.source_time
            && input.current.sha256 == upload.header.sha256
            && report
                .inference
                .as_ref()
                .and_then(|inference| inference.latest.as_ref())
                .is_some_and(|latest| latest.capture_tick == upload.header.capture_tick)
    }
    fn validate_frame(&self, request: &Request, frame: &Frame) -> Result<u64> {
        ensure!(
            self.close.is_none(),
            "analysis input is closing; new frames refused"
        );
        let a = frame
            .analysis
            .as_ref()
            .context("lookahead frame identity required")?;
        self.validate_scope(request)?;
        ensure!(
            a.feed_id == self.binding.feed_id
                && a.role == self.binding.role
                && a.generation == self.binding.generation
                && frame.epoch == self.binding.capture_epoch,
            "lookahead frame role or generation differs"
        );
        ensure!(
            a.visible_media_us >= 0
                && a.visible_media_us <= BROWSER_MEDIA_MAX_EXACT_INTEGER as i64
                && frame.media_us >= 0
                && frame.media_us <= BROWSER_MEDIA_MAX_EXACT_INTEGER as i64
                && a.visible_presented_frames > 0
                && a.presented_frames > 0
                && [
                    a.visible_tick_us,
                    a.analysis_tick_us,
                    a.visible_presented_frames,
                    a.presented_frames
                ]
                .iter()
                .all(|v| *v <= BROWSER_MEDIA_MAX_EXACT_INTEGER)
                && a.analysis_tick_us
                    .checked_sub(a.visible_tick_us)
                    .is_some_and(|age| age <= 1_000_000),
            "lookahead frame has no recent visible-player sample"
        );
        ensure!(
            self.latest
                .as_ref()
                .is_none_or(|old| a.presented_frames > old.presented_frames
                    && a.analysis_tick_us >= old.analysis_tick_us
                    && a.visible_presented_frames >= old.visible_presented_frames
                    && a.visible_tick_us >= old.visible_tick_us
                    && a.visible_media_us >= old.visible_media_us),
            "lookahead callback order regressed"
        );
        let lead = u64::try_from(
            frame
                .media_us
                .checked_sub(a.visible_media_us)
                .context("lookahead difference overflow")?,
        )?;
        ensure!(
            (self.binding.policy.minimum_lead_us..=self.binding.policy.maximum_lead_us)
                .contains(&lead),
            "observed analysis lead is outside the original policy"
        );
        Ok(lead)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    epoch: AttemptId,
    media_us: i64,
    #[serde(default)]
    analysis: Option<AnalysisFrameIdentity>,
}
struct Pending {
    report: LiveCaptureReport,
    bulk: Option<LiveBulkClient>,
    sequence: u64,
    eligible_after: Instant,
}
struct Preview {
    client: SessionClient,
    project: ProjectId,
    revision: RevisionId,
    options: LiveCaptureOptions,
    report: Option<LiveCaptureReport>,
    analysis_feed: Option<AnalysisFeed>,
    analysis_successor: Option<AnalysisSuccessor>,
    analysis_commit: Option<AnalysisCommit>,
    analysis_last_transfer: Option<BrowserAnalysisTransferReport>,
    analysis_cancelled: bool,
    bulk: Option<LiveBulkClient>,
    sequence: u64,
    last_time: Option<i64>,
    blocked: bool,
    playback_epoch: Option<AttemptId>,
    pending: Option<Pending>,
    eligible_after: Instant,
    handoffs: u64,
    transition_drops: u64,
    recording_maximum: Option<u16>,
    recordings: Vec<RecordingIntent>,
    retired_recordings: Vec<RecordingIntent>,
    candidate: Option<CandidateIntent>,
    export: Option<ExportIntent>,
    gap: Option<GapIntent>,
}
fn set_browser_capture_anchor(
    options: &mut LiveCaptureOptions,
    media_us: i64,
    basis: LiveSourceTimeBasis,
    binding: Option<&BrowserMediaBinding>,
) -> Result<()> {
    ensure!(
        media_us >= 0,
        "browser presentation time must be nonnegative"
    );
    options.source_anchor = SourceTimestamp::new(media_us, 1_000_000)?;
    options.source_time_basis = basis;
    options.browser_media = binding.cloned();
    options.browser_analysis = None;
    if let Some(binding) = binding {
        binding.validate()?;
        let exact = binding
            .clock
            .project_at(options.source_anchor, &binding.selection.placement.spec)?;
        options.project_anchor = exact.quantize_nanoseconds()?.time;
        options.exact_project_anchor = Some(exact);
        binding.validate_capture_anchor(
            basis,
            options.source_anchor,
            options.project_anchor,
            options.exact_project_anchor,
        )?;
    }
    Ok(())
}
impl Preview {
    fn operation_deadline(&mut self, deadline: Instant) -> Result<()> {
        ensure!(
            Instant::now() < deadline,
            "Browser operation deadline expired"
        );
        self.client.set_operation_deadline(Some(deadline));
        if let Some(bulk) = &mut self.bulk {
            bulk.set_operation_deadline(Some(deadline));
        }
        if let Some(bulk) = self.pending.as_mut().and_then(|p| p.bulk.as_mut()) {
            bulk.set_operation_deadline(Some(deadline));
        }
        Ok(())
    }
    fn recover_recording_candidate(&mut self) -> Result<()> {
        let directory = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let saved = crate::private_recovery::transaction(
            &directory,
            &candidate_namespace(&self.project),
            self.client.session_id(),
            None::<SavedRecordingCandidate>,
            |saved| {
                if let Some(saved) = saved {
                    saved.validate(&self.project, &binding)?;
                }
                Ok((saved.clone(), false))
            },
        )?;
        let prior = self.candidate.take();
        self.candidate = saved.filter(|saved| !saved.retired).map(|saved| {
            let mut recovered = saved.into_intent();
            if let Some(prior) = prior {
                if prior.request == recovered.request && prior.candidate == recovered.candidate {
                    recovered.proposal.reviewed = prior.proposal.reviewed;
                }
            }
            recovered
        });
        Ok(())
    }
    fn recover_recordings(&mut self) -> Result<()> {
        let directory = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let (active, retired) = crate::private_recovery::transaction(
            &directory,
            &recording_namespace(&self.project),
            self.client.session_id(),
            RecordingIndex {
                schema: 1,
                project: self.project.clone(),
                credential_binding: binding.clone(),
                recordings: Vec::new(),
                retired: Vec::new(),
            },
            |index| {
                index.validate(&self.project, &binding)?;
                Ok(((index.recordings.clone(), index.retired.clone()), false))
            },
        )?;
        self.recordings = active;
        self.retired_recordings = retired;
        Ok(())
    }
    fn active_proposal(&self) -> Result<&Proposal> {
        if let Some(gap) = &self.gap {
            return Ok(&gap.proposal);
        }
        Ok(&self
            .candidate
            .as_ref()
            .context("candidate absent")?
            .proposal)
    }
    fn active_proposal_mut(&mut self) -> Result<&mut Proposal> {
        if let Some(gap) = &mut self.gap {
            return Ok(&mut gap.proposal);
        }
        Ok(&mut self
            .candidate
            .as_mut()
            .context("candidate absent")?
            .proposal)
    }
    fn recover_gap(&mut self) -> Result<()> {
        let directory = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let saved = crate::private_recovery::transaction(
            &directory,
            &gap_namespace(&self.project),
            self.client.session_id(),
            None::<SavedGap>,
            |saved| {
                if let Some(i) = saved {
                    i.validate(&self.project, &binding)?;
                }
                Ok((saved.clone(), false))
            },
        )?;
        let prior = self.gap.take();
        self.gap = saved.filter(|saved| !saved.retired).map(|saved| {
            let mut recovered = saved.into_intent();
            if let Some(prior) = prior {
                if prior.request == recovered.request
                    && prior.proposal.candidate == recovered.proposal.candidate
                {
                    recovered.proposal.reviewed = prior.proposal.reviewed;
                }
            }
            recovered
        });
        Ok(())
    }
    fn gap_status(&mut self) -> Result<serde_json::Value> {
        self.recover_gap()?;
        let i = self.gap.as_ref().context("gap proposal not prepared")?;
        Ok(
            serde_json::json!({"request_id":i.request,"base_revision":i.proposal.revision,
            "gaps":i.gaps,"attempted":i.attempted,"candidate_id":i.proposal.candidate,
            "begin_request_id":i.upload.begin_request,"finish_request_id":i.upload.finish_request,
            "lease_id":i.upload.lease.as_ref().map(|l| &l.lease_id),
            "finalization_attempted":i.upload.finalization_attempted,
            "commit_request_id":i.proposal.commit.as_ref().map(|c| &c.request),
            "commit_acknowledged_revision":i.proposal.commit.as_ref().and_then(|c| c.acknowledged_revision),
            "qualified":false}),
        )
    }
    fn prepare_gaps(&mut self, input: GapApproval) -> Result<serde_json::Value> {
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before gap preparation"
        );
        ensure!(
            input.acknowledge_linear_edge_hold,
            "explicit linear-bridge/nearest-edge-hold acknowledgement required"
        );
        self.recover_gap()?;
        if let Some(i) = &self.gap {
            ensure!(
                input.revision == i.proposal.revision
                    && input.program_sha256 == i.program_sha256
                    && input.gaps == i.gaps,
                "different gap intent already retained"
            );
            return self.gap_status();
        }
        let p = crate::get_snapshot(&mut self.client, &self.project)?;
        ensure!(
            p.revision == input.revision && p.motion.program.sha256 == input.program_sha256,
            "committed master changed after gap inspection"
        );
        let ResponseBody::Diagnostics(d) = self.client.execute(
            Command::Diagnostics {
                candidate_id: None,
                maximum_speed_percent_per_second: None,
            },
            Some(self.project.clone()),
            None,
        )?
        else {
            bail!("gap protection response differs");
        };
        ensure!(
            d.project_id == self.project && d.revision == p.revision,
            "gap protection scope changed"
        );
        let recipe =
            resolve_selected_gaps(&p.program, Axis::Stroke, &input.gaps, &d.protected_regions)?;
        self.gap = Some(GapIntent {
            request: crate::session::new_request_id(),
            program_sha256: input.program_sha256,
            gaps: input.gaps,
            upload: crate::motion::RetainedEditUpload::new(
                self.project.clone(),
                p.revision,
                &recipe.program,
                format!(
                    "gap-resolution-v1 linear-interior/nearest-edge-hold; selected={}",
                    recipe.filled.len()
                ),
            )?,
            program: recipe.program,
            attempted: false,
            proposal: Proposal {
                revision: p.revision,
                candidate: None,
                reviewed: None,
                commit: None,
            },
        });
        let mut saved = SavedGap::from_intent(
            self.gap.as_ref().expect("prepared gap"),
            &self.project,
            self.client.recovery_credential_binding(),
        );
        saved.validate(&self.project, &saved.credential_binding)?;
        crate::private_recovery::transaction(
            &self.client.output_state_directory()?,
            &gap_namespace(&self.project),
            self.client.session_id(),
            None::<SavedGap>,
            |prior| {
                if let Some(old) = prior {
                    old.validate(&self.project, &saved.credential_binding)?;
                    ensure!(
                        old.retired,
                        "gap intent already retained; inspect original before replacement"
                    );
                    saved.history = old.history.clone();
                }
                saved.validate(&self.project, &saved.credential_binding)?;
                *prior = Some(saved);
                Ok(((), true))
            },
        )?;
        self.gap_status()
    }
    fn stage_gaps(&mut self) -> Result<serde_json::Value> {
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before gap staging"
        );
        self.recover_gap()?;
        let request = self
            .gap
            .as_ref()
            .context("prepare gap proposal first")?
            .request
            .clone();
        let directory = self.client.output_state_directory()?;
        let session = self.client.session_id().clone();
        let binding = self.client.recovery_credential_binding();
        let project = self.project.clone();
        let saved = crate::private_recovery::checkpoint_transaction(
            &directory,
            &gap_namespace(&project),
            &session,
            None::<SavedGap>,
            |journal, checkpoint| {
                let i = journal.as_mut().context("durable gap preparation absent")?;
                i.validate(&project, &binding)?;
                ensure!(!i.retired, "gap proposal intent is retired");
                ensure!(i.request == request, "durable original gap intent differs");
                if i.candidate.is_none() {
                    if !i.attempted {
                        let p = crate::get_snapshot(&mut self.client, &project)?;
                        ensure!(
                            p.revision == i.revision && p.motion.program.sha256 == i.program_sha256,
                            "committed master changed after gap preparation"
                        );
                        let ResponseBody::Diagnostics(d) = self.client.execute(
                            Command::Diagnostics {
                                candidate_id: None,
                                maximum_speed_percent_per_second: None,
                            },
                            Some(project.clone()),
                            None,
                        )?
                        else {
                            bail!("gap protection response differs");
                        };
                        ensure!(
                            d.project_id == project && d.revision == p.revision,
                            "gap protection scope changed"
                        );
                        let recipe = resolve_selected_gaps(
                            &p.program,
                            Axis::Stroke,
                            &i.gaps,
                            &d.protected_regions,
                        )?;
                        ensure!(recipe.program == i.program, "prepared gap recipe changed");
                        i.attempted = true;
                        checkpoint(&Some(i.clone()))?;
                    }
                    let program = i.program.clone();
                    let mut upload = i.upload.clone();
                    let c =
                        self.client
                            .upload_edit_retained(&mut upload, &program, &mut |state| {
                                i.upload = state.clone();
                                i.validate(&project, &binding)?;
                                checkpoint(&Some(i.clone()))
                            })?;
                    ensure!(
                        c.program.as_ref() == &program,
                        "prepared gap proposal values or evidence changed"
                    );
                    i.upload = upload;
                    i.candidate = Some(c.candidate_id);
                    checkpoint(&Some(i.clone()))?;
                }
                let id = i
                    .candidate
                    .as_ref()
                    .context("gap candidate outcome absent")?;
                let ResponseBody::Candidate(candidate) = self.client.execute(
                    Command::GetCandidate {
                        candidate_id: id.clone(),
                    },
                    Some(project.clone()),
                    None,
                )?
                else {
                    bail!("gap candidate readback differs");
                };
                ensure!(
                    candidate.candidate_id == *id
                        && candidate.project_id == project
                        && candidate.base_revision == i.revision
                        && candidate.job_id.is_none()
                        && candidate.program.as_ref() == &i.program,
                    "recovered candidate does not match original gap recipe"
                );
                Ok(i.clone())
            },
        )?;
        // Preserve an in-memory reviewed/commit pin when this is a confirmed repeat.
        let prior = self.gap.take();
        let mut recovered = saved.into_intent();
        if let Some(prior) = prior {
            if prior.proposal.candidate == recovered.proposal.candidate {
                recovered.proposal.reviewed = prior.proposal.reviewed;
            }
        }
        self.gap = Some(recovered);
        self.gap_status()
    }
    fn inspect_export(&mut self) -> Result<serde_json::Value> {
        let p = crate::get_snapshot(&mut self.client, &self.project)?;
        let track = p
            .program
            .track(Axis::Stroke)
            .context("committed Stroke axis absent")?;
        Ok(
            serde_json::json!({"revision":p.revision,"program_sha256":p.motion.program.sha256,
            "sample_count":track.actions().len(),"gaps":track.gaps(),"qualified":false}),
        )
    }
    fn export_status(&mut self) -> Result<serde_json::Value> {
        let root = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let saved = crate::private_recovery::transaction(
            &root,
            &export_namespace(&self.project),
            self.client.session_id(),
            ExportCatalog::default(),
            |journal| {
                journal.validate(&self.project, &binding, &root)?;
                let saved = journal
                    .active
                    .as_ref()
                    .context("export intent not prepared")?;
                saved.validate(&self.project, &binding, &root)?;
                Ok((saved.clone(), false))
            },
        )?;
        self.export = Some(saved.intent.clone());
        Ok(
            serde_json::json!({"request_id":saved.intent.request,"revision":saved.intent.revision,
            "acknowledged_revision":saved.acknowledged_revision,"output_sha256":saved.output_sha256,"qualified":false}),
        )
    }
    #[cfg(unix)]
    fn prepare_export(&mut self, approval: ExportApproval) -> Result<serde_json::Value> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before export preparation"
        );
        let root = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let project = self.project.clone();
        let session = self.client.session_id().clone();
        let saved = crate::private_recovery::transaction(
            &root,
            &export_namespace(&project),
            &session,
            ExportCatalog::default(),
            |journal| {
                journal.validate(&project, &binding, &root)?;
                if let Some(saved) = &journal.active {
                    saved.validate(&project, &binding, &root)?;
                    ensure!(saved.intent.revision == approval.revision && saved.intent.program_sha256 == approval.program_sha256,
                    "different export intent already retained; inspect the original before replacement");
                    return Ok((saved.clone(), false));
                }
                let p = crate::get_snapshot(&mut self.client, &project)?;
                ensure!(
                    p.revision == approval.revision
                        && p.motion.program.sha256 == approval.program_sha256,
                    "master changed after export inspection"
                );
                let expected = checked_script(&p.program)?;
                let temp = tempfile::Builder::new()
                    .prefix("browser-export-")
                    .permissions(std::fs::Permissions::from_mode(0o700))
                    .tempdir_in(&root)?;
                let meta = temp.path().metadata()?;
                let intent = ExportIntent {
                    request: crate::session::new_request_id(),
                    revision: approval.revision,
                    program_sha256: approval.program_sha256,
                    path: temp.path().join("stroke.funscript"),
                    directory: PublicationDirectoryIdentity {
                        device: meta.dev(),
                        inode: meta.ino(),
                    },
                    expected,
                };
                let saved = SavedExport {
                    schema: 1,
                    project: project.clone(),
                    credential_binding: binding.clone(),
                    intent,
                    acknowledged_revision: None,
                    output_sha256: None,
                };
                saved.validate(&project, &binding, &root)?;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(temp.path().join("intent.json"))?;
                file.write_all(&serde_json::to_vec(&saved)?)?;
                file.sync_all()?;
                std::fs::File::open(temp.path())?.sync_all()?;
                let _retained = temp.keep();
                journal.active = Some(saved.clone());
                Ok((saved, true))
            },
        )?;
        self.export = Some(saved.intent);
        self.export_status()
    }
    #[cfg(not(unix))]
    fn prepare_export(&mut self, _: ExportApproval) -> Result<serde_json::Value> {
        bail!("browser script download requires pinned Unix publication storage")
    }
    fn download_script(&mut self) -> Result<serde_json::Value> {
        use sha2::{Digest, Sha256};
        let root = self.client.output_state_directory()?;
        let project = self.project.clone();
        let session = self.client.session_id().clone();
        let binding = self.client.recovery_credential_binding();
        let (intent, script) = crate::private_recovery::checkpoint_transaction(
            &root,
            &export_namespace(&project),
            &session,
            ExportCatalog::default(),
            |journal, checkpoint| {
                journal.validate(&project, &binding, &root)?;
                let mut saved = journal
                    .active
                    .clone()
                    .context("inspect and prepare export first")?;
                saved.validate(&project, &binding, &root)?;
                let i = &saved.intent;
                let ResponseBody::AxisExported {
                    path,
                    revision,
                    axis,
                } = self.client.execute_identified(
                    i.request.clone(),
                    Command::ExportAxisPinned {
                        path: i.path.clone(),
                        axis: Axis::Stroke,
                        directory_identity: i.directory,
                    },
                    Some(project.clone()),
                    Some(i.revision),
                )?
                else {
                    bail!("Engine export acknowledgement differs");
                };
                ensure!(
                    path == i.path && revision == i.revision && axis == Axis::Stroke,
                    "Engine export scope differs"
                );
                saved.acknowledged_revision = Some(revision);
                journal.active = Some(saved.clone());
                checkpoint(journal)?;
                // Open the pinned parent again after acknowledgement; use descriptor-relative reads.
                let directory = saved.validate(&project, &binding, &root)?;
                let bytes = read_export_bytes(&directory)?;
                let hash = format!("{:x}", Sha256::digest(&bytes));
                ensure!(
                    saved.output_sha256.as_ref().is_none_or(|old| old == &hash),
                    "export bytes changed after confirmed download"
                );
                let script: serde_json::Value = serde_json::from_slice(&bytes)?;
                ensure!(
                    script == saved.intent.expected,
                    "exported script differs from pinned inspected master"
                );
                saved.output_sha256 = Some(hash);
                journal.active = Some(saved.clone());
                checkpoint(journal)?;
                Ok((saved.intent.clone(), script))
            },
        )?;
        self.export = Some(intent.clone());
        Ok(
            serde_json::json!({"request_id":intent.request,"revision":intent.revision,"qualified":false,"script":script}),
        )
    }
    fn retire_export(&mut self, input: RetireExport) -> Result<serde_json::Value> {
        use sha2::{Digest, Sha256};
        ensure!(
            input.acknowledge_preserve_files,
            "explicit preserve-files retirement acknowledgement required"
        );
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before export retirement"
        );
        let root = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let receipt = crate::private_recovery::transaction(
            &root,
            &export_namespace(&self.project),
            self.client.session_id(),
            ExportCatalog::default(),
            |journal| {
                journal.validate(&self.project, &binding, &root)?;
                if let Some(old) = journal
                    .retired
                    .iter()
                    .find(|e| e.request == input.request_id)
                {
                    ensure!(
                        old.revision == input.revision && old.output_sha256 == input.output_sha256,
                        "retirement replay differs from original export"
                    );
                    return Ok((retirement_receipt(old), false));
                }
                ensure!(
                    journal.retired.len() < 64,
                    "export history full; preserve original active intent"
                );
                let saved = journal.active.as_ref().context("active export absent")?;
                ensure!(
                    saved.intent.request == input.request_id
                        && saved.intent.revision == input.revision
                        && saved.output_sha256.as_deref() == Some(input.output_sha256.as_str()),
                    "retirement differs from confirmed original export"
                );
                let retired = RetiredExport::from_saved(saved)?;
                let directory = saved.validate(&self.project, &binding, &root)?;
                let bytes = read_export_bytes(&directory)?;
                ensure!(
                    format!("{:x}", Sha256::digest(&bytes)) == retired.output_sha256
                        && serde_json::from_slice::<serde_json::Value>(&bytes)?
                            == saved.intent.expected,
                    "confirmed export file changed; preserve active intent"
                );
                let receipt = retirement_receipt(&retired);
                journal.retired.push(retired);
                journal.active = None;
                Ok((receipt, true))
            },
        )?;
        self.export = None;
        Ok(receipt)
    }
    fn export_history(&self) -> Result<serde_json::Value> {
        let root = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        crate::private_recovery::transaction(
            &root,
            &export_namespace(&self.project),
            self.client.session_id(),
            ExportCatalog::default(),
            |journal| {
                journal.validate(&self.project, &binding, &root)?;
                Ok((
                    serde_json::json!({"exports":journal.retired.iter().map(|e| serde_json::json!({"request_id":e.request,"revision":e.revision,"output_sha256":e.output_sha256})).collect::<Vec<_>>(),"qualified":false}),
                    false,
                ))
            },
        )
    }
    fn download_history_export(&mut self, input: HistoryExportRead) -> Result<serde_json::Value> {
        use sha2::{Digest, Sha256};
        let root = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let session = self.client.session_id().clone();
        let project = self.project.clone();
        crate::private_recovery::checkpoint_transaction(
            &root,
            &export_namespace(&project),
            &session,
            ExportCatalog::default(),
            |journal, _| {
                journal.validate(&project, &binding, &root)?;
                let entry = journal
                    .retired
                    .iter()
                    .find(|e| e.request == input.request_id)
                    .context("retained export history request absent")?;
                let directory = entry.metadata().validate(&project, &binding, &root)?;
                let original = read_export_side(&directory)?;
                original.validate_metadata(&project, &binding, &root)?;
                ensure!(
                    original.intent.request == entry.request
                        && original.intent.revision == entry.revision
                        && original.intent.path == entry.path
                        && original.intent.directory == entry.directory
                        && original.intent.program_sha256 == entry.program_sha256,
                    "retained side journal differs from original export"
                );
                let bytes = read_export_bytes(&directory)?;
                ensure!(
                    format!("{:x}", Sha256::digest(&bytes)) == entry.output_sha256,
                    "retained export bytes changed; no replacement publication requested"
                );
                let script: serde_json::Value = serde_json::from_slice(&bytes)?;
                ensure!(
                    script == original.intent.expected,
                    "retained script differs from original projection"
                );
                let ResponseBody::AxisExported {
                    path,
                    revision,
                    axis,
                } = self.client.execute_identified(
                    entry.request.clone(),
                    Command::ExportAxisPinned {
                        path: entry.path.clone(),
                        axis: Axis::Stroke,
                        directory_identity: entry.directory,
                    },
                    Some(project.clone()),
                    Some(entry.revision),
                )?
                else {
                    bail!("retained export Engine acknowledgement differs");
                };
                ensure!(
                    path == entry.path && revision == entry.revision && axis == Axis::Stroke,
                    "retained export Engine scope differs"
                );
                Ok(
                    serde_json::json!({"request_id":entry.request,"revision":entry.revision,"qualified":false,"script":script}),
                )
            },
        )
    }
    fn gap_history(&mut self) -> Result<serde_json::Value> {
        let binding = self.client.recovery_credential_binding();
        crate::private_recovery::transaction(
            &self.client.output_state_directory()?,
            &gap_namespace(&self.project),
            self.client.session_id(),
            None::<SavedGap>,
            |saved| {
                if let Some(saved) = saved {
                    saved.validate(&self.project, &binding)?;
                }
                Ok((
                    serde_json::json!({"proposals":saved.as_ref().map(|s|s.history.clone()).unwrap_or_default(),"qualified":false}),
                    false,
                ))
            },
        )
    }
    fn retire_gap(&mut self, input: RetireProposal) -> Result<serde_json::Value> {
        ensure!(
            input.acknowledge_preserve_history,
            "explicit history-preservation acknowledgement required"
        );
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before candidate retirement"
        );
        let binding = self.client.recovery_credential_binding();
        let project = self.project.clone();
        let session = self.client.session_id().clone();
        let receipt = crate::private_recovery::transaction(
            &self.client.output_state_directory()?,
            &gap_namespace(&project),
            &session,
            None::<SavedGap>,
            |saved| {
                let saved = saved.as_mut().context("gap proposal intent absent")?;
                saved.validate(&project, &binding)?;
                if let Some(old) = saved
                    .history
                    .iter()
                    .find(|old| old.request == input.request_id)
                {
                    ensure!(
                        input.matches(old),
                        "retirement differs from original receipt"
                    );
                    return Ok((old.clone(), false));
                }
                ensure!(
                    !saved.retired && saved.history.len() < 64,
                    "gap proposal history is full or retired"
                );
                let receipt = RetiredProposal::from_gap(saved)?;
                ensure!(
                    input.matches(&receipt),
                    "retirement differs from confirmed recording commit"
                );
                let ResponseBody::Candidate(candidate) = self.client.execute(
                    Command::GetCandidate {
                        candidate_id: receipt.candidate.clone(),
                    },
                    Some(project.clone()),
                    None,
                )?
                else {
                    bail!("candidate retirement readback differs");
                };
                let current = crate::get_snapshot(&mut self.client, &project)?;
                ensure!(
                    candidate.project_id == project
                        && candidate.candidate_id == receipt.candidate
                        && candidate.base_revision == receipt.base_revision
                        && candidate.motion.program.sha256 == receipt.program_sha256
                        && candidate.job_id.is_none()
                        && candidate.program.as_ref() == &saved.program
                        && current.project_id == project
                        && current.revision >= receipt.acknowledged_revision
                        && (current.revision != receipt.acknowledged_revision
                            || current.motion.program.sha256 == receipt.program_sha256),
                    "confirmed gap proposal differs from current Engine readback"
                );
                saved.history.push(receipt.clone());
                saved.retired = true;
                saved.validate(&project, &binding)?;
                Ok((receipt, true))
            },
        )?;
        self.recover_gap()?;
        Ok(serde_json::json!({"retired":receipt,"qualified":false}))
    }
    fn candidate_history(&mut self) -> Result<serde_json::Value> {
        let binding = self.client.recovery_credential_binding();
        crate::private_recovery::transaction(
            &self.client.output_state_directory()?,
            &candidate_namespace(&self.project),
            self.client.session_id(),
            None::<SavedRecordingCandidate>,
            |saved| {
                if let Some(saved) = saved {
                    saved.validate(&self.project, &binding)?;
                }
                Ok((
                    serde_json::json!({"candidates":saved.as_ref().map(|s|s.history.clone()).unwrap_or_default(),"qualified":false}),
                    false,
                ))
            },
        )
    }
    fn retire_recording_candidate(&mut self, input: RetireProposal) -> Result<serde_json::Value> {
        ensure!(
            input.acknowledge_preserve_history,
            "explicit history-preservation acknowledgement required"
        );
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before candidate retirement"
        );
        let binding = self.client.recovery_credential_binding();
        let project = self.project.clone();
        let session = self.client.session_id().clone();
        let receipt = crate::private_recovery::transaction(
            &self.client.output_state_directory()?,
            &candidate_namespace(&project),
            &session,
            None::<SavedRecordingCandidate>,
            |saved| {
                let saved = saved
                    .as_mut()
                    .context("recording candidate intent absent")?;
                saved.validate(&project, &binding)?;
                if let Some(old) = saved
                    .history
                    .iter()
                    .find(|old| old.request == input.request_id)
                {
                    ensure!(
                        input.matches(old),
                        "retirement differs from original receipt"
                    );
                    return Ok((old.clone(), false));
                }
                ensure!(
                    !saved.retired && saved.history.len() < 64,
                    "recording candidate history is full or retired"
                );
                let receipt = RetiredProposal::from_saved(saved)?;
                ensure!(
                    input.matches(&receipt),
                    "retirement differs from confirmed recording commit"
                );
                let ResponseBody::Candidate(candidate) = self.client.execute(
                    Command::GetCandidate {
                        candidate_id: receipt.candidate.clone(),
                    },
                    Some(project.clone()),
                    None,
                )?
                else {
                    bail!("candidate retirement readback differs");
                };
                let current = crate::get_snapshot(&mut self.client, &project)?;
                ensure!(
                    candidate.project_id == project
                        && candidate.candidate_id == receipt.candidate
                        && candidate.base_revision == receipt.base_revision
                        && candidate.motion.program.sha256 == receipt.program_sha256
                        && candidate.job_id.is_none()
                        && current.project_id == project
                        && current.revision >= receipt.acknowledged_revision
                        && (current.revision != receipt.acknowledged_revision
                            || current.motion.program.sha256 == receipt.program_sha256),
                    "confirmed recording candidate differs from current Engine readback"
                );
                saved.history.push(receipt.clone());
                saved.retired = true;
                saved.validate(&project, &binding)?;
                Ok((receipt, true))
            },
        )?;
        self.recover_recording_candidate()?;
        Ok(serde_json::json!({"retired":receipt,"qualified":false}))
    }
    fn prepare_candidate(&mut self, input: CandidatePrepare) -> Result<serde_json::Value> {
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before preparing a candidate"
        );
        let playback = self
            .recordings
            .iter()
            .find(|r| r.epoch == input.epoch)
            .context("select a recording retained by this browser server")?
            .playback
            .clone();
        let selections: Vec<_> = self
            .recordings
            .iter()
            .filter(|r| r.playback == playback)
            .map(|r| LiveRecordingSelection {
                epoch: r.epoch.clone(),
                recording_request_id: r.request.clone(),
            })
            .collect();
        let first = self.recording_page(&selections[0].epoch, 0)?;
        let options = LiveRecordingStageOptions {
            engine_epoch: first.engine_epoch,
            recordings: selections,
            range: input.range,
            maximum_interpolation_ns: input.maximum_interpolation_ns,
        };
        options.validate()?;
        let binding = self.client.recovery_credential_binding();
        let directory = self.client.output_state_directory()?;
        crate::private_recovery::transaction(
            &directory,
            &candidate_namespace(&self.project),
            self.client.session_id(),
            None::<SavedRecordingCandidate>,
            |saved| {
                if let Some(intent) = saved.as_ref() {
                    intent.validate(&self.project, &binding)?;
                }
                if let Some(intent) = saved.as_ref().filter(|intent| !intent.retired) {
                    intent.validate(&self.project, &binding)?;
                    ensure!(serde_json::to_vec(&intent.options)? == serde_json::to_vec(&options)
                        ? && intent.revision == first.identity.revision,
                        "a different candidate intent is already retained; preserve or reconcile it first");
                    return Ok(((), false));
                }
                let history = saved
                    .as_ref()
                    .map(|i| i.history.clone())
                    .unwrap_or_default();
                *saved = Some(SavedRecordingCandidate {
                    schema: 1,
                    project: self.project.clone(),
                    credential_binding: binding.clone(),
                    request: crate::session::new_request_id(),
                    revision: first.identity.revision,
                    options,
                    attempted: false,
                    candidate: None,
                    commit: None,
                    retired: false,
                    history,
                });
                saved
                    .as_ref()
                    .expect("prepared candidate")
                    .validate(&self.project, &binding)?;
                Ok(((), true))
            },
        )?;
        self.candidate_status()
    }
    fn candidate_status(&mut self) -> Result<serde_json::Value> {
        self.recover_recording_candidate()?;
        let intent = self
            .candidate
            .as_ref()
            .context("candidate intent not prepared")?;
        Ok(
            serde_json::json!({"request_id":intent.request,"options":intent.options,
            "base_revision":intent.revision,"candidate_id":intent.candidate,"qualified":false,
            "commit_request_id":intent.commit.as_ref().map(|c| &c.request),
            "commit_acknowledged_revision":intent.commit.as_ref().and_then(|c| c.acknowledged_revision)}),
        )
    }
    fn stage_candidate(&mut self) -> Result<serde_json::Value> {
        ensure!(
            self.gap.is_none(),
            "gap proposal is active; inspect and review it before another recording proposal"
        );
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before staging a candidate"
        );
        let directory = self.client.output_state_directory()?;
        let session = self.client.session_id().clone();
        let binding = self.client.recovery_credential_binding();
        let project = self.project.clone();
        let candidate = crate::private_recovery::checkpoint_transaction(
            &directory,
            &candidate_namespace(&project),
            &session,
            None::<SavedRecordingCandidate>,
            |journal, checkpoint| {
                let intent = journal.as_mut().context("prepare candidate intent first")?;
                intent.validate(&project, &binding)?;
                ensure!(!intent.retired, "recording candidate intent is retired");
                intent.attempted = true;
                checkpoint(&Some(intent.clone()))?;
                let ResponseBody::Candidate(candidate) = self.client.execute_identified(
                    intent.request.clone(),
                    Command::StageLiveRecording {
                        options: intent.options.clone(),
                    },
                    Some(project.clone()),
                    Some(intent.revision),
                )?
                else {
                    bail!("candidate response differs");
                };
                ensure!(
                    candidate.base_revision == intent.revision
                        && candidate.project_id == project
                        && candidate.job_id.is_none(),
                    "candidate scope differs from prepared intent"
                );
                ensure!(
                    intent
                        .candidate
                        .as_ref()
                        .is_none_or(|id| id == &candidate.candidate_id),
                    "retained staging reply changed candidate identity"
                );
                intent.candidate = Some(candidate.candidate_id.clone());
                checkpoint(&Some(intent.clone()))?;
                Ok(candidate)
            },
        )?;
        self.recover_recording_candidate()?;
        let track = candidate
            .program
            .tracks()
            .iter()
            .find(|t| t.axis() == Axis::Stroke)
            .context("candidate Stroke track absent")?;
        Ok(
            serde_json::json!({"request_id":self.candidate.as_ref().expect("intent").request,
            "candidate_id":candidate.candidate_id,"base_revision":candidate.base_revision,
            "sample_count":track.actions().len(),"gaps":track.gaps(),"review":candidate.review,
            "samples":track.actions().iter().take(64).collect::<Vec<_>>(),"qualified":false}),
        )
    }
    fn review_candidate(&mut self) -> Result<serde_json::Value> {
        if self.gap.is_none() {
            self.recover_recording_candidate()?;
        }
        let id = self
            .active_proposal()?
            .candidate
            .clone()
            .context("stage a candidate first")?;
        self.active_proposal_mut()?.reviewed = None;
        let ResponseBody::Candidate(candidate) = self.client.execute(
            Command::GetCandidate {
                candidate_id: id.clone(),
            },
            Some(self.project.clone()),
            None,
        )?
        else {
            bail!("candidate readback differs")
        };
        ensure!(
            candidate.candidate_id == id && candidate.project_id == self.project,
            "candidate review scope differs"
        );
        let ResponseBody::Diagnostics(report) = self.client.execute(
            Command::Diagnostics {
                candidate_id: Some(id.clone()),
                maximum_speed_percent_per_second: None,
            },
            Some(self.project.clone()),
            None,
        )?
        else {
            bail!("candidate diagnostics response differs");
        };
        ensure!(
            report.candidate_id.as_ref() == Some(&id) && report.project_id == self.project,
            "candidate diagnostics scope differs"
        );
        let ready = report.base_revision == Some(candidate.base_revision)
            && report.revision == candidate.base_revision
            && candidate.base_revision == self.active_proposal()?.revision;
        if ready {
            self.active_proposal_mut()?.reviewed = Some(ReviewPin {
                candidate: id.clone(),
                revision: candidate.base_revision,
                program_sha256: candidate.motion.program.sha256.clone(),
            });
        }
        Ok(
            serde_json::json!({"candidate_id":id,"base_revision":candidate.base_revision,
            "program_sha256":candidate.motion.program.sha256,"commit_ready":ready,"qualified":false,"diagnostics":report}),
        )
    }
    fn prepare_commit(&mut self, approval: CommitApproval) -> Result<serde_json::Value> {
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before commit preparation"
        );
        ensure!(
            approval.acknowledge,
            "explicit review acknowledgement required"
        );
        let intent = self.active_proposal()?;
        let pin = intent
            .reviewed
            .as_ref()
            .context("review current candidate diagnostics before commit")?
            .clone();
        approval.validate(&pin)?;
        if let Some(commit) = &intent.commit {
            ensure!(
                commit.pin.candidate == pin.candidate
                    && commit.pin.revision == pin.revision
                    && commit.pin.program_sha256 == pin.program_sha256,
                "different commit intent already retained"
            );
            return if self.gap.is_some() {
                self.gap_status()
            } else {
                self.candidate_status()
            };
        }
        let ResponseBody::Candidate(candidate) = self.client.execute(
            Command::GetCandidate {
                candidate_id: pin.candidate.clone(),
            },
            Some(self.project.clone()),
            None,
        )?
        else {
            bail!("candidate preflight response differs")
        };
        let current = crate::get_snapshot(&mut self.client, &self.project)?;
        ensure!(
            candidate.candidate_id == pin.candidate
                && candidate.project_id == self.project
                && candidate.base_revision == pin.revision
                && candidate.motion.program.sha256 == pin.program_sha256
                && current.revision == pin.revision
                && current.project_id == self.project,
            "candidate or project changed after review; refresh diagnostics"
        );
        self.active_proposal_mut()?.commit = Some(CommitIntent {
            request: crate::session::new_request_id(),
            pin,
            acknowledged_revision: None,
        });
        if let Some(gap) = &self.gap {
            let request = gap.request.clone();
            let commit = gap.proposal.commit.clone().expect("prepared commit");
            let binding = self.client.recovery_credential_binding();
            crate::private_recovery::transaction(
                &self.client.output_state_directory()?,
                &gap_namespace(&self.project),
                self.client.session_id(),
                None::<SavedGap>,
                |journal| {
                    let saved = journal.as_mut().context("durable gap intent absent")?;
                    saved.validate(&self.project, &binding)?;
                    ensure!(!saved.retired, "gap proposal intent is retired");
                    ensure!(
                        saved.request == request
                            && saved.candidate.as_ref() == Some(&commit.pin.candidate)
                            && saved.program == *candidate.program,
                        "durable gap candidate differs from reviewed proposal"
                    );
                    if let Some(prior) = &saved.commit {
                        ensure!(
                            prior.request == commit.request
                                && serde_json::to_vec(&prior.pin)?
                                    == serde_json::to_vec(&commit.pin)?,
                            "different approved commit identity already retained"
                        );
                        return Ok(((), false));
                    }
                    saved.commit = Some(commit);
                    saved.validate(&self.project, &binding)?;
                    Ok(((), true))
                },
            )?;
            self.gap_status()
        } else {
            let intent = self
                .candidate
                .as_ref()
                .context("recording candidate absent")?;
            let request = intent.request.clone();
            let commit = intent
                .proposal
                .commit
                .clone()
                .context("prepared recording commit absent")?;
            let binding = self.client.recovery_credential_binding();
            crate::private_recovery::transaction(
                &self.client.output_state_directory()?,
                &candidate_namespace(&self.project),
                self.client.session_id(),
                None::<SavedRecordingCandidate>,
                |journal| {
                    let saved = journal
                        .as_mut()
                        .context("durable recording candidate absent")?;
                    saved.validate(&self.project, &binding)?;
                    ensure!(!saved.retired, "recording candidate intent is retired");
                    ensure!(
                        saved.request == request
                            && saved.candidate.as_ref() == Some(&commit.pin.candidate)
                            && saved.revision == commit.pin.revision
                            && candidate.job_id.is_none(),
                        "durable recording candidate differs from reviewed proposal"
                    );
                    if let Some(prior) = &saved.commit {
                        ensure!(
                            serde_json::to_vec(&prior.pin)? == serde_json::to_vec(&commit.pin)?,
                            "different approved recording commit already retained"
                        );
                        return Ok(((), false));
                    }
                    saved.commit = Some(commit);
                    saved.validate(&self.project, &binding)?;
                    Ok(((), true))
                },
            )?;
            self.candidate_status()
        }
    }
    fn commit_candidate(&mut self) -> Result<serde_json::Value> {
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before committing"
        );
        if self.gap.is_none() && self.candidate.is_none() {
            self.recover_gap()?;
        }
        if self.gap.is_some() {
            return self.commit_saved_gap();
        }
        self.commit_saved_recording()
    }
    fn commit_saved_recording(&mut self) -> Result<serde_json::Value> {
        let directory = self.client.output_state_directory()?;
        let session = self.client.session_id().clone();
        let binding = self.client.recovery_credential_binding();
        let project_id = self.project.clone();
        let (saved, acknowledged, remaining_gaps) =
            crate::private_recovery::checkpoint_transaction(
                &directory,
                &candidate_namespace(&project_id),
                &session,
                None::<SavedRecordingCandidate>,
                |journal, checkpoint| {
                    let saved = journal
                        .as_mut()
                        .context("durable recording candidate absent")?;
                    saved.validate(&project_id, &binding)?;
                    ensure!(!saved.retired, "recording candidate intent is retired");
                    let commit = saved
                        .commit
                        .clone()
                        .context("prepare an explicitly acknowledged original commit first")?;
                    let ResponseBody::Candidate(candidate) = self.client.execute(
                        Command::GetCandidate {
                            candidate_id: commit.pin.candidate.clone(),
                        },
                        Some(project_id.clone()),
                        None,
                    )?
                    else {
                        bail!("saved commit candidate readback differs");
                    };
                    ensure!(
                        candidate.project_id == project_id
                            && candidate.candidate_id == commit.pin.candidate
                            && candidate.base_revision == commit.pin.revision
                            && candidate.motion.program.sha256 == commit.pin.program_sha256
                            && candidate.job_id.is_none(),
                        "saved commit differs from authenticated original recording candidate"
                    );
                    let ResponseBody::Project(project) = self.client.execute_identified(
                        commit.request.clone(),
                        Command::CommitCandidate {
                            candidate_id: commit.pin.candidate.clone(),
                        },
                        Some(project_id.clone()),
                        Some(commit.pin.revision),
                    )?
                    else {
                        bail!("saved commit response differs");
                    };
                    ensure!(
                        project.project_id == project_id
                            && project.revision == commit.pin.revision.checked_next()?
                            && project.motion.program.sha256 == commit.pin.program_sha256,
                        "saved commit acknowledgement scope or program differs"
                    );
                    saved
                        .commit
                        .as_mut()
                        .expect("saved commit")
                        .acknowledged_revision = Some(project.revision);
                    checkpoint(&Some(saved.clone()))?;
                    Ok((
                        saved.clone(),
                        project.revision,
                        project
                            .program
                            .tracks()
                            .iter()
                            .map(|t| t.gaps().len())
                            .sum::<usize>(),
                    ))
                },
            )?;
        let request = saved.commit.as_ref().expect("saved commit").request.clone();
        self.candidate = Some(saved.into_intent());
        let current = crate::get_snapshot(&mut self.client, &self.project)?;
        ensure!(
            current.project_id == self.project,
            "current project differs after saved commit"
        );
        self.revision = current.revision;
        Ok(
            serde_json::json!({"commit_request_id":request,"acknowledged_revision":acknowledged,
            "current_revision":current.revision,"remaining_gaps":remaining_gaps,"qualified":false}),
        )
    }
    fn commit_saved_gap(&mut self) -> Result<serde_json::Value> {
        let directory = self.client.output_state_directory()?;
        let session = self.client.session_id().clone();
        let binding = self.client.recovery_credential_binding();
        let project_id = self.project.clone();
        let (saved, acknowledged, remaining_gaps) =
            crate::private_recovery::checkpoint_transaction(
                &directory,
                &gap_namespace(&project_id),
                &session,
                None::<SavedGap>,
                |journal, checkpoint| {
                    let saved = journal.as_mut().context("durable gap intent absent")?;
                    saved.validate(&project_id, &binding)?;
                    ensure!(!saved.retired, "gap proposal intent is retired");
                    let commit = saved
                        .commit
                        .clone()
                        .context("prepare an explicitly acknowledged original commit first")?;
                    let ResponseBody::Candidate(candidate) = self.client.execute(
                        Command::GetCandidate {
                            candidate_id: commit.pin.candidate.clone(),
                        },
                        Some(project_id.clone()),
                        None,
                    )?
                    else {
                        bail!("saved commit candidate readback differs");
                    };
                    ensure!(
                        candidate.project_id == project_id
                            && candidate.candidate_id == commit.pin.candidate
                            && candidate.base_revision == commit.pin.revision
                            && candidate.motion.program.sha256 == commit.pin.program_sha256
                            && candidate.program.as_ref() == &saved.program
                            && candidate.job_id.is_none(),
                        "saved commit differs from authenticated original gap candidate"
                    );
                    let ResponseBody::Project(project) = self.client.execute_identified(
                        commit.request.clone(),
                        Command::CommitCandidate {
                            candidate_id: commit.pin.candidate.clone(),
                        },
                        Some(project_id.clone()),
                        Some(commit.pin.revision),
                    )?
                    else {
                        bail!("saved commit response differs");
                    };
                    ensure!(
                        project.project_id == project_id
                            && project.revision == commit.pin.revision.checked_next()?
                            && project.motion.program.sha256 == commit.pin.program_sha256,
                        "saved commit acknowledgement scope or program differs"
                    );
                    saved
                        .commit
                        .as_mut()
                        .expect("saved commit")
                        .acknowledged_revision = Some(project.revision);
                    checkpoint(&Some(saved.clone()))?;
                    Ok((
                        saved.clone(),
                        project.revision,
                        project
                            .program
                            .tracks()
                            .iter()
                            .map(|t| t.gaps().len())
                            .sum::<usize>(),
                    ))
                },
            )?;
        let request = saved.commit.as_ref().expect("saved commit").request.clone();
        self.gap = Some(saved.into_intent());
        let current = crate::get_snapshot(&mut self.client, &self.project)?;
        ensure!(
            current.project_id == self.project,
            "current project differs after saved commit"
        );
        self.revision = current.revision;
        Ok(
            serde_json::json!({"commit_request_id":request,"acknowledged_revision":acknowledged,
            "current_revision":current.revision,"remaining_gaps":remaining_gaps,"qualified":false}),
        )
    }
    fn command(&mut self, command: Command) -> Result<LiveCaptureReport> {
        self.command_identified(crate::session::new_request_id(), command)
    }
    fn command_identified(&mut self, id: RequestId, command: Command) -> Result<LiveCaptureReport> {
        let ResponseBody::LiveCapture(r) = self.client.execute_identified(
            id,
            command,
            Some(self.project.clone()),
            Some(self.revision),
        )?
        else {
            bail!("unexpected capture reply")
        };
        r.validate()?;
        Ok(*r)
    }
    fn capture_status(&mut self, expected: &LiveCaptureReport) -> Result<LiveCaptureReport> {
        let report = self.command(Command::LiveCaptureStatus {
            epoch: expected.identity.epoch.clone(),
        })?;
        ensure!(
            report.identity == expected.identity
                && report.engine_epoch == expected.engine_epoch
                && report.clock == expected.clock
                && report.options.source_time_basis == expected.options.source_time_basis
                && report.options.browser_media == expected.options.browser_media
                && report.options.browser_analysis == expected.options.browser_analysis,
            "capture status subject differs; preserve original outcomes"
        );
        for feed in self
            .analysis_feed
            .iter()
            .chain(self.analysis_successor.iter().map(|next| &next.feed))
        {
            if feed.binding.capture_epoch == report.identity.epoch {
                feed.cpu_mode.validate_report(&report)?;
            }
        }
        Ok(report)
    }
    fn begin_recording(&mut self, report: &LiveCaptureReport, playback: AttemptId) -> Result<()> {
        let Some(maximum_samples) = self.recording_maximum else {
            return Ok(());
        };
        let id = crate::session::new_request_id();
        let directory = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        // Persist the original identity before Begin; loss never permits replacement.
        self.recordings = crate::private_recovery::transaction(
            &directory,
            &recording_namespace(&self.project),
            self.client.session_id(),
            RecordingIndex {
                schema: 1,
                project: self.project.clone(),
                credential_binding: binding.clone(),
                recordings: Vec::new(),
                retired: Vec::new(),
            },
            |index| {
                index.validate(&self.project, &binding)?;
                ensure!(index.recordings.len() < 64,
                    "browser retained recording limit reached; preserve recordings before a new run");
                index.recordings.push(RecordingIntent {
                    epoch: report.identity.epoch.clone(),
                    request: id.clone(),
                    playback,
                    revision: report.identity.revision,
                });
                index.validate(&self.project, &binding)?;
                Ok((index.recordings.clone(), true))
            },
        )?;
        let ResponseBody::LiveRecordingPage(page) = self.client.execute_identified(
            id.clone(),
            Command::StartLiveRecording {
                epoch: report.identity.epoch.clone(),
                maximum_samples,
            },
            Some(self.project.clone()),
            Some(self.revision),
        )?
        else {
            bail!("unexpected recording acknowledgement");
        };
        page.validate()?;
        ensure!(
            page.identity == report.identity
                && page.recording_request_id == id
                && !page.discarded
                && page.total_samples == 0,
            "recording acknowledgement differs"
        );
        Ok(())
    }
    fn recording_page(&mut self, epoch: &AttemptId, after: u32) -> Result<LiveRecordingPage> {
        let intent = self
            .recordings
            .iter()
            .chain(&self.retired_recordings)
            .find(|r| &r.epoch == epoch)
            .context("recording belongs to another preview server")?;
        let request = intent.request.clone();
        let revision = intent.revision;
        let ResponseBody::LiveRecordingPage(page) = self.client.execute(
            Command::LiveRecordingPage {
                epoch: epoch.clone(),
                after,
                limit: 64,
            },
            Some(self.project.clone()),
            Some(revision),
        )?
        else {
            bail!("unexpected recording readback");
        };
        page.validate()?;
        ensure!(
            page.identity.epoch == *epoch
                && page.identity.project == self.project
                && page.identity.revision == revision
                && page.recording_request_id == request
                && page.after == after,
            "recording readback scope differs"
        );
        Ok(*page)
    }
    fn recording_history(&mut self) -> Result<serde_json::Value> {
        self.recover_recordings()?;
        Ok(serde_json::json!({"recordings":self.retired_recordings,"qualified":false}))
    }
    fn retire_recordings(&mut self, input: RetireRecordingGroup) -> Result<serde_json::Value> {
        ensure!(
            input.acknowledge_preserve_records,
            "explicit recording-preservation acknowledgement required"
        );
        ensure!(
            self.report.is_none() && self.pending.is_none(),
            "stop preview before recording-index retirement"
        );
        self.recover_recordings()?;
        let directory = self.client.output_state_directory()?;
        let binding = self.client.recovery_credential_binding();
        let session = self.client.session_id().clone();
        let project = self.project.clone();
        let retired = crate::private_recovery::transaction(
            &directory,
            &recording_namespace(&project),
            &session,
            RecordingIndex {
                schema: 1,
                project: project.clone(),
                credential_binding: binding.clone(),
                recordings: Vec::new(),
                retired: Vec::new(),
            },
            |index| {
                index.validate(&project, &binding)?;
                if let Some(old) = index.retired.iter().find(|r| r.epoch == input.epoch) {
                    ensure!(
                        old.request == input.recording_request_id,
                        "retirement differs from original recording request"
                    );
                    return Ok((
                        index
                            .retired
                            .iter()
                            .filter(|r| r.playback == old.playback)
                            .cloned()
                            .collect::<Vec<_>>(),
                        false,
                    ));
                }
                let selected = index
                    .recordings
                    .iter()
                    .find(|r| r.epoch == input.epoch)
                    .context("active recording selection absent")?;
                ensure!(
                    selected.request == input.recording_request_id,
                    "retirement differs from original recording request"
                );
                let playback = selected.playback.clone();
                let group = index
                    .recordings
                    .iter()
                    .filter(|r| r.playback == playback)
                    .cloned()
                    .collect::<Vec<_>>();
                ensure!(
                    index.retired.len() + group.len() <= 64,
                    "recording history full; preserve active index"
                );
                for record in &group {
                    let page = self.recording_page(&record.epoch, 0)?;
                    ensure!(
                        !page.active
                            && !page.discarded
                            && page.recording_request_id == record.request,
                        "recording must be owned, stopped and retained before index retirement"
                    );
                }
                index.recordings.retain(|r| r.playback != playback);
                index.retired.extend(group.clone());
                index.validate(&project, &binding)?;
                Ok((group, true))
            },
        )?;
        self.recover_recordings()?;
        Ok(serde_json::json!({"retired":retired,"qualified":false}))
    }
    fn recordings_view(&mut self) -> Result<serde_json::Value> {
        self.recover_recordings()?;
        let scopes: Vec<_> = self
            .recordings
            .iter()
            .map(|r| (r.epoch.clone(), r.playback.clone()))
            .collect();
        let mut summaries = Vec::new();
        for (epoch, playback) in scopes {
            match self.recording_page(&epoch,0) {
                Ok(page)=>summaries.push(serde_json::json!({"epoch":epoch,"playback_epoch":playback,"model_pack":page.model_pack,"source_time_basis":page.source_time_basis,"total_samples":page.total_samples,"maximum_samples":page.maximum_samples,"active":page.active,"discarded":page.discarded,"exhausted":page.exhausted,"retirement":page.retirement,"qualified":false})),
                Err(error)=>summaries.push(serde_json::json!({"epoch":epoch,"playback_epoch":playback,"unknown":true,"error":error.to_string().chars().take(500).collect::<String>(),"qualified":false})),
            }
        }
        Ok(serde_json::json!({"schema":1,"qualified":false,"recordings":summaries}))
    }
    fn recording_archive(&mut self, epoch: &AttemptId) -> Result<serde_json::Value> {
        let mut pages = Vec::new();
        let mut cursor = 0;
        let mut bytes = 0usize;
        let mut first = None;
        for _ in 0..64 {
            let page = self.recording_page(epoch, cursor)?;
            ensure!(
                !page.active && !page.discarded,
                "stop the capture before saving retained recording"
            );
            // Preserve all declaration fields, including fallback and captured
            // runtime provenance, while allowing only the page cursor to move.
            let mut header = serde_json::to_value(&page)?;
            let fields = header
                .as_object_mut()
                .context("recording page header absent")?;
            for key in ["after", "next", "samples"] {
                fields.remove(key);
            }
            let signature = serde_json::to_vec(&header)?;
            if let Some(expected) = &first {
                ensure!(*expected == signature, "recording changed during readback");
            } else {
                first = Some(signature);
            }
            bytes = bytes
                .checked_add(serde_json::to_vec(&page)?.len())
                .context("recording archive overflow")?;
            ensure!(
                bytes <= 32 * 1024 * 1024,
                "recording archive exceeds bounded admission"
            );
            let next = page.next;
            pages.push(page);
            if let Some(next) = next {
                ensure!(next > cursor, "recording cursor did not advance");
                cursor = next;
            } else {
                return Ok(
                    serde_json::json!({"schema":1,"qualified":false,"kind":"pulsar-live-inference-recording","pages":pages,"limitations":["Inferred/provisional estimates; no physical qualification","No raw media/pixels or final funscript; preserve explicit missing coverage during candidate assembly"]}),
                );
            }
        }
        bail!("recording page count exceeds bounded admission")
    }
    fn prepare_analysis_successor(&mut self, request: &Request) -> Result<serde_json::Value> {
        ensure!(request.body.is_empty(), "analysis preparation body refused");
        let old = self
            .analysis_feed
            .as_ref()
            .context("analysis feed absent")?;
        ensure!(
            old.continuity && old.close.is_none() && old.permit.current(),
            "analysis continuity was not selected or input closed"
        );
        let prior = self.report.as_ref().context("capture absent")?.clone();
        ensure!(
            prior.active && old.capture_deadline.is_some_and(|end| Instant::now() < end),
            "analysis predecessor expired"
        );
        if self.analysis_successor.is_none() {
            let feed = AnalysisFeed::new(
                request,
                BrowserAnalysisRequest {
                    role: old.binding.role,
                    policy: old.binding.policy,
                    continuity: true,
                },
                AttemptId::new("pending-analysis-successor")?,
            )?;
            ensure!(
                feed.binding.player == old.binding.player
                    && feed.media.selection == old.media.selection
                    && feed.media.clock == old.media.clock
                    && feed.media != old.media
                    && feed.browser_timing != old.browser_timing,
                "analysis successor source or player differs"
            );
            ensure!(
                self.recording_maximum.is_none() || self.recordings.len() < 64,
                "recording retention full"
            );
            let options = BrowserAnalysisSuccessorOptions {
                browser_media: feed.media.clone(),
                browser_analysis: feed.declaration(),
                lease_ms: prior.options.lease_ms,
            };
            options.validate()?;
            self.analysis_successor = Some(AnalysisSuccessor {
                request_id: crate::session::new_request_id(),
                options,
                predecessor: prior,
                feed,
                admitted_before: Instant::now(),
                report: None,
                bulk: None,
                transfer_id: None,
                boundary: None,
                transfer: None,
                pair_sent: false,
            });
        }
        let next = self.analysis_successor.as_ref().expect("retained prepare");
        next.feed.validate_scope(request)?;
        let id = next.request_id.clone();
        let predecessor = next.predecessor.identity.clone();
        let options = next.options.clone();
        let command = Command::PrepareBrowserAnalysisSuccessor {
            engine_epoch: next.predecessor.engine_epoch.clone(),
            predecessor: predecessor.epoch.clone(),
            options: options.clone(),
        };
        let ResponseBody::BrowserAnalysisSuccessor(report) = self.client.execute_identified(
            id.clone(),
            command,
            Some(self.project.clone()),
            Some(self.revision),
        )?
        else {
            bail!("unexpected analysis successor report")
        };
        report.validate()?;
        ensure!(
            report.original_request_id == id
                && report.predecessor == predecessor
                && report.successor.options.browser_media.as_ref() == Some(&options.browser_media)
                && report.successor.options.browser_analysis.as_ref()
                    == Some(&options.browser_analysis)
                && report.successor.options.lease_ms == options.lease_ms,
            "analysis successor report identity or options differ"
        );
        ensure!(
            report.phase != BrowserAnalysisSuccessorPhase::Aborted,
            "analysis successor aborted: {:?}",
            report.successor.retirement
        );
        let first = self.analysis_successor.as_ref().unwrap().report.is_none();
        {
            let next = self.analysis_successor.as_mut().unwrap();
            if let Some(old) = &next.report {
                ensure!(
                    old.successor.identity == report.successor.identity
                        && old.successor.clock == report.successor.clock,
                    "successor identity changed"
                );
            }
            if first {
                next.feed.cpu_mode = AnalysisCpuMode::from_report(&report.successor)?;
            } else {
                next.feed.cpu_mode.validate_report(&report.successor)?;
            }
            next.feed.binding.capture_epoch = report.successor.identity.epoch.clone();
            next.feed.capture_deadline = Some(
                next.admitted_before
                    .checked_add(Duration::from_millis(u64::from(options.lease_ms)))
                    .context("successor deadline overflow")?,
            );
            next.report = Some((*report).clone());
        }
        if first {
            // Record native outcome before auxiliary IO. A recording failure
            // keeps B addressable for Stop and cannot create a second intent.
            self.blocked = true;
            self.begin_recording(
                &report.successor,
                self.playback_epoch
                    .clone()
                    .context("playback origin absent")?,
            )?;
            self.blocked = false;
        }
        self.view()
    }
    fn cancel_analysis_successor(&mut self, request: &Request) -> Result<serde_json::Value> {
        ensure!(
            request.body.is_empty(),
            "analysis cancellation body refused"
        );
        self.analysis_feed
            .as_ref()
            .context("analysis absent")?
            .validate_scope(request)?;
        if let Some(next) = &self.analysis_successor {
            ensure!(
                next.transfer_id.is_none(),
                "reserved transfer must settle or Stop"
            );
            let successor = next
                .report
                .as_ref()
                .context("original preparation must be inspected before cancellation")?
                .successor
                .identity
                .epoch
                .clone();
            let stopped = self.command(Command::StopLiveCapture { epoch: successor })?;
            ensure!(!stopped.active, "successor stop unconfirmed");
            self.analysis_successor = None;
        }
        self.analysis_cancelled = true;
        self.view()
    }
    fn transfer_analysis(&mut self, request: &Request, inspect: bool) -> Result<serde_json::Value> {
        ensure!(request.body.is_empty(), "analysis transfer body refused");
        self.analysis_feed
            .as_ref()
            .context("analysis absent")?
            .validate_scope(request)?;
        if inspect && self.analysis_successor.is_none() && self.analysis_last_transfer.is_some() {
            return self.view();
        }
        let next = self
            .analysis_successor
            .as_mut()
            .context("no original analysis successor")?;
        let successor = next
            .report
            .as_ref()
            .context("successor preparation unconfirmed")?
            .successor
            .identity
            .clone();
        if next.transfer_id.is_none() {
            ensure!(
                !inspect
                    && next
                        .report
                        .as_ref()
                        .is_some_and(|r| r.phase == BrowserAnalysisSuccessorPhase::Prepared),
                "successor is not prepared"
            );
            let boundary = self
                .analysis_feed
                .as_ref()
                .and_then(|f| f.last_upload.as_ref())
                .context("no original boundary upload")?
                .header
                .clone();
            ensure!(
                self.report.as_ref().and_then(|r| r.last_frame.as_ref()) == Some(&boundary),
                "boundary is not final native upload"
            );
            next.transfer_id = Some(crate::session::new_request_id());
            next.boundary = Some(boundary);
        }
        let id = next.transfer_id.clone().unwrap();
        let boundary = next.boundary.clone().unwrap();
        let predecessor = next.predecessor.identity.clone();
        let prepare = next.request_id.clone();
        let command = if inspect {
            Command::BrowserAnalysisTransferStatus {
                transfer_request_id: id.clone(),
            }
        } else {
            Command::TransferBrowserAnalysisInput {
                predecessor: predecessor.epoch.clone(),
                successor: successor.epoch.clone(),
                boundary: boundary.clone(),
            }
        };
        let request_id = if inspect {
            crate::session::new_request_id()
        } else {
            id.clone()
        };
        let ResponseBody::BrowserAnalysisTransfer(report) = self.client.execute_identified(
            request_id,
            command,
            Some(self.project.clone()),
            if inspect { None } else { Some(self.revision) },
        )?
        else {
            bail!("unexpected analysis transfer reply")
        };
        report.validate()?;
        ensure!(
            report.original_request_id == id
                && report.preparation_request_id == prepare
                && report.predecessor == predecessor
                && report.successor == successor
                && report.boundary == boundary,
            "analysis transfer changed original subject"
        );
        ensure!(
            report.phase != BrowserAnalysisTransferPhase::Aborted,
            "original analysis transfer aborted: {:?}",
            report.error
        );
        let transferred = report.phase == BrowserAnalysisTransferPhase::Transferred;
        self.analysis_successor.as_mut().unwrap().transfer = Some(*report);
        if transferred {
            self.promote_analysis_successor()?;
        } else {
            let next = self.analysis_successor.as_mut().unwrap();
            if next.bulk.is_none()
                && next.transfer.as_ref().is_some_and(|transfer| {
                    transfer.predecessor_worker_joined && transfer.predecessor_delivery_settled
                })
            {
                // Capture G only after A has actually joined: an early accepted
                // G can otherwise age out while predecessor custody retires.
                let prepared = next
                    .report
                    .as_ref()
                    .context("successor preparation absent")?;
                next.bulk = Some(self.client.open_live_bulk(&prepared.successor)?);
            }
        }
        self.view()
    }
    fn promote_analysis_successor(&mut self) -> Result<()> {
        let next = self
            .analysis_successor
            .as_ref()
            .context("successor absent")?;
        let transfer = next.transfer.as_ref().context("transfer absent")?;
        ensure!(
            transfer.phase == BrowserAnalysisTransferPhase::Transferred
                && transfer.predecessor_worker_joined
                && transfer.predecessor_delivery_settled
                && transfer.successor_pair_accepted
                && next.pair_sent,
            "native source transfer did not commit"
        );
        let expected = next.report.as_ref().unwrap().successor.clone();
        let active = self.capture_status(&expected)?;
        ensure!(
            active.active
                && active
                    .inference
                    .as_ref()
                    .is_some_and(|i| i.state == LiveCaptureInferenceState::Running),
            "successor is not actually active"
        );
        let next = self.analysis_successor.take().unwrap();
        self.analysis_last_transfer = next.transfer.clone();
        let old = self
            .analysis_feed
            .replace(next.feed)
            .context("predecessor feed absent")?;
        let new = self.analysis_feed.as_ref().unwrap();
        self.analysis_commit = Some(AnalysisCommit {
            old_media: old.media,
            new_media: new.media.clone(),
            old_timing: old.browser_timing,
            new_timing: new.browser_timing.clone(),
            transfer: next.transfer.unwrap(),
        });
        self.report = Some(active);
        self.bulk = next.bulk;
        self.sequence = 2;
        self.handoffs = self
            .handoffs
            .checked_add(1)
            .context("handoff count overflow")?;
        Ok(())
    }
    fn successor_analysis_frame(&mut self, request: &Request) -> Result<serde_json::Value> {
        let split = request
            .body
            .iter()
            .position(|b| *b == b'\n')
            .context("frame header absent")?;
        ensure!(
            split <= 1024 && request.body.len() - split - 1 == LIVE_FRAME_BYTES,
            "successor frame size differs"
        );
        let frame: Frame = serde_json::from_slice(&request.body[..split])?;
        let next = self
            .analysis_successor
            .as_mut()
            .context("successor absent")?;
        ensure!(
            !next.pair_sent
                && next
                    .transfer
                    .as_ref()
                    .is_some_and(|t| t.predecessor_worker_joined
                        && t.predecessor_delivery_settled
                        && matches!(
                            t.phase,
                            BrowserAnalysisTransferPhase::BoundaryReserved
                                | BrowserAnalysisTransferPhase::JoiningPredecessor
                                | BrowserAnalysisTransferPhase::AwaitingSuccessorPair
                        )),
            "successor pair already submitted, transfer not reserved, or predecessor join pending"
        );
        ensure!(
            self.last_time.is_none_or(|t| frame.media_us > t),
            "successor PTS did not advance"
        );
        let lead = next.feed.validate_frame(request, &frame)?;
        // Latch before IO: an unknown G upload is never replaced or retried.
        next.pair_sent = true;
        let upload = next
            .bulk
            .as_mut()
            .context("successor bulk absent")?
            .upload_captured(
                FrameId::new(1),
                SourceTimestamp::new(frame.media_us, 1_000_000)?,
                request.received,
                &request.body[split + 1..],
            )?;
        next.feed.last_upload = Some(AnalysisUploadReceipt {
            header: upload.stamp.header,
            submitted: AnalysisSubmittedFrame {
                media_us: frame.media_us,
                observation: frame
                    .analysis
                    .clone()
                    .context("successor observation absent")?,
            },
        });
        next.feed.latest = frame.analysis;
        next.feed.measured_lead_us = Some(lead);
        next.report.as_mut().unwrap().successor = upload.report;
        self.last_time = Some(frame.media_us);
        self.view()
    }
    fn finish_analysis(&mut self, request: &Request, inspect: bool) -> Result<serde_json::Value> {
        ensure!(
            self.analysis_successor.is_none(),
            "settle the original transfer before final input close"
        );
        let finish: AnalysisFinishRequest = serde_json::from_slice(&request.body)?;
        let feed = self
            .analysis_feed
            .as_ref()
            .context("no original analysis feed")?;
        feed.validate_finish(request, &finish)?;
        let prior = self
            .report
            .as_ref()
            .context("capture report absent")?
            .clone();
        ensure!(
            prior.active && prior.identity.epoch == feed.binding.capture_epoch,
            "original analysis capture retired"
        );
        let final_frame = feed.last_upload.as_ref().map(|u| u.header.clone());
        ensure!(
            prior.last_frame == final_frame,
            "final native ingress receipt differs"
        );
        ensure!(
            feed.capture_deadline
                .is_some_and(|end| Instant::now() < end),
            "original analysis capture deadline elapsed"
        );
        if inspect {
            ensure!(feed.close.is_some(), "no original input-close request");
        } else if feed.close.is_none() {
            self.analysis_feed.as_mut().expect("checked feed").close = Some(AnalysisCloseIntent {
                request_id: crate::session::new_request_id(),
                final_frame,
                verified_at: None,
            });
            // Keep the original transport/capture alive; the close latch fences further ingress.
        }
        let intent = self
            .analysis_feed
            .as_ref()
            .expect("checked feed")
            .close
            .as_ref()
            .expect("original close");
        let id = intent.request_id.clone();
        let expected_final = intent.final_frame.clone();
        let report = if inspect {
            self.capture_status(&prior)?
        } else {
            self.command_identified(
                id.clone(),
                Command::CloseBrowserAnalysisInput {
                    epoch: prior.identity.epoch.clone(),
                    final_frame: expected_final.clone(),
                },
            )?
        };
        ensure!(
            report.identity == prior.identity
                && report.engine_epoch == prior.engine_epoch
                && report.clock == prior.clock
                && report.options.browser_analysis == prior.options.browser_analysis
                && report.options.browser_media == prior.options.browser_media
                && report.last_frame == expected_final,
            "input-close report changed original capture subject"
        );
        if let Some(closed) = &report.input_close {
            ensure!(
                closed.original_request_id == id && closed.final_frame == expected_final,
                "input-close report belongs to another original request"
            );
        } else {
            ensure!(inspect, "input close was not acknowledged");
        }
        self.analysis_feed
            .as_mut()
            .expect("original feed")
            .close
            .as_mut()
            .expect("original close")
            .verified_at = Some(request.received);
        self.report = Some(report);
        self.view()
    }
    fn requires_frame_heartbeat(&self) -> bool {
        !self
            .analysis_feed
            .as_ref()
            .zip(self.report.as_ref())
            .is_some_and(|(feed, report)| feed.valid_closed_input(report, Instant::now()))
    }
    fn stop(&mut self) -> Result<()> {
        let mut failure = None;
        if let Some(next) = &self.analysis_successor {
            let transfer_id = next.transfer_id.clone();
            let successor = next
                .report
                .as_ref()
                .map(|r| r.successor.identity.epoch.clone());
            let cancelled = if let Some(id) = transfer_id {
                self.client
                    .execute(
                        Command::CancelBrowserAnalysisTransfer {
                            transfer_request_id: id.clone(),
                        },
                        Some(self.project.clone()),
                        None,
                    )
                    .and_then(|reply| match reply {
                        ResponseBody::BrowserAnalysisTransfer(report) => {
                            report.validate()?;
                            ensure!(
                                report.original_request_id == id
                                    && report.phase == BrowserAnalysisTransferPhase::Aborted,
                                "transfer cancellation unconfirmed"
                            );
                            Ok(())
                        }
                        _ => bail!("unexpected transfer cancellation reply"),
                    })
            } else if let Some(epoch) = successor {
                self.command(Command::StopLiveCapture { epoch })
                    .and_then(|report| {
                        ensure!(!report.active, "successor stop unconfirmed");
                        Ok(())
                    })
            } else {
                Err(anyhow::anyhow!(
                    "original successor preparation outcome remains unknown"
                ))
            };
            match cancelled {
                Ok(()) => self.analysis_successor = None,
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        self.analysis_commit = None;
        self.analysis_last_transfer = None;
        self.analysis_cancelled = false;
        self.analysis_feed = None;
        self.bulk = None;
        if let Some(pending) = self.pending.as_mut() {
            pending.bulk = None;
        }
        if let Some(expected) = self.pending.as_ref().map(|p| p.report.clone()) {
            let result = self.command(Command::StopLiveCapture {
                epoch: expected.identity.epoch.clone(),
            });
            match result {
                Ok(r)
                    if !r.active
                        && r.identity == expected.identity
                        && r.engine_epoch == expected.engine_epoch
                        && r.clock == expected.clock =>
                {
                    self.pending = None
                }
                Ok(_) => {
                    failure
                        .get_or_insert_with(|| anyhow::anyhow!("pending capture stop not acknowledged"));
                }
                Err(e) => {
                    failure.get_or_insert(e);
                }
            }
        }
        if let Some(expected) = self.report.clone() {
            let epoch = expected.identity.epoch.clone();
            let result = self.command(Command::StopLiveCapture { epoch });
            match result {
                Ok(r)
                    if !r.active
                        && r.identity == expected.identity
                        && r.engine_epoch == expected.engine_epoch
                        && r.clock == expected.clock =>
                {
                    self.report = None;
                }
                Ok(_) => {
                    failure.get_or_insert_with(|| {
                        anyhow::anyhow!("capture stop acknowledgement scope or state differs")
                    });
                }
                Err(e) => {
                    failure.get_or_insert(e);
                }
            }
        }
        if let Some(error) = failure {
            self.blocked = true;
            return Err(error);
        }
        self.last_time = None;
        self.playback_epoch = None;
        Ok(())
    }
    pub(crate) fn active_analysis_feed(&self) -> Result<Option<BrowserAnalysisFeedSnapshot>> {
        let Some(feed) = &self.analysis_feed else {
            return Ok(None);
        };
        let Some(report) = &self.report else {
            return Ok(None);
        };
        if self.blocked || !report.active || !feed.permit.current() {
            return Ok(None);
        }
        ensure!(
            report.options.browser_media.as_ref() == Some(&feed.media)
                && report.options.browser_analysis.as_ref() == Some(&feed.declaration())
                && report.identity.epoch == feed.binding.capture_epoch
                && self.playback_epoch.is_some(),
            "active analysis feed differs from its exact capture/source"
        );
        feed.cpu_mode.validate_report(report)?;
        Ok(Some(BrowserAnalysisFeedSnapshot {
            binding: feed.binding.clone(),
            browser_media: feed.media.clone(),
            browser_timing: feed.browser_timing.clone(),
            engine_epoch: report.engine_epoch.clone(),
            capture: report.identity.clone(),
            inference_attempt_id: report
                .inference
                .as_ref()
                .map(|inference| inference.attempt_id.clone()),
            inference_profile: report
                .options
                .inference
                .as_ref()
                .map(|inference| inference.profile.clone()),
            estimated: feed.cpu_mode.estimated_input(report).is_some(),
            maximum_analysis_age_ns: report.options.policy.maximum_analysis_age_ns,
            latest_observation: feed.latest.clone(),
            measured_lead_us: feed.measured_lead_us,
        }))
    }
    fn view(&self) -> Result<serde_json::Value> {
        let mut value =
            serde_json::to_value(self.report.as_ref().context("capture report absent")?)?;
        value["browser"] = serde_json::json!({"playback_epoch":self.playback_epoch,"handoffs":self.handoffs,"transition_drops":self.transition_drops,"pending_epoch":self.pending.as_ref().map(|p| &p.report.identity.epoch)});
        if let Some(notice) = fallback::notice(self) {
            value["browser"]["analysis_fallback"] = notice;
            if value["browser"].get("analysis_feed").is_none() {
                if let Some(feed) = &self.analysis_feed {
                    value["browser"]["analysis_feed"] = serde_json::to_value(&feed.binding)?;
                }
            }
        }
        if let Some(snapshot) = self.active_analysis_feed()? {
            value["browser"]["analysis_feed"] = serde_json::to_value(snapshot.binding)?;
            value["browser"]["measured_analysis_lead_us"] =
                serde_json::to_value(self.analysis_feed.as_ref().and_then(|f| f.measured_lead_us))?;
        }
        if let Some(feed) = &self.analysis_feed {
            if feed.continuity {
                value["browser"]["analysis_continuity"] = serde_json::json!({
                    "enabled":true,
                    "media_preparation_request_id":feed.media.preparation_request_id,
                    "prepare_due":self.analysis_successor.is_none() && (self.report.as_ref().is_some_and(|r| r.server_elapsed.is_some_and(|t| t.0 >= u64::from(r.options.lease_ms) * 500_000))
                        || feed.media_expires.and_then(|end| end.checked_sub(Duration::from_millis(u64::from(feed.media.lease_ms) / 2))).is_some_and(|at| Instant::now() >= at)),
                    "capture_remaining_ms":feed.capture_deadline.map(|end| u64::try_from(end.saturating_duration_since(Instant::now()).as_millis()).unwrap_or(u64::MAX)),
                });
            }
            value["browser"]["analysis_last_upload"] =
                serde_json::to_value(feed.last_upload.as_ref().map(|u| &u.submitted))?;
            value["browser"]["analysis_finish_request_id"] =
                serde_json::to_value(feed.close.as_ref().map(|c| &c.request_id))?;
            value["browser"]["analysis_final_result_matches"] = serde_json::Value::Bool(
                feed.final_result_matches(self.report.as_ref().expect("view report")),
            );
        }
        if let Some(next) = &self.analysis_successor {
            value["browser"]["analysis_successor"] = serde_json::json!({
                "original_request_id":next.request_id,
                "report":next.report,
                "feed":next.feed.binding,
                "transfer":next.transfer,
                "pair_sent":next.pair_sent,
                "last_upload":next.feed.last_upload.as_ref().map(|u| &u.submitted),
            });
        }
        if let Some(transfer) = &self.analysis_last_transfer {
            value["browser"]["analysis_transfer_completed"] = serde_json::to_value(transfer)?;
        }
        Ok(value)
    }
    fn poll_pending(&mut self) -> Result<bool> {
        let Some(pending) = self.pending.as_ref().map(|pending| pending.report.clone()) else {
            return Ok(false);
        };
        let next = self.capture_status(&pending)?;
        ensure!(
            next.active,
            "prepared browser capture retired; preserve original outcomes"
        );
        let state = next
            .inference
            .as_ref()
            .context("prepared inference absent")?
            .state;
        if state == LiveCaptureInferenceState::Running {
            let prior = self.report.as_ref().context("predecessor absent")?.clone();
            ensure!(
                next.continuation.as_ref().is_some_and(|c| {
                    c.predecessor == prior.identity && c.predecessor_clock == prior.clock
                }),
                "prepared predecessor differs"
            );
            let retired = self.capture_status(&prior)?;
            ensure!(
                !retired.active && retired.retirement == Some(LiveCaptureRetirement::Superseded),
                "Engine handoff not confirmed"
            );
            let pending = self.pending.take().expect("validated pending capture");
            self.bulk = pending.bulk;
            self.sequence = pending.sequence;
            self.eligible_after = pending.eligible_after;
            self.report = Some(next);
            self.handoffs = self
                .handoffs
                .checked_add(1)
                .context("handoff counter overflow")?;
            return Ok(true);
        }
        ensure!(
            state == LiveCaptureInferenceState::Starting,
            "prepared capture failed"
        );
        self.pending.as_mut().expect("pending capture").report = next;
        Ok(false)
    }
    fn prepare_at(&mut self, time: SourceTimestamp) -> Result<()> {
        if self.pending.is_some() {
            return Ok(());
        }
        let prior = self.report.as_ref().context("capture report absent")?;
        if !prior.active
            || prior
                .inference
                .as_ref()
                .is_none_or(|i| i.state != LiveCaptureInferenceState::Running)
            || prior
                .server_elapsed
                .is_none_or(|t| t.0 < u64::from(prior.options.lease_ms) * 500_000)
        {
            return Ok(());
        }
        ensure!(
            self.recording_maximum.is_none() || self.recordings.len() < 64,
            "preserve retained recordings before further continuation"
        );
        let mut options = prior.continuation_options(time, self.options.lease_ms)?;
        options.continuation_mode = LiveContinuationMode::WhenReady;
        let command = Command::ContinueLiveCapture {
            engine_epoch: prior.engine_epoch.clone(),
            predecessor: prior.identity.epoch.clone(),
            options,
        };
        self.blocked = true;
        let report = self.command(command)?;
        self.pending = Some(Pending {
            report: report.clone(),
            bulk: None,
            sequence: 0,
            eligible_after: Instant::now(),
        });
        self.begin_recording(
            &report,
            self.playback_epoch
                .clone()
                .context("playback identity absent")?,
        )?;
        let bulk = self.client.open_live_bulk(&report)?;
        let pending = self.pending.as_mut().expect("admitted pending capture");
        pending.bulk = Some(bulk);
        pending.eligible_after = Instant::now();
        self.blocked = false;
        Ok(())
    }
    fn upload_frame(
        &mut self,
        time: SourceTimestamp,
        pixels: &[u8],
        received: Instant,
    ) -> Result<Option<LiveFrameHeader>> {
        let mut accepted_header = None;
        self.poll_pending()?;
        if self.analysis_feed.is_none() {
            self.prepare_at(time)?;
        }
        let mut sent_pending = false;
        if let Some(pending) = &mut self.pending {
            if received >= pending.eligible_after {
                let upload = pending
                    .bulk
                    .as_mut()
                    .context("pending pixel channel absent")?
                    .upload_captured(FrameId::new(pending.sequence), time, received, pixels)?;
                pending.sequence = pending
                    .sequence
                    .checked_add(1)
                    .context("pending sequence overflow")?;
                pending.report = upload.report;
                sent_pending = true;
            }
        }
        if self.poll_pending()? && sent_pending {
            return Ok(None);
        }
        if received < self.eligible_after {
            self.transition_drops = self
                .transition_drops
                .checked_add(1)
                .context("drop counter overflow")?;
            return Ok(None);
        }
        let result = self
            .bulk
            .as_mut()
            .context("pixel channel absent")?
            .upload_captured(FrameId::new(self.sequence), time, received, pixels);
        match result {
            Ok(upload) => {
                self.sequence = self
                    .sequence
                    .checked_add(1)
                    .context("frame sequence overflow")?;
                accepted_header = Some(upload.stamp.header);
                self.report = Some(upload.report);
            }
            Err(TransportError::Protocol(ref error))
                if error.code == ErrorCode::DependencyMismatch && self.pending.is_some() =>
            {
                ensure!(
                    self.poll_pending()?,
                    "predecessor refused without a confirmed handoff"
                );
                if !sent_pending {
                    if received < self.eligible_after {
                        self.transition_drops = self
                            .transition_drops
                            .checked_add(1)
                            .context("drop counter overflow")?;
                    } else {
                        let upload = self
                            .bulk
                            .as_mut()
                            .context("successor pixel channel absent")?
                            .upload_captured(FrameId::new(self.sequence), time, received, pixels)?;
                        self.sequence = self
                            .sequence
                            .checked_add(1)
                            .context("frame sequence overflow")?;
                        self.report = Some(upload.report);
                    }
                }
            }
            Err(error) => return Err(error.into()),
        }
        Ok(accepted_header)
    }
    fn request(&mut self, request: &Request) -> Result<serde_json::Value> {
        let received = request.received;
        if request.path == "/retire-export" {
            return self.retire_export(serde_json::from_slice(&request.body)?);
        }
        if request.path == "/history-download" {
            return self.download_history_export(serde_json::from_slice(&request.body)?);
        }
        if request.path == "/export-history" {
            ensure!(request.body.is_empty(), "history read body refused");
            return self.export_history();
        }
        if request.path == "/prepare-export" {
            return self.prepare_export(serde_json::from_slice(&request.body)?);
        }
        if matches!(
            request.path.as_str(),
            "/inspect-export" | "/download-script" | "/export-status"
        ) {
            ensure!(request.body.is_empty(), "export action body refused");
            return if request.path == "/inspect-export" {
                self.inspect_export()
            } else if request.path == "/export-status" {
                self.export_status()
            } else {
                self.download_script()
            };
        }
        if request.path == "/retire-gap" {
            return self.retire_gap(serde_json::from_slice(&request.body)?);
        }
        if request.path == "/gap-history" {
            ensure!(request.body.is_empty(), "gap history body refused");
            return self.gap_history();
        }
        if request.path == "/retire-candidate" {
            return self.retire_recording_candidate(serde_json::from_slice(&request.body)?);
        }
        if request.path == "/candidate-history" {
            ensure!(request.body.is_empty(), "candidate history body refused");
            return self.candidate_history();
        }
        if request.path == "/prepare-candidate" {
            return self.prepare_candidate(serde_json::from_slice(&request.body)?);
        }
        if request.path == "/prepare-gaps" {
            return self.prepare_gaps(serde_json::from_slice(&request.body)?);
        }
        if matches!(request.path.as_str(), "/stage-gaps" | "/gap-status") {
            ensure!(request.body.is_empty(), "gap action body refused");
            return if request.path == "/stage-gaps" {
                self.stage_gaps()
            } else {
                self.gap_status()
            };
        }
        if request.path == "/prepare-commit" {
            return self.prepare_commit(serde_json::from_slice(&request.body)?);
        }
        if request.path == "/commit-candidate" {
            ensure!(request.body.is_empty(), "commit action body refused");
            return self.commit_candidate();
        }
        if matches!(
            request.path.as_str(),
            "/stage-candidate" | "/candidate-status" | "/review-candidate"
        ) {
            ensure!(request.body.is_empty(), "candidate action body refused");
            return match request.path.as_str() {
                "/stage-candidate" => self.stage_candidate(),
                "/review-candidate" => self.review_candidate(),
                _ => self.candidate_status(),
            };
        }
        if request.path == "/retire-recordings" {
            return self.retire_recordings(serde_json::from_slice(&request.body)?);
        }
        if request.path == "/recording-history" {
            ensure!(request.body.is_empty(), "recording history body refused");
            return self.recording_history();
        }
        if request.path == "/recordings" {
            ensure!(request.body.is_empty(), "recording catalog body refused");
            return self.recordings_view();
        }
        if matches!(
            request.path.as_str(),
            "/recording-page" | "/recording-download"
        ) {
            let read: RecordingRead = serde_json::from_slice(&request.body)?;
            if request.path == "/recording-download" {
                ensure!(read.after == 0, "archive starts from original recording");
                return self.recording_archive(&read.epoch);
            }
            return Ok(serde_json::to_value(
                self.recording_page(&read.epoch, read.after)?,
            )?);
        }
        if request.path == "/stop" {
            ensure!(request.body.is_empty(), "stop body refused");
            self.stop()?;
            return Ok(serde_json::json!({"active":false}));
        }
        ensure!(
            !self.blocked,
            "prior Engine outcome uncertain; restart preview after inspecting capture state"
        );
        if request.path == "/analysis/prepare" {
            return self.prepare_analysis_successor(request);
        }
        if request.path == "/analysis/cancel-successor" {
            return self.cancel_analysis_successor(request);
        }
        if matches!(
            request.path.as_str(),
            "/analysis/transfer" | "/analysis/transfer-status"
        ) {
            return self.transfer_analysis(request, request.path.ends_with("-status"));
        }
        if request.path == "/analysis/successor-frame" {
            return self.successor_analysis_frame(request);
        }
        if matches!(
            request.path.as_str(),
            "/analysis/finish" | "/analysis/finish-status"
        ) {
            return self.finish_analysis(request, request.path == "/analysis/finish-status");
        }
        if request.path == "/start" {
            ensure!(
                self.report.is_none(),
                "stop the prior capture before starting"
            );
            let start: Start = serde_json::from_slice(&request.body)?;
            ensure!(start.media_us >= 0, "negative media time");
            ensure!(
                start
                    .recording_maximum
                    .is_none_or(|v| (1..=4096).contains(&v)),
                "recording maximum must be 1..4096"
            );
            ensure!(
                start.recording_maximum.is_none() || self.recordings.len() < 64,
                "preserve retained recordings before another run"
            );
            // Validate native source/player membership before starting Engine work.
            let mut analysis_feed = start
                .analysis
                .clone()
                .map(|selection| {
                    AnalysisFeed::new(
                        request,
                        selection,
                        AttemptId::new("pending-analysis-capture")?,
                    )
                })
                .transpose()?;
            self.recording_maximum = start.recording_maximum;
            let mut options = self.options.clone();
            set_browser_capture_anchor(
                &mut options,
                start.media_us,
                start.source_time_basis,
                request.browser_media.as_ref(),
            )?;
            options.browser_analysis = analysis_feed.as_ref().map(AnalysisFeed::declaration);
            let boot =
                crate::capture_engine_epoch(&mut self.client, &self.project, self.revision, None)?;
            // Latch before dispatch: a lost Start reply never authorizes automatic replacement.
            self.blocked = true;
            let admitted_before = Instant::now();
            let report = self.command(Command::StartLiveCapture {
                engine_epoch: boot,
                options,
            })?;
            if let Some(feed) = &mut analysis_feed {
                feed.cpu_mode = AnalysisCpuMode::from_report(&report)?;
                feed.binding.capture_epoch = report.identity.epoch.clone();
                feed.capture_deadline = Some(
                    admitted_before
                        .checked_add(Duration::from_millis(u64::from(report.options.lease_ms)))
                        .context("capture deadline overflow")?,
                );
            }
            self.analysis_feed = analysis_feed;
            self.report = Some(report.clone());
            self.sequence = 0;
            self.last_time = None;
            if report.active && report.options.inference_fallback.is_none() {
                self.begin_recording(&report, report.identity.epoch.clone())?;
                self.bulk = Some(self.client.open_live_bulk(&report)?);
            } else if !report.active {
                ensure!(
                    report
                        .inference
                        .as_ref()
                        .is_some_and(|i| i.fallback_transition.is_some()),
                    "capture retired before its original Start was confirmed"
                );
            }
            self.eligible_after = Instant::now();
            self.playback_epoch = Some(report.identity.epoch.clone());
            self.handoffs = 0;
            self.transition_drops = 0;
            self.blocked = false;
            return self.view();
        }
        self.report
            .as_ref()
            .context("start playback capture first")?;
        if request.path == "/frame" {
            if self
                .report
                .as_ref()
                .is_some_and(|r| r.options.inference_fallback.is_some())
            {
                let prior = self.report.clone().expect("current capture");
                self.report = Some(self.capture_status(&prior)?);
                if fallback::reconcile(self)? {
                    return fallback::discarded_view(self);
                }
            }
            ensure!(
                self.analysis_successor
                    .as_ref()
                    .is_none_or(|n| n.transfer_id.is_none()),
                "predecessor input reserved for original transfer"
            );
            let split = request
                .body
                .iter()
                .position(|b| *b == b'\n')
                .context("frame header absent")?;
            ensure!(split <= 1024, "frame header limit");
            let frame: Frame = serde_json::from_slice(&request.body[..split])?;
            ensure!(
                (if self.analysis_feed.is_some() {
                    self.report
                        .as_ref()
                        .is_some_and(|r| r.identity.epoch == frame.epoch)
                } else {
                    self.playback_epoch.as_ref() == Some(&frame.epoch)
                }) && frame.media_us >= 0
                    && self.last_time.is_none_or(|t| frame.media_us > t),
                "frame epoch/time discontinuity; stop and start at the new time"
            );
            let measured_lead = if let Some(feed) = &self.analysis_feed {
                Some(feed.validate_frame(request, &frame)?)
            } else {
                ensure!(
                    frame.analysis.is_none(),
                    "ordinary preview cannot accept a lookahead feed frame"
                );
                None
            };
            let pixels = &request.body[split + 1..];
            ensure!(
                pixels.len() == LIVE_FRAME_BYTES,
                "one 64x64 RGB frame required"
            );
            let time = SourceTimestamp::new(frame.media_us, 1_000_000)?;
            let uploaded = match self.upload_frame(time, pixels, received) {
                Ok(uploaded) => uploaded,
                Err(error)
                    if self
                        .report
                        .as_ref()
                        .is_some_and(|r| r.options.inference_fallback.is_some()) =>
                {
                    let prior = self.report.clone().expect("original capture");
                    self.report = Some(self.capture_status(&prior)?);
                    if fallback::reconcile(self)? {
                        return fallback::discarded_view(self);
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            self.last_time = Some(frame.media_us);
            if let Some(feed) = &mut self.analysis_feed {
                feed.last_upload = Some(AnalysisUploadReceipt {
                    header: uploaded.context("analysis frame was not admitted")?,
                    submitted: AnalysisSubmittedFrame {
                        media_us: frame.media_us,
                        observation: frame
                            .analysis
                            .clone()
                            .context("analysis observation absent")?,
                    },
                });
                feed.latest = frame.analysis;
                feed.measured_lead_us = measured_lead;
            }
        } else {
            ensure!(
                request.path == "/status" && request.body.is_empty(),
                "status body refused"
            );
        }
        self.refresh_status()
    }
    fn refresh_status(&mut self) -> Result<serde_json::Value> {
        self.poll_pending()?;
        let prior = self
            .report
            .as_ref()
            .context("capture report absent")?
            .clone();
        let report = self.capture_status(&prior)?;
        if !report.active
            && report.retirement == Some(LiveCaptureRetirement::Superseded)
            && self.pending.is_some()
        {
            // Readiness can advance between the successor and predecessor reads.
            // Reconcile existing identities once within the same operation deadline;
            // never publish a superseded predecessor as the current playback.
            ensure!(
                self.poll_pending()?,
                "prepared successor not current after predecessor was superseded; preserve original outcomes"
            );
            return self.view();
        }
        self.report = Some(report);
        fallback::reconcile(self)?;
        self.view()
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(crate) struct ServerControl<F> {
    pub port: u16,
    pub duration_seconds: u16,
    pub media_lease_ms: Option<u32>,
    pub cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub ready: F,
    pub guard: Option<GuardConfig>,
    pub playback: Option<crate::browser_handy_playback::Config>,
    pub hsp: Option<crate::browser_handy_hsp::Config>,
}

pub(crate) struct LaunchOptions {
    pub port: u16,
    pub duration_seconds: u16,
    pub media_lease_ms: Option<u32>,
    pub guard: Option<GuardConfig>,
    pub playback: Option<crate::browser_handy_playback::Config>,
    pub hsp: Option<crate::browser_handy_hsp::Config>,
}

/// Only the standalone CLI owns these process signal registrations. The desktop
/// launcher supplies its own cancellation flag directly to `run_controlled`.
#[cfg(unix)]
struct CliShutdownSignals(Vec<signal_hook::SigId>);
#[cfg(unix)]
impl CliShutdownSignals {
    fn install(cancelled: &std::sync::Arc<std::sync::atomic::AtomicBool>) -> Result<Self> {
        let mut owned = Self(Vec::with_capacity(2));
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            owned.0.push(signal_hook::flag::register(
                signal,
                std::sync::Arc::clone(cancelled),
            )?);
        }
        Ok(owned)
    }
}
#[cfg(unix)]
impl Drop for CliShutdownSignals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

pub(crate) fn run(
    client: SessionClient,
    project: ProjectId,
    revision: RevisionId,
    options: LiveCaptureOptions,
    launch: LaunchOptions,
) -> Result<()> {
    let LaunchOptions {
        port,
        duration_seconds,
        media_lease_ms,
        guard,
        playback,
        hsp,
    } = launch;
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Registration precedes every browser worker. Partial installation rolls
    // back through Drop; cleanup runs on normal threads, never in the handler.
    #[cfg(unix)]
    let _signals = CliShutdownSignals::install(&cancelled)?;
    run_controlled(
        client,
        project,
        revision,
        options,
        ServerControl {
            port,
            duration_seconds,
            media_lease_ms,
            guard,
            playback,
            hsp,
            cancelled,
            ready: |url: String| {
                println!("Browser preview: {url}\nVideo Play starts analysis/timing only. Configured Handy Start requires a separate explicit action; physical response remains unconfirmed. Preview server stops after {duration_seconds} seconds.");
                std::io::stdout().flush()?;
                Ok(())
            },
        },
    )
}

pub(crate) fn run_controlled(
    client: SessionClient,
    project: ProjectId,
    revision: RevisionId,
    options: LiveCaptureOptions,
    control: ServerControl<impl FnOnce(String) -> Result<()>>,
) -> Result<()> {
    let ServerControl {
        port,
        duration_seconds,
        media_lease_ms,
        cancelled,
        ready,
        guard,
        playback,
        hsp,
    } = control;
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(());
    }
    options.validate()?;
    ensure!(
        (options.inference.is_some() || options.model_pack.is_some())
            && options.encoded_source.is_none()
            && options.continuation_mode == LiveContinuationMode::Immediate,
        "browser preview requires explicit RGB inference and immediate capture options"
    );
    ensure!(
        (1..=600).contains(&duration_seconds),
        "preview duration must be 1..=600 seconds"
    );
    let players = crate::browser_player_control::PlayerControl::new_with_hsp(
        &client,
        project.clone(),
        revision,
        guard,
        playback,
        hsp,
    )?;
    let media = source_media::MediaController::new(
        &client,
        project.clone(),
        revision,
        duration_seconds,
        media_lease_ms,
    )?;
    let preview = Preview {
        client,
        project,
        revision,
        options,
        report: None,
        analysis_feed: None,
        analysis_successor: None,
        analysis_commit: None,
        analysis_last_transfer: None,
        analysis_cancelled: false,
        bulk: None,
        sequence: 0,
        last_time: None,
        blocked: false,
        playback_epoch: None,
        pending: None,
        eligible_after: Instant::now(),
        handoffs: 0,
        transition_drops: 0,
        recording_maximum: None,
        recordings: Vec::new(),
        retired_recordings: Vec::new(),
        candidate: None,
        export: None,
        gap: None,
    };
    player_http::run_with_media(
        preview,
        players,
        media,
        port,
        duration_seconds,
        cancelled,
        ready,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn capture_cleanup_preserves_first_failure_and_attempts_both_originals() {
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().unwrap();
        let endpoint = root.path().join("engine.sock");
        let listener = UnixListener::bind(&endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut epochs = Vec::new();
            while epochs.len() < 2 && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                        let request: pulsar_protocol::Request =
                            pulsar_protocol::read_message(&mut stream).unwrap();
                        let Command::StopLiveCapture { epoch } = &request.command else {
                            panic!("unexpected cleanup command")
                        };
                        epochs.push(epoch.clone());
                        let message = if epochs.len() == 1 {
                            "first pending stop failed"
                        } else {
                            "second active stop failed"
                        };
                        pulsar_protocol::write_message(
                            &mut stream,
                            &pulsar_protocol::Response::failure(
                                request.request_id,
                                ProtocolError::new(ErrorCode::Unavailable, message),
                            ),
                        )
                        .unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("listener failed: {error}"),
                }
            }
            epochs
        });
        let original: LiveCaptureReport = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/live-cpu-capture-report.json"
        ))
        .unwrap();
        let mut pending = original.clone();
        pending.identity.epoch = AttemptId::new("pending-capture").unwrap();
        let mut client = SessionClient::from_credentials(
            &endpoint,
            SessionId::new("cleanup-session").unwrap(),
            "synthetic-token".into(),
        )
        .unwrap();
        client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(3)));
        let mut preview = Preview {
            client,
            project: original.identity.project.clone(),
            revision: original.identity.revision,
            options: original.options.clone(),
            report: Some(original.clone()),
            analysis_feed: None,
            analysis_successor: None,
            analysis_commit: None,
            analysis_last_transfer: None,
            analysis_cancelled: false,
            bulk: None,
            sequence: 0,
            last_time: None,
            blocked: false,
            playback_epoch: None,
            pending: Some(Pending {
                report: pending.clone(),
                bulk: None,
                sequence: 0,
                eligible_after: Instant::now(),
            }),
            eligible_after: Instant::now(),
            handoffs: 0,
            transition_drops: 0,
            recording_maximum: None,
            recordings: vec![],
            retired_recordings: vec![],
            candidate: None,
            export: None,
            gap: None,
        };
        let error = preview.stop().unwrap_err().to_string();
        let epochs = server.join().unwrap();
        assert_eq!(epochs, vec![pending.identity.epoch, original.identity.epoch]);
        assert!(preview.blocked && preview.pending.is_some() && preview.report.is_some());
        preview.pending = None;
        preview.report = None;
        assert!(error.contains("first pending stop failed"), "{error}");
    }

    #[test]
    fn recording_candidate_recovery_restores_no_review_or_commit_authority() {
        let project = ProjectId::new("candidate-project").unwrap();
        let binding = "a".repeat(64);
        let mut saved = SavedRecordingCandidate {
            schema: 1,
            project: project.clone(),
            credential_binding: binding.clone(),
            request: RequestId::new("original-stage").unwrap(),
            revision: RevisionId::new(0),
            options: LiveRecordingStageOptions {
                engine_epoch: AttemptId::new("boot").unwrap(),
                recordings: vec![LiveRecordingSelection {
                    epoch: AttemptId::new("recording").unwrap(),
                    recording_request_id: RequestId::new("original-record").unwrap(),
                }],
                range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000_000))
                    .unwrap(),
                maximum_interpolation_ns: 200_000_000,
            },
            attempted: false,
            candidate: None,
            commit: None,
            retired: false,
            history: Vec::new(),
        };
        saved.validate(&project, &binding).unwrap();
        saved.candidate = Some(CandidateId::new("original-candidate").unwrap());
        assert!(saved.validate(&project, &binding).is_err());
        saved.attempted = true;
        saved.validate(&project, &binding).unwrap();
        let restored: SavedRecordingCandidate =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let intent = restored.into_intent();
        assert_eq!(intent.request.as_str(), "original-stage");
        assert!(intent.reviewed.is_none() && intent.commit.is_none());
        assert!(saved
            .validate(&ProjectId::new("foreign").unwrap(), &binding)
            .is_err());
        assert!(saved.validate(&project, &"b".repeat(64)).is_err());
        let legacy = serde_json::to_value(&saved).unwrap();
        assert!(legacy.get("commit").is_none());
        let legacy: SavedRecordingCandidate = serde_json::from_value(legacy).unwrap();
        assert!(legacy.commit.is_none());
        saved.commit = Some(CommitIntent {
            request: RequestId::new("approved-original-commit").unwrap(),
            pin: ReviewPin {
                candidate: saved.candidate.clone().unwrap(),
                revision: saved.revision,
                program_sha256: "a".repeat(64),
            },
            acknowledged_revision: None,
        });
        saved.validate(&project, &binding).unwrap();
        let recovered = saved.clone().into_intent();
        assert!(recovered.reviewed.is_none());
        assert_eq!(
            recovered.commit.as_ref().unwrap().request.as_str(),
            "approved-original-commit"
        );
        saved.commit.as_mut().unwrap().acknowledged_revision = Some(RevisionId::new(9));
        assert!(saved.validate(&project, &binding).is_err());
        saved.commit.as_mut().unwrap().acknowledged_revision = Some(RevisionId::new(1));
        saved.validate(&project, &binding).unwrap();
        saved.commit.as_mut().unwrap().pin.program_sha256 = "bad".into();
        assert!(saved.validate(&project, &binding).is_err());
        saved.commit.as_mut().unwrap().pin.program_sha256 = "a".repeat(64);
        let receipt = RetiredProposal::from_saved(&saved).unwrap();
        saved.history.push(receipt.clone());
        assert!(saved.validate(&project, &binding).is_err());
        saved.retired = true;
        saved.validate(&project, &binding).unwrap();
        let approval = RetireProposal {
            request_id: receipt.request.clone(),
            candidate_id: receipt.candidate.clone(),
            commit_request_id: receipt.commit_request.clone(),
            acknowledged_revision: receipt.acknowledged_revision,
            acknowledge_preserve_history: true,
        };
        assert!(approval.matches(&receipt));
        saved.history.push(receipt);
        assert!(saved.validate(&project, &binding).is_err());
        saved.retired = false;
        saved.history.clear();
        saved.commit = None;
        saved
            .options
            .recordings
            .push(saved.options.recordings[0].clone());
        assert!(saved.validate(&project, &binding).is_err());
    }
    #[test]
    fn recording_index_retains_original_ids_and_refuses_foreign_duplicate_and_overbound_records() {
        let project = ProjectId::new("recording-project").unwrap();
        let binding = "a".repeat(64);
        let mut index = RecordingIndex {
            schema: 1,
            project: project.clone(),
            credential_binding: binding.clone(),
            retired: Vec::new(),
            recordings: vec![RecordingIntent {
                epoch: AttemptId::new("original-epoch").unwrap(),
                request: RequestId::new("original-request").unwrap(),
                playback: AttemptId::new("original-playback").unwrap(),
                revision: RevisionId::new(3),
            }],
        };
        index.validate(&project, &binding).unwrap();
        let bytes = serde_json::to_vec(&index).unwrap();
        let restored: RecordingIndex = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.recordings[0].request.as_str(), "original-request");
        assert_eq!(restored.recordings[0].revision, RevisionId::new(3));
        assert!(restored
            .validate(&ProjectId::new("foreign").unwrap(), &binding)
            .is_err());
        assert!(restored.validate(&project, &"b".repeat(64)).is_err());
        assert!(serde_json::to_value(&index)
            .unwrap()
            .get("retired")
            .is_none());
        index.retired = index.recordings.clone();
        index.recordings.clear();
        index.validate(&project, &binding).unwrap();
        index.recordings.push(index.retired[0].clone());
        assert!(index.validate(&project, &binding).is_err());
        index.recordings = index.retired.clone();
        index.retired.clear();
        index.recordings.push(index.recordings[0].clone());
        assert!(index.validate(&project, &binding).is_err());
        index.recordings = (0..65)
            .map(|i| RecordingIntent {
                epoch: AttemptId::new(format!("epoch-{i}")).unwrap(),
                request: RequestId::new(format!("request-{i}")).unwrap(),
                playback: AttemptId::new("playback").unwrap(),
                revision: RevisionId::new(3),
            })
            .collect();
        assert!(index.validate(&project, &binding).is_err());
        index.schema = 2;
        assert!(index.validate(&project, &binding).is_err());
        let mut malformed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        malformed["authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<RecordingIndex>(malformed).is_err());
    }
    #[test]
    fn script_download_preflight_keeps_engine_gap_and_quantization_refusals() {
        let action = |ns| {
            MotionAction::new(
                ProjectTime::from_nanos(ns),
                NormalizedPosition::new(0.375).unwrap(),
                EvidenceKind::Inferred,
            )
            .unwrap()
        };
        let program = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![action(0), action(1_000_000)],
        )
        .unwrap()])
        .unwrap();
        assert_eq!(checked_script(&program).unwrap()["actions"][0]["pos"], 38);
        let collision = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![action(0), action(1)],
        )
        .unwrap()])
        .unwrap();
        assert!(checked_script(&collision).is_err());
        let gap = MotionProgram::new(vec![MotionTrack::with_gaps(
            Axis::Stroke,
            vec![action(0), action(2_000_000)],
            vec![TimeRange::new(
                ProjectTime::from_nanos(1),
                ProjectTime::from_nanos(2_000_000),
            )
            .unwrap()],
        )
        .unwrap()])
        .unwrap();
        assert!(checked_script(&gap).is_err());
    }
    #[test]
    fn commit_approval_requires_exact_reviewed_scope_digest_and_acknowledgement() {
        let pin = ReviewPin {
            candidate: CandidateId::new("reviewed").unwrap(),
            revision: RevisionId::new(0),
            program_sha256: "a".repeat(64),
        };
        let mut approval = CommitApproval {
            candidate_id: pin.candidate.clone(),
            base_revision: pin.revision,
            program_sha256: pin.program_sha256.clone(),
            acknowledge: false,
        };
        assert!(approval.validate(&pin).is_err());
        approval.acknowledge = true;
        approval.validate(&pin).unwrap();
        approval.program_sha256 = "b".repeat(64);
        assert!(approval.validate(&pin).is_err());
        approval.program_sha256 = pin.program_sha256.clone();
        approval.base_revision = RevisionId::new(1);
        assert!(approval.validate(&pin).is_err());
    }
    #[test]
    fn saved_gap_binds_values_scope_and_restores_no_review_or_commit() {
        let project = ProjectId::new("saved-gap-project").unwrap();
        let program = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![
                MotionAction::new(
                    ProjectTime::from_nanos(0),
                    NormalizedPosition::new(0.2).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(1_000_000_000),
                    NormalizedPosition::new(0.8).unwrap(),
                    EvidenceKind::Inferred,
                )
                .unwrap(),
            ],
        )
        .unwrap()])
        .unwrap();
        let upload = crate::motion::RetainedEditUpload::new(
            project.clone(),
            RevisionId::new(1),
            &program,
            "saved gap".into(),
        )
        .unwrap();
        let saved = SavedGap {
            schema: 1,
            credential_binding: "binding".into(),
            project: project.clone(),
            request: crate::session::new_request_id(),
            revision: RevisionId::new(1),
            program_sha256: "a".repeat(64),
            gaps: vec![
                TimeRange::new(ProjectTime::from_nanos(0), ProjectTime::from_nanos(100)).unwrap(),
            ],
            program,
            attempted: false,
            upload,
            candidate: None,
            commit: None,
            retired: false,
            history: Vec::new(),
        };
        saved.validate(&project, "binding").unwrap();
        assert!(saved.validate(&project, "changed credentials").is_err());
        assert!(saved
            .validate(&ProjectId::new("other-project").unwrap(), "binding")
            .is_err());
        let mut corrupt = saved.clone();
        corrupt.upload.sha256 = "b".repeat(64);
        assert!(corrupt.validate(&project, "binding").is_err());
        let mut corrupt = saved.clone();
        corrupt.candidate = Some(CandidateId::new("forged-candidate").unwrap());
        assert!(corrupt.validate(&project, "binding").is_err());
        let legacy = serde_json::to_value(&saved).unwrap();
        assert!(legacy.get("commit").is_none());
        let old: SavedGap = serde_json::from_value(legacy).unwrap();
        assert!(old.commit.is_none());
        let restored = saved.clone().into_intent();
        assert!(restored.proposal.reviewed.is_none() && restored.proposal.commit.is_none());
        let mut approved = saved;
        approved.attempted = true;
        approved.upload.finalization_attempted = true;
        approved.upload.lease = Some(TransferLease {
            lease_id: TransferId::new("saved-lease").unwrap(),
            engine_epoch: "saved-epoch".into(),
            project_id: project.clone(),
            direction: TransferDirection::Upload,
            sha256: approved.upload.sha256.clone(),
            total_byte_len: approved.upload.byte_len,
            offset: 0,
            byte_len: approved.upload.byte_len,
            accepted_prefix: approved.upload.byte_len,
            expires_after_ms: 300000,
            bulk_endpoint: "/private/bulk.sock".into(),
        });
        let id = CandidateId::new("approved-candidate").unwrap();
        approved.candidate = Some(id.clone());
        let request = crate::session::new_request_id();
        approved.commit = Some(CommitIntent {
            request: request.clone(),
            pin: ReviewPin {
                candidate: id,
                revision: RevisionId::new(1),
                program_sha256: "c".repeat(64),
            },
            acknowledged_revision: None,
        });
        approved.validate(&project, "binding").unwrap();
        let restored = approved.clone().into_intent();
        assert!(restored.proposal.reviewed.is_none());
        assert_eq!(restored.proposal.commit.unwrap().request, request);
        let mut altered = approved.clone();
        altered.commit.as_mut().unwrap().pin.candidate =
            CandidateId::new("other-candidate").unwrap();
        assert!(altered.validate(&project, "binding").is_err());
        let mut altered = approved.clone();
        altered.commit.as_mut().unwrap().acknowledged_revision = Some(RevisionId::new(99));
        assert!(altered.validate(&project, "binding").is_err());
        approved.commit.as_mut().unwrap().acknowledged_revision = Some(RevisionId::new(2));
        approved.validate(&project, "binding").unwrap();
        let entry = RetiredProposal::from_gap(&approved).unwrap();
        approved.history.push(entry.clone());
        assert!(approved.validate(&project, "binding").is_err());
        approved.retired = true;
        approved.validate(&project, "binding").unwrap();
        approved.history.push(entry);
        assert!(approved.validate(&project, "binding").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn export_recovery_refuses_wrong_binding_and_replaced_parent_without_redirecting_reads() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let output = tempfile::Builder::new()
            .prefix("browser-export-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(root.path())
            .unwrap();
        let bytes = b"{\"actions\":[{\"at\":0,\"pos\":38}]}";
        std::fs::write(output.path().join("stroke.funscript"), bytes).unwrap();
        let meta = output.path().metadata().unwrap();
        let project = ProjectId::new("export-project").unwrap();
        let saved = SavedExport {
            schema: 1,
            project: project.clone(),
            credential_binding: "binding".into(),
            intent: ExportIntent {
                request: crate::session::new_request_id(),
                revision: RevisionId::new(2),
                program_sha256: "a".repeat(64),
                path: output.path().join("stroke.funscript"),
                directory: PublicationDirectoryIdentity {
                    device: meta.dev(),
                    inode: meta.ino(),
                },
                expected: serde_json::from_slice(bytes).unwrap(),
            },
            acknowledged_revision: None,
            output_sha256: None,
        };
        let legacy = serde_json::to_value(Some(saved.clone())).unwrap();
        let catalog: ExportCatalog = serde_json::from_value(legacy).unwrap();
        catalog.validate(&project, "binding", root.path()).unwrap();
        assert_eq!(
            catalog.active.as_ref().unwrap().intent.request,
            saved.intent.request
        );
        let empty: ExportCatalog = serde_json::from_str("null").unwrap();
        assert!(empty.active.is_none() && empty.retired.is_empty());
        assert!(RetiredExport::from_saved(&saved).is_err());
        let mut confirmed = saved.clone();
        confirmed.acknowledged_revision = Some(confirmed.intent.revision);
        confirmed.output_sha256 = Some("b".repeat(64));
        let retired = RetiredExport::from_saved(&confirmed).unwrap();
        assert!(serde_json::to_value(&retired)
            .unwrap()
            .get("expected")
            .is_none());
        let mut duplicate = ExportCatalog {
            format: 2,
            active: Some(confirmed),
            retired: vec![retired],
        };
        assert!(duplicate
            .validate(&project, "binding", root.path())
            .is_err());
        duplicate.active = None;
        duplicate
            .validate(&project, "binding", root.path())
            .unwrap();
        let directory = saved.validate(&project, "binding", root.path()).unwrap();
        assert!(saved.validate(&project, "changed", root.path()).is_err());
        let mut redirected = saved.clone();
        redirected.intent.path = root.path().join("outside.funscript");
        assert!(redirected
            .validate(&project, "binding", root.path())
            .is_err());
        let held = root.path().join("held-export");
        std::fs::rename(output.path(), &held).unwrap();
        std::fs::create_dir(output.path()).unwrap();
        std::fs::set_permissions(output.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(saved.validate(&project, "binding", root.path()).is_err());
        assert_eq!(read_export_bytes(&directory).unwrap(), bytes);
        std::fs::remove_dir(output.path()).unwrap();
        symlink(&held, output.path()).unwrap();
        assert!(saved.validate(&project, "binding", root.path()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn export_descriptor_read_refuses_symlink_and_shared_inode() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let outside = root.path().join("outside.funscript");
        std::fs::write(&outside, b"owned fixture").unwrap();
        let output = tempfile::Builder::new()
            .prefix("browser-export-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(root.path())
            .unwrap();
        let meta = output.path().metadata().unwrap();
        let directory = open_export_directory(
            output.path(),
            &PublicationDirectoryIdentity {
                device: meta.dev(),
                inode: meta.ino(),
            },
        )
        .unwrap();
        let path = output.path().join("stroke.funscript");
        symlink(&outside, &path).unwrap();
        assert!(read_export_bytes(&directory).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&outside, &path).unwrap();
        assert!(read_export_bytes(&directory).is_err());
    }

    #[test]
    fn candidate_routes_require_the_same_checked_origin_as_live_ingress() {
        for path in [
            "/prepare-candidate",
            "/stage-candidate",
            "/candidate-status",
            "/review-candidate",
            "/prepare-commit",
            "/commit-candidate",
            "/inspect-export",
            "/prepare-export",
            "/download-script",
            "/export-status",
            "/retire-export",
            "/export-history",
            "/history-download",
            "/prepare-gaps",
            "/stage-gaps",
            "/gap-status",
        ] {
            let good=format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:1\r\nOrigin: http://127.0.0.1:1\r\nContent-Type: application/octet-stream\r\nContent-Length: 0\r\n\r\n");
            assert!(parse(good.as_bytes()).is_ok());
            assert!(parse(
                good.replace("Origin: http://127.0.0.1:1", "Origin: http://evil.invalid")
                    .as_bytes()
            )
            .is_err());
        }
    }
    fn parse(bytes: &[u8]) -> Result<Request> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(bytes).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        read_request(&mut server, "127.0.0.1:1")
    }
    #[test]
    fn browser_ingress_refuses_cross_origin_ambiguity_and_unbounded_bodies() {
        for tail in [
            "Origin: http://evil.invalid\r\nContent-Length: 0\r\n",
            "Origin: http://127.0.0.1:1\r\nContent-Length: 999999\r\n",
            "Origin: http://127.0.0.1:1\r\nContent-Length: 0\r\nContent-Length: 0\r\n",
            "Origin: http://127.0.0.1:1\r\nTransfer-Encoding: chunked\r\n",
        ] {
            let request=format!("POST /status HTTP/1.1\r\nHost: 127.0.0.1:1\r\nContent-Type: application/octet-stream\r\n{tail}\r\n");
            assert!(parse(request.as_bytes()).is_err());
        }
        assert!(parse(b"GET /../../secret HTTP/1.1\r\nHost: 127.0.0.1:1\r\n\r\n").is_err());
        assert!(parse(b"GET / HTTP/1.1\r\nHost: localhost:1\r\n\r\n").is_err());
        let valid=parse(b"POST /status HTTP/1.1\r\nHost: 127.0.0.1:1\r\nOrigin: http://127.0.0.1:1\r\nContent-Type: application/octet-stream\r\nContent-Length: 0\r\n\r\n").unwrap();
        assert!(valid.body.is_empty());
    }
}

#[cfg(test)]
#[path = "browser_source_media_native_tests.rs"]
mod browser_source_media_native_tests;

#[cfg(all(test, unix))]
#[path = "browser_live_handoff_tests.rs"]
mod browser_live_handoff_tests;

#[cfg(all(test, unix))]
#[path = "browser_timing_native_tests.rs"]
mod browser_timing_native_tests;

#[cfg(test)]
#[path = "browser_analysis_feed_tests.rs"]
mod browser_analysis_feed_tests;

#[cfg(test)]
#[path = "browser_captured_cpu_tests.rs"]
mod browser_captured_cpu_tests;
