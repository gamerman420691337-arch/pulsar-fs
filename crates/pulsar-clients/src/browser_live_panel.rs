//! Desktop ownership of the loopback player and explicitly selected Handy controls.
use crate::{browser_live, session::SessionClient};
use anyhow::{ensure, Result};
use eframe::egui;
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::JoinHandle,
};

type Binding = (ProjectId, RevisionId);

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionsSource {
    File,
    Generator,
    Pack,
}

enum LaunchOptions {
    File(PathBuf),
    Captured(Box<LiveCaptureOptions>),
}

struct LiveForm {
    pack: Option<ModelPackChoice>,
    pack_profile: Option<DeclaredInferenceProfile>,
    inference: Option<LiveCaptureInference>,
    maximum_age_ms: u64,
    added_buffer_ms: u64,
    lease_ms: u32,
    project_start_ms: u64,
}
impl Default for LiveForm {
    fn default() -> Self {
        Self {
            pack: None,
            pack_profile: None,
            inference: None,
            maximum_age_ms: 1000,
            added_buffer_ms: 0,
            lease_ms: 30_000,
            project_start_ms: 0,
        }
    }
}
impl LiveForm {
    fn build(&self) -> Result<LiveCaptureOptions> {
        let (profile, inference) = if let Some(choice) = &self.pack {
            choice.validate()?;
            let profile = self
                .pack_profile
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Capture the selected pack profile first"))?;
            (profile, None)
        } else {
            let inference = self.inference.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Capture an explicit Generator model/profile first")
            })?;
            (&inference.profile, Some(inference.clone()))
        };
        profile.validate()?;
        let input = &profile.preprocessing.input;
        ensure!(
            input.decoder == DetectorDecoder::TemporalScriptPosition
                && input.width == 64
                && input.height == 64,
            "Browser frames require an explicit 64×64 temporal script-position profile"
        );
        ensure!(
            (1..=30_000).contains(&self.maximum_age_ms) && self.added_buffer_ms <= 30_000,
            "Preview age must be 1..=30000 ms and buffer 0..=30000 ms"
        );
        let project_ns = self
            .project_start_ms
            .checked_mul(1_000_000)
            .and_then(|v| i64::try_from(v).ok())
            .ok_or_else(|| anyhow::anyhow!("Project offset exceeds checked nanoseconds"))?;
        let options = LiveCaptureOptions {
            inference_fallback: None,
            browser_analysis: None,
            browser_media: None,
            model_pack: self.pack.clone(),
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::Immediate,
            encoded_source: None,
            inference,
            policy: LivePolicy {
                maximum_analysis_age_ns: self.maximum_age_ms * 1_000_000,
                added_buffer_ns: self.added_buffer_ms * 1_000_000,
            },
            source_anchor: SourceTimestamp::new(0, 1)?,
            project_anchor: ProjectTime::from_nanos(project_ns),
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: self.lease_ms,
        };
        options.validate()?;
        Ok(options)
    }
}

struct Running {
    binding: Binding,
    cancelled: Arc<AtomicBool>,
    ready: mpsc::Receiver<String>,
    thread: JoinHandle<Result<(), String>>,
}

pub(crate) struct BrowserLivePanel {
    endpoint: PathBuf,
    bootstrap: PathBuf,
    options_path: String,
    options_source: OptionsSource,
    guard_path: String,
    allow_guard_monitoring: bool,
    playback_path: String,
    allow_playback_controls: bool,
    hsp_path: String,
    allow_hsp_controls: bool,
    form: LiveForm,
    duration_seconds: u16,
    running: Option<Running>,
    url: Option<String>,
    status: String,
}

impl BrowserLivePanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf) -> Self {
        Self {
            endpoint,
            bootstrap,
            options_path: String::new(),
            options_source: OptionsSource::File,
            guard_path: String::new(),
            allow_guard_monitoring: false,
            playback_path: String::new(),
            allow_playback_controls: false,
            hsp_path: String::new(),
            allow_hsp_controls: false,
            form: LiveForm::default(),
            duration_seconds: 300,
            running: None,
            url: None,
            status: "Stopped".into(),
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.running.is_some()
    }

    fn capture_guard(&self) -> Result<Option<crate::browser_player_control::GuardConfig>> {
        let path = self.guard_path.trim();
        if path.is_empty() && !self.allow_guard_monitoring {
            return Ok(None);
        }
        ensure!(
            self.allow_guard_monitoring,
            "Explicitly allow Handy stop guard monitoring before launch"
        );
        ensure!(!path.is_empty(), "Choose a Handy stop guard JSON file");
        let path = std::path::Path::new(path);
        ensure!(
            path.is_absolute(),
            "Handy stop guard JSON path must be absolute"
        );
        let guard: crate::browser_player_control::GuardConfig =
            serde_json::from_slice(&crate::inference::read_bounded_profile(path)?)?;
        guard.validate()?;
        Ok(Some(guard))
    }

