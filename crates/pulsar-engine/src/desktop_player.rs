//! One ephemeral Desktop playback owner, with bounded HSP buffering and player custody.
use crate::desktop_hsp::{build_points, offset_points, HspClient, HspStream};
use crate::desktop_mpv::MpvPlayer;
use anyhow::{ensure, Context, Result};
use pulsar_protocol::{
    DesktopMediaReview, DesktopPlaybackAction as Action, DesktopPlaybackStatus as Status,
    EffectReview, ErrorCode, ProtocolError, RoleplayConsentDocument, RoleplayEffect,
    RoleplayEffectApproval, RoleplayEffectScope, SessionId,
};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_REVIEWED_VIDEO_BYTES: u64 = 512 * 1024 * 1024;
const AHEAD_MS: u64 = 5_000;
const SESSION_LIFETIME: Duration = Duration::from_secs(20 * 60);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);
const START_LEAD_MS: u64 = 1_500;

// Higher-priority interrupts subsume lower-priority ones.
fn interrupt_priority(action: &Action) -> u8 {
    match action {
        Action::Close => 4,
        Action::Stop => 3,
        Action::Arm { enabled: false, .. } => 2,
        Action::Pause => 1,
        _ => 0,
    }
}

struct Shared {
    status: Mutex<Status>,
    queue: Mutex<VecDeque<(u64, Action)>>,
    owner: Mutex<Option<String>>,
    heartbeat: Mutex<Instant>,
    recent: Mutex<VecDeque<(String, String, String)>>,
    epoch: AtomicU64,
    shutdown: AtomicBool,
}

impl Shared {
    fn new() -> Self {
        Self {
            status: Mutex::new(Status::default()),
            queue: Mutex::new(VecDeque::new()),
            owner: Mutex::new(None),
            heartbeat: Mutex::new(Instant::now()),
            recent: Mutex::new(VecDeque::new()),
            epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        }
    }

    fn check_device_session(
        &self,
        epoch: u64,
        armed_until: Option<Instant>,
        start: Instant,
    ) -> Result<()> {
        ensure!(
            self.epoch.load(Ordering::SeqCst) == epoch,
            "Handy control cancelled"
        );
        ensure!(
            armed_until.is_some_and(|until| until > start),
            "Handy session expired; enable again"
        );
        let heartbeat = self
            .heartbeat
            .lock()
            .map_err(|_| anyhow::anyhow!("Handy controller unavailable"))?;
        ensure!(
            heartbeat.elapsed() < HEARTBEAT_TIMEOUT,
            "Handy controller lost; enable again"
        );
        Ok(())
    }

    fn publish_completion(&self, player: &Player, message: String) {
        // Serialize with enqueue, so completion cannot acknowledge a later Stop.
        let (Ok(queue), Ok(mut status)) = (self.queue.lock(), self.status.lock()) else {
            return;
        };
        status.busy = !queue.is_empty();
        if !status.busy {
            status.message = message;
        }
        status.media_review = player
            .pending_review
            .as_ref()
            .map(|pending| pending.status.clone());
        status.loaded = player.mpv.is_some();
        status.armed = player.armed_until.is_some();
        status.assistant_grant = player
            .assistant_grant
            .as_ref()
            .map(|grant| grant.id.clone());
        status.connected = player.hsp.is_some();
        status.playing = player.playing;
        status.expires_in_ms = player.armed_until.map_or(0, |t| {
            t.saturating_duration_since(Instant::now()).as_millis() as u64
        });
    }
}

pub(crate) struct Service {
    shared: Arc<Shared>,
}
impl Service {
    pub(crate) fn new(directory: PathBuf) -> Result<Self> {
        let shared = Arc::new(Shared::new());
        let state = shared.clone();
        thread::Builder::new()
            .name("pulsar-player".into())
            .spawn(move || run(state, directory))
            .context("start Desktop player supervisor")?;
        Ok(Self { shared })
    }

