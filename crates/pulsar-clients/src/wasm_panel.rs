//! Desktop intent and status only. Guest compilation and project effects belong to Engine.
use crate::{
    motion::ResponseBody,
    session::{EngineApi, SessionClient},
};
use anyhow::{bail, ensure, Result};
use eframe::egui;
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
};

type Binding = (ProjectId, RevisionId);
#[derive(Clone)]
struct Draft {
    binding: Binding,
    spec: WasmEditSpec,
}
#[derive(Clone)]
struct StartIntent {
    binding: Binding,
    request_id: RequestId,
    spec: WasmEditSpec,
}
use crate::wasm_archive_recovery::ArchiveIntent;
use crate::wasm_cleanup::RemovalIntent;
#[derive(Clone)]
enum Action {
    Info,
    Recover,
    DismissRecovery(RequestId),
    DismissCleanup(RequestId),
    DismissArchive(RequestId),
    Load {
        module: PathBuf,
        manifest: PathBuf,
        worker: ArtifactIdentity,
        memory: u64,
        label: String,
    },
    Start(StartIntent),
    ResolveStart(crate::wasm_recovery::Intent),
    Remove(RemovalIntent),
    Archive(Box<ArchiveIntent>),
    List,
    ListArchived(Option<AttemptId>),
    Status(AttemptId),
    Cancel {
        request_id: RequestId,
        attempt: AttemptId,
    },
}
struct Work {
    binding: Binding,
    action: Action,
}
enum Output {
    Recovery,
    Info(WasmEditorInfo),
    Draft(Box<Draft>),
    Started(Box<WasmEditReport>),
    Resolved {
        intent: Box<crate::wasm_recovery::Intent>,
        resolution: Box<WasmStartResolution>,
    },
    Catalog(WasmEditCatalog),
    History(WasmArchivePage),
    Report(Box<WasmEditReport>),
    Archived {
        intent: Box<ArchiveIntent>,
        report: Box<WasmEditReport>,
    },
    Removed {
        intent: RemovalIntent,
        report: Box<WasmEditReport>,
    },
}
struct Reply {
    binding: Binding,
    result: Result<Output, String>,
    recovery: Result<Vec<crate::wasm_recovery::Entry>, String>,
    cleanup: Result<Vec<crate::wasm_cleanup::Entry>, String>,
    archival: Result<Vec<crate::wasm_archive_recovery::Entry>, String>,
}
fn read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = options.open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "Select a regular declaration file"
    );
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= maximum,
        "Declaration exceeds byte admission; nothing was truncated"
    );
    Ok(bytes)
}
fn load(
    binding: &Binding,
    module: &Path,
    manifest: &Path,
    worker: ArtifactIdentity,
    memory: u64,
    label: String,
) -> Result<Draft> {
    let manifest: ExtensionManifest = serde_json::from_slice(&read(manifest, 16 * 1024)?)?;
    let module = read(module, WASM_MODULE_MAX_BYTES)?;
    ensure!(
        manifest.artifact.sha256 == format!("{:x}", Sha256::digest(&module))
            && manifest.artifact.byte_len == module.len() as u64,
        "Module bytes differ from declared identity"
    );
    let mut spec = WasmEditSpec {
        manifest,
        module,
        guest_memory_bytes: memory,
        expected_worker: worker,
        approve_execution: true,
        label,
    };
    spec.validate()?;
    spec.approve_execution = false;
    Ok(Draft {
        binding: binding.clone(),
        spec,
    })
}
fn request(binding: &Binding, id: RequestId, command: Command, revision: bool) -> Request {
    Request::new(id, command).in_project(binding.0.clone(), revision.then_some(binding.1))
}
fn query(client: &mut impl EngineApi, binding: &Binding, action: Action) -> Result<Output> {
    match action {
        Action::Recover
        | Action::DismissRecovery(_)
        | Action::DismissCleanup(_)
        | Action::DismissArchive(_) => {
            bail!("Recovery storage requires the session-bound client")
        }
        Action::Load {
            module,
            manifest,
            worker,
            memory,
            label,
        } => Ok(Output::Draft(
            (load(binding, &module, &manifest, worker, memory, label)?).into(),
        )),
        Action::Info => {
            let ResponseBody::WasmEditorInfo(info) =
                client.execute(Command::WasmEditorInfo, Some(binding.0.clone()), None)?
            else {
                bail!("Unexpected editor information");
            };
            info.worker.validate()?;
            ensure!(
                info.abi == WASM_PROCESSING_ABI
                    && info.maximum_buffer_bytes == WASM_BUFFER_MAX_BYTES as u64,
                "Editor ABI differs"
            );
            Ok(Output::Info(info))
        }
        Action::Start(intent) => {
            ensure!(
                intent.binding == *binding && intent.spec.approve_execution,
                "Approval does not match this project revision"
            );
            intent.spec.validate()?;
            let req = request(
                binding,
                intent.request_id.clone(),
                Command::StartWasmEdit { spec: intent.spec },
                true,
            );
            let ResponseBody::WasmEdit(report) = client.execute_identified(
                req.request_id.clone(),
                req.command.clone(),
                req.project.clone(),
                req.expected_revision,
            )?
            else {
                bail!("Unexpected editor start response; outcome is unknown");
            };
            report.validate()?;
            ensure!(
                report.project_id == binding.0
                    && report.base_revision == binding.1
                    && report.request_id == req.request_id,
                "Editor start correspondence differs"
            );
            if let Some(receipt) = &report.receipt {
                let Command::StartWasmEdit { spec } = &req.command else {
                    unreachable!()
                };
                ensure!(
                    receipt.module == spec.manifest.artifact
                        && receipt.worker == spec.expected_worker,
                    "Editor start artifact identity differs"
                );
            }
            Ok(Output::Started(report))
        }
        Action::ResolveStart(intent) => {
            ensure!(
                intent.project == binding.0,
                "Resolve the original owning project"
            );
            let reference = intent.reference()?;
            let req = request(
                binding,
                reference.resolution_request_id(&intent.project)?,
                Command::ResolveWasmStart { reference },
                false,
            );
            let ResponseBody::WasmStartResolution(resolution) = client.execute_identified(
                req.request_id.clone(),
                req.command.clone(),
                req.project.clone(),
                None,
            )?
            else {
                bail!("Unexpected start resolution; original facts remain unknown");
            };
            resolution.validate_for(&req)?;
            Ok(Output::Resolved {
                intent: Box::new(intent),
                resolution,
            })
        }
        Action::ListArchived(after) => {
            let req = request(
                binding,
                crate::session::new_request_id(),
                Command::ListArchivedWasmEdits {
                    after,
                    limit: WASM_ARCHIVE_PAGE_LIMIT,
                },
                false,
            );
            let ResponseBody::WasmArchivePage(page) = client.execute_identified(
                req.request_id.clone(),
                req.command.clone(),
                req.project.clone(),
                None,
            )?
            else {
                bail!("Unexpected archive history response");
            };
            page.validate_for(&req)?;
            Ok(Output::History(page))
        }
        Action::List => {
            let req = request(
                binding,
                crate::session::new_request_id(),
                Command::ListWasmEdits,
                false,
            );
            let ResponseBody::WasmEditCatalog(catalog) =
                client.execute(req.command.clone(), req.project.clone(), None)?
            else {
                bail!("Unexpected editor catalog");
            };
            catalog.validate_for(&req)?;
            Ok(Output::Catalog(catalog))
        }
        Action::Status(attempt) => {
            let req = request(
                binding,
                crate::session::new_request_id(),
                Command::WasmEditStatus {
                    attempt_id: attempt,
                },
                false,
            );
            let ResponseBody::WasmEdit(report) =
                client.execute(req.command.clone(), req.project.clone(), None)?
            else {
                bail!("Unexpected editor status");
            };
            report.validate_for(&req)?;
            Ok(Output::Report(report))
        }
        Action::Archive(intent) => {
            intent.validate()?;
            ensure!(intent.project == binding.0, "Archive project differs");
            let req = request(
                binding,
                intent.request_id.clone(),
                Command::ArchiveCompletedWasmEdit {
                    attempt_id: intent.attempt.clone(),
                    original_request_id: intent.original_request.clone(),
                },
                false,
            );
            let ResponseBody::WasmEditArchived(report) =
                client.execute_identified(req.request_id, req.command, req.project, None)?
            else {
                bail!("Unexpected archive response; retain and retry the original request")
            };
            intent.check_report(&report)?;
            Ok(Output::Archived {
                intent: (intent),
                report: (report),
            })
        }
        Action::Remove(intent) => {
            intent.validate()?;
            ensure!(
                intent.project == binding.0 && intent.observed_state.is_unsuccessful_terminal(),
                "Removal scope or observed state differs"
            );
            let req = request(
                binding,
                intent.request_id.clone(),
                Command::RemoveUnsuccessfulWasmEdit {
                    attempt_id: intent.attempt.clone(),
                    original_request_id: intent.original_request.clone(),
                    expected_state: intent.observed_state,
                },
                false,
            );
            let ResponseBody::WasmEditRemoved(report) = client.execute_identified(
                req.request_id.clone(),
                req.command.clone(),
                req.project.clone(),
                None,
            )?
            else {
                bail!("Unexpected cleanup response; retain and retry the original removal request")
            };
            report.validate_removal_for(&req)?;
            ensure!(
                report.base_revision == intent.observed_revision,
                "Removal acknowledgement observed revision differs"
            );
            Ok(Output::Removed {
                intent,
                report: (report),
            })
        }
        Action::Cancel {
            request_id,
            attempt,
        } => {
            let req = request(
                binding,
                request_id,
                Command::CancelWasmEdit {
                    attempt_id: attempt,
                },
                false,
            );
            let ResponseBody::WasmEdit(report) = client.execute_identified(
                req.request_id.clone(),
                req.command.clone(),
                req.project.clone(),
                None,
            )?
            else {
                bail!("Unexpected editor cancellation; outcome is unknown");
            };
            report.validate_for(&req)?;
            Ok(Output::Report(report))
        }
    }
}
fn execute_stored(
    client: &mut SessionClient,
    directory: &Path,
    endpoint: &Path,
    binding: &Binding,
    action: Action,
) -> Result<Output> {
    use crate::wasm_recovery as store;
    let session = client.session_id().clone();
    let credential = client.recovery_credential_binding();
    match action {
        Action::Recover => {
            store::load(directory, endpoint, &session, &credential)?;
            crate::wasm_cleanup::load(directory, endpoint, &session, &credential)?;
            crate::wasm_archive_recovery::load(directory, endpoint, &session, &credential)?;
            Ok(Output::Recovery)
        }
        Action::ResolveStart(intent) => {
            ensure!(intent.project == binding.0, "Resolution project differs");
            store::prepare(directory, endpoint, &session, &credential, &intent)?;
            let output = query(client, binding, Action::ResolveStart(intent.clone()))?;
            let Output::Resolved { resolution, .. } = &output else {
                bail!("Unexpected stored resolution");
            };
            match &resolution.outcome {
                WasmStartOutcome::Accepted { report } => {
                    store::acknowledge(directory, endpoint, &session, &credential, &intent, report)?
                }
                WasmStartOutcome::Withdrawn => store::withdraw(
                    directory,
                    endpoint,
                    &session,
                    &credential,
                    &intent,
                    resolution,
                )?,
            }
            Ok(output)
        }
        Action::DismissRecovery(id) => {
            store::dismiss(directory, endpoint, &session, &credential, &id)?;
            Ok(Output::Recovery)
        }
        Action::DismissCleanup(id) => {
            crate::wasm_cleanup::dismiss(directory, endpoint, &session, &credential, &id)?;
            Ok(Output::Recovery)
        }
        Action::DismissArchive(id) => {
            crate::wasm_archive_recovery::dismiss(directory, endpoint, &session, &credential, &id)?;
            Ok(Output::Recovery)
        }
        Action::Archive(intent) => {
            ensure!(intent.project == binding.0, "Archive project differs");
            crate::wasm_archive_recovery::prepare(
                directory,
                endpoint,
                &session,
                &credential,
                &intent,
            )?;
            let output = query(client, binding, Action::Archive(intent.clone()))?;
            let Output::Archived { report, .. } = &output else {
                bail!("Unexpected stored archive result")
            };
            crate::wasm_archive_recovery::acknowledge(
                directory,
                endpoint,
                &session,
                &credential,
                &intent,
                report,
            )
            .map_err(|e| {
                e.context(format!(
                    "Engine acknowledged archive {}; local publication failed",
                    intent.request_id
                ))
            })?;
            Ok(output)
        }
        Action::Remove(intent) => {
            ensure!(intent.project == binding.0, "Cleanup project differs");
            crate::wasm_cleanup::prepare(directory, endpoint, &session, &credential, &intent)?;
            let output = query(client, binding, Action::Remove(intent.clone()))?;
            let Output::Removed { report, .. } = &output else {
                bail!("Unexpected stored cleanup result")
            };
            crate::wasm_cleanup::acknowledge(directory,endpoint,&session,&credential,&intent,report).map_err(|e|e.context(format!("Engine acknowledged cleanup request {}; local acknowledgement publication failed",intent.request_id)))?;
            Ok(output)
        }
        Action::Start(intent) => {
            let metadata = store::Intent::capture(
                intent.binding.0.clone(),
                intent.binding.1,
                intent.request_id.clone(),
                &intent.spec,
            )?;
            store::prepare(directory, endpoint, &session, &credential, &metadata)?;
            let output = query(client, binding, Action::Start(intent))?;
            let Output::Started(report) = &output else {
                bail!("Unexpected stored start result");
            };
            store::acknowledge(
                directory,
                endpoint,
                &session,
                &credential,
                &metadata,
                report,
            )
            .map_err(|e| {
                e.context(format!(
                    "Engine acknowledged request {}; local recovery acknowledgement failed",
                    metadata.request_id
                ))
            })?;
            Ok(output)
        }
        Action::List => {
            let output = query(client, binding, Action::List)?;
            let Output::Catalog(catalog) = &output else {
                bail!("Unexpected recovery catalog");
            };
            store::reconcile(directory, endpoint, &session, &credential, catalog)?;
            Ok(output)
        }
        action => query(client, binding, action),
    }
}
#[derive(Default)]
struct State {
    binding: Option<Binding>,
    info: Option<WasmEditorInfo>,
    draft: Option<Draft>,
    approved: bool,
    pending: Option<StartIntent>,
    pending_removal: Option<RemovalIntent>,
    pending_archive: Option<ArchiveIntent>,
    reports: Vec<WasmEditReport>,
    history: Option<WasmArchivePage>,
    history_pending: Option<Option<AttemptId>>,
    selected: Option<AttemptId>,
    recovery: Vec<crate::wasm_recovery::Entry>,
    cleanup: Vec<crate::wasm_cleanup::Entry>,
    archival: Vec<crate::wasm_archive_recovery::Entry>,
}
impl State {
    fn bind(&mut self, current: Option<Binding>) {
        if self.binding != current {
            self.binding = current;
            self.info = None;
            self.draft = None;
            self.approved = false;
            self.reports.clear();
            self.history = None;
            self.history_pending = None;
            self.selected = None;
        }
    }
    fn invalidate_draft(&mut self) {
        self.draft = None;
        self.approved = false;
    }
    fn intent(&self) -> Result<StartIntent> {
        ensure!(
            self.pending.is_none(),
            "Reconcile the existing invocation before a new start"
        );
        ensure!(self.approved, "Approve this exact invocation first");
        let binding = self
            .binding
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Open a project"))?;
        let info = self
            .info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Refresh editor information"))?;
        ensure!(
            info.supported,
            "Editor is unavailable in this engine configuration"
        );
        let draft = self
            .draft
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Load declarations first"))?;
        ensure!(
            draft.binding == *binding && draft.spec.expected_worker == info.worker,
            "Loaded declaration scope or worker has changed"
        );
        let mut spec = draft.spec.clone();
        spec.approve_execution = true;
        spec.validate()?;
        Ok(StartIntent {
            binding: binding.clone(),
            request_id: crate::session::new_request_id(),
            spec,
        })
    }
    fn restore(&mut self, entries: Vec<crate::wasm_recovery::Entry>) {
        self.recovery = entries;
        self.approved = false;
    }
    fn restore_cleanup(&mut self, entries: Vec<crate::wasm_cleanup::Entry>) {
        self.cleanup = entries;
        self.approved = false;
    }
    fn restore_archival(&mut self, entries: Vec<crate::wasm_archive_recovery::Entry>) {
        self.archival = entries;
        self.approved = false;
    }
    fn set_aside_archive(&mut self) -> Option<ArchiveIntent> {
        self.approved = false;
        self.pending_archive.take()
    }
    fn recovered_intent(&self, entry: &crate::wasm_recovery::Entry) -> Result<StartIntent> {
        ensure!(
            entry.attempt.is_none(),
            "Inspect the already acknowledged attempt instead of starting it again"
        );
        let mut intent = self.intent()?;
        intent.request_id = entry.intent.request_id.clone();
        let metadata = crate::wasm_recovery::Intent::capture(
            intent.binding.0.clone(),
            intent.binding.1,
            intent.request_id.clone(),
            &intent.spec,
        )?;
        ensure!(metadata.matches(&entry.intent)?,"Reload the original declarations, worker, label, memory and project revision before replay");
        Ok(intent)
    }
    fn accept(&mut self, binding: &Binding, output: Output) -> Result<()> {
        if matches!(&output, Output::Recovery) {
            return Ok(());
        }
        if self.binding.as_ref() != Some(binding) {
            bail!("Reply belongs to an earlier project revision; refresh the current catalog");
        }
        match output {
            Output::Recovery => {}
            Output::Info(info) => {
                if self
                    .info
                    .as_ref()
                    .is_some_and(|old| old.worker != info.worker || old.supported != info.supported)
                {
                    self.invalidate_draft();
                }
                self.info = Some(info);
            }
            Output::Draft(draft) => {
                ensure!(
                    draft.binding == *binding
                        && self.info.as_ref().is_some_and(|info| info.worker
                            == draft.spec.expected_worker
                            && info.supported),
                    "Loaded declaration worker changed"
                );
                self.draft = Some(*draft);
                self.approved = false;
            }
            Output::Resolved { intent, resolution } => {
                ensure!(intent.project == binding.0, "Resolution project differs");
                let reference = intent.reference()?;
                let req = request(
                    binding,
                    reference.resolution_request_id(&intent.project)?,
                    Command::ResolveWasmStart { reference },
                    false,
                );
                resolution.validate_for(&req)?;
                if let Some(pending) = &self.pending {
                    if pending.request_id == intent.request_id {
                        let original = crate::wasm_recovery::Intent::capture(
                            pending.binding.0.clone(),
                            pending.binding.1,
                            pending.request_id.clone(),
                            &pending.spec,
                        )?;
                        ensure!(
                            original.matches(&intent)?,
                            "Resolution differs from pending start facts"
                        );
                        self.pending = None;
                    }
                }
                self.approved = false;
                if let WasmStartOutcome::Accepted { report } = resolution.outcome {
                    self.update(*report);
                }
            }
            Output::Started(report) => {
                ensure!(
                    self.pending
                        .as_ref()
                        .is_some_and(|intent| intent.binding == *binding
                            && intent.request_id == report.request_id),
                    "Start acknowledgement does not match pending intent"
                );
                self.pending = None;
                self.update(*report);
            }
            Output::History(page) => {
                page.validate()?;
                ensure!(
                    page.project_id == binding.0
                        && page.limit == WASM_ARCHIVE_PAGE_LIMIT
                        && self.history_pending.as_ref() == Some(&page.after),
                    "Archive history reply differs from the requested page"
                );
                self.history = Some(page);
                self.history_pending = None;
            }
            Output::Catalog(catalog) => {
                catalog.validate()?;
                ensure!(catalog.project_id == binding.0, "Catalog project changed");
                if let Some(intent) = &self.pending {
                    if intent.binding.0 == binding.0 {
                        if let Some(report) = catalog
                            .operations
                            .iter()
                            .find(|r| r.request_id == intent.request_id)
                        {
                            ensure!(
                                report.base_revision == intent.binding.1,
                                "Recovered invocation revision differs"
                            );
                            if let Some(receipt) = &report.receipt {
                                ensure!(
                                    receipt.module == intent.spec.manifest.artifact
                                        && receipt.worker == intent.spec.expected_worker,
                                    "Recovered invocation artifact identity differs"
                                );
                            }
                            self.selected = Some(report.attempt_id.clone());
                            self.pending = None;
                        }
                    }
                }
                self.reports = catalog.operations;
            }
            Output::Archived { intent, report } => {
                ensure!(
                    intent.project == binding.0,
                    "Archive acknowledgement project differs"
                );
                let pending = self
                    .pending_archive
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("No pending archival intent"))?;
                ensure!(
                    pending.matches(&intent)?,
                    "Archive acknowledgement differs from the pending request"
                );
                intent.check_report(&report)?;
                self.reports.retain(|old| old.attempt_id != intent.attempt);
                if self.selected.as_ref() == Some(&intent.attempt) {
                    self.selected = None;
                }
                self.pending_archive = None;
                self.history = None;
                self.approved = false;
            }
            Output::Removed { intent, report } => {
                ensure!(
                    self.pending_removal.as_ref() == Some(&intent),
                    "Removal acknowledgement differs from the pending cleanup"
                );
                let req = request(
                    binding,
                    intent.request_id.clone(),
                    Command::RemoveUnsuccessfulWasmEdit {
                        attempt_id: intent.attempt.clone(),
                        original_request_id: intent.original_request.clone(),
                        expected_state: intent.observed_state,
                    },
                    false,
                );
                report.validate_removal_for(&req)?;
                ensure!(
                    report.base_revision == intent.observed_revision,
                    "Removal revision differs"
                );
                self.reports.retain(|old| old.attempt_id != intent.attempt);
                if self.selected.as_ref() == Some(&intent.attempt) {
                    self.selected = None;
                }
                self.pending_removal = None;
                self.approved = false;
            }
            Output::Report(report) => self.update(*report),
        }
        Ok(())
    }
    fn set_aside(&mut self) -> Option<StartIntent> {
        self.approved = false;
        self.pending.take()
    }
    fn update(&mut self, report: WasmEditReport) {
        self.selected = Some(report.attempt_id.clone());
        self.reports
            .retain(|old| old.attempt_id != report.attempt_id);
        if self.reports.len() >= 64 {
            self.reports.remove(0);
        }
        self.reports.push(report);
    }
}
pub(crate) struct WasmPanel {
    state: State,
    sender: SyncSender<Work>,
    receiver: Receiver<Reply>,
    busy: bool,
    recovery_ready: bool,
    cleanup_ready: bool,
    archive_ready: bool,
    module: String,
    manifest: String,
    memory: u64,
    label: String,
    notice: String,
}
impl WasmPanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf, repaint: egui::Context) -> Self {
        let (sender, requests) = mpsc::sync_channel::<Work>(1);
        let (replies, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut client = None;
            while let Ok(work) = requests.recv() {
                let result = (|| -> Result<Output> {
                    if client.is_none() {
                        client = Some(SessionClient::connect(&endpoint, &bootstrap)?);
                    }
                    let directory = bootstrap
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("Recovery directory is absent"))?;
                    execute_stored(
                        client.as_mut().expect("connected session"),
                        directory,
                        &endpoint,
                        &work.binding,
                        work.action,
                    )
                })()
                .map_err(|e| e.to_string());
                let recovery = client
                    .as_ref()
                    .ok_or_else(|| "Session is not connected".to_owned())
                    .and_then(|client| {
                        let directory = bootstrap
                            .parent()
                            .ok_or_else(|| "Recovery directory is absent".to_owned())?;
                        crate::wasm_recovery::load(
                            directory,
                            &endpoint,
                            client.session_id(),
                            &client.recovery_credential_binding(),
                        )
                        .map_err(|e| e.to_string())
                    });
                let cleanup = client
                    .as_ref()
                    .ok_or_else(|| "Session is not connected".to_owned())
                    .and_then(|client| {
                        let directory = bootstrap
                            .parent()
                            .ok_or_else(|| "Recovery directory is absent".to_owned())?;
                        crate::wasm_cleanup::load(
                            directory,
                            &endpoint,
                            client.session_id(),
                            &client.recovery_credential_binding(),
                        )
                        .map_err(|e| e.to_string())
                    });
                let archival = client
                    .as_ref()
                    .ok_or_else(|| "Session is not connected".to_owned())
                    .and_then(|client| {
                        let directory = bootstrap
                            .parent()
                            .ok_or_else(|| "Recovery directory is absent".to_owned())?;
                        crate::wasm_archive_recovery::load(
                            directory,
                            &endpoint,
                            client.session_id(),
                            &client.recovery_credential_binding(),
                        )
                        .map_err(|e| e.to_string())
                    });
                if replies
                    .send(Reply {
                        binding: work.binding,
                        result,
                        recovery,
                        cleanup,
                        archival,
                    })
                    .is_err()
                {
                    break;
                }
                repaint.request_repaint();
            }
        });
        let queued = sender.try_send(Work {
            binding: (
                ProjectId::new("recovery-inspection").expect("literal project id"),
                RevisionId::new(0),
            ),
            action: Action::Recover,
        });
        debug_assert!(queued.is_ok());
        Self {
            state: State::default(),
            sender,
            receiver,
            busy: true,
            recovery_ready: false,
            cleanup_ready: false,
            archive_ready: false,
            module: String::new(),
            manifest: String::new(),
            memory: 1024 * 1024,
            label: "WASM values proposal".into(),
            notice: String::new(),
        }
    }
    fn send(&mut self, action: Action) -> bool {
        if self.busy {
            return false;
        }
        let binding = match self.state.binding.clone() {
            Some(binding) => binding,
            None if matches!(
                &action,
                Action::Recover
                    | Action::DismissRecovery(_)
                    | Action::DismissCleanup(_)
                    | Action::DismissArchive(_)
            ) =>
            {
                (
                    ProjectId::new("recovery-inspection").expect("literal project id"),
                    RevisionId::new(0),
                )
            }
            None => return false,
        };
        match self.sender.try_send(Work { binding, action }) {
            Ok(()) => {
                self.busy = true;
                true
            }
            Err(e) => {
                self.notice = format!("Editor queue unavailable: {e}");
                false
            }
        }
    }
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        current: Option<Binding>,
    ) -> Option<CandidateId> {
        self.state.bind(current);
        while let Ok(reply) = self.receiver.try_recv() {
            self.busy = false;
            match reply.recovery {
                Ok(entries) => {
                    self.state.restore(entries);
                    self.recovery_ready = true;
                }
                Err(error) => {
                    self.recovery_ready = false;
                    self.state.approved = false;
                    self.notice = format!("Recovery storage unavailable: {error}");
                }
            }
            match reply.cleanup {
                Ok(entries) => {
                    self.state.restore_cleanup(entries);
                    self.cleanup_ready = true;
                }
                Err(error) => {
                    self.cleanup_ready = false;
                    self.state.approved = false;
                    self.notice = format!("Cleanup recovery storage unavailable: {error}");
                }
            }
            match reply.archival {
                Ok(entries) => {
                    self.state.restore_archival(entries);
                    self.archive_ready = true;
                }
                Err(error) => {
                    self.archive_ready = false;
                    self.state.approved = false;
                    self.notice = format!("Archive recovery storage unavailable: {error}");
                }
            }
            match reply.result {
                Ok(output) => {
                    let resolved=match &output {
                        Output::Resolved {intent,resolution} => Some(match &resolution.outcome {
                            WasmStartOutcome::Withdrawn=>format!("Original start {} was withdrawn before acceptance; its ID cannot execute later.",intent.request_id),
                            WasmStartOutcome::Accepted {report}=>format!("Original start {} was already accepted as attempt {} ({:?}); inspect or cancel it explicitly.",intent.request_id,report.attempt_id,report.state),
                        }),
                        _=>None,
                    };
                    if let Err(e) = self.state.accept(&reply.binding, output) {
                        self.notice = e.to_string();
                    } else if let Some(message) = resolved {
                        self.notice = message;
                    }
                }
                Err(e) => {
                    self.notice = e;
                    self.state.history_pending = None;
                    self.state.approved = false;
                }
            }
        }
        ui.heading("WASM editor");
        ui.label("Propose values for review. Running an editor does not commit motion.");
        ui.collapsing("Saved invocation recovery",|ui| {
            ui.label("Saved facts never approve execution. Resolve an unknown start to recover accepted work or permanently withdraw its unpublished original ID. Accepted work is not cancelled.");
            if ui.add_enabled(!self.busy,egui::Button::new("Reload saved recovery")).clicked(){self.send(Action::Recover);}
            for entry in self.state.recovery.clone() {
                ui.monospace(format!("Request {} · project {} · revision {}",entry.intent.request_id,entry.intent.project,entry.intent.revision));
                ui.label(format!("Module {} · worker {} · guest memory {} · label {}",entry.intent.manifest.artifact.sha256,entry.intent.worker.sha256,entry.intent.guest_memory_bytes,entry.intent.label));
                if let Some(attempt)=entry.attempt {
                    ui.label(format!("Acknowledged attempt {attempt}"));
                    if ui.add_enabled(!self.busy && self.state.binding.as_ref().is_some_and(|b|b.0==entry.intent.project),egui::Button::new("Inspect recorded attempt")).clicked(){self.send(Action::Status(attempt));}
                    if ui.add_enabled(!self.busy && self.recovery_ready,egui::Button::new("Dismiss acknowledged recovery record")).clicked(){self.send(Action::DismissRecovery(entry.intent.request_id));}
                } else {
                    if ui.add_enabled(!self.busy && self.recovery_ready && self.state.binding.as_ref().is_some_and(|b|b.0==entry.intent.project),egui::Button::new("Resolve or withdraw unpublished start")).clicked(){self.send(Action::ResolveStart(entry.intent.clone()));}
                    if ui.add_enabled(!self.busy && self.recovery_ready && self.state.approved && self.state.pending.is_none(),egui::Button::new("Retry original request with freshly approved declarations")).clicked(){match self.state.recovered_intent(&entry){Ok(intent)=>{if self.send(Action::Start(intent.clone())){self.state.pending=Some(intent);self.state.approved=false;}},Err(e)=>self.notice=e.to_string()}}
                }
            }
        });
        ui.collapsing("Saved cleanup recovery",|ui| {
            ui.label("Saved cleanup facts never dispatch themselves. An absent attempt does not settle an unknown request.");
            for entry in self.state.cleanup.clone() {
                ui.monospace(format!("Cleanup {} · attempt {} · project {} · observed {:?}",entry.intent.request_id,entry.intent.attempt,entry.intent.project,entry.intent.observed_state));
                if entry.acknowledged {
                    ui.label("Cleanup acknowledgement retained.");
                    if ui.add_enabled(!self.busy && self.cleanup_ready,egui::Button::new("Dismiss acknowledged cleanup recovery")).clicked() {self.send(Action::DismissCleanup(entry.intent.request_id));}
                } else if ui.add_enabled(!self.busy && self.cleanup_ready && self.state.binding.as_ref().is_some_and(|b|b.0==entry.intent.project) && self.state.pending_removal.as_ref().is_none_or(|p|p==&entry.intent),egui::Button::new("Retry saved original cleanup request")).clicked()
                    && self.send(Action::Remove(entry.intent.clone())) {self.state.pending_removal=Some(entry.intent);self.state.approved=false;}
            }
        });
        let mut retained_candidate = None;
        ui.collapsing("Saved archive recovery",|ui| {
            ui.label("Archiving frees an operational slot and retains invocation evidence. Saved records never dispatch automatically.");
            for entry in self.state.archival.clone() {
                ui.monospace(format!("Archive {} · attempt {} · project {} · revision {}",entry.intent.request_id,entry.intent.attempt,entry.intent.project,entry.intent.observed_revision));
                if entry.acknowledged {
                    ui.label("Archive acknowledgement retained.");
                    if ui.add_enabled(!self.busy && self.archive_ready,egui::Button::new("Dismiss acknowledged archive recovery")).clicked(){self.send(Action::DismissArchive(entry.intent.request_id.clone()));}
                    if ui.add_enabled(!self.busy && self.state.binding.as_ref().is_some_and(|b|b.0==entry.intent.project),egui::Button::new("Open retained candidate for review")).clicked(){retained_candidate=Some(entry.intent.candidate);}
                } else if ui.add_enabled(!self.busy && self.archive_ready && self.state.binding.as_ref().is_some_and(|b|b.0==entry.intent.project) && self.state.pending_archive.as_ref().is_none_or(|p|p.matches(&entry.intent).unwrap_or(false)),egui::Button::new("Retry saved original archive request")).clicked()
                    && self.send(Action::Archive((entry.intent.clone()).into())) {self.state.pending_archive=Some(entry.intent);self.state.approved=false;}
            }
        });
        if self.state.binding.is_none() {
            ui.label("Open a project first.");
            return None;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Refresh editor information"))
                .clicked()
            {
                self.send(Action::Info);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Recover / refresh attempts"))
                .clicked()
            {
                self.send(Action::List);
            }
        });
        ui.collapsing("Retained completed invocation history",|ui| {
            ui.label("Historical facts retain candidate and receipt access. Pages reflect current history; refresh to include newly archived invocations.");
            if ui.add_enabled(!self.busy,egui::Button::new("Refresh first history page")).clicked() && self.send(Action::ListArchived(None)) {
                self.state.history_pending=Some(None);
            }
            if let Some(page)=self.state.history.clone() {
                if page.operations.is_empty() {ui.label("No archived invocations on this page.");}
                for report in page.operations {
                    ui.monospace(format!("Attempt {} · original request {} · revision {}",report.attempt_id,report.request_id,report.base_revision));
                    if let Some(receipt)=report.receipt {ui.monospace(format!("Module {} · output {}",receipt.module.sha256,receipt.output_sha256));}
                    if let Some(candidate)=report.candidate_id {
                        if ui.add_enabled(!self.busy,egui::Button::new("Open historical candidate for review")).clicked(){retained_candidate=Some(candidate);}
                    }
                }
                if let Some(next)=page.next {
                    if ui.add_enabled(!self.busy,egui::Button::new("Next history page")).clicked() && self.send(Action::ListArchived(Some(next.clone()))) {
                        self.state.history_pending=Some(Some(next));
                    }
                }
            }
        });
        if let Some(info) = &self.state.info {
            ui.label(if info.supported {
                "Editor execution is available"
            } else {
                "Editor execution is unavailable"
            });
            ui.monospace(format!(
                "Worker {} · {} bytes",
                info.worker.sha256, info.worker.byte_len
            ));
            if !info.native_runtime_qualified {
                ui.label("Native runtime qualification remains incomplete.");
            }
        }
        let mut changed = false;
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal(|ui| {
                ui.label("Module file");
                changed |= ui.text_edit_singleline(&mut self.module).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Manifest file");
                changed |= ui.text_edit_singleline(&mut self.manifest).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Proposal label");
                changed |= ui.text_edit_singleline(&mut self.label).changed();
            });
            changed |= ui
                .add(
                    egui::Slider::new(&mut self.memory, 65536..=16 * 1024 * 1024)
                        .text("Guest memory bytes"),
                )
                .changed();
        });
        if changed {
            self.state.invalidate_draft();
        }
        if ui
            .add_enabled(
                !self.busy && self.state.info.as_ref().is_some_and(|i| i.supported),
                egui::Button::new("Load and inspect declarations"),
            )
            .clicked()
        {
            let worker = self.state.info.as_ref().unwrap().worker.clone();
            self.state.invalidate_draft();
            self.send(Action::Load {
                module: PathBuf::from(&self.module),
                manifest: PathBuf::from(&self.manifest),
                worker,
                memory: self.memory,
                label: self.label.clone(),
            });
        }
        if let Some(draft) = &self.state.draft {
            ui.label(format!(
                "{} · {:?} · {}",
                draft.spec.manifest.name, draft.spec.manifest.role, draft.spec.manifest.abi
            ));
            ui.monospace(format!(
                "Module {} · {} bytes",
                draft.spec.manifest.artifact.sha256,
                draft.spec.module.len()
            ));
            ui.label(format!(
                "Project revision {} · {} ms · {} native memory bytes · {} output bytes",
                draft.binding.1,
                draft.spec.manifest.budget.wall_time_ms,
                draft.spec.manifest.budget.memory_bytes,
                draft.spec.manifest.budget.output_bytes
            ));
            ui.add_enabled(
                !self.busy && self.state.pending.is_none(),
                egui::Checkbox::new(
                    &mut self.state.approved,
                    "Approve this exact module and worker for one invocation",
                ),
            );
        }
        if ui
            .add_enabled(
                !self.busy
                    && self.recovery_ready
                    && self.state.approved
                    && self.state.pending.is_none(),
                egui::Button::new("Run editor proposal"),
            )
            .clicked()
        {
            match self.state.intent() {
                Ok(intent) => {
                    if self.send(Action::Start(intent.clone())) {
                        self.state.pending = Some(intent);
                        self.state.approved = false;
                        self.notice="Invocation dispatched. Recover attempts before starting again if its acknowledgement is lost.".into();
                    }
                }
                Err(e) => self.notice = e.to_string(),
            }
        }
        if let Some(pending) = self.state.pending.clone() {
            ui.monospace(format!(
                "Unconfirmed request {} · project {} · revision {}",
                pending.request_id, pending.binding.0, pending.binding.1
            ));
            ui.label("Closing this window does not cancel engine work. Recover attempts by original request before starting again.");
            if ui
                .add_enabled(
                    !self.busy && self.state.binding.as_ref() == Some(&pending.binding),
                    egui::Button::new("Retry exact approved invocation"),
                )
                .clicked()
            {
                self.send(Action::Start(pending.clone()));
            }
            ui.label("Set aside only after checking the original request, or to deliberately start a separate invocation. This does not cancel engine work.");
            if ui
                .add_enabled(!self.busy, egui::Button::new("Set aside local retry"))
                .clicked()
            {
                if let Some(intent) = self.state.set_aside() {
                    self.notice=format!("Request {} in project {} was set aside locally. Engine work may continue; recover attempts before starting again. No cancellation was sent.",intent.request_id,intent.binding.0);
                }
            }
        }
        ui.separator();
        if let Some(intent) = self.state.pending_removal.clone() {
            ui.label(format!("Cleanup request {} for attempt {} is awaiting acknowledgement. An empty catalog does not settle this request.", intent.request_id, intent.attempt));
            if ui
                .add_enabled(
                    !self.busy
                        && self.cleanup_ready
                        && self
                            .state
                            .binding
                            .as_ref()
                            .is_some_and(|b| b.0 == intent.project),
                    egui::Button::new("Retry original cleanup request"),
                )
                .clicked()
            {
                self.send(Action::Remove(intent));
            }
        }
        if let Some(intent) = self.state.pending_archive.clone() {
            ui.label(format!("Archive request {} for attempt {} awaits acknowledgement. An empty catalog does not settle it.",intent.request_id,intent.attempt));
            if ui
                .add_enabled(
                    !self.busy
                        && self.archive_ready
                        && self
                            .state
                            .binding
                            .as_ref()
                            .is_some_and(|b| b.0 == intent.project),
                    egui::Button::new("Retry original archive request"),
                )
                .clicked()
            {
                self.send(Action::Archive((intent).into()));
            }
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Set aside local archive retry"),
                )
                .clicked()
            {
                if let Some(intent) = self.state.set_aside_archive() {
                    self.notice=format!("Archive request {} was set aside locally. Its saved record and engine outcome are unchanged; inspect saved recovery before another archival.",intent.request_id);
                }
            }
        }
        let mut selected = self.state.selected.clone();
        for report in &self.state.reports {
            if ui
                .selectable_label(
                    selected.as_ref() == Some(&report.attempt_id),
                    format!(
                        "{:?} · {} · request {}",
                        report.state, report.attempt_id, report.request_id
                    ),
                )
                .clicked()
            {
                selected = Some(report.attempt_id.clone());
            }
        }
        self.state.selected = selected;
        let mut candidate = retained_candidate;
        if let Some(report) = self
            .state
            .reports
            .iter()
            .find(|r| Some(&r.attempt_id) == self.state.selected.as_ref())
            .cloned()
        {
            ui.label(format!("Base revision {}", report.base_revision));
            if let Some(error) = &report.error {
                ui.label(&error.message);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Refresh selected attempt"))
                    .clicked()
                {
                    self.send(Action::Status(report.attempt_id.clone()));
                }
                if ui
                    .add_enabled(
                        !self.busy
                            && matches!(
                                report.state,
                                WasmEditState::Running | WasmEditState::Interrupted
                            ),
                        egui::Button::new("Cancel selected attempt"),
                    )
                    .clicked()
                {
                    self.send(Action::Cancel {
                        request_id: crate::session::new_request_id(),
                        attempt: report.attempt_id.clone(),
                    });
                }
                if ui
                    .add_enabled(
                        !self.busy
                            && self.cleanup_ready
                            && self.state.pending_removal.is_none()
                            && report.state.is_unsuccessful_terminal(),
                        egui::Button::new("Remove unsuccessful attempt"),
                    )
                    .clicked()
                {
                    match RemovalIntent::capture(
                        self.state.binding.as_ref().expect("open project"),
                        &report,
                    ) {
                        Ok(intent) => {
                            if self.send(Action::Remove(intent.clone())) {
                                self.state.pending_removal = Some(intent);
                                self.state.approved = false;
                            }
                        }
                        Err(error) => self.notice = error.to_string(),
                    }
                }
                if ui
                    .add_enabled(
                        !self.busy
                            && self.archive_ready
                            && self.state.pending_archive.is_none()
                            && report.state == WasmEditState::Completed
                            && !self.state.archival.iter().any(|e| {
                                e.intent.project == report.project_id
                                    && e.intent.attempt == report.attempt_id
                            }),
                        egui::Button::new("Archive completed attempt, retain evidence"),
                    )
                    .clicked()
                {
                    match ArchiveIntent::capture(
                        self.state.binding.as_ref().expect("open project"),
                        &report,
                    ) {
                        Ok(intent) => {
                            if self.send(Action::Archive((intent.clone()).into())) {
                                self.state.pending_archive = Some(intent);
                                self.state.approved = false;
                            }
                        }
                        Err(error) => self.notice = error.to_string(),
                    }
                }
                if let Some(id) = report.candidate_id {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Open candidate for review"))
                        .clicked()
                    {
                        candidate = Some(id);
                    }
                }
            });
        }
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        if self.busy {
            ui.spinner();
        }
        candidate
    }
}

#[cfg(test)]
#[path = "wasm_panel_tests.rs"]
mod tests;
