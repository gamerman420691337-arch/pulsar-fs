//! Captured desktop intents; saved receipts never substitute for current selection.
use crate::{
    handy_registration_recovery::{Action, RecoveryEntry},
    motion::ResponseBody,
};
use anyhow::{ensure, Result};
use eframe::egui;
use pulsar_protocol::*;

struct Draft {
    device: String,
    label: String,
    api_env: String,
    connection_env: String,
    disabled: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            device: String::new(),
            label: String::new(),
            api_env: "HANDY_API_KEY".into(),
            connection_env: "HANDY_CONNECTION_KEY".into(),
            disabled: false,
        }
    }
}
pub(crate) struct HandyRegistrationPanel {
    draft: Draft,
    catalog: Option<HandyRegistrationCatalog>,
    selected: Option<HandyRegistration>,
    pending: Option<(RequestId, HandyRegistrationIntent)>,
    active: Option<Action>,
    recovery: Vec<RecoveryEntry>,
    recovery_ready: bool,
    consent: bool,
    observation: Option<HandyRegisteredInspection>,
    refresh: bool,
    select_after_refresh: Option<DeviceId>,
    status: String,
}
impl Default for HandyRegistrationPanel {
    fn default() -> Self {
        Self {
            draft: Draft::default(),
            catalog: None,
            selected: None,
            pending: None,
            active: None,
            recovery: vec![],
            recovery_ready: false,
            consent: false,
            observation: None,
            refresh: true,
            select_after_refresh: None,
            status: "Loading saved references requires a local Engine connection.".into(),
        }
    }
}
impl HandyRegistrationPanel {
    fn current(&self) -> Option<&HandyRegistration> {
        let selected = self.selected.as_ref()?;
        self.catalog
            .as_ref()?
            .registrations
            .iter()
            .find(|record| *record == selected)
    }
    fn inspection_selection(&self) -> Option<&HandyRegistration> {
        let record = self.current()?;
        (!record.disabled
            && self.draft.device == record.device_id.as_str()
            && self.draft.label == record.settings.label
            && self.draft.api_env == record.settings.api_key_env
            && self.draft.connection_env == record.settings.connection_key_env
            && self.draft.disabled == record.disabled)
            .then_some(record)
    }
    fn select(&mut self, record: HandyRegistration) {
        self.draft = Draft {
            device: record.device_id.to_string(),
            label: record.settings.label.clone(),
            api_env: record.settings.api_key_env.clone(),
            connection_env: record.settings.connection_key_env.clone(),
            disabled: record.disabled,
        };
        self.selected = Some(record);
        self.consent = false;
        self.observation = None;
    }
    fn editable(&self) -> bool {
        self.active.is_none()
            && self.pending.is_none()
            && self.recovery_ready
            && self.catalog.is_some()
    }
    fn save_intent(&self) -> Result<HandyRegistrationIntent> {
        ensure!(
            self.editable(),
            "Settle the retained change and refresh recovery before editing"
        );
        let device_id = DeviceId::new(self.draft.device.clone())?;
        let expected_revision = if let Some(record) = &self.selected {
            ensure!(
                self.current().is_some() && record.device_id == device_id,
                "Select the current saved reference before updating"
            );
            record.revision
        } else {
            ensure!(
                !self
                    .catalog
                    .as_ref()
                    .unwrap()
                    .registrations
                    .iter()
                    .any(|r| r.device_id == device_id),
                "Device ID already exists; select its current entry"
            );
            0
        };
        let intent = HandyRegistrationIntent::Save {
            device_id,
            expected_revision,
            settings: HandyRegistrationSettings {
                label: self.draft.label.clone(),
                api_key_env: self.draft.api_env.clone(),
                connection_key_env: self.draft.connection_env.clone(),
            },
            disabled: self.draft.disabled,
        };
        intent.validate()?;
        Ok(intent)
    }
    fn begin(&mut self, action: Action) -> Result<Action> {
        ensure!(
            self.active.is_none(),
            "Handy registration work is already owned"
        );
        if let Action::Change { id, intent } | Action::Resolve { id, intent } = &action {
            intent.validate()?;
            if let Some(prior) = &self.pending {
                ensure!(
                    prior == &(id.clone(), intent.clone()),
                    "Original Handy intent must not be replaced"
                );
            }
            self.pending = Some((id.clone(), intent.clone()));
        }
        if let Action::Inspect(record) = &action {
            ensure!(
                self.consent
                    && self.pending.is_none()
                    && self.inspection_selection() == Some(record),
                "Select the current saved reference and allow this inspection"
            );
        }
        self.consent = false;
        self.observation = None;
        self.active = Some(action.clone());
        Ok(action)
    }
    fn prepare(&mut self, intent: HandyRegistrationIntent) -> Result<Action> {
        intent.validate()?;
        ensure!(
            self.pending.is_none(),
            "Original Handy change remains unsettled"
        );
        self.begin(Action::Change {
            id: RequestId::new(uuid::Uuid::new_v4().to_string())?,
            intent,
        })
    }
    pub(crate) fn status(&self) -> &str {
        &self.status
    }
    pub(crate) fn finish(
        &mut self,
        action: &Action,
        response: &Result<ResponseBody, String>,
        recovery: &Result<Vec<RecoveryEntry>, String>,
    ) {
        if self.active.as_ref() != Some(action) {
            self.status = "Unmatched Handy reply ignored; original intent retained.".into();
            return;
        }
        self.active = None;
        match recovery {
            Ok(entries) => {
                self.recovery = entries.clone();
                self.recovery_ready = true;
            }
            Err(error) => {
                self.recovery_ready = false;
                self.status =
                    format!("Recovery could not be read: {error}. Original intent retained.");
            }
        }
        let checked = (|| -> Result<()> {
            let response = response
                .as_ref()
                .map_err(|error| anyhow::anyhow!(error.clone()))?;
            match (action, response) {
                (Action::Refresh, ResponseBody::HandyRegistrations(catalog)) => {
                    catalog.validate()?;
                    self.catalog = Some((**catalog).clone());
                    if let Some(id) = self.select_after_refresh.take() {
                        if let Some(record) =
                            catalog.registrations.iter().find(|r| r.device_id == id)
                        {
                            self.select(record.clone());
                        } else {
                            self.selected = None;
                        }
                    }
                    if self.current().is_none() {
                        self.consent = false;
                        self.observation = None;
                    }
                    self.status = format!(
                        "{} saved references. Registration does not qualify playback.",
                        catalog.registrations.len()
                    );
                }
                (Action::Change { id, intent }, ResponseBody::HandyRegistrationSaved(receipt)) => {
                    receipt.validate()?;
                    ensure!(
                        receipt.request_id == *id && intent.matches(receipt),
                        "Handy receipt changed its captured intent"
                    );
                    self.pending = None;
                    self.catalog = None;
                    self.select_after_refresh = Some(intent.device().clone());
                    self.refresh = true;
                    self.status = format!(
                        "Request {id} acknowledged at revision {}. Refreshing current state.",
                        receipt.registration.revision
                    );
                }
                (
                    Action::Resolve { id, intent },
                    ResponseBody::HandyRegistrationResolution(resolution),
                ) => {
                    resolution.validate()?;
                    ensure!(
                        resolution.request_id == *id && resolution.intent == *intent,
                        "Handy resolution changed its captured intent"
                    );
                    self.pending = None;
                    self.catalog = None;
                    self.select_after_refresh = Some(intent.device().clone());
                    self.refresh = true;
                    self.status = match resolution.outcome {
                        HandyRegistrationOutcome::Applied(_) => {
                            format!("Original request {id} was applied; refreshing current state.")
                        }
                        HandyRegistrationOutcome::Withdrawn => {
                            format!("Original request {id} withdrawn. A delayed copy cannot apply.")
                        }
                    };
                }
                (Action::Dismiss(_), ResponseBody::Ack) => {
                    self.status =
                        "Settled local recovery record dismissed; Engine history remains.".into();
                }
                (Action::Inspect(selected), ResponseBody::HandyRegisteredInspection(report)) => {
                    report.validate()?;
                    ensure!(
                        &report.registration == selected && self.current() == Some(selected),
                        "Selected Handy reference changed during inspection"
                    );
                    self.observation = Some((**report).clone());
                    self.status =
                        "Read-only observation received. Playback remains unqualified.".into();
                }
                _ => anyhow::bail!("Unexpected Handy registration response"),
            }
            Ok(())
        })();
        if let Err(error) = checked {
            self.status=format!("Handy action not confirmed: {error}. Retained changes require original retry or resolution.");
        }
        if self.pending.is_none() {
            if let Some(entry) = self.recovery.iter().find(|entry| entry.outcome.is_none()) {
                self.pending = Some((entry.id.clone(), entry.intent.clone()));
            }
        }
        if !self.recovery_ready {
            self.status
                .push_str(" Recovery storage is unavailable; new changes are disabled.");
        }
    }
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, busy: bool) -> Option<Action> {
        ui.heading("Saved Handy references");
        ui.label("Save credential-variable names without reading keys or contacting a device. Disabled lookup sends no stop command; physical playback remains unavailable.");
        if self.refresh && !busy && self.active.is_none() {
            self.refresh = false;
            return self.begin(Action::Refresh).ok();
        }
        let mut action = None;
        let idle = !busy && self.active.is_none();
        if ui
            .add_enabled(
                idle,
                egui::Button::new("Refresh saved references and recovery"),
            )
            .clicked()
        {
            action = Some(Action::Refresh);
        }
        if let Some(catalog) = &self.catalog {
            let mut selected = None;
            for record in &catalog.registrations {
                if ui
                    .add_enabled(
                        idle && self.pending.is_none(),
                        egui::Button::new(format!(
                            "{} · {} · revision {}{}",
                            record.settings.label,
                            record.device_id,
                            record.revision,
                            if record.disabled {
                                " · lookup disabled"
                            } else {
                                ""
                            }
                        )),
                    )
                    .clicked()
                {
                    selected = Some(record.clone());
                }
            }
            if let Some(record) = selected {
                self.select(record);
            }
        }
        ui.add_enabled_ui(idle && self.editable(), |ui| {
            if ui.button("New reference").clicked() {
                self.selected = None;
                self.draft = Draft::default();
                self.consent = false;
                self.observation = None;
            }
            ui.label("Logical device ID");
            let mut changed = ui
                .add_enabled(
                    self.selected.is_none(),
                    egui::TextEdit::singleline(&mut self.draft.device).char_limit(128),
                )
                .changed();
            ui.label("Display label");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.draft.label).char_limit(128))
                .changed();
            ui.label("API key environment name");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.draft.api_env).char_limit(128))
                .changed();
            ui.label("Connection key environment name");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.draft.connection_env).char_limit(128))
                .changed();
            changed |= ui
                .checkbox(&mut self.draft.disabled, "Disable saved lookup")
                .changed();
            if changed {
                self.consent = false;
                self.observation = None;
            }
            let intent = self.save_intent();
            if let Err(error) = &intent {
                ui.small(error.to_string());
            }
            if ui
                .add_enabled(intent.is_ok(), egui::Button::new("Save reference"))
                .clicked()
            {
                match self.prepare(intent.unwrap()) {
                    Ok(value) => action = Some(value),
                    Err(error) => self.status = error.to_string(),
                }
            }
            if let Some(record) = self.current().filter(|r| !r.disabled).cloned() {
                if ui.button("Disable saved lookup now").clicked() {
                    match self.prepare(HandyRegistrationIntent::Disable {
                        device_id: record.device_id,
                        expected_revision: record.revision,
                    }) {
                        Ok(value) => action = Some(value),
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
        });
        if let Some((id, intent)) = &self.pending {
            ui.label(format!("Unsettled change for {}", intent.device()));
            ui.monospace(id.as_str());
            if ui
                .add_enabled(idle, egui::Button::new("Retry original change"))
                .clicked()
            {
                action = Some(Action::Change {
                    id: id.clone(),
                    intent: intent.clone(),
                });
            }
            if ui
                .add_enabled(
                    idle,
                    egui::Button::new("Resolve or withdraw original change"),
                )
                .clicked()
            {
                action = Some(Action::Resolve {
                    id: id.clone(),
                    intent: intent.clone(),
                });
            }
        }
        for entry in &self.recovery {
            ui.horizontal(|ui| {
                ui.small(format!(
                    "{} · {} · {}",
                    entry.intent.device(),
                    entry.id,
                    if entry.outcome.is_some() {
                        "settled"
                    } else {
                        "unconfirmed"
                    }
                ));
                if ui
                    .add_enabled(
                        idle && entry.outcome.is_some(),
                        egui::Button::new("Dismiss settled record"),
                    )
                    .clicked()
                {
                    action = Some(Action::Dismiss(entry.id.clone()));
                }
            });
        }
        let selected = self.inspection_selection().cloned();
        ui.add_enabled_ui(idle && self.pending.is_none() && selected.is_some(), |ui| {
            ui.checkbox(
                &mut self.consent,
                "Allow this saved reference's online inspection",
            );
            if ui
                .add_enabled(
                    self.consent,
                    egui::Button::new("Inspect saved Handy online"),
                )
                .clicked()
            {
                action = selected.clone().map(Action::Inspect);
            }
        });
        if let Some(report) = &self.observation {
            ui.label(format!(
                "{} at Unix millisecond {} (snapshot only)",
                if report.inspection.connected {
                    "Connected"
                } else {
                    "Disconnected"
                },
                report.inspection.observed_at_unix_ms
            ));
            if let Some(identity) = &report.inspection.identity {
                ui.label(format!(
                    "{} · firmware {}",
                    identity.model_name, identity.firmware
                ));
            }
        }
        ui.label(&self.status);
        match action {
            Some(action) if self.active.as_ref() == Some(&action) => Some(action),
            Some(action) => match self.begin(action) {
                Ok(action) => Some(action),
                Err(error) => {
                    self.status = error.to_string();
                    None
                }
            },
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(revision: u64, disabled: bool) -> HandyRegistration {
        HandyRegistration {
            device_id: DeviceId::new("handy").unwrap(),
            revision,
            settings: HandyRegistrationSettings {
                label: "Handy".into(),
                api_key_env: "HANDY_API_KEY".into(),
                connection_key_env: "HANDY_CONNECTION_KEY".into(),
            },
            disabled,
            binding_sha256: format!("{revision:064x}"),
        }
    }
    fn ready() -> HandyRegistrationPanel {
        let mut panel = HandyRegistrationPanel {
            refresh: false,
            ..Default::default()
        };
        let action = panel.begin(Action::Refresh).unwrap();
        panel.finish(
            &action,
            &Ok(ResponseBody::HandyRegistrations(Box::new(
                HandyRegistrationCatalog {
                    registrations: vec![record(1, false)],
                },
            ))),
            &Ok(vec![]),
        );
        panel.select(record(1, false));
        panel
    }
    #[test]
    fn captured_change_survives_errors_and_later_draft_edits() {
        let mut panel = ready();
        panel.draft.label = "captured".into();
        let action = panel.prepare(panel.save_intent().unwrap()).unwrap();
        panel.finish(&action, &Err("lost reply".into()), &Ok(vec![]));
        panel.draft.label = "later".into();
        assert!(!panel.editable());
        let (id, intent) = panel.pending.clone().unwrap();
        let retry = panel.begin(Action::Change { id, intent }).unwrap();
        assert_eq!(retry, action);
        let Action::Change {
            intent: HandyRegistrationIntent::Save { settings, .. },
            ..
        } = retry
        else {
            panic!("save");
        };
        assert_eq!(settings.label, "captured");
    }
    #[test]
    fn restored_facts_and_matching_catalog_do_not_settle_or_dispatch_an_intent() {
        let mut panel = ready();
        let intent = panel.save_intent().unwrap();
        let id = RequestId::new("original").unwrap();
        let entry = RecoveryEntry {
            id: id.clone(),
            intent: intent.clone(),
            owner_binding: "a".repeat(64),
            resolution_id: None,
            outcome: None,
        };
        let action = panel.begin(Action::Refresh).unwrap();
        panel.finish(
            &action,
            &Ok(ResponseBody::HandyRegistrations(Box::new(
                HandyRegistrationCatalog {
                    registrations: vec![record(2, false)],
                },
            ))),
            &Ok(vec![entry]),
        );
        assert_eq!(panel.pending, Some((id, intent)));
        assert!(panel.active.is_none());
        assert!(!panel.consent);
    }
    #[test]
    fn inspection_cannot_use_edited_or_obsolete_selection_or_restored_consent() {
        let mut panel = ready();
        let selected = record(1, false);
        assert!(panel.begin(Action::Inspect(selected.clone())).is_err());
        panel.consent = true;
        panel.draft.api_env = "DIFFERENT_API_KEY".into();
        assert!(panel.begin(Action::Inspect(selected.clone())).is_err());
        panel.select(selected.clone());
        panel.consent = true;
        let action = panel.begin(Action::Inspect(selected.clone())).unwrap();
        assert!(!panel.consent);
        panel.catalog = Some(HandyRegistrationCatalog {
            registrations: vec![record(2, false)],
        });
        panel.finish(
            &action,
            &Ok(ResponseBody::HandyRegisteredInspection(Box::new(
                HandyRegisteredInspection {
                    registration: selected,
                    inspection: HandyInspection {
                        connected: false,
                        identity: None,
                        observed_at_unix_ms: 1,
                    },
                },
            ))),
            &Ok(vec![]),
        );
        assert!(panel.observation.is_none());
    }
    #[test]
    fn device_controls_draw_and_initial_refresh_is_read_only_and_single_shot() {
        let context = egui::Context::default();
        let mut panel = HandyRegistrationPanel::default();
        let mut action = None;
        let output = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                action = panel.ui(ui, false);
            });
        });
        assert!(matches!(action, Some(Action::Refresh)));
        assert!(!output.shapes.is_empty());
        let mut second = None;
        let _ = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                second = panel.ui(ui, true);
            });
        });
        assert!(second.is_none());
        assert!(panel.pending.is_none());
    }

    #[test]
    #[ignore = "requires an explicitly isolated current Engine and private recovery directory"]
    fn native_desktop_registration_recovery_and_resolution() -> Result<()> {
        use crate::{handy_registration_recovery as recovery, session::SessionClient};
        let state = std::path::PathBuf::from(std::env::var("PULSAR_HANDY_DESKTOP_TEST_STATE")?);
        let report_path =
            std::path::PathBuf::from(std::env::var("PULSAR_HANDY_DESKTOP_TEST_REPORT")?);
        let mut client =
            SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))?;
        fn perform(
            panel: &mut HandyRegistrationPanel,
            client: &mut SessionClient,
            state: &std::path::Path,
            action: Action,
        ) -> std::result::Result<ResponseBody, String> {
            let response = recovery::execute(state, client, &action).map_err(|e| e.to_string());
            let restored = recovery::load(
                state,
                client.session_id(),
                &client.recovery_credential_binding(),
            )
            .map_err(|e| e.to_string());
            panel.finish(&action, &response, &restored);
            response
        }
        fn refresh(
            panel: &mut HandyRegistrationPanel,
            client: &mut SessionClient,
            state: &std::path::Path,
        ) -> Result<()> {
            panel.refresh = false;
            let action = panel.begin(Action::Refresh)?;
            perform(panel, client, state, action).map_err(anyhow::Error::msg)?;
            Ok(())
        }
        let mut panel = HandyRegistrationPanel::default();
        refresh(&mut panel, &mut client, &state)?;
        panel.draft = Draft {
            device: "desktop-handy".into(),
            label: "Desktop saved".into(),
            api_env: "GUI_API_MISSING".into(),
            connection_env: "GUI_CONNECTION_MISSING".into(),
            disabled: false,
        };
        let save = panel.prepare(panel.save_intent()?)?;
        perform(&mut panel, &mut client, &state, save).map_err(anyhow::Error::msg)?;
        refresh(&mut panel, &mut client, &state)?;
        assert_eq!(panel.current().unwrap().revision, 1);
        panel.draft.label = "Desktop unknown".into();
        let lost = panel.prepare(panel.save_intent()?)?;
        let captured = panel.pending.clone().unwrap();
        assert!(perform(&mut panel, &mut client, &state, lost).is_err());
        drop(panel);
        let mut panel = HandyRegistrationPanel::default();
        refresh(&mut panel, &mut client, &state)?;
        assert_eq!(panel.pending.as_ref(), Some(&captured));
        assert!(panel.active.is_none());
        assert!(!panel.consent);
        let resolve = panel.begin(Action::Resolve {
            id: captured.0.clone(),
            intent: captured.1.clone(),
        })?;
        let resolved =
            perform(&mut panel, &mut client, &state, resolve).map_err(anyhow::Error::msg)?;
        assert!(
            matches!(resolved,ResponseBody::HandyRegistrationResolution(ref r) if matches!(r.outcome,HandyRegistrationOutcome::Applied(_)))
        );
        refresh(&mut panel, &mut client, &state)?;
        let current = panel.current().unwrap().clone();
        assert_eq!(current.revision, 2);
        assert_eq!(current.settings.label, "Desktop unknown");
        panel.draft.label = "Withdrawn draft".into();
        let unsent = panel.prepare(panel.save_intent()?)?;
        let late = panel.pending.clone().unwrap();
        let entries = recovery::load(
            &state,
            client.session_id(),
            &client.recovery_credential_binding(),
        )?;
        panel.finish(
            &unsent,
            &Err("synthetic pre-dispatch interruption".into()),
            &Ok(entries),
        );
        let resolve = panel.begin(Action::Resolve {
            id: late.0.clone(),
            intent: late.1.clone(),
        })?;
        let resolved =
            perform(&mut panel, &mut client, &state, resolve).map_err(anyhow::Error::msg)?;
        assert!(
            matches!(resolved,ResponseBody::HandyRegistrationResolution(ref r) if matches!(r.outcome,HandyRegistrationOutcome::Withdrawn))
        );
        refresh(&mut panel, &mut client, &state)?;
        assert_eq!(panel.current(), Some(&current));
        assert!(
            crate::handy_registration::change(&mut client, &state, late.0.clone(), &late.1)
                .is_err()
        );
        panel.consent = true;
        let inspect = panel.begin(Action::Inspect(current.clone()))?;
        assert!(perform(&mut panel, &mut client, &state, inspect).is_err());
        assert!(!panel.consent);
        assert!(panel.observation.is_none());
        let entries = recovery::load(
            &state,
            client.session_id(),
            &client.recovery_credential_binding(),
        )?;
        assert!(entries.iter().all(|entry| entry.outcome.is_some()));
        for entry in entries {
            let dismiss = panel.begin(Action::Dismiss(entry.id))?;
            perform(&mut panel, &mut client, &state, dismiss).map_err(anyhow::Error::msg)?;
        }
        assert!(recovery::load(
            &state,
            client.session_id(),
            &client.recovery_credential_binding()
        )?
        .is_empty());
        std::fs::write(
            report_path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"passed":true,"qualified":false,"rendered_desktop":false,"lost_committed_reply_recovered":true,"restored_intent_was_inert":true,"original_resolution_retained":true,"late_original_refused":true,"no_inspection_consent_restored":true,"local_settled_records_dismissed":true,"current":current,"withdrawn_request":late.0,"withdrawn_intent":late.1}),
            )?,
        )?;
        Ok(())
    }
}
