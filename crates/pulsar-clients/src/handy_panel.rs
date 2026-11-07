//! Explicit read-only Handy observation through the existing owner-scoped broker.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::Result;
use eframe::egui;
use pulsar_protocol::{Command, HandyInspection, HandyInspectionRequest};
use std::path::Path;

/// Call on the existing background request thread. The directory is the trusted
/// session bootstrap parent; callers cannot choose an endpoint or send motion.
pub fn inspect(
    client: &mut impl EngineApi,
    owner_directory: &Path,
    inspection: HandyInspectionRequest,
) -> Result<HandyInspection> {
    inspection.validate()?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(owner_directory, "handy-owner.token")?;
    let response = client.execute(
        Command::HandyInspect {
            inspection,
            owner_proof,
        },
        None,
        None,
    )?;
    let ResponseBody::HandyInspection(report) = response else {
        anyhow::bail!("Handy inspection returned an unexpected response");
    };
    report.validate()?;
    Ok(report)
}

pub(crate) struct HandyPanel {
    api_env: String,
    connection_env: String,
    consent: bool,
    report: Option<HandyInspection>,
}
impl Default for HandyPanel {
    fn default() -> Self {
        Self {
            api_env: "HANDY_API_KEY".into(),
            connection_env: "HANDY_CONNECTION_KEY".into(),
            consent: false,
            report: None,
        }
    }
}
impl HandyPanel {
    fn request(&self) -> Result<HandyInspectionRequest> {
        let request = HandyInspectionRequest {
            api_key_env: self.api_env.clone(),
            connection_key_env: self.connection_env.clone(),
            allow_online_inspection: self.consent,
        };
        request.validate()?;
        Ok(request)
    }
    pub(crate) fn accept(&mut self, report: HandyInspection) -> Result<()> {
        report.validate()?;
        self.report = Some(report);
        Ok(())
    }
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, busy: bool) -> Option<HandyInspectionRequest> {
        ui.heading("Handy connectivity and identity");
        ui.label("Inspect connectivity and firmware identity through Handy's online API. This observation does not qualify playback or send scripts, movement or firmware changes.");
        ui.small("Enter distinct credential environment-variable names only. Add keys with secrets set and launch the Engine through secrets run. Changing names does not configure credentials or restart an existing Engine.");
        let mut selected = None;
        ui.add_enabled_ui(!busy, |ui| {
            ui.label("API key environment name");
            let api_changed = ui
                .add(egui::TextEdit::singleline(&mut self.api_env).char_limit(128))
                .changed();
            ui.label("Connection key environment name");
            let connection_changed = ui
                .add(egui::TextEdit::singleline(&mut self.connection_env).char_limit(128))
                .changed();
            if api_changed || connection_changed {
                self.consent = false;
                self.report = None;
            }
            ui.checkbox(
                &mut self.consent,
                "Allow this online connectivity and identity inspection",
            );
            let request = self.request();
            if ui
                .add_enabled(request.is_ok(), egui::Button::new("Inspect Handy online"))
                .clicked()
            {
                selected = request.ok();
                self.consent = false;
                self.report = None;
            }
            if self.consent {
                if let Err(error) = self.request() {
                    ui.colored_label(egui::Color32::YELLOW, error.to_string());
                }
            }
        });
        if let Some(report) = &self.report {
            ui.label(format!(
                "Observed at Unix millisecond {}. Snapshot only; not continuously monitored.",
                report.observed_at_unix_ms
            ));
            ui.label(if report.connected {
                "Connected at observation time"
            } else {
                "Disconnected at observation time"
            });
            if let Some(identity) = &report.identity {
                ui.label(format!(
                    "Model {} (number {}, variant {}); firmware {} (status {})",
                    identity.model_name,
                    identity.model_number,
                    identity.model_variant,
                    identity.firmware,
                    identity.firmware_status
                ));
            }
        } else {
            ui.label("No current Handy observation.");
        }
        ui.small("Physical playback remains disabled in this build. Connectivity is not admission, synchronization, stop-response or physical qualification evidence.");
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(consent: bool) -> HandyInspectionRequest {
        HandyInspectionRequest {
            api_key_env: "TEST_API_KEY".into(),
            connection_key_env: "TEST_CONNECTION_KEY".into(),
            allow_online_inspection: consent,
        }
    }
    struct Api {
        calls: usize,
        response: u8,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<pulsar_protocol::ProjectId>,
            revision: Option<pulsar_protocol::RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(project.is_none() && revision.is_none());
            let Command::HandyInspect {
                inspection,
                owner_proof,
            } = command
            else {
                panic!("inspection only")
            };
            assert!(inspection.allow_online_inspection);
            assert_eq!(owner_proof, "a".repeat(64));
            self.calls += 1;
            Ok(match self.response {
                1 => ResponseBody::Ack,
                n => ResponseBody::HandyInspection(HandyInspection {
                    connected: n == 2,
                    identity: None,
                    observed_at_unix_ms: 1,
                }),
            })
        }
    }
    #[cfg(unix)]
    #[test]
    fn approved_inspection_runs_off_thread_and_accepts_only_valid_report() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("handy-owner.token");
        std::fs::write(&path, "a".repeat(64)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let owner = root.path().to_path_buf();
        let parent = std::thread::current().id();
        std::thread::spawn(move || {
            assert_ne!(parent, std::thread::current().id());
            for n in 0..3 {
                let mut api = Api {
                    calls: 0,
                    response: n,
                };
                let result = inspect(&mut api, &owner, request(true));
                assert_eq!(result.is_ok(), n == 0);
                assert_eq!(api.calls, 1);
            }
        })
        .join()
        .unwrap();
    }
    #[test]
    fn consent_and_missing_local_proof_refuse_before_engine_call() {
        let root = tempfile::tempdir().unwrap();
        let mut api = Api {
            calls: 0,
            response: 0,
        };
        assert!(inspect(&mut api, root.path(), request(false)).is_err());
        assert!(inspect(&mut api, root.path(), request(true)).is_err());
        assert_eq!(api.calls, 0);
        let mut r = request(true);
        r.connection_key_env = r.api_key_env.clone();
        assert!(inspect(&mut api, root.path(), r).is_err());
        assert_eq!(api.calls, 0);
    }
    #[test]
    fn malformed_observation_does_not_replace_checked_snapshot() {
        let mut panel = HandyPanel::default();
        panel
            .accept(HandyInspection {
                connected: false,
                identity: None,
                observed_at_unix_ms: 1,
            })
            .unwrap();
        assert!(panel
            .accept(HandyInspection {
                connected: true,
                identity: None,
                observed_at_unix_ms: 2
            })
            .is_err());
        assert_eq!(panel.report.as_ref().unwrap().observed_at_unix_ms, 1);
    }
    fn draw(
        context: &egui::Context,
        panel: &mut HandyPanel,
        busy: bool,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<HandyInspectionRequest>) {
        let mut action = None;
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 800.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    action = panel.ui(ui, busy);
                });
            },
        );
        (output, action)
    }
    fn text(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(s) if s.galley.job.text == label => {
                Some(s.pos + s.galley.rect.center().to_vec2())
            }
            egui::epaint::Shape::Vec(s) => s.iter().find_map(|s| text(s, label)),
            _ => None,
        }
    }
    fn click(
        context: &egui::Context,
        panel: &mut HandyPanel,
        busy: bool,
        label: &str,
    ) -> Option<HandyInspectionRequest> {
        draw(context, panel, busy, vec![]);
        let (output, _) = draw(context, panel, busy, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| text(&s.shape, label))
            .expect("drawn Handy control");
        let mut selected = None;
        for pressed in [true, false] {
            let (_, action) = draw(
                context,
                panel,
                busy,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
            selected = selected.or(action);
        }
        selected
    }
    #[test]
    fn drawn_action_requires_consent_captures_names_and_consumes_permission() {
        let context = egui::Context::default();
        let mut panel = HandyPanel::default();
        assert!(click(&context, &mut panel, false, "Inspect Handy online").is_none());
        click(
            &context,
            &mut panel,
            false,
            "Allow this online connectivity and identity inspection",
        );
        assert!(panel.consent);
        assert!(click(&context, &mut panel, true, "Inspect Handy online").is_none());
        assert!(panel.consent);
        panel.connection_env = panel.api_env.clone();
        assert!(click(&context, &mut panel, false, "Inspect Handy online").is_none());
        panel.api_env = "TEST_API_KEY".into();
        panel.connection_env = "TEST_CONNECTION_KEY".into();
        panel
            .accept(HandyInspection {
                connected: false,
                identity: None,
                observed_at_unix_ms: 1,
            })
            .unwrap();
        let captured = click(&context, &mut panel, false, "Inspect Handy online")
            .expect("explicit inspection");
        assert!(!panel.consent && panel.report.is_none());
        panel.api_env = "LATER_API_KEY".into();
        assert_eq!(captured.api_key_env, "TEST_API_KEY");
        assert_eq!(captured.connection_key_env, "TEST_CONNECTION_KEY");
        assert!(captured.allow_online_inspection);
        assert!(click(&context, &mut panel, false, "Inspect Handy online").is_none());
    }
}
