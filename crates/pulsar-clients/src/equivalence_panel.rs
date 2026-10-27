//! Disposable evidence-review view state. All owner I/O runs on the Desktop worker.
use crate::{
    inference_equivalence_recovery as recovery,
    motion::ResponseBody,
    session::{EngineApi, SessionClient},
};
use anyhow::{bail, ensure, Context, Result};
use eframe::egui;
use pulsar_protocol::*;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scope {
    session: SessionId,
    credential: String,
}
impl Scope {
    pub(crate) fn current(client: &SessionClient) -> Self {
        Self {
            session: client.session_id().clone(),
            credential: client.recovery_credential_binding(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    List {
        after: Option<ArtifactId>,
    },
    Status {
        dossier: ArtifactId,
    },
    Import {
        id: RequestId,
        path: String,
        sha256: String,
    },
    Decide {
        id: RequestId,
        intent: recovery::Intent,
    },
    Retry {
        id: RequestId,
        intent: recovery::Intent,
    },
    Dismiss {
        id: RequestId,
    },
    Recoveries,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Action {
    ticket: RequestId,
    view: RequestId,
    scope: Option<Scope>,
    kind: Kind,
}
/// Kept narrow so state-machine tests can observe dispatch without fabricating receipts.
trait Backend {
    fn scope(&self) -> Scope;
    fn proof(&self) -> Result<String>;
    fn read(&mut self, command: Command) -> Result<ResponseBody>;
    fn mutate(&mut self, id: RequestId, intent: recovery::Intent) -> Result<ResponseBody>;
    fn retry(&mut self, id: &RequestId, intent: &recovery::Intent) -> Result<ResponseBody>;
    fn dismiss(&mut self, id: &RequestId) -> Result<()>;
}
struct LocalBackend<'a> {
    directory: &'a Path,
    client: &'a mut SessionClient,
}
impl Backend for LocalBackend<'_> {
    fn scope(&self) -> Scope {
        Scope::current(self.client)
    }
    fn proof(&self) -> Result<String> {
        crate::project_package::read_local_owner_proof(self.directory, "model-pack-owner.token")
    }
    fn read(&mut self, command: Command) -> Result<ResponseBody> {
        self.client.execute(command, None, None)
    }
    fn mutate(&mut self, id: RequestId, intent: recovery::Intent) -> Result<ResponseBody> {
        recovery::execute(self.directory, self.client, id, intent)
    }
    fn retry(&mut self, id: &RequestId, intent: &recovery::Intent) -> Result<ResponseBody> {
        let current = recovery::load(
            self.directory,
            self.client.session_id(),
            &self.client.recovery_credential_binding(),
        )?
        .into_iter()
        .find(|entry| &entry.request_id == id)
        .context("Original recovery entry is absent")?;
        ensure!(
            &current.intent == intent,
            "Original recovery intent changed; refresh recovery records"
        );
        recovery::replay(self.directory, self.client, id)
    }
    fn dismiss(&mut self, id: &RequestId) -> Result<()> {
        recovery::dismiss(
            self.directory,
            self.client.session_id(),
            &self.client.recovery_credential_binding(),
            id,
        )
    }
}
pub(crate) fn execute(
    directory: &Path,
    client: &mut SessionClient,
    action: &Action,
) -> Result<ResponseBody> {
    execute_with(&mut LocalBackend { directory, client }, action)
}
fn execute_with(backend: &mut impl Backend, action: &Action) -> Result<ResponseBody> {
    if let Some(scope) = &action.scope {
        ensure!(
            *scope == backend.scope(),
            "Review session changed; refresh before making a decision"
        );
    }
    match &action.kind {
        Kind::List { after } => backend.read(Command::ListInferenceEquivalences {
            after: after.clone(),
            limit: 8,
            owner_proof: backend.proof()?,
        }),
        Kind::Status { dossier } => backend.read(Command::InferenceEquivalenceStatus {
            dossier_id: dossier.clone(),
            owner_proof: backend.proof()?,
        }),
        Kind::Import { id, path, sha256 } => backend.mutate(
            id.clone(),
            recovery::Intent::import(path.clone(), sha256.clone())?,
        ),
        Kind::Decide { id, intent } => {
            ensure!(
                action.scope.is_some(),
                "Owner decision lacks an observed session"
            );
            intent.validate()?;
            backend.mutate(id.clone(), intent.clone())
        }
        Kind::Retry { id, intent } => {
            ensure!(
                action.scope.is_some(),
                "Recovery lacks its observed session"
            );
            backend.retry(id, intent)
        }
        Kind::Dismiss { id } => {
            ensure!(
                action.scope.is_some(),
                "Recovery lacks its observed session"
            );
            backend.dismiss(id)?;
            Ok(ResponseBody::Ack)
        }
        Kind::Recoveries => Ok(ResponseBody::Ack),
    }
}

pub(crate) struct EquivalencePanel {
    view: RequestId,
    scope: Option<Scope>,
    active: Option<Action>,
    page: Option<InferenceEquivalencePage>,
    page_after: Option<ArtifactId>,
    selected: Option<ArtifactId>,
    current: Option<InferenceEquivalenceStatus>,
    last_receipt: Option<InferenceEquivalenceReceipt>,
    source: String,
    source_sha256: String,
    lookup: String,
    operator: String,
    consent: bool,
    reason: String,
    recoveries: Vec<recovery::Entry>,
    recovery_id: Option<RequestId>,
    pub(crate) message: String,
}
impl Default for EquivalencePanel {
    fn default() -> Self {
        Self {
            view: crate::session::new_request_id(),
            scope: None,
            active: None,
            page: None,
            page_after: None,
            selected: None,
            current: None,
            last_receipt: None,
            source: String::new(),
            source_sha256: String::new(),
            lookup: String::new(),
            operator: String::new(),
            consent: false,
            reason: String::new(),
            recoveries: vec![],
            recovery_id: None,
            message: "Refresh evidence records or import a reviewed evidence manifest.".into(),
        }
    }
}
impl EquivalencePanel {
    fn select(&mut self, dossier: ArtifactId) {
        if self.selected.as_ref() != Some(&dossier) {
            self.view = crate::session::new_request_id();
            self.selected = Some(dossier);
            self.current = None;
            self.reason.clear();
            self.operator.clear();
        }
        self.consent = false;
    }
    fn selected_status(&self) -> Result<&InferenceEquivalenceStatus> {
        let status = self
            .current
            .as_ref()
            .context("Refresh the selected evidence record first")?;
        ensure!(
            self.selected.as_ref() == Some(&status.report.dossier_id),
            "Selection changed; refresh its status"
        );
        status.validate()?;
        Ok(status)
    }
    fn accept_kind(&self) -> Result<Kind> {
        let status = self.selected_status()?;
        ensure!(
            self.scope.is_some() && self.consent,
            "Review this exact record and explicitly confirm acceptance first"
        );
        ensure!(
            status.report.manifest.subject.origin == InferenceEquivalenceOrigin::Measured,
            "Synthetic evidence cannot be accepted"
        );
        ensure!(
            !status.withdrawal_pending,
            "Finish the pending withdrawal before another owner decision"
        );
        let intent = recovery::Intent::Accept {
            dossier_id: status.report.dossier_id.clone(),
            expected_record_revision: status.report.revision,
            operator: self.operator.clone(),
        };
        intent.validate()?;
        Ok(Kind::Decide {
            id: crate::session::new_request_id(),
            intent,
        })
    }
    fn withdraw_kind(&self) -> Result<Kind> {
        let status = self.selected_status()?;
        ensure!(
            self.scope.is_some() && !status.withdrawal_pending,
            "Refresh the record and finish any pending withdrawal first"
        );
        let intent = recovery::Intent::Withdraw {
            dossier_id: status.report.dossier_id.clone(),
            expected_record_revision: status.report.revision,
            reason: self.reason.clone(),
        };
        intent.validate()?;
        Ok(Kind::Decide {
            id: crate::session::new_request_id(),
            intent,
        })
    }
    fn recovery_kind(&self, dismiss: bool) -> Result<Kind> {
        let entry = self
            .recoveries
            .iter()
            .find(|entry| Some(&entry.request_id) == self.recovery_id.as_ref())
            .context("Select a retained request first")?;
        if dismiss {
            ensure!(
                entry.acknowledged_receipt_sha256.is_some(),
                "Only acknowledged receipts can be dismissed"
            );
            Ok(Kind::Dismiss {
                id: entry.request_id.clone(),
            })
        } else {
            Ok(Kind::Retry {
                id: entry.request_id.clone(),
                intent: entry.intent.clone(),
            })
        }
    }
    fn begin(&mut self, kind: Kind) -> Result<Action> {
        ensure!(
            self.active.is_none(),
            "An evidence request is already pending"
        );
        let action = Action {
            ticket: crate::session::new_request_id(),
            view: self.view.clone(),
            scope: self.scope.clone(),
            kind,
        };
        self.consent = false;
        if !matches!(action.kind, Kind::Recoveries | Kind::Dismiss { .. }) {
            self.current = None;
        }
        self.active = Some(action.clone());
        self.message =
            "Evidence request pending. A recorded decision is not a runtime admission.".into();
        Ok(action)
    }
    pub(crate) fn enqueue_failed(&mut self, action: &Action) {
        if self.active.as_ref() == Some(action) {
            self.active = None;
            self.consent = false;
            self.message = "Request was not queued. No owner operation was dispatched.".into();
        }
    }
    pub(crate) fn finish(
        &mut self,
        action: &Action,
        scope: Option<Scope>,
        response: &std::result::Result<ResponseBody, String>,
        recoveries: &std::result::Result<Vec<recovery::Entry>, String>,
    ) {
        if self.active.as_ref() != Some(action) {
            return;
        }
        self.active = None;
        if action.view != self.view {
            self.message = "Reply belongs to the earlier selection; the newer selection is unchanged. Refresh retained requests if needed.".into();
            return;
        }
        self.consent = false;
        let Some(scope) = scope else {
            self.current = None;
            self.message = "Session unavailable; original intent may remain in recovery.".into();
            return;
        };
        if action
            .scope
            .as_ref()
            .is_some_and(|expected| *expected != scope)
            || self
                .scope
                .as_ref()
                .is_some_and(|previous| *previous != scope)
        {
            self.scope = Some(scope);
            self.current = None;
            self.selected = None;
            self.page = None;
            self.last_receipt = None;
            self.operator.clear();
            self.reason.clear();
            self.recoveries.clear();
            self.recovery_id = None;
            self.view = crate::session::new_request_id();
            self.message = "Review session changed. Refresh evidence and recovery records; earlier consent was cleared.".into();
            return;
        }
        self.scope = Some(scope.clone());
        match recoveries {
            Ok(entries) => {
                self.recoveries = entries.clone();
                if !self
                    .recoveries
                    .iter()
                    .any(|entry| Some(&entry.request_id) == self.recovery_id.as_ref())
                {
                    self.recovery_id = None;
                }
            }
            Err(_) => {
                self.recoveries.clear();
                self.recovery_id = None;
            }
        }
        let result = (|| -> Result<()> {
            let response = response
                .as_ref()
                .map_err(|message| anyhow::anyhow!(message.clone()))?;
            match (&action.kind, response) {
                (Kind::List { after }, ResponseBody::InferenceEquivalenceList(page)) => {
                    page.validate()?;
                    ensure!(
                        page.entries.len() <= 8
                            && page.entries.iter().all(|entry| after.as_ref().is_none_or(
                                |after| entry.report.dossier_id.as_str() > after.as_str()
                            )),
                        "Evidence page differs from requested cursor"
                    );
                    self.page = Some((**page).clone());
                    self.page_after = after.clone();
                    self.message = "Evidence page refreshed. Select a record and refresh its status before deciding.".into();
                }
                (Kind::Status { dossier }, ResponseBody::InferenceEquivalenceStatus(status)) => {
                    status.validate()?;
                    ensure!(
                        status.report.dossier_id == *dossier
                            && self.selected.as_ref() == Some(dossier),
                        "Status belongs to another selected record"
                    );
                    self.current = Some((**status).clone());
                    self.message = "Status observed. Acceptance, expiry and pending withdrawal remain subject to Engine rechecks.".into();
                }
                (
                    Kind::Import { .. } | Kind::Decide { .. } | Kind::Retry { .. },
                    ResponseBody::InferenceEquivalenceSaved(receipt),
                ) => {
                    check_saved(&action.kind, &scope, receipt)?;
                    self.last_receipt = Some((**receipt).clone());
                    self.current = None;
                    if matches!(action.kind, Kind::Import { .. }) {
                        self.select(receipt.report.dossier_id.clone());
                    }
                    self.message = "Exact operation receipt acknowledged. Refresh selected status to observe the current record.".into();
                }
                (Kind::Recoveries, ResponseBody::Ack) => {
                    self.message =
                        "Local recovery records refreshed; no original operation was retried."
                            .into()
                }
                (Kind::Dismiss { .. }, ResponseBody::Ack) => {
                    self.message =
                        "Acknowledged local recovery entry dismissed. Engine decision unchanged."
                            .into()
                }
                _ => bail!("Reply does not match the captured evidence request"),
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.current = None;
            self.message = format!("{error}. Inspect retained requests before retrying.");
        }
        if let Err(error) = recoveries {
            self.message
                .push_str(&format!(" Recovery storage unavailable: {error}"));
        }
    }
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, busy: bool) -> Option<Action> {
        let available = !busy && self.active.is_none();
        let mut kind: Option<Result<Kind>> = None;
        ui.heading("Model equivalence evidence");
        ui.label("Compare declared model profiles and record explicit owner decisions. Import starts Pending; synthetic evidence cannot be accepted. Records describe evidence and decisions; they do not grant fresh execution authority. Generator offers explicit fallback selection; each attempt still requires current accepted evidence and fresh Engine admission.");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(available, egui::Button::new("Refresh first page"))
                .clicked()
            {
                kind = Some(Ok(Kind::List { after: None }));
            }
            if ui
                .add_enabled(
                    available
                        && self
                            .page
                            .as_ref()
                            .is_some_and(|page| page.next_after.is_some()),
                    egui::Button::new("Next page"),
                )
                .clicked()
            {
                kind = Some(Ok(Kind::List {
                    after: self.page.as_ref().and_then(|page| page.next_after.clone()),
                }));
            }
            if ui
                .add_enabled(
                    available && self.page_after.is_some(),
                    egui::Button::new("Return to first page"),
                )
                .clicked()
            {
                kind = Some(Ok(Kind::List { after: None }));
            }
        });
        let mut selection = None;
        if let Some(page) = &self.page {
            if page.entries.is_empty() {
                ui.label("No evidence records on this page.");
            }
            for entry in &page.entries {
                let r = &entry.report;
                let subject = &r.manifest.subject;
                if ui
                    .selectable_label(
                        self.selected.as_ref() == Some(&r.dossier_id),
                        format!(
                            "{:?} · {:?} · {:?} · {}",
                            subject.scope.tier,
                            subject.origin,
                            r.state,
                            short(r.dossier_id.as_str())
                        ),
                    )
                    .clicked()
                {
                    selection = Some(r.dossier_id.clone());
                }
            }
        }
        if let Some(id) = selection {
            self.select(id);
        }
        ui.horizontal(|ui| {
            ui.label("Evidence reference");
            if ui.text_edit_singleline(&mut self.lookup).changed() {
                self.consent = false;
            }
            if ui
                .add_enabled(available, egui::Button::new("Select reference"))
                .clicked()
            {
                match ArtifactId::new(self.lookup.trim()) {
                    Ok(id) => match validate_sha256(id.as_str()) {
                        Ok(()) => self.select(id),
                        Err(error) => self.message = error.to_string(),
                    },
                    Err(error) => self.message = error.to_string(),
                }
            }
        });
        if let Some(id) = &self.selected {
            ui.label(format!("Selected reference: {id}"));
            if ui
                .add_enabled(available, egui::Button::new("Refresh selected status"))
                .clicked()
            {
                kind = Some(Ok(Kind::Status {
                    dossier: id.clone(),
                }));
            }
        }
        if let Some(status) = &self.current {
            show_status(ui, status);
        }
        let measured = self.current.as_ref().is_some_and(|status| {
            status.report.manifest.subject.origin == InferenceEquivalenceOrigin::Measured
                && !status.withdrawal_pending
        });
        ui.separator();
        ui.label("Owner decision for the separately refreshed record");
        ui.horizontal(|ui| {
            ui.label("Operator attribution");
            if ui
                .add_enabled(available, egui::TextEdit::singleline(&mut self.operator))
                .changed()
            {
                self.consent = false;
            }
        });
        ui.add_enabled(available && measured, egui::Checkbox::new(&mut self.consent, "I reviewed this exact record and accept measured equivalence only within its stated scope"));
        if ui
            .add_enabled(
                available && measured && self.consent && !self.operator.trim().is_empty(),
                egui::Button::new("Record owner acceptance"),
            )
            .clicked()
        {
            kind = Some(self.accept_kind());
        }
        ui.label("Withdrawal reason");
        ui.add_enabled(
            available,
            egui::TextEdit::multiline(&mut self.reason).desired_rows(2),
        );
        if ui
            .add_enabled(
                available
                    && self.current.as_ref().is_some_and(|s| !s.withdrawal_pending)
                    && !self.reason.trim().is_empty(),
                egui::Button::new("Withdraw this recorded revision"),
            )
            .clicked()
        {
            kind = Some(self.withdraw_kind());
        }
        egui::CollapsingHeader::new("Import evidence").show(ui, |ui| {
            ui.label("Absolute manifest file");
            ui.add_enabled(available, egui::TextEdit::singleline(&mut self.source));
            ui.label("Manifest SHA-256");
            ui.add_enabled(
                available,
                egui::TextEdit::singleline(&mut self.source_sha256),
            );
            if ui
                .add_enabled(available, egui::Button::new("Import as pending evidence"))
                .clicked()
            {
                kind = Some((|| {
                    validate_sha256(self.source_sha256.trim())?;
                    ensure!(
                        Path::new(self.source.trim()).is_absolute(),
                        "Choose an absolute manifest file"
                    );
                    Ok(Kind::Import {
                        id: crate::session::new_request_id(),
                        path: self.source.trim().into(),
                        sha256: self.source_sha256.trim().into(),
                    })
                })());
            }
        });
        egui::CollapsingHeader::new("Original request recovery").default_open(true).show(ui, |ui| {
            if ui.add_enabled(available, egui::Button::new("Refresh retained requests")).clicked() { kind = Some(Ok(Kind::Recoveries)); }
            ui.label("Retry keeps the original request, revision and intent. Historical receipts cannot restore an old decision. Unacknowledged operations remain available for explicit retry.");
            for entry in &self.recoveries {
                let name = match entry.intent { recovery::Intent::Import { .. } => "Import", recovery::Intent::Accept { .. } => "Acceptance", recovery::Intent::Withdraw { .. } => "Withdrawal" };
                if ui.selectable_label(self.recovery_id.as_ref() == Some(&entry.request_id), format!("{name} · {} · {}", if entry.acknowledged_receipt_sha256.is_some() { "receipt acknowledged" } else { "outcome unacknowledged" }, entry.request_id)).clicked() { self.recovery_id = Some(entry.request_id.clone()); self.consent = false; }
            }
            if let Some(entry) = self.recoveries.iter().find(|entry| Some(&entry.request_id) == self.recovery_id.as_ref()) {
                match &entry.intent {
                    recovery::Intent::Import { path, manifest_sha256, .. } => { ui.label(format!("Original manifest: {path}\nSHA-256: {manifest_sha256}")); }
                    recovery::Intent::Accept { dossier_id, expected_record_revision, operator } => { ui.label(format!("Original acceptance: {dossier_id}, revision {expected_record_revision}, operator {operator}")); }
                    recovery::Intent::Withdraw { dossier_id, expected_record_revision, reason } => { ui.label(format!("Original withdrawal: {dossier_id}, revision {expected_record_revision}\n{reason}")); }
                }
            }
            if ui.add_enabled(available && self.recovery_id.is_some(), egui::Button::new("Retry original request")).clicked() { kind = Some(self.recovery_kind(false)); }
            let acknowledged = self.recoveries.iter().any(|entry| Some(&entry.request_id) == self.recovery_id.as_ref() && entry.acknowledged_receipt_sha256.is_some());
            if ui.add_enabled(available && acknowledged, egui::Button::new("Dismiss acknowledged request")).clicked() { kind = Some(self.recovery_kind(true)); }
        });
        if let Some(receipt) = &self.last_receipt {
            ui.label(format!("Last operation receipt: {:?}, revision {}. Original request {}. Refresh status for the current record.", receipt.report.state, receipt.report.revision, receipt.request_id));
        }
        ui.label(&self.message);
        match kind {
            Some(Ok(kind)) => match self.begin(kind) {
                Ok(action) => Some(action),
                Err(error) => {
                    self.message = error.to_string();
                    None
                }
            },
            Some(Err(error)) => {
                self.consent = false;
                self.message = error.to_string();
                None
            }
            None => None,
        }
    }
}
fn short(value: &str) -> &str {
    &value[..value.len().min(12)]
}
fn check_saved(kind: &Kind, scope: &Scope, receipt: &InferenceEquivalenceReceipt) -> Result<()> {
    receipt.validate()?;
    match kind {
        Kind::Import { id, sha256, .. } => ensure!(
            receipt.request_id == *id && receipt.input_sha256.as_ref() == Some(sha256),
            "Import receipt differs from captured request"
        ),
        Kind::Decide { id, intent } | Kind::Retry { id, intent } => {
            ensure!(
                receipt.request_id == *id,
                "Receipt has another original request"
            );
            match intent {
                recovery::Intent::Import {
                    manifest_sha256,
                    expected_dossier_id,
                    ..
                } => ensure!(
                    receipt.input_sha256.as_ref() == Some(manifest_sha256)
                        && receipt.report.dossier_id == *expected_dossier_id,
                    "Import recovery receipt differs"
                ),
                recovery::Intent::Accept {
                    dossier_id,
                    expected_record_revision,
                    operator,
                }
                | recovery::Intent::Withdraw {
                    dossier_id,
                    expected_record_revision,
                    reason: operator,
                } => {
                    let decision = receipt
                        .report
                        .decision
                        .as_ref()
                        .context("Decision absent")?;
                    ensure!(
                        receipt.input_sha256.is_none()
                            && receipt.report.dossier_id == *dossier_id
                            && expected_record_revision.checked_add(1)
                                == Some(receipt.report.revision)
                            && decision.actor_session == scope.session
                            && decision.request_id == *id,
                        "Decision receipt differs from captured owner, record or revision"
                    );
                    match intent {
                        recovery::Intent::Accept { .. } => ensure!(
                            decision.state == InferenceEquivalenceState::Accepted
                                && decision.operator.as_ref() == Some(operator),
                            "Acceptance attribution differs"
                        ),
                        recovery::Intent::Withdraw { .. } => ensure!(
                            decision.state == InferenceEquivalenceState::Withdrawn
                                && decision.note == *operator,
                            "Withdrawal reason differs"
                        ),
                        _ => unreachable!(),
                    }
                }
            }
        }
        _ => bail!("Unexpected receipt for read-only action"),
    }
    Ok(())
}
fn show_status(ui: &mut egui::Ui, status: &InferenceEquivalenceStatus) {
    let r = &status.report;
    let s = &r.manifest.subject;
    ui.strong(format!(
        "{:?} · revision {} · {:?} evidence",
        r.state, r.revision, s.origin
    ));
    ui.label(format!(
        "Quality tier: {:?}\nRecipe: {:?}\nAxes: {:?}",
        s.scope.tier, s.scope.recipe, s.scope.axes
    ));
    for (label, p, environment) in [
        ("From", &s.from_profile, &s.from_environment),
        ("To", &s.to_profile, &s.to_environment),
    ] {
        ui.label(format!(
            "{label}: {:?}, {}×{}, {} CPU threads; {} / {}",
            p.provider,
            p.preprocessing.input.width,
            p.preprocessing.input.height,
            p.cpu_threads,
            environment.os,
            environment.architecture
        ));
        ui.small(format!(
            "Model SHA-256: {}\nRuntime SHA-256: {}",
            p.model.sha256, p.runtime.sha256
        ));
    }
    ui.label(format!("Declared validity: {} to {} Unix milliseconds (exclusive end). Status does not renew this interval.", s.not_before_unix_ms, s.expires_unix_ms));
    if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        if now.as_millis() >= u128::from(s.expires_unix_ms) {
            ui.colored_label(egui::Color32::YELLOW, "The declared interval has elapsed by this computer's clock. The Engine rechecks validity at submission.");
        } else if now.as_millis() < u128::from(s.not_before_unix_ms) {
            ui.label("The declared interval has not started by this computer's clock.");
        }
    }
    ui.label(format!("Scope limitations: {}", s.scope.limitations));
    ui.small(format!(
        "Source policy: {}\nView policy: {}",
        s.scope.source_policy.sha256, s.scope.view_policy.sha256
    ));
    egui::CollapsingHeader::new("Exact declared profiles and evidence references").show(ui, |ui| {
        for (label, profile, environment) in [("From", &s.from_profile, &s.from_environment), ("To", &s.to_profile, &s.to_environment)] {
            ui.label(format!("{label} preprocessing: {:?}", profile.preprocessing));
            ui.label(format!("Declared minimum memory: {} bytes; output budget: {} bytes", profile.minimum_memory_bytes, profile.minimum_output_bytes));
            ui.small(format!("Engine: {}\nWorker: {}\nPlatform build: {}\nHardware: {}", environment.engine.sha256, environment.worker.sha256, environment.platform_build.sha256, environment.hardware.sha256));
            for dependency in &environment.dependencies { ui.small(format!("Dependency: {} ({} bytes)", dependency.sha256, dependency.byte_len)); }
        }
        for evidence in &r.manifest.evidence { ui.small(format!("{:?}: {} ({} bytes)", evidence.kind, evidence.identity.sha256, evidence.identity.byte_len)); }
        ui.label("These fingerprints identify the supplied evidence. Review its actual contents before recording acceptance.");
    });
    if status.withdrawal_pending {
        ui.colored_label(egui::Color32::YELLOW, "Withdrawal pending. This record cannot admit new equivalence use; retry the original withdrawal request.");
    }
    if let Some(decision) = &r.decision {
        ui.label(format!(
            "Recorded decision: {:?}\nAttribution: {}\nNote: {}\nRecorded Unix milliseconds: {}",
            decision.state,
            decision
                .operator
                .as_deref()
                .unwrap_or("No acceptance attribution"),
            decision.note,
            decision
                .recorded_at_unix_ms
                .map(|v| v.to_string())
                .unwrap_or_else(|| "Unavailable".into())
        ));
        ui.small(format!("Decision request: {}", decision.request_id));
    } else {
        ui.label("No owner decision recorded.");
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::inference_equivalence_recovery::tests::pending_report;
    fn scope() -> Scope {
        Scope {
            session: SessionId::new("desktop-evidence-fixture").unwrap(),
            credential: "a".repeat(64),
        }
    }
    fn status() -> InferenceEquivalenceStatus {
        InferenceEquivalenceStatus {
            report: pending_report(),
            withdrawal_pending: false,
            clock_floor_unix_ms: None,
            report_only: true,
        }
    }
    fn ready() -> EquivalencePanel {
        let value = status();
        let mut panel = EquivalencePanel::default();
        panel.select(value.report.dossier_id.clone());
        let action = panel
            .begin(Kind::Status {
                dossier: value.report.dossier_id.clone(),
            })
            .unwrap();
        panel.finish(
            &action,
            Some(scope()),
            &Ok(ResponseBody::InferenceEquivalenceStatus(Box::new(value))),
            &Ok(vec![]),
        );
        assert!(panel.current.is_some());
        panel
    }
    fn entry(id: &RequestId, intent: &recovery::Intent, acknowledged: bool) -> recovery::Entry {
        serde_json::from_value(serde_json::json!({"request_id":id,"intent":intent,"acknowledged_receipt_sha256":acknowledged.then(|| "b".repeat(64)),"owner_binding":"c".repeat(64)})).unwrap()
    }
    fn withdrawn(id: &RequestId, intent: &recovery::Intent) -> InferenceEquivalenceReceipt {
        let recovery::Intent::Withdraw {
            dossier_id,
            expected_record_revision,
            reason,
        } = intent
        else {
            panic!("withdrawal only");
        };
        let mut report = pending_report();
        assert_eq!(&report.dossier_id, dossier_id);
        report.revision = expected_record_revision + 1;
        report.state = InferenceEquivalenceState::Withdrawn;
        report.decision = Some(InferenceEquivalenceDecision {
            revision: report.revision,
            state: report.state,
            request_id: id.clone(),
            actor_session: scope().session,
            operator: None,
            note: reason.clone(),
            recorded_at_unix_ms: None,
            review_identity: None,
        });
        let receipt = InferenceEquivalenceReceipt {
            input_sha256: None,
            request_id: id.clone(),
            report,
        };
        receipt.validate().unwrap();
        receipt
    }
    #[test]
    fn delayed_status_cannot_overwrite_new_selection_or_newer_pending_action() {
        let mut panel = ready();
        let original = panel.selected.clone().unwrap();
        let action = panel.begin(Kind::Status { dossier: original }).unwrap();
        let other = ArtifactId::new("f".repeat(64)).unwrap();
        panel.select(other.clone());
        panel.finish(
            &action,
            Some(scope()),
            &Ok(ResponseBody::InferenceEquivalenceStatus(Box::new(status()))),
            &Ok(vec![]),
        );
        assert_eq!(panel.selected.as_ref(), Some(&other));
        assert!(panel.current.is_none());
        assert!(!panel.consent);
        let newer = panel.begin(Kind::Status { dossier: other }).unwrap();
        panel.finish(
            &action,
            Some(scope()),
            &Err("late duplicate".into()),
            &Ok(vec![]),
        );
        assert_eq!(panel.active.as_ref(), Some(&newer));
    }
    #[test]
    fn refresh_and_wrong_reply_clear_consent_without_installing_unrelated_status() {
        let mut panel = ready();
        panel.consent = true;
        let action = panel
            .begin(Kind::Status {
                dossier: panel.selected.clone().unwrap(),
            })
            .unwrap();
        assert!(!panel.consent);
        assert!(panel.current.is_none());
        let mut unrelated = status();
        unrelated
            .report
            .manifest
            .subject
            .scope
            .limitations
            .push_str(" unrelated scope");
        unrelated.report.dossier_id = unrelated.report.manifest.id().unwrap();
        panel.finish(
            &action,
            Some(scope()),
            &Ok(ResponseBody::InferenceEquivalenceStatus(Box::new(
                unrelated,
            ))),
            &Ok(vec![]),
        );
        assert!(panel.current.is_none());
        assert!(panel.message.contains("another selected record"));
        assert!(panel.accept_kind().is_err());
    }
    #[test]
    fn synthetic_and_pending_withdrawal_refuse_owner_acceptance_in_the_view() {
        let mut panel = ready();
        panel.consent = true;
        panel.operator = "Authored operator text; no actual acceptance".into();
        assert!(panel
            .accept_kind()
            .unwrap_err()
            .to_string()
            .contains("Synthetic"));
        // This is only an authored view-state declaration, never submitted or accepted.
        let mut declared = status();
        declared.report.manifest.subject.origin = InferenceEquivalenceOrigin::Measured;
        declared.report.dossier_id = declared.report.manifest.id().unwrap();
        panel.select(declared.report.dossier_id.clone());
        panel.current = Some(declared.clone());
        panel.operator = "Authored view-state operator".into();
        assert!(panel.accept_kind().is_err());
        panel.consent = true;
        let kind = panel.accept_kind().unwrap();
        let Kind::Decide {
            id,
            intent:
                recovery::Intent::Accept {
                    dossier_id,
                    expected_record_revision,
                    operator,
                },
        } = &kind
        else {
            panic!("explicit acceptance request");
        };
        assert_eq!(dossier_id, &declared.report.dossier_id);
        assert_eq!(*expected_record_revision, 1);
        assert_eq!(operator, "Authored view-state operator");
        let captured = id.clone();
        let action = panel.begin(kind).unwrap();
        assert!(!panel.consent);
        assert!(matches!(&action.kind, Kind::Decide { id, .. } if *id == captured));
        panel.enqueue_failed(&action);
        panel.current = Some(declared);
        panel.current.as_mut().unwrap().withdrawal_pending = true;
        panel.consent = true;
        assert!(panel.accept_kind().is_err());
    }
    #[test]
    fn exact_recovery_preserves_original_id_and_acknowledgment_controls_dismissal() {
        let mut panel = ready();
        panel.reason = "Authored synthetic withdrawal".into();
        let Kind::Decide { id, intent } = panel.withdraw_kind().unwrap() else {
            panic!("withdrawal");
        };
        panel.recoveries = vec![entry(&id, &intent, false)];
        panel.recovery_id = Some(id.clone());
        assert!(panel.recovery_kind(true).is_err());
        assert!(
            matches!(panel.recovery_kind(false).unwrap(), Kind::Retry { id: retry_id, intent: retry_intent } if retry_id == id && retry_intent == intent)
        );
        let action = panel.begin(panel.recovery_kind(false).unwrap()).unwrap();
        let receipt = withdrawn(&id, &intent);
        panel.finish(
            &action,
            Some(scope()),
            &Ok(ResponseBody::InferenceEquivalenceSaved(Box::new(
                receipt.clone(),
            ))),
            &Ok(vec![entry(&id, &intent, true)]),
        );
        assert!(
            panel.current.is_none(),
            "A historical receipt must not become fresh status"
        );
        assert_eq!(panel.last_receipt.as_ref().unwrap().request_id, id);
        assert!(
            matches!(panel.recovery_kind(true).unwrap(), Kind::Dismiss { id: dismissed } if dismissed == id)
        );
        let mut wrong = receipt;
        wrong.report.decision.as_mut().unwrap().note = "Substituted reason".into();
        assert!(check_saved(&Kind::Retry { id, intent }, &scope(), &wrong).is_err());
    }
    struct FixtureBackend {
        scope: Scope,
        calls: Vec<(RequestId, recovery::Intent)>,
    }
    impl Backend for FixtureBackend {
        fn scope(&self) -> Scope {
            self.scope.clone()
        }
        fn proof(&self) -> Result<String> {
            bail!("Mutation/recovery fixture must not directly read owner proof")
        }
        fn read(&mut self, _: Command) -> Result<ResponseBody> {
            bail!("Unexpected read")
        }
        fn mutate(&mut self, id: RequestId, intent: recovery::Intent) -> Result<ResponseBody> {
            let receipt = withdrawn(&id, &intent);
            self.calls.push((id, intent));
            Ok(ResponseBody::InferenceEquivalenceSaved(Box::new(receipt)))
        }
        fn retry(&mut self, id: &RequestId, intent: &recovery::Intent) -> Result<ResponseBody> {
            self.mutate(id.clone(), intent.clone())
        }
        fn dismiss(&mut self, _: &RequestId) -> Result<()> {
            bail!("Unexpected dismissal")
        }
    }
    #[test]
    fn background_recovery_uses_exact_captured_request_and_changed_session_blocks_before_effect() {
        let mut panel = ready();
        panel.reason = "Authored background withdrawal".into();
        let Kind::Decide { id, intent } = panel.withdraw_kind().unwrap() else {
            panic!("withdrawal");
        };
        let action = panel
            .begin(Kind::Retry {
                id: id.clone(),
                intent: intent.clone(),
            })
            .unwrap();
        let captured = action.clone();
        let (response, calls) = std::thread::spawn(move || {
            let mut backend = FixtureBackend {
                scope: scope(),
                calls: vec![],
            };
            let response = execute_with(&mut backend, &captured).unwrap();
            (response, backend.calls)
        })
        .join()
        .unwrap();
        assert_eq!(calls, vec![(id.clone(), intent.clone())]);
        panel.finish(
            &action,
            Some(scope()),
            &Ok(response),
            &Ok(vec![entry(&id, &intent, true)]),
        );
        assert_eq!(panel.last_receipt.as_ref().unwrap().request_id, id);
        let mut backend = FixtureBackend {
            scope: Scope {
                credential: "d".repeat(64),
                ..scope()
            },
            calls: vec![],
        };
        assert!(execute_with(&mut backend, &action).is_err());
        assert!(backend.calls.is_empty());
    }
    #[test]
    fn actor_change_and_invalid_page_cannot_refresh_decision_state() {
        let mut panel = ready();
        panel.consent = true;
        let action = panel.begin(Kind::Recoveries).unwrap();
        panel.finish(
            &action,
            Some(Scope {
                session: SessionId::new("replacement-session").unwrap(),
                ..scope()
            }),
            &Ok(ResponseBody::Ack),
            &Ok(vec![]),
        );
        assert!(panel.selected.is_none() && panel.current.is_none() && !panel.consent);
        let after = pending_report().dossier_id;
        let action = panel.begin(Kind::List { after: Some(after) }).unwrap();
        let wrong = InferenceEquivalencePage {
            entries: vec![status()],
            next_after: None,
            report_only: true,
        };
        panel.finish(
            &action,
            panel.scope.clone(),
            &Ok(ResponseBody::InferenceEquivalenceList(Box::new(wrong))),
            &Ok(vec![]),
        );
        assert!(panel.page.is_none());
        assert!(panel.message.contains("cursor"));
    }
    #[test]
    fn headless_render_preserves_pending_review_without_dispatch_or_consent() {
        let mut panel = ready();
        let before = panel.selected.clone();
        let context = egui::Context::default();
        let _ = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                assert!(panel.ui(ui, false).is_none());
            });
        });
        assert_eq!(panel.selected, before);
        assert!(panel.active.is_none() && !panel.consent);
        // This exercises egui state only; it is not a rendered Desktop walkthrough.
    }
}