    pub(crate) fn request(
        &self,
        session: &str,
        request: &str,
        action: &Action,
    ) -> Result<Status, ProtocolError> {
        action.validate()?;
        let shared = &self.shared;
        let mut owner = shared.owner.lock().map_err(|_| internal())?;
        let status = shared.status.lock().map_err(|_| internal())?.clone();
        let is_owner = owner.as_deref() == Some(session);
        if matches!(action, Action::Status) {
            if is_owner {
                *shared.heartbeat.lock().map_err(|_| internal())? = Instant::now();
            }
            return Ok(status);
        }
        // A host owner can always stop; other controls cannot steal a live session.
        let emergency = matches!(
            action,
            Action::Stop | Action::Close | Action::Arm { enabled: false, .. }
        );
        let assistant_play = matches!(action, Action::AssistantPlay { .. });
        if let Action::AssistantPlay { grant, .. } = action {
            if status.assistant_grant.as_ref() != Some(grant)
                || !status.armed
                || status.expires_in_ms == 0
            {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "No matching active assistant Handy grant",
                ));
            }
        }
        let proposal = matches!(action, Action::PrepareMediaReview { .. });
        if owner.is_some() && !is_owner && !emergency && !proposal && !assistant_play {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "another Desktop owns the player; Stop or close it first",
            ));
        }
        let fingerprint = serde_json::to_string(action).map_err(|_| internal())?;
        let mut recent = shared.recent.lock().map_err(|_| internal())?;
        if let Some((_, _, original)) = recent.iter().find(|(s, r, _)| s == session && r == request)
        {
            if original != &fingerprint {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "player request identity changed",
                ));
            }
            return Ok(status);
        }
        let priority = interrupt_priority(action);
        let interrupts = priority > 0;
        let mut queue = shared.queue.lock().map_err(|_| internal())?;
        if interrupts {
            // Every queued action belongs to the superseded playback epoch.
            // Reclaim their slots before admitting Stop, even at capacity.
            let epoch = shared.epoch.fetch_add(1, Ordering::SeqCst) + 1;
            let interrupt = match queue.front() {
                Some((_, pending)) if interrupt_priority(pending) > priority => pending.clone(),
                _ => action.clone(),
            };
            queue.clear();
            queue.push_back((epoch, interrupt));
        } else {
            if queue.len() >= 32 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "player control queue is busy",
                ));
            }
            let epoch = shared.epoch.load(Ordering::SeqCst);
            queue.push_back((epoch, action.clone()));
        }
        if owner.is_none() && !emergency && !proposal && !assistant_play {
            *owner = Some(session.into());
        }
        if owner.as_deref() == Some(session) && !proposal && !assistant_play {
            *shared.heartbeat.lock().map_err(|_| internal())? = Instant::now();
        }
        recent.push_back((session.into(), request.into(), fingerprint));
        while recent.len() > 64 {
            recent.pop_front();
        }
        let mut result = shared.status.lock().map_err(|_| internal())?;
        result.busy = true;
        result.message = if interrupts {
            "Stopping / applying control…"
        } else {
            "Preparing player…"
        }
        .into();
        Ok(result.clone())
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
    }
}
fn internal() -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, "player supervisor unavailable")
}

#[derive(Deserialize)]
struct Script {
    actions: Vec<ScriptPoint>,
}
#[derive(Deserialize)]
struct ScriptPoint {
    at: i64,
    pos: i32,
}

struct PendingMediaReview {
    status: DesktopMediaReview,
    scope: RoleplayEffectScope,
    video: PathBuf,
    snapshot: PathBuf,
    custody: tempfile::TempDir,
}

fn review_now_ns() -> Result<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .try_into()?)
}

#[cfg(not(unix))]
fn read_review_consent(_path: &std::path::Path) -> Result<RoleplayConsentDocument> {
    anyhow::bail!("private media review consent is unavailable on this platform")
}

#[cfg(unix)]
fn read_review_consent(path: &std::path::Path) -> Result<RoleplayConsentDocument> {
    use std::io::Read;
    {
        use std::os::unix::fs::MetadataExt;
        let mut file = crate::artifacts::open_regular(path)?;
        let meta = file.metadata()?;
        ensure!(
            meta.uid() == unsafe { libc::geteuid() }
                && meta.mode() & 0o077 == 0
                && meta.nlink() == 1
                && meta.len() <= 32 * 1024,
            "unsafe or excessive media review consent"
        );
        let mut bytes = Vec::new();
        file.by_ref().take(32 * 1024 + 1).read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        ensure!(
            bytes.len() <= 32 * 1024
                && meta.len() == after.len()
                && meta.mode() == after.mode()
                && meta.nlink() == after.nlink()
                && meta.mtime() == after.mtime()
                && meta.mtime_nsec() == after.mtime_nsec()
                && meta.ctime() == after.ctime()
                && meta.ctime_nsec() == after.ctime_nsec(),
            "media review consent changed during capture"
        );
        let document: RoleplayConsentDocument = serde_json::from_slice(&bytes)?;
        document.validate()?;
        Ok(document)
    }
}

struct ActiveConsent {
    path: PathBuf,
    document: RoleplayConsentDocument,
}

impl ActiveConsent {
    fn check(&self) -> Result<()> {
        let current = read_review_consent(&self.path)
            .map_err(|_| anyhow::anyhow!("Session consent is unavailable"))?;
        ensure!(current == self.document, "Session consent changed");
        current
            .admit_effect(review_now_ns()?)
            .map_err(|reason| anyhow::anyhow!("Session consent refused: {}", reason.label()))
    }
}

struct AssistantDeviceGrant {
    id: SessionId,
    consent: ActiveConsent,
}