    fn capture_playback(&self) -> Result<Option<crate::browser_handy_playback::Config>> {
        let path = self.playback_path.trim();
        if path.is_empty() && !self.allow_playback_controls {
            return Ok(None);
        }
        ensure!(
            self.allow_playback_controls && !path.is_empty(),
            "Prepared playback controls require a selected file and explicit enablement"
        );
        let path = std::path::Path::new(path);
        ensure!(
            path.is_absolute(),
            "Prepared playback configuration path must be absolute"
        );
        let config: crate::browser_handy_playback::Config =
            serde_json::from_slice(&crate::inference::read_bounded_profile(path)?)?;
        config.validate()?;
        Ok(Some(config))
    }

    fn capture_hsp(&self) -> Result<Option<crate::browser_handy_hsp::Config>> {
        let path = self.hsp_path.trim();
        if path.is_empty() && !self.allow_hsp_controls {
            return Ok(None);
        }
        ensure!(
            self.allow_hsp_controls && !path.is_empty(),
            "Handy HSP controls require a configuration file and explicit enablement"
        );
        Ok(Some(crate::read_browser_hsp_config(std::path::Path::new(
            path,
        ))?))
    }

    fn start(&mut self, binding: Binding, repaint: egui::Context) -> Result<()> {
        ensure!(
            !self.busy(),
            "Stop and join the existing browser player before launching another"
        );
        ensure!(
            (1..=600).contains(&self.duration_seconds),
            "Preview duration must be 1..=600 seconds"
        );
        crate::validate_browser_handy_modes(
            !self.guard_path.trim().is_empty() || self.allow_guard_monitoring,
            !self.playback_path.trim().is_empty() || self.allow_playback_controls,
            !self.hsp_path.trim().is_empty() || self.allow_hsp_controls,
        )?;
        let guard = self.capture_guard()?;
        let playback = self.capture_playback()?;
        let hsp = self.capture_hsp()?;
        let launch_options = match self.options_source {
            OptionsSource::File => {
                ensure!(
                    !self.options_path.trim().is_empty(),
                    "Choose an explicit live-options JSON file"
                );
                LaunchOptions::File(crate::absolute_path(std::path::Path::new(
                    self.options_path.trim(),
                ))?)
            }
            OptionsSource::Generator | OptionsSource::Pack => {
                ensure!(
                    (self.options_source == OptionsSource::Pack) == self.form.pack.is_some(),
                    "Capture the explicitly selected model source before launch"
                );
                LaunchOptions::Captured(Box::new(self.form.build()?))
            }
        };
        let endpoint = self.endpoint.clone();
        let bootstrap = self.bootstrap.clone();
        let duration_seconds = self.duration_seconds;
        let captured = binding.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        let (sender, ready) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("pulsar-browser-player".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    let options = match launch_options {
                        LaunchOptions::File(path) => {
                            serde_json::from_slice(&crate::inference::read_bounded_profile(&path)?)?
                        }
                        LaunchOptions::Captured(options) => *options,
                    };
                    options.validate()?;
                    if cancellation.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let client = SessionClient::connect(&endpoint, &bootstrap)?;
                    browser_live::run_controlled(
                        client,
                        captured.0,
                        captured.1,
                        options,
                        browser_live::ServerControl {
                            port: 0,
                            duration_seconds,
                            media_lease_ms: None,
                            guard,
                            playback,
                            hsp,
                            cancelled: cancellation,
                            ready: |url| {
                                sender.send(url)?;
                                repaint.request_repaint();
                                Ok(())
                            },
                        },
                    )
                })()
                .map_err(|e| e.to_string().chars().take(1000).collect());
                repaint.request_repaint();
                result
            })?;
        self.running = Some(Running {
            binding,
            cancelled,
            ready,
            thread,
        });
        self.url = None;
        self.status = "Opening browser player; no capture has been started".into();
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        if let Some(running) = &self.running {
            running.cancelled.store(true, Ordering::Release);
            self.url = None;
            self.status =
                "Stopping browser player; waiting for the current operation and capture cleanup"
                    .into();
        }
    }

    pub(crate) fn poll(&mut self, current: Option<Binding>) {
        let Some(running) = &self.running else {
            return;
        };
        if current.as_ref() != Some(&running.binding) && !running.cancelled.load(Ordering::Acquire)
        {
            self.stop();
        }
        let running = self
            .running
            .as_ref()
            .expect("running retained until joined");
        if let Ok(url) = running.ready.try_recv() {
            if !running.cancelled.load(Ordering::Acquire) {
                self.url = Some(url);
                self.status =
                    "Ready. Open the player, select media and press Play to start capture".into();
            }
        }
        if running.thread.is_finished() {
            let running = self.running.take().expect("finished server retained");
            self.url = None;
            self.status = match running.thread.join() {
                Ok(Ok(())) => "Browser player stopped and joined".into(),
                Ok(Err(error)) => format!("Browser player stopped with an error: {error}. Inspect retained state before retrying"),
                Err(_) => "Browser player failed; capture outcomes may be unknown. Inspect retained state before retrying".into(),
            };
        }
    }

    fn capture_generator(&mut self, inference: LiveCaptureInference) -> Result<()> {
        ensure!(
            !self.busy(),
            "Stop and join the current player before changing its configuration"
        );
        let checked = LiveForm {
            pack: None,
            pack_profile: None,
            inference: Some(inference),
            maximum_age_ms: self.form.maximum_age_ms,
            added_buffer_ms: self.form.added_buffer_ms,
            lease_ms: self.form.lease_ms,
            project_start_ms: self.form.project_start_ms,
        };
        checked.build()?;
        self.form = checked;
        self.options_source = OptionsSource::Generator;
        self.status = "Generator model/profile captured. Review the identities and live settings before launch".into();
        Ok(())
    }

    fn capture_pack(
        &mut self,
        choice: ModelPackChoice,
        profile: DeclaredInferenceProfile,
    ) -> Result<()> {
        ensure!(
            !self.busy(),
            "Stop and join the current player before changing its configuration"
        );
        let checked = LiveForm {
            pack: Some(choice),
            pack_profile: Some(profile),
            inference: None,
            maximum_age_ms: self.form.maximum_age_ms,
            added_buffer_ms: self.form.added_buffer_ms,
            lease_ms: self.form.lease_ms,
            project_start_ms: self.form.project_start_ms,
        };
        checked.build()?;
        self.form = checked;
        self.options_source = OptionsSource::Pack;
        self.status="Owner-selected pack choice/profile captured. The Engine rechecks selection and approval for every fresh lease".into();
        Ok(())
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        current: Option<Binding>,
        generator: Option<LiveCaptureInference>,
        pack: Option<(ModelPackChoice, DeclaredInferenceProfile)>,
    ) {
        ui.heading("Browser live player");
        ui.label("Launch the local player for this project's current revision with an explicit live model/profile.");
        let busy = self.busy();
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.options_source, OptionsSource::Generator, "Generator model/profile");
                ui.selectable_value(&mut self.options_source, OptionsSource::Pack, "Owner-selected pack");
                ui.selectable_value(&mut self.options_source, OptionsSource::File, "Options JSON file");
            });
            if self.options_source == OptionsSource::File {
                ui.label("Live-options JSON path");
                ui.text_edit_singleline(&mut self.options_path);
            } else {
                if self.options_source == OptionsSource::Pack {
                    ui.label("Refresh the current pack selection in Generator, then capture its choice/profile here.");
                    if ui.add_enabled(pack.is_some(),egui::Button::new("Capture owner-selected pack")).clicked() {
                        if let Some((choice,profile))=pack {
                            if let Err(error)=self.capture_pack(choice,profile) { self.status=format!("Captured configuration unchanged: {error}"); }
                        }
                    }
                    if let Some(choice)=&self.form.pack {
                        ui.label(format!("Captured pack: {} / selection revision {}",choice.manifest_sha256,choice.selection_revision));
                        if let Some(profile)=&self.form.pack_profile {
                            ui.label(format!("Model SHA-256: {} / runtime SHA-256: {} / {:?}",profile.model.sha256,profile.runtime.sha256,profile.provider));
                        }
                    }
                } else {
                ui.label("In Generator, enable Model file analysis and load an explicit 64×64 temporal profile. Capture that choice here.");
                if ui.add_enabled(generator.is_some(), egui::Button::new("Capture Generator model/profile")).clicked() {
                    if let Some(inference) = generator {
                        if let Err(error) = self.capture_generator(inference) {
                            self.status = format!("Captured configuration unchanged: {error}");
                        }
                    }
                }
                if let Some(inference) = &self.form.inference {
                    ui.label(format!("Captured model: {}", inference.model_path.display()));
                    ui.label(format!("Model SHA-256: {}", inference.profile.model.sha256));
                    ui.label(format!("Runtime SHA-256: {} / {:?}", inference.profile.runtime.sha256, inference.profile.provider));
                    ui.small("This captured choice stays fixed until you capture another. Engine admission checks exact bytes at Start.");
                }
                }
                for (label, value, range) in [
                    ("Maximum analysis age (ms)", &mut self.form.maximum_age_ms, 1..=30_000),
                    ("Added preview buffer (ms)", &mut self.form.added_buffer_ms, 0..=30_000),
                    ("Project start offset (ms)", &mut self.form.project_start_ms, 0..=i64::MAX as u64 / 1_000_000),
                ] {
                    ui.horizontal(|ui| { ui.label(label); ui.add(egui::DragValue::new(value).range(range)); });
                }
                ui.horizontal(|ui| { ui.label("Capture lease (ms)"); ui.add(egui::DragValue::new(&mut self.form.lease_ms).range(1..=30_000)); });
                ui.small("These are preview admission settings, not qualified latency or device limits. The browser supplies its current presentation timestamp at Play.");
            }
            ui.horizontal(|ui| {
                ui.label("Server lifetime (seconds)");
                ui.add(egui::DragValue::new(&mut self.duration_seconds).range(1..=600));
            });
            ui.separator();
            ui.label("Handy stop guard (optional)");
            ui.label("Absolute guard JSON path");
            if ui.text_edit_singleline(&mut self.guard_path).changed() {
                self.allow_guard_monitoring = false;
            }
            ui.checkbox(
                &mut self.allow_guard_monitoring,
                "Allow monitoring and stopping the selected already-playing Handy controller",
            );
            ui.small("The file specifies the exact controller selection and guard policy. Keep owner proofs and credentials out of this file. The guard starts after explicit browser playback and does not start or synchronize the device.");
            ui.separator();
            ui.label("Prepared Handy browser Start (optional)");
            ui.label("Absolute prepared controller/browser dossier JSON path");
            if ui.text_edit_singleline(&mut self.playback_path).changed() { self.allow_playback_controls = false; }
            ui.checkbox(&mut self.allow_playback_controls,"Enable explicit browser Inspect, Start and Stop controls");
            ui.small("Video Play never actuates. Start requires a separate environment acknowledgment, play consent and ongoing online inspection consent. Prepared controller/clock input can expire during setup; stale input requires explicit fresh preparation.");
            ui.separator();
            ui.label("Handy HSP controls (optional)");
            ui.label("Absolute native HSP configuration JSON path");
            if ui.text_edit_singleline(&mut self.hsp_path).changed() { self.allow_hsp_controls = false; }
            ui.checkbox(&mut self.allow_hsp_controls, "Enable explicit Handy HSP controls");
            ui.small("The native file selects the device session and accepted device/source dossiers. Keep credentials and owner proofs out of it. Launching the player does not start the device.");
            ui.small("Choose only one Handy mode: stop guard, prepared browser Start, or HSP controls.");
        });
        if let Some(running) = &self.running {
            ui.label(format!(
                "Launch project: {} / revision {}",
                running.binding.0,
                running.binding.1.value()
            ));
        }
        if ui
            .add_enabled(
                !busy && current.is_some(),
                egui::Button::new("Launch browser player"),
            )
            .clicked()
        {
            if let Some(binding) = current {
                if let Err(error) = self.start(binding, ui.ctx().clone()) {
                    self.status = error.to_string();
                }
            }
        }
        if let Some(url) = &self.url {
            ui.hyperlink_to("Open local browser player", url);
        }
        if ui
            .add_enabled(busy, egui::Button::new("Stop browser player"))
            .clicked()
        {
            self.stop();
        }
        ui.label(&self.status);
        ui.small("Changing project/revision stops the owned server. Closing the desktop waits for it to finish. Stop waits for any operation already in flight; lost Engine outcomes remain unknown and leases are the fallback.");
        ui.colored_label(egui::Color32::YELLOW, "Experimental preview and recording. Explicit model required. The optional Handy stop guard monitors an already-playing controller; it does not synchronize playback. Tab-sharing time is not the original video's playback clock.");
    }
}

impl Drop for BrowserLivePanel {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.cancelled.store(true, Ordering::Release);
            // Ordinary window close waits in update without blocking rendering.
            // Also join in non-window ownership paths rather than abandoning work.
            let _ = running.thread.join();
        }
    }
}

#[cfg(test)]
#[path = "browser_live_panel_tests.rs"]
mod tests;
