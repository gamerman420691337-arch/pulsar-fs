//! Disposable settings presentation. Filesystem configuration belongs to the executable host.
use anyhow::{ensure, Result};
use eframe::egui;
use pulsar_protocol::validate_sha256;
use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
};

#[derive(Clone, Debug)]
pub struct AssistantHostSetup {
    pub output: PathBuf,
    pub runtime: PathBuf,
    pub model: PathBuf,
    pub projector: Option<PathBuf>,
    pub cpu_threads: u16,
    pub wall_time_ms: u64,
    pub worker_memory_bytes: u64,
    pub approve_host_inference: bool,
}
#[derive(Clone, Debug)]
pub enum AssistantHostRequest {
    Inspect,
    Setup(AssistantHostSetup),
    Select {
        policy: PathBuf,
        approve_host_inference: bool,
    },
    Disable {
        expected_policy_sha256: String,
        approve_disable: bool,
    },
}
#[derive(Clone, Debug)]
pub struct AssistantHostReply {
    pub saved_policy_sha256: Option<String>,
    pub validation_error: Option<String>,
    pub external_override: bool,
    pub message: String,
}
impl AssistantHostReply {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.message.len() <= 4096
                && self
                    .validation_error
                    .as_ref()
                    .is_none_or(|e| e.len() <= 4096),
            "Host settings reply exceeds display bounds"
        );
        if let Some(digest) = &self.saved_policy_sha256 {
            validate_sha256(digest)?;
        }
        ensure!(
            self.validation_error.is_none() || self.saved_policy_sha256.is_some(),
            "Invalid saved settings reply"
        );
        Ok(())
    }
}
pub type AssistantHostService =
    Arc<dyn Fn(AssistantHostRequest) -> Result<AssistantHostReply> + Send + Sync>;