struct Player {
    mpv: Option<MpvPlayer>,
    reviewed_media: Option<tempfile::TempDir>,
    active_media_consent: Option<ActiveConsent>,
    assistant_grant: Option<AssistantDeviceGrant>,
    pending_review: Option<PendingMediaReview>,
    approval: RoleplayEffectApproval,
    hsp: Option<HspClient>,
    stream: Option<HspStream>,
    raw: Vec<(i64, i32)>,
    points: Arc<Vec<(u32, u8)>>,
    armed_until: Option<Instant>,
    playing: bool,
    max_speed: u16,
    min_pct: u8,
    max_pct: u8,
    sync_offset_ms: i16,
}
impl Default for Player {
    fn default() -> Self {
        Self {
            mpv: None,
            reviewed_media: None,
            active_media_consent: None,
            assistant_grant: None,
            pending_review: None,
            approval: {
                let mut nonce = [0; 32];
                nonce[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
                nonce[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
                RoleplayEffectApproval::new(nonce)
            },
            hsp: None,
            stream: None,
            raw: Vec::new(),
            points: Arc::new(Vec::new()),
            armed_until: None,
            playing: false,
            max_speed: 150,
            min_pct: 35,
            max_pct: 65,
            sync_offset_ms: 0,
        }
    }
}
impl Player {
    fn stop(&mut self, disarm: bool) -> Result<()> {
        self.cancel_media_review();
        self.playing = false;
        self.stream = None;
        let video = self.mpv.as_mut().map(|m| m.pause(true)).transpose();
        let device = self.hsp.as_mut().map(|h| h.stop()).transpose();
        if disarm {
            self.armed_until = None;
            self.assistant_grant = None;
            self.hsp = None;
        }
        video?;
        device?;
        Ok(())
    }
    fn close(&mut self) {
        let _ = self.stop(true);
        if let Some(mut mpv) = self.mpv.take() {
            mpv.close();
        }
        self.reviewed_media = None;
        self.active_media_consent = None;
        self.raw.clear();
        self.points = Arc::new(Vec::new());
    }
    fn points(&mut self) {
        self.points = Arc::new(build_points(
            &self.raw,
            self.max_speed as f32,
            self.min_pct as f32,
            self.max_pct as f32,
        ));
    }
    fn end_ms(&self) -> u64 {
        let end = self.points.last().map_or(0, |p| u64::from(p.0));
        if self.hsp.is_some() {
            end + self.sync_offset_ms.max(0) as u64
        } else {
            end
        }
    }
    fn play(&mut self, shared: &Shared, epoch: u64) -> Result<()> {
        if let Some(consent) = &self.active_media_consent {
            consent.check()?;
        }
        if let Some(grant) = &self.assistant_grant {
            grant.consent.check()?;
        }
        let script_end = self.end_ms();
        let mpv = self.mpv.as_mut().context("Open a video and script first")?;
        let state = mpv.state()?;
        let end = if script_end == 0 {
            state.duration_ms
        } else {
            script_end
        };
        mpv.pause(true)?;
        if state.eof || state.position_ms >= end {
            mpv.seek(0)?;
        }
        let position = mpv.state()?.position_ms;
        ensure!(position < end, "seek before the end of playback");
        if let Some(hsp) = self.hsp.as_mut() {
            shared.check_device_session(epoch, self.armed_until, Instant::now())?;
            hsp.stop()?;
            hsp.ensure_hsp_mode()?;
            shared.check_device_session(epoch, self.armed_until, Instant::now())?;
            let points = if self.sync_offset_ms == 0 {
                self.points.clone()
            } else {
                Arc::new(offset_points(&self.points, self.sync_offset_ms))
            };
            let mut stream = HspStream::begin(hsp, points, position as i64)?;
            while !stream.loaded_until(position + AHEAD_MS) {
                shared.check_device_session(epoch, self.armed_until, Instant::now())?;
                stream.load_until(hsp, position + AHEAD_MS)?;
            }
            shared.check_device_session(epoch, self.armed_until, Instant::now())?;
            let observed = hsp.state()?;
            ensure!(
                observed["points"].as_u64() == Some(stream.sent() as u64),
                "Handy buffer count differs; playback was not started"
            );
            let clock = hsp.server_clock(3)?;
            ensure!(
                clock.rtt_ms < START_LEAD_MS,
                "Handy network is too slow to synchronize; retry Play"
            );
            if let Some(grant) = &self.assistant_grant {
                grant.consent.check()?;
            }
            let start = Instant::now() + Duration::from_millis(START_LEAD_MS);
            shared.check_device_session(epoch, self.armed_until, start)?;
            hsp.play(position as i32, 1.0, &clock.advanced(START_LEAD_MS))?;
            while Instant::now() < start {
                shared.check_device_session(epoch, self.armed_until, start)?;
                thread::sleep(Duration::from_millis(5));
            }
            ensure!(
                Instant::now().duration_since(start) < Duration::from_millis(150),
                "Handy start deadline missed; press Play to retry"
            );
            shared.check_device_session(epoch, self.armed_until, Instant::now())?;
            self.stream = Some(stream);
        }
        ensure!(
            shared.epoch.load(Ordering::SeqCst) == epoch,
            "play cancelled"
        );
        mpv.pause(false)?;
        self.playing = true;
        Ok(())
    }
    fn cancel_media_review(&mut self) {
        self.pending_review = None;
        self.approval.stop();
    }

    fn prepare_media_review(
        &mut self,
        video: PathBuf,
        consent: PathBuf,
        shared: &Shared,
        epoch: u64,
        directory: &std::path::Path,
    ) -> Result<()> {
        self.cancel_media_review();
        let document = read_review_consent(&consent)?;
        document
            .admit_effect(review_now_ns()?)
            .map_err(|reason| anyhow::anyhow!("media review refused: {}", reason.label()))?;
        std::fs::create_dir_all(directory)?;
        let custody = tempfile::Builder::new()
            .prefix("reviewed-")
            .tempdir_in(directory)?;
        let (snapshot, sha256, byte_len) = crate::artifacts::snapshot_reserved(
            &video,
            custody.path(),
            MAX_REVIEWED_VIDEO_BYTES,
            0,
        )?;
        ensure!(byte_len > 0, "reviewed media is empty");
        ensure!(
            shared.epoch.load(Ordering::SeqCst) == epoch,
            "media review cancelled"
        );
        let mut media_sha256 = [0; 32];
        for (index, byte) in media_sha256.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&sha256[index * 2..index * 2 + 2], 16)?;
        }
        let scope = RoleplayEffectScope {
            effect: RoleplayEffect::PresentMedia { media_sha256 },
            participants: document
                .session
                .participants
                .iter()
                .map(|record| record.participant.clone())
                .collect(),
            content_class: document.session.declared_class,
            provider: document.session.declared_provider.clone(),
        };
        let current = read_review_consent(&consent)?;
        ensure!(
            current == document,
            "media review consent changed during preparation"
        );
        let review = self
            .approval
            .propose(&scope, &current, review_now_ns()?)
            .map_err(|reason| anyhow::anyhow!("media review refused: {reason:?}"))?;
        self.pending_review = Some(PendingMediaReview {
            status: DesktopMediaReview {
                review,
                consent_path: consent,
                sha256,
                byte_len,
                video_name: video
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
            },
            scope,
            video,
            snapshot,
            custody,
        });
        Ok(())
    }

