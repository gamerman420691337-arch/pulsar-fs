//! Responsive presentation for the Engine-owned local video/Handy player.
use crate::{motion::ResponseBody, EngineApi, SessionClient};
use eframe::egui;
use pulsar_protocol::{
    Command, DesktopPlaybackAction, DesktopPlaybackStatus, ProjectId, DESKTOP_SYNC_OFFSET_LIMIT_MS,
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum Lane {
    Normal,
    Urgent,
    Stop,
}
struct Job {
    id: u64,
    action: DesktopPlaybackAction,
    query: bool,
    lane: Lane,
}
struct Reply {
    id: u64,
    query: bool,
    lane: Lane,
    result: Result<DesktopPlaybackStatus, String>,
}

pub(crate) struct PlayerPanel {
    video: String,
    script: String,
    auto_script: bool,
    max_speed: u16,
    min_pct: u8,
    max_pct: u8,
    sync_offset_ms: i16,
    seek_ms: u64,
    seek_edited: bool,
    status: DesktopPlaybackStatus,
    error: Option<String>,
    normal: SyncSender<Job>,
    urgent: SyncSender<Job>,
    stop: SyncSender<Job>,
    replies: Receiver<Reply>,
    normal_pending: usize,
    urgent_pending: usize,
    stop_pending: bool,
    next_id: u64,
    applied_id: u64,
    last_poll: Instant,
}

impl PlayerPanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf) -> Self {
        let (normal, normal_rx) = mpsc::sync_channel(2);
        let (urgent, urgent_rx) = mpsc::sync_channel(4);
        let (stop, stop_rx) = mpsc::sync_channel(1);
        let (reply_tx, replies) = mpsc::channel();
        let mut error = None;
        for (name, requests) in [
            ("pulsar-player-rpc", normal_rx),
            ("pulsar-player-urgent", urgent_rx),
            ("pulsar-player-stop", stop_rx),
        ] {
            let endpoint = endpoint.clone();
            let bootstrap = bootstrap.clone();
            let reply_tx = reply_tx.clone();
            if let Err(failure) = std::thread::Builder::new()
                .name(name.into())
                .spawn(move || worker(endpoint, bootstrap, requests, reply_tx))
            {
                error = Some(format!("Could not start player controls: {failure}"));
            }
        }
        let video = std::env::var_os("PULSAR_DEMO_VIDEO")
            .map(|p| PathBuf::from(p).to_string_lossy().into_owned())
            .unwrap_or_default();
        let script = std::env::var_os("PULSAR_DEMO_SCRIPT")
            .map(|p| PathBuf::from(p).to_string_lossy().into_owned());
        let auto_script = script.is_none();
        let script = script.unwrap_or_else(|| adjacent_script(&video));
        Self {
            video,
            script,
            auto_script,
            max_speed: 150,
            min_pct: 35,
            max_pct: 65,
            sync_offset_ms: 0,
            seek_ms: 0,
            seek_edited: false,
            status: DesktopPlaybackStatus::default(),
            error,
            normal,
            urgent,
            stop,
            replies,
            normal_pending: 0,
            urgent_pending: 0,
            stop_pending: false,
            next_id: 1,
            applied_id: 0,
            last_poll: Instant::now(),
        }
    }

    fn rpc_pending(&self) -> bool {
        self.normal_pending != 0 || self.urgent_pending != 0 || self.stop_pending
    }

    pub(crate) fn busy(&self) -> bool {
        self.rpc_pending() || self.status.busy
    }

    pub(crate) fn needs_repaint(&self) -> bool {
        self.status.loaded || self.status.media_review.is_some() || self.busy()
    }

    pub(crate) fn session_controls(&mut self, ui: &mut egui::Ui) {
        if !self.status.armed
            && !self.status.playing
            && self.status.media_review.is_none()
            && !self.busy()
        {
            return;
        }
        if self.status.armed {
            ui.label(format!(
                "{} · {} s left",
                if self.status.assistant_grant.is_some() {
                    "Assistant Handy control"
                } else {
                    "Handy enabled"
                },
                self.status.expires_in_ms.div_ceil(1000)
            ));
        }
        if ui.button(egui::RichText::new("STOP").strong()).clicked() {
            self.send(DesktopPlaybackAction::Stop, Lane::Stop);
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::YELLOW, error);
        }
    }

    fn send(&mut self, action: DesktopPlaybackAction, lane: Lane) {
        if matches!(lane, Lane::Stop) && self.stop_pending {
            return;
        }
        let query = matches!(action, DesktopPlaybackAction::Status);
        let job = Job {
            id: self.next_id,
            action,
            query,
            lane,
        };
        self.next_id = self.next_id.saturating_add(1);
        let sender = match lane {
            Lane::Normal => &self.normal,
            Lane::Urgent => &self.urgent,
            Lane::Stop => &self.stop,
        };
        match sender.try_send(job) {
            Ok(()) => {
                match lane {
                    Lane::Normal => self.normal_pending += 1,
                    Lane::Urgent => self.urgent_pending += 1,
                    Lane::Stop => self.stop_pending = true,
                }
                if !query {
                    self.error = None;
                }
            }
            Err(_) if query => {}
            Err(_) => self.error = Some("Player controls are busy or unavailable.".into()),
        }
    }

    pub(crate) fn poll(&mut self, watch_reviews: bool) {
        loop {
            match self.replies.try_recv() {
                Ok(reply) => {
                    match reply.lane {
                        Lane::Normal => self.normal_pending = self.normal_pending.saturating_sub(1),
                        Lane::Urgent => self.urgent_pending = self.urgent_pending.saturating_sub(1),
                        Lane::Stop => self.stop_pending = false,
                    }
                    if reply.id < self.applied_id {
                        continue;
                    }
                    self.applied_id = reply.id;
                    match reply.result {
                        Ok(status) => {
                            if !reply.query {
                                self.error = None;
                                self.seek_edited = false;
                            }
                            if !self.seek_edited {
                                self.seek_ms = status.position_ms;
                            }
                            self.status = status;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if (watch_reviews
            || self.status.loaded
            || self.status.busy
            || self.status.media_review.is_some())
            && !self.rpc_pending()
            && self.last_poll.elapsed() >= Duration::from_millis(250)
        {
            self.last_poll = Instant::now();
            self.send(DesktopPlaybackAction::Status, Lane::Normal);
        }
    }

    fn open(&mut self) {
        if self.auto_script || self.script.trim().is_empty() {
            self.script = adjacent_script(&self.video);
        }
        let video = PathBuf::from(self.video.trim());
        let script = PathBuf::from(self.script.trim());
        if !video.is_absolute() || !script.is_absolute() {
            self.error = Some("Choose absolute local video and funscript paths.".into());
            return;
        }
        self.send(DesktopPlaybackAction::Open { video, script }, Lane::Normal);
    }

    fn media_review_ui(&mut self, ui: &mut egui::Ui, consent: &Path) {
        if let Some(review) = self.status.media_review.clone() {
            ui.label(format!(
                "Present {} locally with video and audio. Handy will be disabled.",
                review.video_name
            ));
            ui.small(format!(
                "{} bytes · SHA-256 {}",
                review.byte_len, review.sha256
            ));
            let same_selection = consent == review.consent_path.as_path();
            if !same_selection {
                ui.label("The consent selection changed. Review the video again to continue.");
            }
            if ui
                .add_enabled(
                    same_selection && !self.busy(),
                    egui::Button::new("Confirm and play"),
                )
                .clicked()
            {
                self.send(
                    DesktopPlaybackAction::ConfirmMediaReview {
                        review: review.review,
                    },
                    Lane::Normal,
                );
            }
        }
        let video = PathBuf::from(self.video.trim());
        if ui
            .add_enabled(
                video.is_absolute() && consent.is_absolute() && !self.busy(),
                egui::Button::new("Review video for presentation"),
            )
            .clicked()
        {
            self.send(
                DesktopPlaybackAction::PrepareMediaReview {
                    video,
                    consent: consent.to_path_buf(),
                },
                Lane::Normal,
            );
        }
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, consent: &Path, project: Option<&ProjectId>) {
        ui.heading("Video + Handy player");
        ui.label("Pulsar controls the video, audio and script. Video opens in an mpv window.");
        ui.small("Drop local files here, or paste their absolute paths. No model is needed.");
        for file in ui.input(|input| input.raw.dropped_files.clone()) {
            if let Some(path) = file.path {
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("funscript"))
                {
                    self.script = path.to_string_lossy().into_owned();
                    self.auto_script = false;
                } else {
                    self.video = path.to_string_lossy().into_owned();
                    if self.auto_script {
                        self.script = adjacent_script(&self.video);
                    }
                }
            }
        }
        egui::Grid::new("desktop-player-files")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Local video");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.video)
                            .char_limit(4096)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                    && self.auto_script
                {
                    self.script = adjacent_script(&self.video);
                }
                ui.end_row();
                ui.label("Funscript");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.script)
                            .char_limit(4096)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.auto_script = self.script.trim().is_empty();
                }
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Open video + script"))
                .clicked()
            {
                self.open();
            }
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Open video only"))
                .clicked()
            {
                let video = PathBuf::from(self.video.trim());
                if video.is_absolute() {
                    self.send(DesktopPlaybackAction::OpenMedia { video }, Lane::Normal);
                } else {
                    self.error = Some("Choose an absolute local video path.".into());
                }
            }
            if ui
                .add_enabled(
                    self.status.loaded || self.status.busy,
                    egui::Button::new("Close player"),
                )
                .clicked()
            {
                self.send(DesktopPlaybackAction::Close, Lane::Urgent);
            }
            if self.busy() {
                ui.spinner();
            }
        });
        if consent.is_absolute() || self.status.media_review.is_some() {
            self.media_review_ui(ui, consent);
        }
        ui.separator();
        let assistant_project = project.filter(|_| consent.is_absolute());
        ui.horizontal(|ui| {
            let mut enabled = self.status.armed;
            if ui
                .add_enabled(
                    self.status.loaded
                        && !self.status.script_name.is_empty()
                        && (!self.busy() || enabled),
                    egui::Checkbox::new(
                        &mut enabled,
                        if self.status.assistant_grant.is_some()
                            || (!self.status.armed && assistant_project.is_some())
                        {
                            "Enable Handy for assistant"
                        } else {
                            "Enable Handy"
                        },
                    ),
                )
                .changed()
            {
                let lane = if enabled { Lane::Normal } else { Lane::Urgent };
                let action = if let Some(project) = assistant_project.filter(|_| enabled) {
                    DesktopPlaybackAction::ArmAssistant {
                        project: project.clone(),
                        consent: consent.to_path_buf(),
                        max_speed: self.max_speed,
                        min_pct: self.min_pct,
                        max_pct: self.max_pct,
                        sync_offset_ms: self.sync_offset_ms,
                    }
                } else {
                    DesktopPlaybackAction::Arm {
                        enabled,
                        max_speed: self.max_speed,
                        min_pct: self.min_pct,
                        max_pct: self.max_pct,
                    }
                };
                self.send(action, lane);
            }
            let state = if self.status.playing {
                "Playing"
            } else if self.status.assistant_grant.is_some() {
                "Assistant Handy control enabled"
            } else if self.status.armed {
                "Handy enabled"
            } else if self.status.loaded {
                "Paused / Handy disabled"
            } else {
                "No media loaded"
            };
            ui.strong(state);
            if self.status.armed {
                ui.label(format!(
                    "Session expires in {} s",
                    self.status.expires_in_ms / 1000
                ));
            }
        });
        ui.add_enabled_ui(!self.status.armed && !self.busy(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Speed cap");
                ui.add(
                    egui::DragValue::new(&mut self.max_speed)
                        .range(1..=300)
                        .suffix(" units/s"),
                );
                ui.label("Stroke range");
                ui.add(
                    egui::DragValue::new(&mut self.min_pct)
                        .range(0..=99)
                        .suffix("%"),
                );
                ui.label("to");
                ui.add(
                    egui::DragValue::new(&mut self.max_pct)
                        .range(1..=100)
                        .suffix("%"),
                );
            });
        });
        ui.add_enabled_ui(!self.status.playing && !self.busy(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Handy sync offset");
                ui.add(
                    egui::DragValue::new(&mut self.sync_offset_ms)
                        .range(-DESKTOP_SYNC_OFFSET_LIMIT_MS..=DESKTOP_SYNC_OFFSET_LIMIT_MS)
                        .suffix(" ms"),
                );
                if ui.button("Reset").clicked() {
                    self.sync_offset_ms = 0;
                }
                ui.small("Negative = earlier, positive = later");
            });
        });
        ui.small("Credentials: HANDY_API_KEY and HANDY_CONNECTION_KEY, supplied by secrets run.");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.status.loaded && !self.busy(),
                    egui::Button::new("Play"),
                )
                .clicked()
            {
                self.send(
                    DesktopPlaybackAction::Play {
                        sync_offset_ms: self.sync_offset_ms,
                    },
                    Lane::Normal,
                );
            }
            if ui
                .add_enabled(self.status.loaded, egui::Button::new("Pause"))
                .clicked()
            {
                self.send(DesktopPlaybackAction::Pause, Lane::Urgent);
            }
            if ui.button(egui::RichText::new("STOP").strong()).clicked() {
                self.send(DesktopPlaybackAction::Stop, Lane::Stop);
            }
            ui.label(format!(
                "{:.2} / {:.2} s",
                self.status.position_ms as f64 / 1000.0,
                self.status.duration_ms as f64 / 1000.0
            ));
        });
        ui.add_enabled_ui(self.status.loaded, |ui| {
            let end = self.status.duration_ms.max(1);
            self.seek_ms = self.seek_ms.min(end);
            if ui
                .add(egui::Slider::new(&mut self.seek_ms, 0..=end).text("Seek (ms)"))
                .changed()
            {
                self.seek_edited = true;
            }
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Apply seek"))
                .clicked()
            {
                self.send(
                    DesktopPlaybackAction::Seek {
                        position_ms: self.seek_ms,
                    },
                    Lane::Normal,
                );
            }
        });
        if self.status.loaded {
            ui.label(format!(
                "{} · {}",
                self.status.video_name, self.status.script_name
            ));
        }
        if !self.status.message.is_empty() {
            ui.label(&self.status.message);
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if self.needs_repaint() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl Drop for PlayerPanel {
    fn drop(&mut self) {
        // Never join or perform IPC on the GUI thread; the Engine also expires a
        // lost controller's heartbeat independently of this best-effort close.
        let _ = self.urgent.try_send(Job {
            id: self.next_id,
            action: DesktopPlaybackAction::Close,
            query: false,
            lane: Lane::Urgent,
        });
    }
}

fn adjacent_script(video: &str) -> String {
    if video.trim().is_empty() {
        String::new()
    } else {
        Path::new(video.trim())
            .with_extension("funscript")
            .to_string_lossy()
            .into_owned()
    }
}

fn worker(
    endpoint: PathBuf,
    bootstrap: PathBuf,
    requests: Receiver<Job>,
    replies: mpsc::Sender<Reply>,
) {
    let mut client: Option<SessionClient> = None;
    while let Ok(job) = requests.recv() {
        let result = (|| -> anyhow::Result<DesktopPlaybackStatus> {
            if client.is_none() {
                client = Some(SessionClient::connect(&endpoint, &bootstrap)?);
            }
            let directory = bootstrap
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Engine state directory is missing"))?;
            let owner_proof =
                crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
            let client = client.as_mut().expect("connected client");
            let budget = match job.lane {
                Lane::Normal => Duration::from_secs(15),
                Lane::Urgent | Lane::Stop => Duration::from_secs(5),
            };
            client.set_operation_deadline(Some(Instant::now() + budget));
            match client.execute(
                Command::DesktopPlayback {
                    action: job.action,
                    owner_proof,
                },
                None,
                None,
            )? {
                ResponseBody::DesktopPlayback(status) => Ok(*status),
                _ => anyhow::bail!("Engine returned an unexpected player response"),
            }
        })()
        .map_err(|error| error.to_string().chars().take(1000).collect());
        if replies
            .send(Reply {
                id: job.id,
                query: job.query,
                lane: job.lane,
                result,
            })
            .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturated_urgent_queue_does_not_block_stop() {
        let (normal, _normal_rx) = mpsc::sync_channel(0);
        let (urgent, _urgent_rx) = mpsc::sync_channel(0);
        let (stop, stop_rx) = mpsc::sync_channel(1);
        let (reply_tx, replies) = mpsc::channel();
        let mut panel = PlayerPanel {
            video: String::new(),
            script: String::new(),
            auto_script: false,
            max_speed: 150,
            min_pct: 35,
            max_pct: 65,
            sync_offset_ms: 0,
            seek_ms: 0,
            seek_edited: false,
            status: DesktopPlaybackStatus::default(),
            error: None,
            normal,
            urgent,
            stop,
            replies,
            normal_pending: 0,
            urgent_pending: 0,
            stop_pending: false,
            next_id: 1,
            applied_id: 0,
            last_poll: Instant::now(),
        };

        panel.send(DesktopPlaybackAction::Pause, Lane::Urgent);
        assert!(panel.error.is_some());
        panel.send(DesktopPlaybackAction::Stop, Lane::Stop);
        panel.send(DesktopPlaybackAction::Stop, Lane::Stop);
        let job = stop_rx.try_recv().unwrap();
        assert!(matches!(job.action, DesktopPlaybackAction::Stop));
        assert!(stop_rx.try_recv().is_err());
        assert!(panel.stop_pending);

        reply_tx
            .send(Reply {
                id: job.id,
                query: false,
                lane: Lane::Stop,
                result: Ok(DesktopPlaybackStatus::default()),
            })
            .unwrap();
        panel.poll(false);
        assert!(!panel.stop_pending);
    }

    #[test]
    fn adjacent_script_keeps_the_selected_local_directory() {
        assert_eq!(adjacent_script("/media/clip.mp4"), "/media/clip.funscript");
        assert_eq!(
            adjacent_script("  /media/clip.webm  "),
            "/media/clip.funscript"
        );
        assert_eq!(adjacent_script(""), "");
    }
}
