//! Disposable historical evidence presentation over the shared session client.
use crate::{evidence_recovery, live_evidence, EngineApi, ResponseBody, SessionClient};
use anyhow::{bail, ensure, Result};
use eframe::egui;
use evidence_recovery::{Entry as RecoveryEntry, RemovalIntent};
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
};

type Binding = (ProjectId, RevisionId);
enum Action {
    Recover,
    DismissRecovery(RequestId),
    List(Option<AttemptId>),
    Inspect(AttemptId),
    Download(AttemptId, LiveCudaEvidencePart, PathBuf),
    Remove(RemovalIntent),
}
enum Output {
    Recovery(Vec<RecoveryEntry>),
    Page(LiveCudaEvidencePage),
    Inspection(Box<RetainedLiveCudaEvidence>),
    Download(live_evidence::DownloadReceipt),
    Removed(RemovalIntent, LiveCudaEvidenceRemoval),
}
struct Work {
    binding: Option<Binding>,
    action: Action,
}
struct Reply {
    binding: Option<Binding>,
    removal: Option<RemovalIntent>,
    result: Result<Output, String>,
}

fn query(client: &mut impl EngineApi, binding: &Binding, action: Action) -> Result<Output> {
    let (project, revision) = binding;
    match action {
        Action::Recover | Action::DismissRecovery(_) => {
            bail!("Recovery requires private session storage")
        }
        Action::List(after) => {
            let ResponseBody::LiveCudaEvidencePage(page) = client.execute(
                Command::ListLiveCudaEvidence {
                    after: after.clone(),
                    limit: 16,
                },
                Some(project.clone()),
                Some(*revision),
            )?
            else {
                bail!("Unexpected capture listing response");
            };
            page.validate()?;
            ensure!(
                page.project == *project && page.after == after && page.limit == 16,
                "Capture listing scope differs"
            );
            Ok(Output::Page(page))
        }
        Action::Inspect(epoch) => {
            let ResponseBody::RetainedLiveCudaEvidence(report) = client.execute(
                Command::InspectLiveCudaEvidence {
                    epoch: epoch.clone(),
                },
                Some(project.clone()),
                Some(*revision),
            )?
            else {
                bail!("Unexpected capture inspection response");
            };
            report.validate()?;
            ensure!(
                report.identity.project == *project && report.identity.epoch == epoch,
                "Capture inspection scope differs"
            );
            Ok(Output::Inspection(report))
        }
        Action::Remove(intent) => {
            ensure!(intent.binding == *binding, "Removal request scope differs");
            validate_sha256(&intent.manifest_sha256)?;
            let ResponseBody::LiveCudaEvidenceRemoval(receipt) = client.execute_identified(
                intent.request_id.clone(),
                Command::RemoveRetainedLiveCudaEvidence {
                    epoch: intent.epoch.clone(),
                    expected_manifest_sha256: intent.manifest_sha256.clone(),
                },
                Some(project.clone()),
                Some(*revision),
            )?
            else {
                bail!("Unexpected capture removal response");
            };
            intent.validate_receipt(&receipt)?;
            Ok(Output::Removed(intent, receipt))
        }
        Action::Download(..) => bail!("Downloads require the session-bound bulk client"),
    }
}

fn remove_journalled(
    client: &mut impl EngineApi,
    directory: &std::path::Path,
    endpoint: &std::path::Path,
    session: &SessionId,
    credential_binding: &str,
    binding: &Binding,
    intent: RemovalIntent,
) -> Result<Output> {
    ensure!(intent.binding == *binding, "Removal request scope differs");
    evidence_recovery::prepare(directory, endpoint, session, credential_binding, &intent)?;
    let output = query(client, binding, Action::Remove(intent.clone()))?;
    let Output::Removed(_, receipt) = &output else {
        bail!("Unexpected removal output");
    };
    evidence_recovery::acknowledge(directory,endpoint,session,credential_binding,&intent,receipt)
        .map_err(|error|error.context(format!("Engine returned removal receipt for {}; recovery acknowledgement could not be persisted",intent.request_id)))?;
    Ok(output)
}