    fn confirm_media_review(
        &mut self,
        review: EffectReview,
        shared: &Shared,
        epoch: u64,
        directory: &std::path::Path,
    ) -> Result<()> {
        let pending = self
            .pending_review
            .as_ref()
            .context("no pending media review")?;
        ensure!(
            pending.status.review == review,
            "media review identity changed"
        );
        let pending = self.pending_review.take().expect("review checked");
        let now = review_now_ns()?;
        let current = read_review_consent(&pending.status.consent_path)?;
        self.approval
            .confirm(review, &pending.scope, &current, now)
            .and_then(|()| self.approval.consume(review, &pending.scope, &current, now))
            .map_err(|reason| anyhow::anyhow!("media confirmation refused: {reason:?}"))?;
        ensure!(
            shared.epoch.load(Ordering::SeqCst) == epoch,
            "media review cancelled"
        );
        self.open_media(&pending.snapshot, &pending.video, shared, directory)?;
        self.reviewed_media = Some(pending.custody);
        self.active_media_consent = Some(ActiveConsent {
            path: pending.status.consent_path,
            document: current,
        });
        self.play(shared, epoch)
    }

    fn open_media(
        &mut self,
        video: &std::path::Path,
        display_path: &std::path::Path,
        shared: &Shared,
        directory: &std::path::Path,
    ) -> Result<()> {
        self.close();
        self.mpv = Some(MpvPlayer::open(video, directory)?);
        let mut status = shared
            .status
            .lock()
            .map_err(|_| anyhow::anyhow!("player status unavailable"))?;
        status.video_name = display_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into();
        status.script_name.clear();
        Ok(())
    }

    fn arm(
        &mut self,
        shared: &Shared,
        epoch: u64,
        max_speed: u16,
        min_pct: u8,
        max_pct: u8,
    ) -> Result<()> {
        ensure!(
            self.mpv.is_some() && self.points.len() >= 2,
            "Open a video and script first"
        );
        let api = std::env::var("HANDY_API_KEY")
            .context("Start Pulsar with secrets run: HANDY_API_KEY missing")?;
        let connection = std::env::var("HANDY_CONNECTION_KEY")
            .context("Start Pulsar with secrets run: HANDY_CONNECTION_KEY missing")?;
        ensure!(
            !api.trim().is_empty() && !connection.trim().is_empty(),
            "Handy credential references are empty"
        );
        let mut hsp = HspClient::new(&api, &connection);
        ensure!(
            hsp.connected()?,
            "Handy is offline; connect it and enable again"
        );
        hsp.ensure_hsp_mode()?;
        hsp.stop()?;
        let _ = hsp.user_offset_ms();
        ensure!(
            shared.epoch.load(Ordering::SeqCst) == epoch,
            "enable cancelled"
        );
        self.max_speed = max_speed;
        self.min_pct = min_pct;
        self.max_pct = max_pct;
        self.points();
        self.hsp = Some(hsp);
        self.armed_until = Some(Instant::now() + SESSION_LIFETIME);
        Ok(())
    }