pub(crate) struct AssistantSettingsPanel {
    service: Option<AssistantHostService>,
    receiver: Option<Receiver<Result<AssistantHostReply, String>>>,
    policy: String,
    runtime: String,
    model: String,
    projector: String,
    threads: u16,
    wall_ms: u64,
    memory_mib: u32,
    approve: bool,
    approve_disable: bool,
    reply: Option<AssistantHostReply>,
    error: Option<String>,
}
impl Default for AssistantSettingsPanel {
    fn default() -> Self {
        Self {
            service: None,
            receiver: None,
            policy: String::new(),
            runtime: String::new(),
            model: String::new(),
            projector: String::new(),
            threads: 1,
            wall_ms: 120000,
            memory_mib: 2048,
            approve: false,
            approve_disable: false,
            reply: None,
            error: None,
        }
    }
}
impl AssistantSettingsPanel {
    pub(crate) fn configure(&mut self, service: AssistantHostService) {
        if self.receiver.is_some() {
            return;
        }
        self.service = Some(service);
        self.send(AssistantHostRequest::Inspect);
    }
    fn setup(&self) -> Result<AssistantHostSetup> {
        let output = PathBuf::from(&self.policy);
        let runtime = PathBuf::from(&self.runtime);
        let model = PathBuf::from(&self.model);
        let projector = (!self.projector.is_empty()).then(|| PathBuf::from(&self.projector));
        ensure!(
            output.is_absolute()
                && runtime.is_absolute()
                && model.is_absolute()
                && projector.as_ref().is_none_or(|p| p.is_absolute()),
            "Choose absolute local file paths"
        );
        ensure!(
            (1..=1024).contains(&self.threads)
                && (1..=3_600_000).contains(&self.wall_ms)
                && (256..=4096).contains(&self.memory_mib),
            "Check CPU, time and memory limits"
        );
        Ok(AssistantHostSetup {
            output,
            runtime,
            model,
            projector,
            cpu_threads: self.threads,
            wall_time_ms: self.wall_ms,
            worker_memory_bytes: u64::from(self.memory_mib) * 1024 * 1024,
            approve_host_inference: self.approve,
        })
    }
    fn send(&mut self, request: AssistantHostRequest) {
        if self.receiver.is_some() {
            return;
        }
        let approved = match &request {
            AssistantHostRequest::Inspect => true,
            AssistantHostRequest::Setup(options) => options.approve_host_inference,
            AssistantHostRequest::Select {
                approve_host_inference,
                ..
            } => *approve_host_inference,
            AssistantHostRequest::Disable {
                approve_disable, ..
            } => *approve_disable,
        };
        if !approved {
            self.error = Some("Approve this settings change before applying it".into());
            return;
        }
        let Some(service) = self.service.clone() else {
            self.error = Some("Local host settings are unavailable in this window".into());
            return;
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("assistant-host-settings".into())
            .spawn(move || {
                let result = service(request)
                    .and_then(|reply| {
                        reply.validate()?;
                        Ok(reply)
                    })
                    .map_err(|error| error.to_string().chars().take(4096).collect());
                let _ = sender.send(result);
            }) {
            Ok(_) => {
                self.receiver = Some(receiver);
                self.error = None;
                self.approve = false;
                self.approve_disable = false;
            }
            Err(error) => self.error = Some(format!("Cannot start settings operation: {error}")),
        }
    }
    pub(crate) fn poll(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.receiver = None;
                match result {
                    Ok(reply) => {
                        self.reply = Some(reply);
                        self.error = None;
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.reply = None;
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.receiver = None;
                self.reply = None;
                self.error = Some("Settings operation disconnected. Inspect saved settings before retrying; the change may have completed.".into());
            }
            Err(mpsc::TryRecvError::Empty) => (),
        }
    }
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        ui.heading("Local assistant settings");
        ui.label("Configure local model files for a future Engine startup. Current sessions keep their original configuration.");
        ui.label("Saving settings does not run inference or approve changes to a project.");
        let busy = self.receiver.is_some();
        if busy {
            ui.spinner();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.service.is_none() {
            ui.label("Local host settings are unavailable in this window.");
        }
        if ui
            .add_enabled(
                !busy && self.service.is_some(),
                egui::Button::new("Inspect saved settings"),
            )
            .clicked()
        {
            self.send(AssistantHostRequest::Inspect);
        }
        if let Some(reply) = &self.reply {
            ui.label(&reply.message);
            if reply.saved_policy_sha256.is_none() {
                ui.label(
                    "No saved assistant choice. Select a measured policy for a future startup.",
                );
            }
            if reply.external_override {
                ui.label("An external startup setting takes precedence. Saved changes apply when that setting is removed.");
            }
            if let Some(error) = &reply.validation_error {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!("Saved configuration needs attention: {error}"),
                );
            }
            if let Some(digest) = &reply.saved_policy_sha256 {
                ui.small("Saved choice identity");
                ui.monospace(digest);
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
            ui.label("A change may have completed; inspect saved settings before retrying.");
        }
        ui.separator();
        ui.add_enabled_ui(!busy && self.service.is_some(), |ui| {
            let mut changed = false;
            for (label, field) in [("Policy file", &mut self.policy), ("Inference executable", &mut self.runtime), ("Model file", &mut self.model), ("Vision projector (optional)", &mut self.projector)] {
                ui.label(label); changed |= ui.text_edit_singleline(field).changed();
            }
            ui.horizontal(|ui| {
                ui.label("CPU threads"); changed |= ui.add(egui::DragValue::new(&mut self.threads).range(1..=1024)).changed();
                ui.label("Time limit (ms)"); changed |= ui.add(egui::DragValue::new(&mut self.wall_ms).range(1..=3_600_000)).changed();
                ui.label("Memory (MiB)"); changed |= ui.add(egui::DragValue::new(&mut self.memory_mib).range(256..=4096)).changed();
            });
            if changed { self.approve = false; self.approve_disable = false; }
            ui.checkbox(&mut self.approve, "I approve this local assistant configuration");
            let setup = self.setup();
            if ui.add_enabled(self.approve && setup.is_ok(), egui::Button::new("Create measured policy")).clicked() { self.send(AssistantHostRequest::Setup(setup.expect("enabled setup"))); }
            ui.small("A new policy never overwrites an existing file. Portable assets must be inside the policy's folder.");
            let policy = PathBuf::from(&self.policy);
            if ui.add_enabled(self.approve && policy.is_absolute(), egui::Button::new("Use this policy at next startup")).clicked() { self.send(AssistantHostRequest::Select { policy, approve_host_inference: self.approve }); }
            ui.separator();
            ui.checkbox(&mut self.approve_disable, "I approve disabling the saved assistant choice");
            let digest = self.reply.as_ref().and_then(|reply| reply.saved_policy_sha256.clone());
            if ui.add_enabled(self.approve_disable && digest.is_some(), egui::Button::new("Disable saved choice")).clicked() { self.send(AssistantHostRequest::Disable { expected_policy_sha256: digest.expect("enabled disable"), approve_disable: self.approve_disable }); }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    fn reply() -> AssistantHostReply {
        AssistantHostReply {
            saved_policy_sha256: Some("a".repeat(64)),
            validation_error: None,
            external_override: false,
            message: "Inspected fixture".into(),
        }
    }
    fn wait(panel: &mut AssistantSettingsPanel) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while panel.receiver.is_some() {
            assert!(std::time::Instant::now() < deadline);
            panel.poll();
            std::thread::yield_now();
        }
    }
    fn draw(
        context: &egui::Context,
        panel: &mut AssistantSettingsPanel,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| panel.ui(ui));
            },
        )
    }
    fn text(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(s) if s.galley.job.text == label => {
                // Long paths can extend beyond the TextEdit clip rectangle.
                // Click the visible start of the drawn text, not its off-screen midpoint.
                Some(s.pos + egui::vec2(3.0, s.galley.rect.height() / 2.0))
            }
            egui::epaint::Shape::Vec(v) => v.iter().find_map(|s| text(s, label)),
            _ => None,
        }
    }
    fn click(context: &egui::Context, panel: &mut AssistantSettingsPanel, label: &str) {
        draw(context, panel, vec![]);
        let output = draw(context, panel, vec![]);
        let position = output
            .shapes
            .iter()
            .find_map(|s| text(&s.shape, label))
            .expect("drawn settings control");
        for pressed in [true, false] {
            draw(
                context,
                panel,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    #[test]
    fn headless_pointer_selection_and_disable_require_separate_approval_and_exact_identity() {
        let context = egui::Context::default();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let main = std::thread::current().id();
        let mut panel = AssistantSettingsPanel {
            policy: crate::test_host_path("/fixture/policy.json"),
            ..Default::default()
        };
        panel.configure(Arc::new(move |request| {
            assert_ne!(std::thread::current().id(), main);
            observed.lock().unwrap().push(request);
            Ok(reply())
        }));
        wait(&mut panel);
        click(&context, &mut panel, "Use this policy at next startup");
        assert_eq!(requests.lock().unwrap().len(), 1);
        click(
            &context,
            &mut panel,
            "I approve this local assistant configuration",
        );
        click(&context, &mut panel, "Use this policy at next startup");
        wait(&mut panel);
        assert!(!panel.approve);
        assert!(
            matches!(&requests.lock().unwrap()[1], AssistantHostRequest::Select { policy, approve_host_inference: true } if policy == &PathBuf::from(crate::test_host_path("/fixture/policy.json")))
        );
        click(&context, &mut panel, "Disable saved choice");
        assert_eq!(requests.lock().unwrap().len(), 2);
        click(
            &context,
            &mut panel,
            "I approve disabling the saved assistant choice",
        );
        click(&context, &mut panel, "Disable saved choice");
        wait(&mut panel);
        assert!(
            matches!(&requests.lock().unwrap()[2], AssistantHostRequest::Disable { expected_policy_sha256, approve_disable: true } if expected_policy_sha256 == &"a".repeat(64))
        );
    }
    #[test]
    fn editing_setup_paths_clears_approval_and_pointer_setup_captures_the_changed_inputs() {
        let context = egui::Context::default();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let seen = observed.clone();
        let mut panel = AssistantSettingsPanel {
            policy: crate::test_host_path("/fixture/policy.json"),
            runtime: crate::test_host_path("/fixture/runtime"),
            model: crate::test_host_path("/fixture/model"),
            ..Default::default()
        };
        panel.configure(Arc::new(move |request| {
            seen.lock().unwrap().push(request);
            Ok(reply())
        }));
        wait(&mut panel);
        click(
            &context,
            &mut panel,
            "I approve this local assistant configuration",
        );
        click(
            &context,
            &mut panel,
            "I approve disabling the saved assistant choice",
        );
        click(
            &context,
            &mut panel,
            &crate::test_host_path("/fixture/model"),
        );
        draw(
            &context,
            &mut panel,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::CTRL,
                },
                egui::Event::Text(crate::test_host_path("/fixture/changed-model")),
            ],
        );
        assert!(
            !panel.approve
                && !panel.approve_disable
                && panel.model != crate::test_host_path("/fixture/model")
        );
        click(&context, &mut panel, "Create measured policy");
        assert_eq!(observed.lock().unwrap().len(), 1);
        assert!(panel.setup().is_ok(), "edited setup path: {}", panel.model);
        let changed = PathBuf::from(&panel.model);
        click(
            &context,
            &mut panel,
            "I approve this local assistant configuration",
        );
        click(&context, &mut panel, "Create measured policy");
        wait(&mut panel);
        assert!(
            matches!(&observed.lock().unwrap()[1], AssistantHostRequest::Setup(options) if options.model == changed && options.approve_host_inference && options.worker_memory_bytes == 2048 * 1024 * 1024)
        );
    }
    #[test]
    fn single_pending_request_captures_original_inputs_and_blocks_duplicates() {
        let (release, wait_release) = mpsc::sync_channel(1);
        let wait_release = Mutex::new(wait_release);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observed = seen.clone();
        let mut panel = AssistantSettingsPanel::default();
        panel.configure(Arc::new(move |request| {
            if !matches!(request, AssistantHostRequest::Inspect) {
                wait_release
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
            }
            observed.lock().unwrap().push(request);
            Ok(reply())
        }));
        wait(&mut panel);
        panel.send(AssistantHostRequest::Select {
            policy: crate::test_host_path("/original").into(),
            approve_host_inference: true,
        });
        panel.send(AssistantHostRequest::Select {
            policy: crate::test_host_path("/replacement").into(),
            approve_host_inference: true,
        });
        release.send(()).unwrap();
        wait(&mut panel);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(
            matches!(&seen.lock().unwrap()[1], AssistantHostRequest::Select { policy, .. } if policy == &PathBuf::from(crate::test_host_path("/original")))
        );
    }
    #[test]
    fn invalid_reply_disconnect_and_host_failure_clear_stale_disable_identity_and_allow_inspection()
    {
        let mut panel = AssistantSettingsPanel::default();
        panel.configure(Arc::new(|_| {
            let mut r = reply();
            r.saved_policy_sha256 = Some("bad".into());
            Ok(r)
        }));
        wait(&mut panel);
        assert!(panel.reply.is_none() && panel.error.is_some());
        panel.configure(Arc::new(|_| Err(anyhow::anyhow!("x".repeat(8192)))));
        wait(&mut panel);
        assert_eq!(panel.error.as_ref().unwrap().len(), 4096);
        panel.configure(Arc::new(|_| {
            panic!("intentional settings disconnect fixture")
        }));
        wait(&mut panel);
        assert!(panel.error.as_ref().unwrap().contains("may have completed"));
        panel.configure(Arc::new(|_| Ok(reply())));
        wait(&mut panel);
        assert!(panel.reply.is_some() && panel.error.is_none());
        let mut oversized = reply();
        oversized.message = "x".repeat(4097);
        assert!(oversized.validate().is_err());
    }
    #[test]
    fn setup_is_local_absolute_bounded_and_never_approved_by_default() {
        let mut panel = AssistantSettingsPanel::default();
        assert!(panel.setup().is_err());
        panel.policy = crate::test_host_path("/root/policy");
        panel.runtime = crate::test_host_path("/root/runtime");
        panel.model = crate::test_host_path("/root/model");
        let setup = panel.setup().unwrap();
        assert!(!setup.approve_host_inference && setup.projector.is_none());
        panel.projector = "relative".into();
        assert!(panel.setup().is_err());
        panel.projector.clear();
        panel.memory_mib = 0;
        assert!(panel.setup().is_err());
        panel.send(AssistantHostRequest::Select {
            policy: crate::test_host_path("/root/policy").into(),
            approve_host_inference: false,
        });
        assert!(panel.receiver.is_none());
    }
}
