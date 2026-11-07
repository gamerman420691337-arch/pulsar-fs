//! One ephemeral Desktop playback owner, with bounded HSP buffering and player custody.
use crate::desktop_hsp::{build_points, HspClient, HspStream};
use crate::desktop_mpv::MpvPlayer;
use anyhow::{ensure, Context, Result};
use pulsar_protocol::{
    DesktopPlaybackAction as Action, DesktopPlaybackStatus as Status, ErrorCode, ProtocolError,
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

const AHEAD_MS: u64 = 5_000;
const SESSION_LIFETIME: Duration = Duration::from_secs(20 * 60);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);
const START_LEAD_MS: u64 = 1_500;

struct Shared {
    status: Mutex<Status>,
    queue: Mutex<VecDeque<(u64, Action)>>,
    owner: Mutex<Option<String>>,
    heartbeat: Mutex<Instant>,
    recent: Mutex<VecDeque<(String, String, String)>>,
    epoch: AtomicU64,
    shutdown: AtomicBool,
}

pub(crate) struct Service {
    shared: Arc<Shared>,
}
impl Service {
    pub(crate) fn new(directory: PathBuf) -> Result<Self> {
        let shared = Arc::new(Shared {
            status: Mutex::new(Status::default()),
            queue: Mutex::new(VecDeque::new()),
            owner: Mutex::new(None),
            heartbeat: Mutex::new(Instant::now()),
            recent: Mutex::new(VecDeque::new()),
            epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        });
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
        if owner.is_some() && !is_owner && !emergency {
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
        recent.push_back((session.into(), request.into(), fingerprint));
        while recent.len() > 64 {
            recent.pop_front();
        }
        if owner.is_none() || matches!(action, Action::Open { .. }) {
            *owner = Some(session.into());
        }
        *shared.heartbeat.lock().map_err(|_| internal())? = Instant::now();
        let interrupts = matches!(
            action,
            Action::Stop | Action::Close | Action::Pause | Action::Arm { enabled: false, .. }
        );
        let epoch = if interrupts {
            shared.epoch.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            shared.epoch.load(Ordering::SeqCst)
        };
        let mut queue = shared.queue.lock().map_err(|_| internal())?;
        if queue.len() >= 32 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "player control queue is busy",
            ));
        }
        if interrupts {
            queue.push_front((epoch, action.clone()));
        } else {
            queue.push_back((epoch, action.clone()));
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

struct Player {
    mpv: Option<MpvPlayer>,
    hsp: Option<HspClient>,
    stream: Option<HspStream>,
    raw: Vec<(i64, i32)>,
    points: Arc<Vec<(u32, u8)>>,
    armed_until: Option<Instant>,
    playing: bool,
    max_speed: u16,
    min_pct: u8,
    max_pct: u8,
}
impl Default for Player {
    fn default() -> Self {
        Self {
            mpv: None,
            hsp: None,
            stream: None,
            raw: Vec::new(),
            points: Arc::new(Vec::new()),
            armed_until: None,
            playing: false,
            max_speed: 150,
            min_pct: 35,
            max_pct: 65,
        }
    }
}
impl Player {
    fn stop(&mut self, disarm: bool) -> Result<()> {
        self.playing = false;
        self.stream = None;
        let video = self.mpv.as_mut().map(|m| m.pause(true)).transpose();
        let device = self.hsp.as_mut().map(|h| h.stop()).transpose();
        if disarm {
            self.armed_until = None;
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
    fn play(&mut self, shared: &Shared, epoch: u64) -> Result<()> {
        let mpv = self.mpv.as_mut().context("Open a video and script first")?;
        let position = mpv.state()?.position_ms;
        mpv.pause(true)?;
        ensure!(
            position < self.points.last().map_or(0, |p| p.0 as u64),
            "seek before the end of the script"
        );
        if let Some(hsp) = self.hsp.as_mut() {
            ensure!(
                self.armed_until.is_some_and(|t| t > Instant::now()),
                "Enable Handy for this session first"
            );
            hsp.stop()?;
            hsp.ensure_hsp_mode()?;
            let mut stream = HspStream::begin(hsp, self.points.clone(), position as i64)?;
            while !stream.loaded_until(position + AHEAD_MS) {
                ensure!(
                    shared.epoch.load(Ordering::SeqCst) == epoch,
                    "play cancelled"
                );
                stream.load_until(hsp, position + AHEAD_MS)?;
            }
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
            ensure!(
                shared.epoch.load(Ordering::SeqCst) == epoch,
                "play cancelled"
            );
            let start = Instant::now() + Duration::from_millis(START_LEAD_MS);
            hsp.play(position as i32, 1.0, &clock.advanced(START_LEAD_MS))?;
            while Instant::now() < start {
                ensure!(
                    shared.epoch.load(Ordering::SeqCst) == epoch,
                    "play cancelled"
                );
                thread::sleep(Duration::from_millis(5));
            }
            ensure!(
                Instant::now().duration_since(start) < Duration::from_millis(150),
                "Handy start deadline missed; press Play to retry"
            );
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
            Action::Arm {
                enabled,
                max_speed,
                min_pct,
                max_pct,
            } => {
                self.stop(true)?;
                if enabled {
                    ensure!(self.mpv.is_some(), "Open a video and script first");
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
                }
            }
            Action::Play => self.play(shared, epoch)?,
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
                self.stop(false)?;
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
    fn tick(&mut self) -> Result<(u64, u64, bool)> {
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
            if state.position_ms >= self.points.last().map_or(0, |p| p.0 as u64) {
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
            if generation != shared.epoch.load(Ordering::SeqCst) {
                continue;
            }
            let closed = matches!(action, Action::Close);
            let result = player.apply(action, &shared, generation, &directory);
            let message = match result {
                Ok(()) => if player.playing {
                    "Playing video and script"
                } else if player.armed_until.is_some() {
                    "Handy enabled; press Play"
                } else if player.mpv.is_some() {
                    "Ready — video-only until Handy is enabled"
                } else {
                    "Player closed"
                }
                .to_owned(),
                Err(error) => {
                    let _ = player.stop(true);
                    format!("{error}").chars().take(400).collect()
                }
            };
            if let Ok(mut status) = shared.status.lock() {
                status.message = message;
                status.busy = false;
            }
            if closed {
                if let Ok(mut owner) = shared.owner.lock() {
                    *owner = None;
                }
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(50) {
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
                    }
                    Err(_) => {
                        status.message =
                            "Video player closed or unavailable; Handy stopped and disarmed".into();
                        status.playing = false;
                    }
                }
            }
            last_tick = Instant::now();
        }
        if let Ok(mut status) = shared.status.lock() {
            status.loaded = player.mpv.is_some();
            status.armed = player.armed_until.is_some();
            status.connected = player.hsp.is_some();
            status.expires_in_ms = player.armed_until.map_or(0, |t| {
                t.saturating_duration_since(Instant::now()).as_millis() as u64
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
    player.close();
}