    fn apply(
        &mut self,
        action: Action,
        shared: &Shared,
        epoch: u64,
        directory: &std::path::Path,
    ) -> Result<()> {
        match action {
            Action::Open { video, script } => {
                self.close();
                let meta = std::fs::metadata(&script).context("Script file unavailable")?;
                ensure!(
                    meta.is_file() && meta.len() <= 32 * 1024 * 1024,
                    "script must be a local file under 32 MiB"
                );
                let file = std::fs::File::open(&script)?;
                let script_data: Script =
                    serde_json::from_reader(std::io::Read::take(file, 32 * 1024 * 1024 + 1))
                        .context("Invalid funscript actions")?;
                ensure!(
                    script_data.actions.len() <= 500_000,
                    "script exceeds 500,000 actions"
                );
                ensure!(
                    script_data
                        .actions
                        .iter()
                        .all(|p| (0..=86_400_000).contains(&p.at)),
                    "script times must be within 24 hours"
                );
                self.raw = script_data
                    .actions
                    .into_iter()
                    .map(|p| (p.at, p.pos))
                    .collect();
                self.points();
                ensure!(
                    self.points.len() >= 2,
                    "script needs two distinct action times"
                );
                self.mpv = Some(MpvPlayer::open(&video, directory)?);
                let mut status = shared
                    .status
                    .lock()
                    .map_err(|_| anyhow::anyhow!("player status unavailable"))?;
                status.video_name = video
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();
                status.script_name = script
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();
            }
            Action::OpenMedia { video } => {
                self.open_media(&video, &video, shared, directory)?;
            }
            Action::OpenReviewedMedia { video, sha256 } => {
                self.close();
                std::fs::create_dir_all(directory)?;
                let custody = tempfile::Builder::new()
                    .prefix("reviewed-")
                    .tempdir_in(directory)?;
                let (snapshot, digest, size) = crate::artifacts::snapshot_reserved(
                    &video,
                    custody.path(),
                    MAX_REVIEWED_VIDEO_BYTES,
                    0,
                )?;
                ensure!(
                    size != 0 && digest == sha256,
                    "reviewed video content changed"
                );
                ensure!(
                    shared.epoch.load(Ordering::SeqCst) == epoch,
                    "reviewed video open cancelled"
                );
                self.open_media(&snapshot, &video, shared, directory)?;
                self.reviewed_media = Some(custody);
            }
            Action::PrepareMediaReview { video, consent } => {
                self.prepare_media_review(video, consent, shared, epoch, directory)?;
            }
            Action::ConfirmMediaReview { review } => {
                self.confirm_media_review(review, shared, epoch, directory)?;
            }
            Action::Arm {
                enabled,
                max_speed,
                min_pct,
                max_pct,
            } => {
                self.stop(true)?;
                if enabled {
                    self.arm(shared, epoch, max_speed, min_pct, max_pct)?;
                }
            }
            Action::ArmAssistant {
                consent,
                max_speed,
                min_pct,
                max_pct,
                sync_offset_ms,
            } => {
                self.stop(true)?;
                let document = read_review_consent(&consent)?;
                let approval = ActiveConsent {
                    path: consent,
                    document,
                };
                approval.check()?;
                self.arm(shared, epoch, max_speed, min_pct, max_pct)?;
                approval.check()?;
                self.sync_offset_ms = sync_offset_ms;
                self.assistant_grant = Some(AssistantDeviceGrant {
                    id: SessionId::new(uuid::Uuid::new_v4().to_string())?,
                    consent: approval,
                });
            }
            Action::AssistantPlay { .. } => self.play(shared, epoch)?,
            Action::Play { sync_offset_ms } => {
                self.sync_offset_ms = sync_offset_ms;
                self.play(shared, epoch)?;
            }
            Action::Pause => self.stop(false)?,
            Action::Seek { position_ms } => {
                let resume = self.playing;
                self.stop(false)?;
                self.mpv
                    .as_mut()
                    .context("Open a video first")?
                    .seek(position_ms)?;
                if resume {
                    self.play(shared, epoch)?;
                }
            }
            Action::Stop => {
                self.stop(true)?;
                if let Some(m) = self.mpv.as_mut() {
                    m.seek(0)?;
                }
            }
            Action::Close => {
                self.close();
            }
            Action::Status => {}
        }
        Ok(())
    }
    fn check_assistant_request(&self, action: &Action, shared: &Shared, epoch: u64) -> Result<()> {
        if let Action::AssistantPlay { grant, consent } = action {
            let current = self
                .assistant_grant
                .as_ref()
                .context("No active assistant Handy grant")?;
            ensure!(
                current.id == *grant && current.consent.path == *consent,
                "Assistant Handy grant changed"
            );
            ensure!(self.hsp.is_some(), "Handy is disconnected");
            shared.check_device_session(epoch, self.armed_until, Instant::now())?;
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<(u64, u64, bool)> {
        if let Some(consent) = &self.active_media_consent {
            consent.check()?;
        }
        if let Some(grant) = &self.assistant_grant {
            grant.consent.check()?;
        }
        let Some(mpv) = self.mpv.as_mut() else {
            return Ok((0, 0, false));
        };
        let state = mpv.state()?;
        if self.playing && (state.eof || state.paused) {
            self.stop(false)?;
            return Ok((state.position_ms, state.duration_ms, false));
        }
        if self.playing {
            if let (Some(stream), Some(hsp)) = (self.stream.as_mut(), self.hsp.as_mut()) {
                stream.load_until(hsp, state.position_ms + AHEAD_MS)?;
            }
            let script_end = self.end_ms();
            if script_end != 0 && state.position_ms >= script_end {
                self.stop(false)?;
            }
        }
        Ok((state.position_ms, state.duration_ms, self.playing))
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.close();
    }
}

fn run(shared: Arc<Shared>, directory: PathBuf) {
    let mut player = Player::default();
    let mut seen_epoch = 0;
    let mut last_tick = Instant::now();
    while !shared.shutdown.load(Ordering::SeqCst) {
        let epoch = shared.epoch.load(Ordering::SeqCst);
        if seen_epoch != epoch {
            let _ = player.stop(false);
            seen_epoch = epoch;
        }
        if player.pending_review.as_ref().is_some_and(|pending| {
            review_now_ns().map_or(true, |now| now >= pending.status.review.expires_at_ns)
        }) {
            player.cancel_media_review();
            if let Ok(mut status) = shared.status.lock() {
                status.media_review = None;
            }
        }
        let expired = player.armed_until.is_some_and(|t| t <= Instant::now());
        let lost = shared
            .heartbeat
            .lock()
            .map_or(true, |t| t.elapsed() >= HEARTBEAT_TIMEOUT);
        if (player.playing || player.armed_until.is_some()) && (expired || lost) {
            let _ = player.stop(true);
            if let Ok(mut status) = shared.status.lock() {
                status.message = if lost {
                    "Controller lost; stopped and disarmed"
                } else {
                    "Handy session expired; enable again"
                }
                .into();
            }
        }
        let command = shared.queue.lock().ok().and_then(|mut q| q.pop_front());
        if let Some((generation, action)) = command {
            // A newer interrupt may arrive after this action leaves the queue.
            // Apply an already-dequeued interrupt before processing the newer one.
            if interrupt_priority(&action) == 0 && generation != shared.epoch.load(Ordering::SeqCst)
            {
                continue;
            }
            let closed = matches!(action, Action::Close);
            let proposal = matches!(action, Action::PrepareMediaReview { .. });
            if let Err(error) = player.check_assistant_request(&action, &shared, generation) {
                shared.publish_completion(&player, error.to_string());
                continue;
            }
            let result = player.apply(action, &shared, generation, &directory);
            let message = match result {
                Ok(()) => if player.playing {
                    if player.hsp.is_some() {
                        "Playing video and script"
                    } else {
                        "Playing video"
                    }
                } else if player.armed_until.is_some() {
                    "Handy enabled; press Play"
                } else if player.mpv.is_some() {
                    "Ready — video-only until Handy is enabled"
                } else {
                    "Player closed"
                }
                .to_owned(),
                Err(error) => {
                    if !proposal {
                        let _ = player.stop(true);
                    }
                    format!("{error}").chars().take(400).collect()
                }
            };
            shared.publish_completion(&player, message);
            if closed {
                if let Ok(mut owner) = shared.owner.lock() {
                    *owner = None;
                }
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(50) {
            let was_playing = player.playing;
            let reviewing_media = player.active_media_consent.is_some();
            let result = player.tick();
            if result.is_err() {
                player.close();
            }
            if let Ok(mut status) = shared.status.lock() {
                match result {
                    Ok((position, duration, playing)) => {
                        status.position_ms = position;
                        status.duration_ms =
                            duration.max(player.points.last().map_or(0, |p| p.0 as u64));
                        status.playing = playing;
                        if was_playing && !playing {
                            status.message = "Playback paused or finished".into();
                        }
                    }
                    Err(error) => {
                        status.message = if reviewing_media {
                            format!("Reviewed media closed: {error}")
                                .chars()
                                .take(400)
                                .collect()
                        } else {
                            "Video player closed or unavailable; Handy stopped and disarmed".into()
                        };
                        status.playing = false;
                    }
                }
            }
            last_tick = Instant::now();
        }
        if let Ok(mut status) = shared.status.lock() {
            status.loaded = player.mpv.is_some();
            status.armed = player.armed_until.is_some();
            status.assistant_grant = player
                .assistant_grant
                .as_ref()
                .map(|grant| grant.id.clone());
            status.connected = player.hsp.is_some();
            status.expires_in_ms = player.armed_until.map_or(0, |t| {
                t.saturating_duration_since(Instant::now()).as_millis() as u64
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
    player.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    #[ignore = "requires mpv, ffmpeg and PULSAR_PLAYER_HEADLESS=1"]
    fn video_only_service_plays_and_stops_without_arming() {
        assert_eq!(std::env::var("PULSAR_PLAYER_HEADLESS").as_deref(), Ok("1"));
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("fixture.mp4");
        let mut child = std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:r=10",
                "-t",
                "4",
                "-c:v",
                "mpeg4",
            ])
            .arg(&video)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("ffmpeg fixture deadline exceeded");
            }
            thread::sleep(Duration::from_millis(20));
        }

        let service = Service::new(directory.path().join("player")).unwrap();
        let wait_for = |predicate: fn(&Status) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let status = service.request("test", "status", &Action::Status).unwrap();
                assert!(!status.armed && !status.connected);
                if predicate(&status) {
                    return status;
                }
                assert!(Instant::now() < deadline, "last player status: {status:?}");
                thread::sleep(Duration::from_millis(20));
            }
        };
        service
            .request("test", "open", &Action::OpenMedia { video })
            .unwrap();
        let opened = wait_for(|status| status.loaded && !status.busy);
        assert!(opened.script_name.is_empty());
        service
            .request(
                "test",
                "arm",
                &Action::Arm {
                    enabled: true,
                    max_speed: 150,
                    min_pct: 35,
                    max_pct: 65,
                },
            )
            .unwrap();
        wait_for(|status| !status.busy && status.message == "Open a video and script first");
        service
            .request("test", "play", &Action::Play { sync_offset_ms: 0 })
            .unwrap();
        wait_for(|status| status.playing && status.position_ms >= 200);
        service.request("test", "stop", &Action::Stop).unwrap();
        wait_for(|status| !status.busy && !status.playing && status.position_ms <= 100);
        service.request("test", "close", &Action::Close).unwrap();
        wait_for(|status| !status.busy && !status.loaded);
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires mpv and PULSAR_PLAYER_HEADLESS=1"]
    fn reviewed_media_plays_from_snapshot_after_original_is_replaced() {
        use sha2::{Digest, Sha256};
        assert_eq!(std::env::var("PULSAR_PLAYER_HEADLESS").as_deref(), Ok("1"));
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("fixture.wav");
        // One second of mono PCM, with no external encoder or data source.
        let samples = [0u8; 16_000];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0]);
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        wav.extend_from_slice(&samples);
        std::fs::write(&video, &wav).unwrap();

        let shared = Shared::new();
        let mut player = Player::default();
        player
            .apply(
                Action::OpenReviewedMedia {
                    video: video.clone(),
                    sha256: format!("{:x}", Sha256::digest(&wav)),
                },
                &shared,
                0,
                directory.path(),
            )
            .unwrap();
        std::fs::write(video, b"replacement").unwrap();
        let custody = player.reviewed_media.as_ref().unwrap().path().to_owned();
        let snapshot = std::fs::read_dir(&custody)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(std::fs::read(snapshot).unwrap(), wav);
        player.play(&shared, 0).unwrap();
        player.close();
        assert!(!custody.exists());
    }

    #[cfg(unix)]
    #[test]
    fn media_review_refuses_withdrawn_consent_and_stop_cancels_pending() {
        use pulsar_protocol::{ContentClass, ParticipantRef};
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("fixture.mp4");
        std::fs::write(&video, b"fixture content").unwrap();
        let mut consent = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        let now = review_now_ns().unwrap();
        let mut document = RoleplayConsentDocument::least_permissive();
        document.attest(now);
        document
            .session
            .grant(
                ParticipantRef::new("operator").unwrap(),
                ContentClass::General,
                now,
            )
            .unwrap();
        consent
            .write_all(&serde_json::to_vec(&document).unwrap())
            .unwrap();
        let shared = Shared::new();
        let mut player = Player::default();
        let runtime = directory.path();
        player
            .prepare_media_review(video.clone(), consent.path().into(), &shared, 0, runtime)
            .unwrap();
        let pending = player.pending_review.as_ref().unwrap();
        let review = pending.status.review;
        let custody = pending.custody.path().to_owned();
        player.apply(Action::Stop, &shared, 0, runtime).unwrap();
        assert!(!custody.exists());
        assert!(player
            .confirm_media_review(review, &shared, 0, runtime)
            .is_err());

        player
            .prepare_media_review(video, consent.path().into(), &shared, 0, runtime)
            .unwrap();
        let review = player.pending_review.as_ref().unwrap().status.review;
        let active = ActiveConsent {
            path: consent.path().into(),
            document: document.clone(),
        };
        active.check().unwrap();
        document.withdraw_attestation(review_now_ns().unwrap());
        std::fs::write(consent.path(), serde_json::to_vec(&document).unwrap()).unwrap();
        player.active_media_consent = Some(active);
        assert_eq!(
            player.tick().unwrap_err().to_string(),
            "Session consent changed"
        );
        player.active_media_consent = None;
        let error = player
            .confirm_media_review(review, &shared, 0, runtime)
            .unwrap_err();
        assert!(error.to_string().contains("AdultAttestationWithdrawn"));
        assert!(player.mpv.is_none());
    }

    #[test]
    fn reviewed_media_refuses_substituted_bytes_before_opening_mpv() {
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("substituted.mp4");
        std::fs::write(&video, b"substituted bytes").unwrap();
        let runtime = directory.path().join("player");
        let mut player = Player::default();
        let error = player
            .apply(
                Action::OpenReviewedMedia {
                    video,
                    sha256: "0".repeat(64),
                },
                &Shared::new(),
                0,
                &runtime,
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "reviewed video content changed");
        assert!(player.mpv.is_none());
        assert_eq!(std::fs::read_dir(runtime).unwrap().count(), 0);
    }

    #[test]
    fn stop_retires_arming_even_without_an_active_stream() {
        let shared = Shared::new();
        let mut player = Player::default();
        player.armed_until = Some(Instant::now() + SESSION_LIFETIME);
        player
            .apply(Action::Stop, &shared, 0, std::path::Path::new("unused"))
            .unwrap();
        assert!(player.armed_until.is_none());
    }

    #[test]
    fn assistant_arming_refuses_missing_consent_before_device_contact() {
        let directory = tempfile::tempdir().unwrap();
        let mut player = Player::default();
        assert!(player
            .apply(
                Action::ArmAssistant {
                    consent: directory.path().join("missing.json"),
                    max_speed: 150,
                    min_pct: 35,
                    max_pct: 65,
                    sync_offset_ms: 0,
                },
                &Shared::new(),
                0,
                directory.path()
            )
            .is_err());
        assert!(player.assistant_grant.is_none());
        assert!(player.hsp.is_none());
    }

    #[test]
    fn arming_requires_prepared_script_points_before_device_contact() {
        let shared = Shared::new();
        let mut player = Player::default();
        let failure = player
            .apply(
                Action::Arm {
                    enabled: true,
                    max_speed: 150,
                    min_pct: 35,
                    max_pct: 65,
                },
                &shared,
                0,
                std::path::Path::new("unused"),
            )
            .unwrap_err();
        assert_eq!(failure.to_string(), "Open a video and script first");
        assert!(player.hsp.is_none());
        assert!(player.armed_until.is_none());
    }

    #[test]
    fn an_earlier_completion_cannot_acknowledge_pending_stop() {
        let service = Service {
            shared: Arc::new(Shared::new()),
        };
        let player = Player::default();
        service.request("operator", "stop", &Action::Stop).unwrap();
        service
            .shared
            .publish_completion(&player, "Earlier action completed".into());
        assert!(
            service
                .request("operator", "status", &Action::Status)
                .unwrap()
                .busy
        );
        service.shared.queue.lock().unwrap().pop_front().unwrap();
        service.shared.publish_completion(&player, "Stopped".into());
        assert!(
            !service
                .request("operator", "status", &Action::Status)
                .unwrap()
                .busy
        );
    }

    #[test]
    fn a_later_pause_cannot_discard_stop_or_close() {
        let service = Service {
            shared: Arc::new(Shared::new()),
        };
        for (index, expected) in [(0, Action::Stop), (1, Action::Close)] {
            service
                .request("player", &format!("interrupt-{index}"), &expected)
                .unwrap();
            service
                .request("player", &format!("pause-{index}"), &Action::Pause)
                .unwrap();
            let mut queue = service.shared.queue.lock().unwrap();
            assert_eq!(queue.len(), 1);
            assert_eq!(
                interrupt_priority(&queue.pop_front().unwrap().1),
                interrupt_priority(&expected)
            );
        }
    }

    #[test]
    fn scheduled_device_start_refuses_expiry_disconnect_and_stop() {
        let shared = Shared::new();
        let now = Instant::now();
        let deadline = now + Duration::from_secs(2);
        shared.check_device_session(0, Some(deadline), now).unwrap();
        assert!(shared
            .check_device_session(0, Some(deadline), deadline)
            .is_err());

        *shared.heartbeat.lock().unwrap() = now - HEARTBEAT_TIMEOUT;
        assert!(shared.check_device_session(0, Some(deadline), now).is_err());
        *shared.heartbeat.lock().unwrap() = Instant::now();
        shared.epoch.fetch_add(1, Ordering::SeqCst);
        assert!(shared.check_device_session(0, Some(deadline), now).is_err());
    }

    #[test]
    fn a_media_proposal_does_not_claim_or_steal_playback_ownership() {
        let service = Service {
            shared: Arc::new(Shared::new()),
        };
        let directory = tempfile::tempdir().unwrap();
        let proposal = Action::PrepareMediaReview {
            video: directory.path().join("fixture.mp4"),
            consent: directory.path().join("consent.json"),
        };
        service.request("broker", "first", &proposal).unwrap();
        assert!(service.shared.owner.lock().unwrap().is_none());
        let play = Action::Play { sync_offset_ms: 0 };
        service.request("desktop", "play", &play).unwrap();
        service.request("broker", "second", &proposal).unwrap();
        assert_eq!(
            service.shared.owner.lock().unwrap().as_deref(),
            Some("desktop")
        );
        assert!(service.request("broker", "play", &play).is_err());
    }

    #[test]
    fn stop_supersedes_a_full_queue_and_allows_rejected_requests_to_retry() {
        let service = Service {
            shared: Arc::new(Shared::new()),
        };
        let play = Action::Play { sync_offset_ms: 0 };
        for index in 0..32 {
            service
                .request("player", &index.to_string(), &play)
                .unwrap();
        }
        assert!(service.request("player", "retry", &play).is_err());
        service.request("operator", "stop", &Action::Stop).unwrap();
        service.request("player", "retry", &play).unwrap();

        let queue = service.shared.queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert!(matches!(queue[0], (1, Action::Stop)));
        assert!(matches!(queue[1], (1, Action::Play { .. })));
    }
}