pub(crate) struct EvidencePanel {
    sender: SyncSender<Work>,
    receiver: Receiver<Reply>,
    binding: Option<Binding>,
    busy: bool,
    available: bool,
    page: Option<LiveCudaEvidencePage>,
    inspection: Option<RetainedLiveCudaEvidence>,
    part: LiveCudaEvidencePart,
    destination: String,
    notice: String,
    removal_confirmed: bool,
    pending_removal: Option<RemovalIntent>,
    recovery_ready: bool,
    recovery_entries: Vec<RecoveryEntry>,
}
impl EvidencePanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf, repaint: egui::Context) -> Self {
        let (sender, requests) = mpsc::sync_channel::<Work>(1);
        let (replies, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut client = None;
            while let Ok(work) = requests.recv() {
                let removal = match &work.action {
                    Action::Remove(intent) => Some(intent.clone()),
                    _ => None,
                };
                let result = (|| -> Result<Output> {
                    if client.is_none() {
                        client = Some(SessionClient::connect(&endpoint, &bootstrap)?);
                    }
                    let client = client.as_mut().expect("connected session");
                    let directory = bootstrap
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("Recovery state directory absent"))?;
                    let credential_binding = client.recovery_credential_binding();
                    let session = client.session_id().clone();
                    match work.action {
                        Action::Recover => Ok(Output::Recovery(evidence_recovery::load(
                            directory,
                            &endpoint,
                            &session,
                            &credential_binding,
                        )?)),
                        Action::DismissRecovery(id) => {
                            Ok(Output::Recovery(evidence_recovery::dismiss(
                                directory,
                                &endpoint,
                                &session,
                                &credential_binding,
                                &id,
                            )?))
                        }
                        Action::Remove(intent) => remove_journalled(
                            client,
                            directory,
                            &endpoint,
                            &session,
                            &credential_binding,
                            work.binding
                                .as_ref()
                                .ok_or_else(|| anyhow::anyhow!("Removal binding absent"))?,
                            intent,
                        ),
                        Action::Download(epoch, part, output) => {
                            let binding = work
                                .binding
                                .as_ref()
                                .ok_or_else(|| anyhow::anyhow!("Download binding absent"))?;
                            Ok(Output::Download(live_evidence::download(
                                client,
                                binding.0.clone(),
                                binding.1,
                                epoch,
                                part,
                                &output,
                            )?))
                        }
                        action => query(
                            client,
                            work.binding
                                .as_ref()
                                .ok_or_else(|| anyhow::anyhow!("Query binding absent"))?,
                            action,
                        ),
                    }
                })()
                .map_err(|error| error.to_string());
                if replies
                    .send(Reply {
                        binding: work.binding,
                        removal,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                repaint.request_repaint();
            }
        });
        let queued = sender.try_send(Work {
            binding: None,
            action: Action::Recover,
        });
        debug_assert!(queued.is_ok());
        Self {
            sender,
            receiver,
            binding: None,
            busy: true,
            available: true,
            page: None,
            inspection: None,
            part: LiveCudaEvidencePart::Trace,
            destination: String::new(),
            notice: String::new(),
            removal_confirmed: false,
            pending_removal: None,
            recovery_ready: false,
            recovery_entries: vec![],
        }
    }

    fn bind(&mut self, current: Option<Binding>) {
        if self.binding != current {
            self.binding = current;
            self.page = None;
            self.inspection = None;
            self.destination.clear();
            self.removal_confirmed = false;
            self.notice.clear();
        }
    }
    fn accept(&mut self, reply: Reply) {
        self.busy = false;
        if reply.binding.is_none() {
            match reply.result {
                Ok(Output::Recovery(entries)) => {
                    self.recovery_entries = entries;
                    self.recovery_ready = true;
                    self.notice =
                        "Removal recovery loaded. Retrying requires an explicit request.".into();
                }
                Err(error) => {
                    self.recovery_ready = false;
                    self.notice=format!("Removal recovery unavailable: {error}. No removal was dispatched by this recovery request.");
                }
                _ => {
                    self.recovery_ready = false;
                    self.notice = "Unexpected removal recovery response".into();
                }
            }
            return;
        }
        if let Ok(Output::Removed(intent, receipt)) = &reply.result {
            if self.pending_removal.as_ref() != Some(intent) {
                return;
            }
            if let Some(entry) = self
                .recovery_entries
                .iter_mut()
                .find(|e| e.intent.request_id == intent.request_id)
            {
                entry.receipt = Some(receipt.clone());
            } else {
                self.recovery_entries.push(RecoveryEntry {
                    intent: intent.clone(),
                    receipt: Some(receipt.clone()),
                });
            }
            self.pending_removal = None;
            self.removal_confirmed = false;
            if self
                .binding
                .as_ref()
                .is_some_and(|binding| binding.0 == intent.binding.0)
            {
                self.page = None;
                self.inspection = None;
                self.destination.clear();
            }
            self.notice = format!("Removed retained originals for capture {} in project {} ({} bytes). This does not securely erase earlier copies. Request {}.",
                receipt.epoch, receipt.project_id, receipt.removed_original_bytes, intent.request_id);
            return;
        }
        if let (Some(intent), Err(error)) = (&reply.removal, &reply.result) {
            if self.pending_removal.as_ref() == Some(intent) {
                self.notice = format!("Removal request {} for capture {} in project {} has no validated receipt: {}. Retry the saved request to reconcile its outcome.",
                    intent.request_id, intent.epoch, intent.binding.0, error);
                self.removal_confirmed = false;
                if self
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.0 == intent.binding.0)
                {
                    self.page = None;
                    self.inspection = None;
                    self.destination.clear();
                }
            }
            return;
        }
        // A requested file export still completes when the visible project changes.
        // Report its original scope without installing old data in the new view.
        if let Ok(Output::Download(receipt)) = &reply.result {
            self.notice = format!(
                "Saved {:?} from capture {} in project {}: {} ({} bytes, SHA-256 {}).",
                receipt.part,
                receipt.epoch,
                reply.binding.as_ref().expect("download binding").0,
                receipt.path.display(),
                receipt.identity.byte_len,
                receipt.identity.sha256
            );
            return;
        }
        if self.binding.as_ref() != reply.binding.as_ref() {
            return;
        }
        match reply.result {
            Ok(Output::Page(page)) => {
                self.notice = format!("{} retained capture(s) on this page.", page.entries.len());
                self.page = Some(page);
                self.inspection = None;
                self.destination.clear();
                self.removal_confirmed = false;
            }
            Ok(Output::Inspection(report)) => {
                self.inspection = Some(*report);
                self.destination.clear();
                self.removal_confirmed = false;
                self.notice = "Original evidence correspondence verified.".into();
            }
            Ok(Output::Download(_) | Output::Removed(..) | Output::Recovery(_)) => {
                unreachable!("effects handled above")
            }
            Err(error) => {
                self.page = None;
                self.inspection = None;
                self.removal_confirmed = false;
                self.notice = error;
            }
        }
    }
    pub(crate) fn poll(&mut self, current: Option<Binding>) {
        self.bind(current);
        loop {
            match self.receiver.try_recv() {
                Ok(reply) => self.accept(reply),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker_stopped();
                    break;
                }
            }
        }
    }
    fn worker_stopped(&mut self) {
        self.available = false;
        self.busy = false;
        self.page = None;
        self.inspection = None;
        self.destination.clear();
        self.removal_confirmed = false;
        self.notice = "Evidence worker stopped. Reopen the desktop window to reconnect; check any requested export destination before retrying. Copy any pending removal command below before closing.".into();
    }
    fn submit(&mut self, action: Action) {
        if !self.available {
            self.worker_stopped();
            return;
        }
        if self.busy {
            return;
        }
        let removal = match &action {
            Action::Remove(intent) => Some(intent.clone()),
            _ => None,
        };
        if removal.is_some() && !self.recovery_ready {
            self.notice = "Load removal recovery before dispatch.".into();
            return;
        }
        if let Some(intent) = &removal {
            if self
                .pending_removal
                .as_ref()
                .is_some_and(|pending| pending != intent)
            {
                return;
            }
        }
        let binding = match &action {
            Action::Recover | Action::DismissRecovery(_) => None,
            Action::Remove(intent) => Some(intent.binding.clone()),
            _ => {
                let Some(binding) = self.binding.clone() else {
                    return;
                };
                Some(binding)
            }
        };
        match self.sender.try_send(Work { binding, action }) {
            Ok(()) => {
                self.busy = true;
                self.notice = if let Some(intent) = removal {
                    self.pending_removal = Some(intent);
                    self.removal_confirmed = false;
                    "Requesting retained evidence removal…".into()
                } else {
                    "Reading retained evidence…".into()
                };
            }
            Err(mpsc::TrySendError::Disconnected(_)) => self.worker_stopped(),
            Err(mpsc::TrySendError::Full(_)) => {
                self.notice =
                    "Evidence queue is full. Wait for the current request before retrying.".into()
            }
        }
    }
    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Capture evidence");
        ui.label(
            "Browse retained CUDA captures, inspect their records, and save original artifacts.",
        );
        ui.small("Historical evidence does not establish motion quality or device qualification.");
        if ui
            .add_enabled(
                !self.busy && self.available,
                egui::Button::new("Reload removal recovery"),
            )
            .clicked()
        {
            self.submit(Action::Recover);
        }
        if !self.recovery_ready {
            ui.small("Removal controls require valid private recovery storage. Browsing and downloads remain available.");
        }
        for entry in self.recovery_entries.clone() {
            ui.group(|ui| {
                ui.label(format!("Saved removal {} · capture {} · project {}",entry.intent.request_id,entry.intent.epoch,entry.intent.binding.0));
                if let Some(receipt)=entry.receipt {
                    ui.small(format!("Validated historical receipt: {} original bytes removed; no physical erasure.",receipt.removed_original_bytes));
                    if ui.add_enabled(!self.busy && self.available,egui::Button::new("Dismiss acknowledged recovery")).clicked() {self.submit(Action::DismissRecovery(entry.intent.request_id));}
                } else {
                    ui.small("Outcome not acknowledged. This entry is retained until a validated receipt is saved.");
                    if ui.add_enabled(!self.busy && self.available && self.recovery_ready && self.pending_removal.as_ref().is_none_or(|intent|intent==&entry.intent),egui::Button::new("Retry retained removal")).clicked() {self.submit(Action::Remove(entry.intent));}
                }
            });
        }
        if let Some(intent) = self.pending_removal.clone() {
            ui.group(|ui| {
                ui.label(format!("Pending removal: capture {} in project {}", intent.epoch, intent.binding.0));
                ui.small("Intent is saved before dispatch and restored with the same Engine and session. A copied command provides manual recovery using the same Pulsar state directory and first-party session. Retry keeps the original request ID and revision.");
                let command=intent.recovery_command();
                if ui.button("Copy removal recovery command").clicked() {ui.ctx().copy_text(command.clone());}
                ui.collapsing("Recovery command",|ui| {ui.monospace(&command);});
                if ui.add_enabled(!self.busy && self.available && self.recovery_ready,egui::Button::new("Retry saved removal")).clicked() {self.submit(Action::Remove(intent));}
                ui.small("Clearing this selection does not cancel or undo removal. Its saved entry remains available after reloading recovery.");
                if ui.add_enabled(!self.busy,egui::Button::new("Clear retry selection")).clicked() {self.pending_removal=None;self.removal_confirmed=false;}
            });
        }
        if self.binding.is_none() {
            ui.label("Open a project to browse its retained captures.");
            ui.label(&self.notice);
            return;
        }
        let mut action = None;
        ui.add_enabled_ui(!self.busy && self.available, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Refresh / first page").clicked() { action = Some(Action::List(None)); }
                let next = self.page.as_ref().and_then(|page| page.next_after.clone());
                if ui.add_enabled(next.is_some(), egui::Button::new("Next page")).clicked() {
                    action = Some(Action::List(next));
                }
            });
            if let Some(page) = &self.page {
                ui.small("Ordered by capture ID. Refresh starts a new browse; pages may change as records are retained.");
                for entry in &page.entries {
                    ui.separator(); ui.monospace(entry.epoch.as_str());
                    ui.label(format!("Source {} · captured revision {}", entry.source, entry.revision.value()));
                    if ui.button("Inspect originals").clicked() { action = Some(Action::Inspect(entry.epoch.clone())); }
                }
            }
            if let Some(report) = &self.inspection {
                ui.separator(); ui.label(format!("Capture {}", report.identity.epoch));
                ui.label(format!("{} successful inference runs; unqualified historical output.", report.evidence.summary.successful_runs));
                egui::ComboBox::from_id_salt("capture-evidence-part").selected_text(format!("{:?}", self.part)).show_ui(ui, |ui| {
                    for part in [LiveCudaEvidencePart::Origin, LiveCudaEvidencePart::Manifest, LiveCudaEvidencePart::Summary, LiveCudaEvidencePart::Receipt, LiveCudaEvidencePart::Trace] {
                        ui.selectable_value(&mut self.part, part, format!("{part:?}"));
                    }
                });
                ui.label("Save as"); ui.text_edit_singleline(&mut self.destination);
                ui.small("Choose a new file path. Existing files are preserved.");
                if ui.add_enabled(!self.destination.trim().is_empty(), egui::Button::new("Save original artifact")).clicked() {
                    match crate::absolute_path(std::path::Path::new(self.destination.trim())) {
                        Ok(path) => action = Some(Action::Download(report.identity.epoch.clone(), self.part, path)),
                        Err(error) => self.notice = error.to_string(),
                    }
                }
                ui.separator();
                ui.label("Remove retained originals");
                ui.small("Requires project administrator and Read permission. This removes the retained record; it does not securely erase files, backups or existing downloads.");
                ui.monospace(format!("Capture {} · manifest SHA-256 {}", report.identity.epoch, report.evidence.manifest.sha256));
                ui.add_enabled_ui(self.pending_removal.is_none() && self.recovery_ready,|ui| {
                    ui.checkbox(&mut self.removal_confirmed,"I confirm removal of the originals for this capture.");
                    if ui.add_enabled(self.removal_confirmed,egui::Button::new("Remove retained originals")).clicked() {
                        if let Some(binding)=self.binding.clone() {
                            action=Some(Action::Remove(RemovalIntent {binding,epoch:report.identity.epoch.clone(),manifest_sha256:report.evidence.manifest.sha256.clone(),request_id:crate::session::new_request_id()}));
                        }
                    }
                });
                ui.collapsing("Verified record", |ui| {
                    ui.monospace(serde_json::to_string_pretty(report).unwrap_or_else(|error| error.to_string()));
                });
            }
        });
        if let Some(action) = action {
            self.submit(action);
        }
        if self.busy {
            ui.spinner();
        }
        ui.label(&self.notice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires explicitly configured retained-evidence Engine and original files"]
    fn native_desktop_worker_browses_inspects_and_saves_originals() {
        let env = |name: &str| std::env::var(name).expect(name);
        let project = ProjectId::new(env("PULSAR_EVIDENCE_PANEL_PROJECT")).unwrap();
        let revision = RevisionId::new(env("PULSAR_EVIDENCE_PANEL_REVISION").parse().unwrap());
        let epoch = AttemptId::new(env("PULSAR_EVIDENCE_PANEL_EPOCH")).unwrap();
        let output = PathBuf::from(env("PULSAR_EVIDENCE_PANEL_OUTPUT"));
        let originals = PathBuf::from(env("PULSAR_EVIDENCE_PANEL_ORIGINALS"));
        std::fs::create_dir(&output).unwrap();
        let binding = (project, revision);
        let context = egui::Context::default();
        let mut panel = EvidencePanel::new(
            PathBuf::from(env("PULSAR_EVIDENCE_PANEL_ENDPOINT")),
            PathBuf::from(env("PULSAR_EVIDENCE_PANEL_BOOTSTRAP")),
            context.clone(),
        );
        panel.bind(Some(binding.clone()));
        let wait = |panel: &mut EvidencePanel| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while panel.busy {
                assert!(
                    std::time::Instant::now() < deadline,
                    "desktop evidence worker deadline"
                );
                panel.poll(Some(binding.clone()));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        wait(&mut panel);
        assert!(panel.recovery_ready, "{}", panel.notice);
        panel.submit(Action::List(None));
        wait(&mut panel);
        assert!(panel
            .page
            .as_ref()
            .expect(&panel.notice)
            .entries
            .iter()
            .any(|entry| entry.epoch == epoch));
        panel.submit(Action::Inspect(epoch.clone()));
        wait(&mut panel);
        assert_eq!(
            panel
                .inspection
                .as_ref()
                .expect(&panel.notice)
                .identity
                .epoch,
            epoch
        );
        let rendered = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.show(ui));
        });
        assert!(!rendered.shapes.is_empty());
        for (part, name) in [
            (LiveCudaEvidencePart::Origin, "origin"),
            (LiveCudaEvidencePart::Manifest, "manifest"),
            (LiveCudaEvidencePart::Summary, "summary"),
            (LiveCudaEvidencePart::Receipt, "receipt"),
            (LiveCudaEvidencePart::Trace, "trace"),
        ] {
            let destination = output.join(format!("{name}.json"));
            panel.submit(Action::Download(epoch.clone(), part, destination.clone()));
            wait(&mut panel);
            assert!(panel.notice.starts_with("Saved "), "{}", panel.notice);
            assert_eq!(
                std::fs::read(&destination).unwrap(),
                std::fs::read(originals.join(format!("{name}.json"))).unwrap()
            );
        }
        let occupied = output.join("occupied.json");
        std::fs::write(&occupied, b"original canary").unwrap();
        panel.submit(Action::Download(
            epoch.clone(),
            LiveCudaEvidencePart::Trace,
            occupied.clone(),
        ));
        wait(&mut panel);
        assert!(panel.notice.contains("already exists"), "{}", panel.notice);
        assert_eq!(std::fs::read(occupied).unwrap(), b"original canary");
        assert_eq!(std::fs::read_dir(output).unwrap().count(), 6);
        if std::env::var("PULSAR_EVIDENCE_PANEL_REMOVAL_REFUSALS").as_deref() == Ok("1") {
            panel.submit(Action::Inspect(epoch.clone()));
            wait(&mut panel);
            assert_ne!(
                panel.inspection.as_ref().unwrap().evidence.manifest.sha256,
                "0".repeat(64)
            );
            let intent = RemovalIntent {
                binding: binding.clone(),
                epoch,
                manifest_sha256: "0".repeat(64),
                request_id: crate::session::new_request_id(),
            };
            panel.submit(Action::Remove(intent.clone()));
            wait(&mut panel);
            assert!(
                panel.notice.contains("manifest changed or differs"),
                "{}",
                panel.notice
            );
            assert_eq!(panel.pending_removal.as_ref(), Some(&intent));
            panel.submit(Action::Remove(intent.clone()));
            wait(&mut panel);
            assert!(
                panel.notice.contains("manifest changed or differs"),
                "{}",
                panel.notice
            );
            assert_eq!(panel.pending_removal.as_ref(), Some(&intent));
            panel.submit(Action::Inspect(intent.epoch.clone()));
            wait(&mut panel);
            assert!(panel.inspection.is_some());
            drop(panel);
            let mut reopened = EvidencePanel::new(
                PathBuf::from(env("PULSAR_EVIDENCE_PANEL_ENDPOINT")),
                PathBuf::from(env("PULSAR_EVIDENCE_PANEL_BOOTSTRAP")),
                context.clone(),
            );
            reopened.bind(Some(binding.clone()));
            wait(&mut reopened);
            assert!(reopened.recovery_ready, "{}", reopened.notice);
            assert!(reopened.pending_removal.is_none());
            assert!(reopened
                .recovery_entries
                .iter()
                .any(|entry| entry.intent == intent && entry.receipt.is_none()));
            reopened.submit(Action::Remove(intent.clone()));
            wait(&mut reopened);
            assert!(
                reopened.notice.contains("manifest changed or differs"),
                "{}",
                reopened.notice
            );
            assert_eq!(reopened.pending_removal.as_ref(), Some(&intent));
        }
    }
    fn binding() -> Binding {
        (ProjectId::new("project").unwrap(), RevisionId::new(3))
    }
    fn page() -> LiveCudaEvidencePage {
        LiveCudaEvidencePage {
            project: binding().0,
            after: None,
            limit: 16,
            entries: vec![],
            next_after: None,
        }
    }
    struct Api {
        page: LiveCudaEvidencePage,
        called: bool,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                command,
                Command::ListLiveCudaEvidence {
                    after: None,
                    limit: 16
                }
            ));
            assert_eq!(project, Some(binding().0));
            assert_eq!(revision, Some(binding().1));
            self.called = true;
            Ok(ResponseBody::LiveCudaEvidencePage(self.page.clone()))
        }
    }
    fn intent() -> RemovalIntent {
        RemovalIntent {
            binding: binding(),
            epoch: AttemptId::new("capture").unwrap(),
            manifest_sha256: "a".repeat(64),
            request_id: RequestId::new("removal-request").unwrap(),
        }
    }
    fn removal_receipt(intent: &RemovalIntent) -> LiveCudaEvidenceRemoval {
        LiveCudaEvidenceRemoval {
            schema_version: 1,
            project_id: intent.binding.0.clone(),
            epoch: intent.epoch.clone(),
            manifest: ArtifactIdentity {
                artifact_id: ArtifactId::new(intent.manifest_sha256.clone()).unwrap(),
                byte_len: 10,
                sha256: intent.manifest_sha256.clone(),
            },
            removed_original_bytes: 20,
            physical_erasure: false,
        }
    }
    struct RemovalApi {
        receipt: LiveCudaEvidenceRemoval,
        calls: Vec<RequestId>,
    }
    impl EngineApi for RemovalApi {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            bail!("unidentified mutation forbidden")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            let expected = intent();
            assert_eq!(id, expected.request_id);
            assert_eq!(project, Some(expected.binding.0));
            assert_eq!(revision, Some(expected.binding.1));
            let Command::RemoveRetainedLiveCudaEvidence {
                epoch,
                expected_manifest_sha256,
            } = command
            else {
                panic!("removal command")
            };
            assert_eq!(epoch, expected.epoch);
            assert_eq!(expected_manifest_sha256, expected.manifest_sha256);
            self.calls.push(id);
            Ok(ResponseBody::LiveCudaEvidenceRemoval(self.receipt.clone()))
        }
    }
    #[test]
    fn removal_controller_binds_exact_retry_and_rejects_receipt_substitution() {
        let intent = intent();
        let mut api = RemovalApi {
            receipt: removal_receipt(&intent),
            calls: vec![],
        };
        assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_ok());
        assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_ok());
        assert_eq!(api.calls, vec![intent.request_id.clone(); 2]);
        assert!(<crate::cli::Cli as clap::Parser>::try_parse_from(
            intent.recovery_command().split_whitespace()
        )
        .is_ok());
        let mut leading_dash = intent.clone();
        leading_dash.binding.0 = ProjectId::new("-project").unwrap();
        leading_dash.epoch = AttemptId::new("-capture").unwrap();
        leading_dash.request_id = RequestId::new("-request").unwrap();
        assert!(<crate::cli::Cli as clap::Parser>::try_parse_from(
            leading_dash.recovery_command().split_whitespace()
        )
        .is_ok());
        api.receipt.epoch = AttemptId::new("foreign").unwrap();
        assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_err());
        api.receipt = removal_receipt(&intent);
        api.receipt.project_id = ProjectId::new("foreign").unwrap();
        assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_err());
        api.receipt = removal_receipt(&intent);
        api.receipt.manifest.sha256 = "b".repeat(64);
        api.receipt.manifest.artifact_id = ArtifactId::new("b".repeat(64)).unwrap();
        assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_err());
        api.receipt = removal_receipt(&intent);
        api.receipt.physical_erasure = true;
        assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_err());
        assert!(query(
            &mut api,
            &(binding().0, RevisionId::new(4)),
            Action::Remove(intent)
        )
        .is_err());
    }
    fn panel_fixture() -> (EvidencePanel, Receiver<Work>, SyncSender<Reply>) {
        let (sender, requests) = mpsc::sync_channel(1);
        let (replies, receiver) = mpsc::sync_channel(1);
        (
            EvidencePanel {
                sender,
                receiver,
                binding: Some(binding()),
                busy: false,
                available: true,
                page: Some(page()),
                inspection: None,
                part: LiveCudaEvidencePart::Trace,
                destination: String::new(),
                notice: String::new(),
                removal_confirmed: true,
                pending_removal: None,
                recovery_ready: true,
                recovery_entries: vec![],
            },
            requests,
            replies,
        )
    }
    #[test]
    fn lost_removal_ack_preserves_intent_across_navigation_retry_and_worker_death() {
        let (mut panel, requests, replies) = panel_fixture();
        let intent = intent();
        panel.submit(Action::Remove(intent.clone()));
        let first = requests.recv().unwrap();
        assert_eq!(first.binding, Some(binding()));
        assert!(!panel.removal_confirmed);
        let other = (ProjectId::new("other-project").unwrap(), RevisionId::new(9));
        panel.bind(Some(other.clone()));
        panel.accept(Reply {
            binding: Some(binding()),
            removal: Some(intent.clone()),
            result: Err("lost acknowledgement".into()),
        });
        assert_eq!(panel.pending_removal.as_ref(), Some(&intent));
        assert!(panel.notice.contains("no validated receipt"));
        panel.submit(Action::Remove(intent.clone()));
        let retry = requests.recv().unwrap();
        assert_eq!(retry.binding, Some(binding()));
        let Action::Remove(retried) = retry.action else {
            panic!("retry")
        };
        assert_eq!(retried, intent);
        drop(replies);
        drop(requests);
        panel.poll(Some(other));
        assert!(!panel.available && !panel.busy);
        assert_eq!(panel.pending_removal.as_ref(), Some(&intent));
        assert!(intent
            .recovery_command()
            .contains("--request-id=removal-request --confirm-removal -- project 3 capture"));
        panel.bind(None);
        assert_eq!(panel.pending_removal.as_ref(), Some(&intent));
        let context = egui::Context::default();
        let rendered = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.show(ui));
        });
        assert!(!rendered.shapes.is_empty());
    }
    #[test]
    fn removal_receipt_clears_same_project_views_and_preserves_other_project_views() {
        let (mut panel, requests, _replies) = panel_fixture();
        let intent = intent();
        panel.submit(Action::Remove(intent.clone()));
        requests.recv().unwrap();
        panel.bind(Some((binding().0, RevisionId::new(4))));
        panel.page = Some(page());
        panel.accept(Reply {
            binding: Some(binding()),
            removal: Some(intent.clone()),
            result: Ok(Output::Removed(intent.clone(), removal_receipt(&intent))),
        });
        assert!(panel.pending_removal.is_none() && panel.page.is_none());
        assert!(panel.notice.starts_with("Removed retained originals"));
        panel.submit(Action::Remove(intent.clone()));
        requests.recv().unwrap();
        panel.bind(Some((ProjectId::new("other").unwrap(), RevisionId::new(0))));
        panel.page = Some(page());
        panel.accept(Reply {
            binding: Some(binding()),
            removal: Some(intent.clone()),
            result: Ok(Output::Removed(intent.clone(), removal_receipt(&intent))),
        });
        assert!(panel.pending_removal.is_none() && panel.page.is_some());
    }
    #[cfg(unix)]
    #[test]
    fn durable_prepare_precedes_dispatch_and_lost_ack_retries_exactly() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let endpoint = dir.path().join("engine.sock");
        let session = SessionId::new("session").unwrap();
        let credential = "b".repeat(64);
        let intent = intent();
        struct Api<'a> {
            directory: &'a std::path::Path,
            endpoint: &'a std::path::Path,
            session: &'a SessionId,
            credential: &'a str,
            calls: usize,
            intent: RemovalIntent,
        }
        impl EngineApi for Api<'_> {
            fn execute(
                &mut self,
                _: Command,
                _: Option<ProjectId>,
                _: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                bail!("unidentified mutation")
            }
            fn execute_identified(
                &mut self,
                id: RequestId,
                _: Command,
                _: Option<ProjectId>,
                _: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                let restored = evidence_recovery::load(
                    self.directory,
                    self.endpoint,
                    self.session,
                    self.credential,
                )?;
                assert_eq!(restored.len(), 1);
                assert_eq!(restored[0].intent, self.intent);
                assert_eq!(id, self.intent.request_id);
                self.calls += 1;
                if self.calls == 1 {
                    bail!("simulated lost acknowledgement");
                }
                Ok(ResponseBody::LiveCudaEvidenceRemoval(removal_receipt(
                    &self.intent,
                )))
            }
        }
        let mut api = Api {
            directory: dir.path(),
            endpoint: &endpoint,
            session: &session,
            credential: &credential,
            calls: 0,
            intent: intent.clone(),
        };
        assert!(remove_journalled(
            &mut api,
            dir.path(),
            &endpoint,
            &session,
            &credential,
            &binding(),
            intent.clone()
        )
        .is_err());
        assert_eq!(api.calls, 1);
        assert!(
            evidence_recovery::load(dir.path(), &endpoint, &session, &credential).unwrap()[0]
                .receipt
                .is_none()
        );
        assert!(remove_journalled(
            &mut api,
            dir.path(),
            &endpoint,
            &session,
            &credential,
            &binding(),
            intent.clone()
        )
        .is_ok());
        assert_eq!(api.calls, 2);
        assert!(
            evidence_recovery::load(dir.path(), &endpoint, &session, &credential).unwrap()[0]
                .receipt
                .is_some()
        );
        let denied = tempfile::tempdir().unwrap();
        std::fs::set_permissions(denied.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(remove_journalled(
            &mut api,
            denied.path(),
            &endpoint,
            &session,
            &credential,
            &binding(),
            intent
        )
        .is_err());
        assert_eq!(api.calls, 2);
    }
    #[test]
    fn restored_recovery_never_dispatches_and_storage_failure_disables_removal() {
        let (mut panel, requests, _replies) = panel_fixture();
        let intent = intent();
        panel.binding = None;
        panel.accept(Reply {
            binding: None,
            removal: None,
            result: Ok(Output::Recovery(vec![RecoveryEntry {
                intent: intent.clone(),
                receipt: None,
            }])),
        });
        assert!(panel.recovery_ready && panel.pending_removal.is_none());
        assert_eq!(panel.recovery_entries[0].intent, intent);
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        panel.accept(Reply {
            binding: None,
            removal: None,
            result: Err("invalid journal".into()),
        });
        assert!(!panel.recovery_ready);
        panel.submit(Action::Remove(intent));
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
    #[test]
    fn controller_preserves_request_scope_and_rejects_mismatched_pages() {
        let mut api = Api {
            page: page(),
            called: false,
        };
        assert!(query(&mut api, &binding(), Action::List(None)).is_ok());
        assert!(api.called);
        api.page.project = ProjectId::new("foreign").unwrap();
        assert!(query(&mut api, &binding(), Action::List(None)).is_err());
        api.page = page();
        api.page.limit = 1;
        assert!(query(&mut api, &binding(), Action::List(None)).is_err());
        api.page = page();
        api.page.after = Some(AttemptId::new("foreign-cursor").unwrap());
        assert!(query(&mut api, &binding(), Action::List(None)).is_err());
    }
    #[test]
    fn stopped_worker_retires_busy_view_and_refuses_further_requests() {
        let (sender, requests) = mpsc::sync_channel(1);
        let (replies, receiver) = mpsc::sync_channel(1);
        let mut panel = EvidencePanel {
            sender,
            receiver,
            binding: Some(binding()),
            busy: true,
            available: true,
            page: Some(page()),
            inspection: None,
            part: LiveCudaEvidencePart::Trace,
            destination: "pending.json".into(),
            notice: String::new(),
            removal_confirmed: false,
            pending_removal: None,
            recovery_ready: false,
            recovery_entries: vec![],
        };
        drop(replies);
        drop(requests);
        panel.poll(Some(binding()));
        assert!(!panel.busy && !panel.available);
        assert!(panel.page.is_none());
        assert!(panel.destination.is_empty());
        assert!(panel
            .notice
            .contains("check any requested export destination"));
        panel.submit(Action::List(None));
        assert!(!panel.busy && !panel.available);
        // Navigation cannot make a dead worker usable again.
        panel.poll(Some((binding().0, RevisionId::new(4))));
        assert!(!panel.available);
        assert!(panel.notice.contains("worker stopped"));
        let context = egui::Context::default();
        let rendered = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.show(ui));
        });
        assert!(!rendered.shapes.is_empty());
    }
    #[test]
    fn view_discards_old_replies_and_renders_without_engine_or_filesystem_access() {
        let (sender, _requests) = mpsc::sync_channel(1);
        let (_replies, receiver) = mpsc::sync_channel(1);
        let mut panel = EvidencePanel {
            sender,
            receiver,
            binding: Some(binding()),
            busy: true,
            available: true,
            page: None,
            inspection: None,
            part: LiveCudaEvidencePart::Trace,
            destination: "old.json".into(),
            notice: String::new(),
            removal_confirmed: false,
            pending_removal: None,
            recovery_ready: false,
            recovery_entries: vec![],
        };
        panel.bind(Some((binding().0, RevisionId::new(4))));
        assert!(panel.destination.is_empty());
        assert!(panel.busy);
        panel.accept(Reply {
            binding: Some(binding()),
            removal: None,
            result: Ok(Output::Page(page())),
        });
        assert!(panel.page.is_none());
        assert!(!panel.busy);
        panel.bind(Some(binding()));
        panel.accept(Reply {
            binding: Some(binding()),
            removal: None,
            result: Ok(Output::Page(page())),
        });
        assert!(panel.page.is_some());
        let context = egui::Context::default();
        let rendered = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.show(ui));
        });
        assert!(!rendered.shapes.is_empty());
    }
}
