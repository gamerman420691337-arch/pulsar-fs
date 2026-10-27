//! Portable-project UI and its bounded, headless production controller.
//! The shared clients own byte transport, path safety and publication semantics.
use crate::project_package::{
    allow_packaging_local, PackageDownloadControl, PackagePublicationError, PACKAGE_PRIVACY_WARNING,
};
pub use crate::project_package::{
    PackagePrivacyConsent, PackagePublicationOutcome, PublishedProjectPackage,
};
pub use crate::project_package_import::PackageImportPhase;
use crate::project_package_import::{
    PackageImportControl, PreparedPackageImport, PACKAGE_IMPORT_NOTICE,
};
use crate::session::{new_request_id, SessionClient};
use anyhow::{anyhow, bail, Context, Result};
use eframe::egui;
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortableTicket(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortableActionKind {
    AllowPackaging,
    StartExport,
    ExportStatus,
    CancelExport,
    ReleaseExport,
    DownloadExport,
    ExportMotionEvidence,
    ResumeMotionEvidence,
    ExportNeutralBundle,
    ResumeNeutralBundle,
    PrepareImport,
    StartImport,
    ContinueImport,
    ImportStatus,
    CancelImport,
}

#[derive(Debug)]
pub enum PortableAction {
    AllowPackaging {
        project_id: ProjectId,
        consent: PackagePrivacyConsent,
    },
    StartExport {
        project_id: ProjectId,
        revision: RevisionId,
        request_id: RequestId,
        consent: PackagePrivacyConsent,
    },
    ExportStatus {
        project_id: ProjectId,
        operation_id: PackageOperationId,
    },
    CancelExport {
        project_id: ProjectId,
        operation_id: PackageOperationId,
    },
    ReleaseExport {
        project_id: ProjectId,
        operation_id: PackageOperationId,
    },
    DownloadExport {
        project_id: ProjectId,
        operation_id: PackageOperationId,
        destination: PathBuf,
        request_id: RequestId,
        consent: PackagePrivacyConsent,
    },
    ExportMotionEvidence {
        project_id: ProjectId,
        revision: RevisionId,
        axis: Axis,
        destination: PathBuf,
        consent: PackagePrivacyConsent,
    },
    ResumeMotionEvidence {
        staging: PathBuf,
        consent: PackagePrivacyConsent,
    },
    ExportNeutralBundle {
        project_id: ProjectId,
        revision: RevisionId,
        destination: PathBuf,
        axes: Vec<Axis>,
    },
    ResumeNeutralBundle {
        staging: PathBuf,
    },
    PrepareImport {
        source: PathBuf,
    },
    StartImport {
        request_id: RequestId,
    },
    ContinueImport {
        operation_id: PackageImportOperationId,
        begin_request_id: RequestId,
        seal_request_id: RequestId,
    },
    ImportStatus {
        operation_id: PackageImportOperationId,
    },
    CancelImport {
        operation_id: PackageImportOperationId,
    },
}
impl PortableAction {
    fn kind(&self) -> PortableActionKind {
        match self {
            Self::AllowPackaging { .. } => PortableActionKind::AllowPackaging,
            Self::StartExport { .. } => PortableActionKind::StartExport,
            Self::ExportStatus { .. } => PortableActionKind::ExportStatus,
            Self::CancelExport { .. } => PortableActionKind::CancelExport,
            Self::ReleaseExport { .. } => PortableActionKind::ReleaseExport,
            Self::DownloadExport { .. } => PortableActionKind::DownloadExport,
            Self::ExportMotionEvidence { .. } => PortableActionKind::ExportMotionEvidence,
            Self::ResumeMotionEvidence { .. } => PortableActionKind::ResumeMotionEvidence,
            Self::ExportNeutralBundle { .. } => PortableActionKind::ExportNeutralBundle,
            Self::ResumeNeutralBundle { .. } => PortableActionKind::ResumeNeutralBundle,
            Self::PrepareImport { .. } => PortableActionKind::PrepareImport,
            Self::StartImport { .. } => PortableActionKind::StartImport,
            Self::ContinueImport { .. } => PortableActionKind::ContinueImport,
            Self::ImportStatus { .. } => PortableActionKind::ImportStatus,
            Self::CancelImport { .. } => PortableActionKind::CancelImport,
        }
    }
    fn is_transfer(&self) -> bool {
        matches!(
            self,
            Self::DownloadExport { .. }
                | Self::ExportMotionEvidence { .. }
                | Self::ResumeMotionEvidence { .. }
                | Self::ExportNeutralBundle { .. }
                | Self::ResumeNeutralBundle { .. }
                | Self::PrepareImport { .. }
                | Self::StartImport { .. }
                | Self::ContinueImport { .. }
        )
    }
}

#[derive(Debug)]
pub enum PortableReply {
    PackagingAllowed {
        project_id: ProjectId,
    },
    ExportStatus(PackageExportStatus),
    ImportPrepared {
        declaration: PackageUploadDeclaration,
    },
    ImportStatus(PackageImportStatus),
    Downloaded(PublishedProjectPackage),
    MotionEvidencePublished(crate::motion_evidence_export::PublishedMotionEvidence),
    NeutralBundlePublished(crate::neutral_bundle::PublishedNeutralBundle),
}
#[derive(Clone, Debug)]
pub struct PortablePublicationFailure {
    pub destination: PathBuf,
    pub outcome: PackagePublicationOutcome,
    pub recovery_staging_path: Option<PathBuf>,
}
#[derive(Debug)]
pub struct PortableFailure {
    pub message: String,
    pub publication: Option<PortablePublicationFailure>,
    pub protocol_error: Option<ProtocolError>,
}
impl PortableFailure {
    fn from_error(error: anyhow::Error) -> Self {
        let publication = error.chain().find_map(|cause| {
            cause
                .downcast_ref::<PackagePublicationError>()
                .map(|value| PortablePublicationFailure {
                    destination: value.destination.clone(),
                    outcome: value.outcome,
                    recovery_staging_path: value.recovery_staging_path.clone(),
                })
        });
        let publication = publication.or_else(|| {
            error.chain().find_map(|cause| {
                cause
                    .downcast_ref::<crate::motion_evidence_export::MotionEvidenceExportError>()
                    .map(|value| PortablePublicationFailure {
                        destination: value.destination.clone(),
                        outcome: value.outcome,
                        recovery_staging_path: Some(value.recovery_staging_path.clone()),
                    })
            })
        });
        let publication = publication.or_else(|| {
            error.chain().find_map(|cause| {
                cause
                    .downcast_ref::<crate::neutral_bundle::NeutralBundleError>()
                    .map(|value| PortablePublicationFailure {
                        destination: value.destination.clone(),
                        outcome: value.outcome,
                        recovery_staging_path: Some(value.recovery_staging_path.clone()),
                    })
            })
        });
        let protocol_error =
            error
                .chain()
                .find_map(|cause| match cause.downcast_ref::<TransportError>() {
                    Some(TransportError::Protocol(error)) => Some(error.clone()),
                    Some(_) => None,
                    None => cause.downcast_ref::<ProtocolError>().cloned(),
                });
        Self {
            message: format!("{error:#}").chars().take(8192).collect(),
            publication,
            protocol_error,
        }
    }
    fn export_start_rejected_before_operation(&self) -> bool {
        // Both codes precede admission in the engine Start path. In contrast,
        // InvalidRequest/Unsupported can result from validating an accepted reply.
        self.protocol_error.as_ref().is_some_and(|error| {
            matches!(
                error.code,
                ErrorCode::Forbidden | ErrorCode::ResourceExhausted
            )
        })
    }
}
#[derive(Debug)]
pub struct PortableEvent {
    pub ticket: PortableTicket,
    pub kind: PortableActionKind,
    pub result: std::result::Result<PortableReply, PortableFailure>,
}
#[derive(Clone, Debug)]
pub enum PortableProgress {
    Idle,
    NeutralBundle,
    Import {
        phase: PackageImportPhase,
        hashed_bytes: u64,
        accepted_bytes: u64,
        verified_bytes: u64,
        total_bytes: u64,
    },
    Download {
        received_bytes: u64,
        verification_bytes: u64,
        total_bytes: u64,
    },
}

#[derive(Clone, Default)]
struct LocalControl {
    stopped: Arc<AtomicBool>,
    neutral_bundle: bool,
    import: Option<PackageImportControl>,
    download: Option<PackageDownloadControl>,
}
impl LocalControl {
    fn for_action(action: &PortableAction) -> Self {
        Self {
            neutral_bundle: matches!(
                action,
                PortableAction::ExportNeutralBundle { .. }
                    | PortableAction::ResumeNeutralBundle { .. }
            ),
            import: matches!(
                action,
                PortableAction::PrepareImport { .. }
                    | PortableAction::StartImport { .. }
                    | PortableAction::ContinueImport { .. }
            )
            .then(PackageImportControl::default),
            download: matches!(
                action,
                PortableAction::DownloadExport { .. }
                    | PortableAction::ExportMotionEvidence { .. }
                    | PortableAction::ResumeMotionEvidence { .. }
                    | PortableAction::ExportNeutralBundle { .. }
                    | PortableAction::ResumeNeutralBundle { .. }
            )
            .then(PackageDownloadControl::default),
            ..Self::default()
        }
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(control) = &self.import {
            control.cancel();
        }
        if let Some(control) = &self.download {
            control.cancel();
        }
    }
    fn check(&self) -> Result<()> {
        if self.stopped.load(Ordering::Acquire) {
            bail!("Local transfer stopped; engine status must be queried separately");
        }
        Ok(())
    }
    fn progress(&self) -> PortableProgress {
        if self.neutral_bundle {
            PortableProgress::NeutralBundle
        } else if let Some(control) = &self.import {
            PortableProgress::Import {
                phase: control.phase(),
                hashed_bytes: control.hashed_bytes(),
                accepted_bytes: control.accepted_bytes(),
                verified_bytes: control.verified_bytes(),
                total_bytes: control.total_bytes(),
            }
        } else if let Some(control) = &self.download {
            PortableProgress::Download {
                received_bytes: control.received_bytes(),
                verification_bytes: control.verification_bytes(),
                total_bytes: control.total_bytes(),
            }
        } else {
            PortableProgress::Idle
        }
    }
}
struct Work {
    ticket: PortableTicket,
    action: PortableAction,
    local: LocalControl,
}
#[derive(Clone, Copy)]
struct Pending {
    ticket: PortableTicket,
    kind: PortableActionKind,
}
struct Lane {
    commands: SyncSender<Work>,
    events: Receiver<PortableEvent>,
    pending: Option<Pending>,
}
impl Lane {
    fn submit(&mut self, work: Work) -> Result<()> {
        if self.pending.is_some() {
            bail!("This portable worker is still running or awaiting result delivery");
        }
        let pending = Pending {
            ticket: work.ticket,
            kind: work.action.kind(),
        };
        self.commands
            .try_send(work)
            .map_err(|_| anyhow!("Portable worker is unavailable; no new action was queued"))?;
        self.pending = Some(pending);
        Ok(())
    }
    fn poll(&mut self) -> Option<PortableEvent> {
        match self.events.try_recv() {
            Ok(event) => {
                if self.pending.is_some_and(|pending| pending.ticket == event.ticket && pending.kind == event.kind) {
                    self.pending = None;
                    Some(event)
                } else {
                    // An obsolete reply never clears ownership of a newer action.
                    None
                }
            }
            Err(TryRecvError::Disconnected) => self.pending.take().map(|pending| PortableEvent {
                ticket: pending.ticket, kind: pending.kind,
                result: Err(PortableFailure::from_error(anyhow!(
                    "Portable worker disconnected; submitted request IDs remain required for recovery"
                ))),
            }),
            Err(TryRecvError::Empty) => None,
        }
    }
}

type Wake = Arc<dyn Fn() + Send + Sync>;
type SessionPin = Arc<Mutex<Option<SessionId>>>;

#[derive(Clone, Debug)]
pub struct PortablePublicationRecovery {
    pub ticket: PortableTicket,
    pub project_id: ProjectId,
    pub operation_id: PackageOperationId,
    pub request_id: RequestId,
    pub publication: PortablePublicationFailure,
    pub message: String,
}
struct PublicationAttempt {
    ticket: PortableTicket,
    project_id: ProjectId,
    operation_id: PackageOperationId,
    request_id: RequestId,
}
#[derive(Clone)]
struct ImportStartPin {
    request_id: RequestId,
    package: PackageUploadDeclaration,
    operation_id: Option<PackageImportOperationId>,
}
#[derive(Default)]
struct ControllerRecovery {
    prepared: Option<PackageUploadDeclaration>,
    import_start: Option<ImportStartPin>,
    download: Option<PublicationAttempt>,
    publication: Option<PortablePublicationRecovery>,
}

/// Two bounded lanes executing the actual shared clients, not an alternate transport.
/// Session pinning compares claimed identities; every engine call still authenticates.
pub struct PortableController {
    control: Lane,
    transfer: Lane,
    next_ticket: u64,
    local: LocalControl,
    session: SessionPin,
    recovery: ControllerRecovery,
}
impl PortableController {
    pub fn new(endpoint: PathBuf, bootstrap: PathBuf) -> Result<Self> {
        Self::with_wake(endpoint, bootstrap, Arc::new(|| {}))
    }
    fn with_wake(endpoint: PathBuf, bootstrap: PathBuf, wake: Wake) -> Result<Self> {
        let session = Arc::new(Mutex::new(None));
        let control = spawn_lane(
            "pulsar-portable-control",
            endpoint.clone(),
            bootstrap.clone(),
            session.clone(),
            wake.clone(),
        )?;
        let transfer = spawn_lane(
            "pulsar-portable-transfer",
            endpoint,
            bootstrap,
            session.clone(),
            wake,
        )?;
        Ok(Self {
            control,
            transfer,
            next_ticket: 1,
            local: LocalControl::default(),
            session,
            recovery: ControllerRecovery::default(),
        })
    }
    pub fn submit(&mut self, action: PortableAction) -> Result<PortableTicket> {
        let preparing = matches!(&action, PortableAction::PrepareImport { .. });
        if preparing && self.recovery.import_start.is_some() {
            bail!("The submitted import Start still pins its retained source and declaration; recover its terminal outcome before replacing the source");
        }
        if let PortableAction::ContinueImport { operation_id, .. } = &action {
            if !self.can_continue_import(operation_id) {
                bail!("The retained source belongs to another or unresolved import Start; recover that exact Start before continuing this operation");
            }
        }
        let import_start = if let PortableAction::StartImport { request_id } = &action {
            let package = self
                .recovery
                .prepared
                .as_ref()
                .context("Prepare the selected source before submitting an import")?;
            if let Some(pin) = &self.recovery.import_start {
                if &pin.request_id != request_id || &pin.package != package {
                    bail!("Replay the exact pinned import Start request and declaration; its outcome has not been resolved");
                }
                Some(pin.clone())
            } else {
                Some(ImportStartPin {
                    request_id: request_id.clone(),
                    package: package.clone(),
                    operation_id: None,
                })
            }
        } else {
            None
        };
        if matches!(&action, PortableAction::DownloadExport { .. })
            && self.recovery.publication.is_some()
        {
            bail!("Record and handle the retained publication outcome, then explicitly acknowledge its recovery disposition before another download");
        }
        let ticket = PortableTicket(self.next_ticket);
        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .context("Portable ticket counter exhausted")?;
        let download = if let PortableAction::DownloadExport {
            project_id,
            operation_id,
            request_id,
            ..
        } = &action
        {
            Some(PublicationAttempt {
                ticket,
                project_id: project_id.clone(),
                operation_id: operation_id.clone(),
                request_id: request_id.clone(),
            })
        } else {
            None
        };
        let local = LocalControl::for_action(&action);
        let transfer = action.is_transfer();
        let work = Work {
            ticket,
            action,
            local: local.clone(),
        };
        if transfer {
            self.transfer.submit(work)?;
            self.local = local;
            if preparing {
                self.recovery.prepared = None;
            }
            if let Some(pin) = import_start {
                self.recovery.import_start = Some(pin);
            }
            if let Some(download) = download {
                self.recovery.download = Some(download);
            }
        } else {
            self.control.submit(work)?;
        }
        Ok(ticket)
    }
    /// At most one terminal reply per lane. Ownership ends only when consumed.
    pub fn poll(&mut self) -> Vec<PortableEvent> {
        let mut events = Vec::with_capacity(2);
        if let Some(event) = self.control.poll() {
            events.push(event);
        }
        if let Some(event) = self.transfer.poll() {
            events.push(event);
        }
        for event in &events {
            self.observe_recovery(event);
        }
        events
    }
    fn observe_recovery(&mut self, event: &PortableEvent) {
        match &event.result {
            Ok(PortableReply::ImportPrepared { declaration }) => {
                self.recovery.prepared = Some(declaration.clone());
            }
            Ok(PortableReply::ImportStatus(status)) => {
                if let Some(pin) = &mut self.recovery.import_start {
                    if pin.request_id == status.request_id
                        && pin.package == status.package
                        && pin
                            .operation_id
                            .as_ref()
                            .is_none_or(|operation| operation == &status.operation_id)
                    {
                        if status.state.is_terminal() {
                            self.recovery.import_start = None;
                            self.recovery.prepared = None;
                        } else {
                            pin.operation_id = Some(status.operation_id.clone());
                        }
                    }
                }
            }
            _ => {}
        }
        if event.kind == PortableActionKind::DownloadExport
            && self
                .recovery
                .download
                .as_ref()
                .is_some_and(|attempt| attempt.ticket == event.ticket)
        {
            let attempt = self
                .recovery
                .download
                .take()
                .expect("matched download attempt");
            if let Err(failure) = &event.result {
                if let Some(publication) = &failure.publication {
                    if publication.outcome != PackagePublicationOutcome::NotPublished
                        || publication.recovery_staging_path.is_some()
                    {
                        // One bounded slot: another download cannot enter while occupied.
                        self.recovery.publication = Some(PortablePublicationRecovery {
                            ticket: attempt.ticket,
                            project_id: attempt.project_id,
                            operation_id: attempt.operation_id,
                            request_id: attempt.request_id,
                            publication: publication.clone(),
                            message: failure.message.clone(),
                        });
                    }
                }
            }
        }
    }
    pub fn publication_recovery(&self) -> Option<&PortablePublicationRecovery> {
        self.recovery.publication.as_ref()
    }
    /// Records an explicit human disposition; it changes no files or engine state.
    pub fn acknowledge_publication_recovery(&mut self, ticket: PortableTicket) -> Result<()> {
        if !self
            .recovery
            .publication
            .as_ref()
            .is_some_and(|record| record.ticket == ticket)
        {
            bail!("Publication recovery acknowledgement does not match the retained ticket");
        }
        self.recovery.publication = None;
        Ok(())
    }
    fn import_source_pinned(&self) -> bool {
        self.recovery.import_start.is_some()
    }
    fn can_continue_import(&self, operation_id: &PackageImportOperationId) -> bool {
        self.recovery
            .import_start
            .as_ref()
            .is_none_or(|pin| pin.operation_id.as_ref() == Some(operation_id))
    }
    pub fn control_busy(&self) -> bool {
        self.control.pending.is_some()
    }
    pub fn transfer_busy(&self) -> bool {
        self.transfer.pending.is_some()
    }
    /// Does not assert engine cancellation or release local worker ownership early.
    pub fn stop_local_transfer(&self) {
        self.local.stop();
    }
    pub fn progress(&self) -> PortableProgress {
        if self.local.neutral_bundle && !self.transfer_busy() {
            return PortableProgress::Idle;
        }
        self.local.progress()
    }
    pub fn claimed_session(&self) -> Option<SessionId> {
        self.session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}
impl Drop for PortableController {
    fn drop(&mut self) {
        self.local.stop();
    }
}

#[cfg(test)]
thread_local! {
    static FAIL_SPAWN_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}
fn spawn_thread(
    name: &str,
    task: impl FnOnce() + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    #[cfg(test)]
    if FAIL_SPAWN_AFTER.with(|flag| match flag.get() {
        Some(0) => {
            flag.set(None);
            true
        }
        Some(left) => {
            flag.set(Some(left - 1));
            false
        }
        None => false,
    }) {
        return Err(std::io::Error::other(
            "injected portable worker spawn failure",
        ));
    }
    std::thread::Builder::new().name(name.into()).spawn(task)
}
fn pin_session(pin: &SessionPin, claimed: &SessionId) -> Result<()> {
    let mut prior = pin
        .lock()
        .map_err(|_| anyhow!("Portable session identity lock failed"))?;
    if let Some(prior) = prior.as_ref() {
        if prior != claimed {
            bail!("Portable workers resolved different session identities; no action was submitted. Retain recovery IDs and reconnect deliberately");
        }
    } else {
        *prior = Some(claimed.clone());
    }
    Ok(())
}
fn spawn_lane(
    name: &str,
    endpoint: PathBuf,
    bootstrap: PathBuf,
    pin: SessionPin,
    wake: Wake,
) -> Result<Lane> {
    let (commands, input) = mpsc::sync_channel::<Work>(1);
    let (output, events) = mpsc::sync_channel::<PortableEvent>(1);
    spawn_thread(name, move || {
        let mut client = None;
        let mut prepared = None;
        while let Ok(work) = input.recv() {
            let kind = work.action.kind();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute_action(work.action, &work.local, &mut client, &mut prepared, &endpoint, &bootstrap, &pin)
            }));
            let result = match result {
                Ok(value) => value.map_err(PortableFailure::from_error),
                Err(_) => {
                    prepared = None;
                    Err(PortableFailure::from_error(anyhow!(
                        "Portable worker action panicked; retain exact IDs and query status before retrying"
                    )))
                }
            };
            // A terminal result is never overwritten by progress or another action.
            if output.send(PortableEvent { ticket: work.ticket, kind, result }).is_err() { break; }
            wake();
        }
    }).context("Unable to start required portable worker")?;
    Ok(Lane {
        commands,
        events,
        pending: None,
    })
}
fn execute_action(
    action: PortableAction,
    local: &LocalControl,
    client: &mut Option<SessionClient>,
    prepared: &mut Option<PreparedPackageImport>,
    endpoint: &std::path::Path,
    bootstrap: &std::path::Path,
    pin: &SessionPin,
) -> Result<PortableReply> {
    local.check()?;
    if let PortableAction::PrepareImport { source } = action {
        *prepared = None;
        let value = PreparedPackageImport::open(
            &source,
            local
                .import
                .as_ref()
                .context("Missing import progress control")?,
        )?;
        let declaration = value.declaration().clone();
        *prepared = Some(value);
        return Ok(PortableReply::ImportPrepared { declaration });
    }
    if client.is_none() {
        *client = Some(SessionClient::connect(endpoint, bootstrap)?);
    }
    let client = client.as_mut().context("Portable client is unavailable")?;
    pin_session(pin, client.session_id())?;
    local.check()?;
    match action {
        PortableAction::AllowPackaging {
            project_id,
            consent,
        } => {
            allow_packaging_local(client, project_id.clone(), bootstrap, &consent)?;
            Ok(PortableReply::PackagingAllowed { project_id })
        }
        PortableAction::StartExport {
            project_id,
            revision,
            request_id,
            consent,
        } => Ok(PortableReply::ExportStatus(
            client
                .start_project_export_with_request_id(project_id, revision, &consent, request_id)?,
        )),
        PortableAction::ExportStatus {
            project_id,
            operation_id,
        } => Ok(PortableReply::ExportStatus(
            client.project_package_status(project_id, operation_id)?,
        )),
        PortableAction::CancelExport {
            project_id,
            operation_id,
        } => Ok(PortableReply::ExportStatus(
            client.cancel_project_export(project_id, operation_id)?,
        )),
        PortableAction::ReleaseExport {
            project_id,
            operation_id,
        } => Ok(PortableReply::ExportStatus(
            client.release_project_export(project_id, operation_id)?,
        )),
        PortableAction::DownloadExport {
            project_id,
            operation_id,
            destination,
            request_id,
            consent,
        } => Ok(PortableReply::Downloaded(
            client.download_project_package_with_request_id(
                project_id,
                operation_id,
                &destination,
                &consent,
                local
                    .download
                    .as_ref()
                    .context("Missing download progress control")?,
                request_id,
            )?,
        )),
        PortableAction::ExportMotionEvidence {
            project_id,
            revision,
            axis,
            destination,
            consent,
        } => Ok(PortableReply::MotionEvidencePublished(
            crate::motion_evidence_export::export_motion_evidence(
                client,
                project_id,
                revision,
                axis,
                &destination,
                &consent,
                local
                    .download
                    .as_ref()
                    .context("Missing pair transfer control")?,
            )?,
        )),
        PortableAction::ResumeMotionEvidence { staging, consent } => {
            Ok(PortableReply::MotionEvidencePublished(
                crate::motion_evidence_export::resume_motion_evidence(
                    client,
                    &staging,
                    &consent,
                    local
                        .download
                        .as_ref()
                        .context("Missing pair transfer control")?,
                )?,
            ))
        }
        PortableAction::ExportNeutralBundle {
            project_id,
            revision,
            destination,
            axes,
        } => Ok(PortableReply::NeutralBundlePublished(
            crate::neutral_bundle::export_neutral_axis_bundle(
                client,
                project_id,
                revision,
                &destination,
                &axes,
                local
                    .download
                    .as_ref()
                    .context("Missing neutral bundle stop control")?,
            )?,
        )),
        PortableAction::ResumeNeutralBundle { staging } => Ok(
            PortableReply::NeutralBundlePublished(crate::neutral_bundle::resume_neutral_bundle(
                client,
                &staging,
                local
                    .download
                    .as_ref()
                    .context("Missing neutral bundle stop control")?,
            )?),
        ),
        PortableAction::StartImport { request_id } => {
            let status = client.start_project_import(
                prepared
                    .as_ref()
                    .context("Prepare the selected source before submitting an import")?,
                request_id,
            )?;
            if status.state.is_terminal() {
                *prepared = None;
            }
            Ok(PortableReply::ImportStatus(status))
        }
        PortableAction::ContinueImport {
            operation_id,
            begin_request_id,
            seal_request_id,
        } => {
            let status = client.continue_project_import(
                prepared
                    .as_mut()
                    .context("Reopen and hash the exact source package before resuming")?,
                operation_id,
                begin_request_id,
                seal_request_id,
                local
                    .import
                    .as_ref()
                    .context("Missing import progress control")?,
            )?;
            if status.state.is_terminal() {
                *prepared = None;
            }
            Ok(PortableReply::ImportStatus(status))
        }
        PortableAction::ImportStatus { operation_id } => Ok(PortableReply::ImportStatus(
            client.project_import_status(operation_id)?,
        )),
        PortableAction::CancelImport { operation_id } => Ok(PortableReply::ImportStatus(
            client.cancel_project_import(operation_id)?,
        )),
        PortableAction::PrepareImport { .. } => {
            unreachable!("local preparation handled before connection")
        }
    }
}

#[derive(Default)]
struct ConsentState {
    context: Option<(ProjectId, RevisionId)>,
    export: bool,
    allow: bool,
    download: bool,
}
impl ConsentState {
    fn bind(&mut self, context: Option<(ProjectId, RevisionId)>) {
        if self.context != context {
            self.context = context;
            self.export = false;
            self.allow = false;
            self.download = false;
        }
    }
}
#[derive(Clone)]
struct ImportIds {
    start: RequestId,
    begin: RequestId,
    seal: RequestId,
}
impl Default for ImportIds {
    fn default() -> Self {
        Self {
            start: new_request_id(),
            begin: new_request_id(),
            seal: new_request_id(),
        }
    }
}
struct ExportView {
    request: RequestId,
    download_request: RequestId,
    target: Option<(ProjectId, RevisionId)>,
    attempted: bool,
    start_rejected: bool,
    start_ambiguous: bool,
    inspected: Option<PackageExportStatus>,
    previous: Option<Box<ExportView>>,
    previous_disposition: bool,
    status: Option<PackageExportStatus>,
    destination: String,
    recovery_project: String,
    recovery_operation: String,
    released_intentionally: bool,
    published: Option<PublishedProjectPackage>,
}
impl Default for ExportView {
    fn default() -> Self {
        Self {
            request: new_request_id(),
            download_request: new_request_id(),
            target: None,
            attempted: false,
            start_rejected: false,
            start_ambiguous: false,
            inspected: None,
            previous: None,
            previous_disposition: false,
            status: None,
            destination: String::new(),
            recovery_project: String::new(),
            recovery_operation: String::new(),
            released_intentionally: false,
            published: None,
        }
    }
}
struct MotionEvidenceView {
    destination: String,
    staging: String,
    axis: Axis,
    acknowledged: bool,
    target: Option<(ProjectId, RevisionId)>,
    published: Option<crate::motion_evidence_export::PublishedMotionEvidence>,
}
impl Default for MotionEvidenceView {
    fn default() -> Self {
        Self {
            destination: String::new(),
            staging: String::new(),
            axis: Axis::Stroke,
            acknowledged: false,
            target: None,
            published: None,
        }
    }
}
impl MotionEvidenceView {
    fn bind(&mut self, target: Option<(ProjectId, RevisionId)>) {
        if self.target != target {
            self.acknowledged = false;
            self.target = target;
        }
    }
}
#[derive(Clone, Default)]
struct ImportView {
    ids: ImportIds,
    source: String,
    declaration: Option<PackageUploadDeclaration>,
    status: Option<PackageImportStatus>,
    notice: bool,
    start_attempted: bool,
    upload_attempted: bool,
    auto_continue: bool,
    recovery_operation: String,
    previous_attempt: Option<String>,
}

/// Thin egui adapter; all effects go through PortableController and shared clients.
pub(crate) struct PortablePanel {
    controller: Option<PortableController>,
    endpoint: PathBuf,
    bootstrap: PathBuf,
    ctx: egui::Context,
    init_error: Option<String>,
    failure: Option<PortableFailure>,
    publication_disposition: bool,
    message: Option<String>,
    consent: ConsentState,
    export: ExportView,
    motion_evidence: MotionEvidenceView,
    neutral_bundle: crate::neutral_bundle_panel::NeutralBundleView,
    import: ImportView,
    pinned_import: Option<ImportView>,
    queued_control: Option<PortableAction>,
    next_poll: Instant,
    poll_import_first: bool,
}
impl PortablePanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf, ctx: egui::Context) -> Self {
        let wake_ctx = ctx.clone();
        let controller = PortableController::with_wake(
            endpoint.clone(),
            bootstrap.clone(),
            Arc::new(move || wake_ctx.request_repaint()),
        );
        let (controller, init_error) = match controller {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
        Self {
            controller,
            endpoint,
            bootstrap,
            ctx,
            init_error,
            failure: None,
            publication_disposition: false,
            message: None,
            consent: ConsentState::default(),
            export: ExportView::default(),
            motion_evidence: MotionEvidenceView::default(),
            neutral_bundle: Default::default(),
            import: ImportView::default(),
            pinned_import: None,
            queued_control: None,
            next_poll: Instant::now(),
            poll_import_first: false,
        }
    }
    fn control_busy(&self) -> bool {
        self.queued_control.is_some()
            || self
                .controller
                .as_ref()
                .is_none_or(|value| value.control_busy())
    }
    fn transfer_busy(&self) -> bool {
        self.controller
            .as_ref()
            .is_none_or(|value| value.transfer_busy())
    }
    fn submit(&mut self, action: PortableAction) -> bool {
        let result = self
            .controller
            .as_mut()
            .context("Portable workers are unavailable")
            .and_then(|controller| controller.submit(action));
        match result {
            Ok(_) => {
                self.remember_pinned_import_context();
                true
            }
            Err(error) => {
                self.failure = Some(PortableFailure::from_error(error));
                false
            }
        }
    }
    fn remember_pinned_import_context(&mut self) {
        let Some(pin) = self
            .controller
            .as_ref()
            .and_then(|controller| controller.recovery.import_start.as_ref())
        else {
            self.pinned_import = None;
            return;
        };
        if self.import.ids.start == pin.request_id
            && self.import.declaration.as_ref() == Some(&pin.package)
        {
            let mut retained = self.import.clone();
            retained.start_attempted = true;
            retained.auto_continue = false;
            self.pinned_import = Some(retained);
        }
    }
    fn queue_control(&mut self, action: PortableAction) {
        if self.queued_control.is_some() {
            self.message =
                Some("A control action is already queued; its outcome remains pending".into());
        } else {
            self.queued_control = Some(action);
            self.ctx.request_repaint();
        }
    }
    /// Called by the desktop even when this tab is hidden.
    pub(crate) fn poll(&mut self) {
        self.remember_pinned_import_context();
        let events = self
            .controller
            .as_mut()
            .map(PortableController::poll)
            .unwrap_or_default();
        for event in events {
            let kind = event.kind;
            match event.result {
                Err(error) => {
                    if kind == PortableActionKind::StartExport && self.export.status.is_none() {
                        let rejected = error.export_start_rejected_before_operation();
                        // Authorization runs before replay lookup. A later Forbidden
                        // cannot resolve an earlier lost Start acknowledgement.
                        self.export.start_rejected = rejected && !self.export.start_ambiguous;
                        self.export.start_ambiguous |= !rejected;
                    }
                    if matches!(
                        kind,
                        PortableActionKind::StartImport | PortableActionKind::ContinueImport
                    ) {
                        self.import.auto_continue = false;
                    }
                    if matches!(
                        kind,
                        PortableActionKind::ExportMotionEvidence
                            | PortableActionKind::ResumeMotionEvidence
                    ) {
                        self.motion_evidence.acknowledged = false;
                        if let Some(publication) = &error.publication {
                            if publication.outcome == PackagePublicationOutcome::NotPublished {
                                if let Some(path) = &publication.recovery_staging_path {
                                    self.motion_evidence.staging =
                                        path.to_string_lossy().into_owned();
                                }
                            }
                        }
                    }
                    if matches!(
                        kind,
                        PortableActionKind::ExportNeutralBundle
                            | PortableActionKind::ResumeNeutralBundle
                    ) {
                        self.neutral_bundle.failure(&error);
                    }
                    self.failure = Some(error);
                }
                Ok(PortableReply::PackagingAllowed { project_id }) => {
                    self.message = Some(format!("Packaging permission acknowledged for {project_id}; other project grants are unchanged"));
                }
                Ok(PortableReply::ExportStatus(status)) => {
                    // A status query is not resolution of another Start. Keep the
                    // exact replay context until its full request binding matches.
                    if self.export.attempted
                        && self.export.status.is_none()
                        && (status.request_id != self.export.request
                            || self
                                .export
                                .target
                                .as_ref()
                                .is_none_or(|(project, revision)| {
                                    *project != status.project_id
                                        || *revision != status.requested_revision
                                }))
                    {
                        self.export.inspected = Some(status);
                        continue;
                    }
                    self.export.inspected = None;
                    if self
                        .export
                        .status
                        .as_ref()
                        .is_none_or(|prior| prior.operation_id != status.operation_id)
                    {
                        self.consent.download = false;
                        self.export.released_intentionally = false;
                    }
                    self.export.request = status.request_id.clone();
                    self.export.target =
                        Some((status.project_id.clone(), status.requested_revision));
                    self.export.recovery_project = status.project_id.to_string();
                    self.export.recovery_operation = status.operation_id.to_string();
                    self.export.attempted = true;
                    self.export.status = Some(status);
                }
                Ok(PortableReply::ImportPrepared { declaration }) => {
                    self.import.declaration = Some(declaration);
                    self.message = Some("Source handle retained and hashed. This is not engine archive validation or clone publication".into());
                }
                Ok(PortableReply::ImportStatus(status)) => {
                    if let Some(prior) = &self.import.status {
                        if prior.operation_id == status.operation_id && prior.state.is_terminal() {
                            if status.state.is_terminal()
                                && !same_import_terminal_binding(prior, &status)
                            {
                                self.failure = Some(PortableFailure::from_error(anyhow!(
                                    "Contradictory terminal import status for {}; retained the original terminal receipt",
                                    status.operation_id
                                )));
                            }
                            self.import.auto_continue = false;
                            continue;
                        }
                    }
                    self.import.ids.start = status.request_id.clone();
                    self.import.recovery_operation = status.operation_id.to_string();
                    self.import.start_attempted = true;
                    if status.state.is_terminal() {
                        self.import.auto_continue = false;
                    }
                    self.import.status = Some(status);
                    self.remember_pinned_import_context();
                }
                Ok(PortableReply::NeutralBundlePublished(receipt)) => {
                    self.message = Some("Six neutral master axes published with full file/directory sync. Content integrity only; no device or quality qualification.".into());
                    self.neutral_bundle.published(receipt);
                }
                Ok(PortableReply::MotionEvidencePublished(receipt)) => {
                    self.message=Some("Neutral script, full private project and checked association published together. Content integrity only; no model or device qualification. Engine copy retained.".into());
                    self.motion_evidence.acknowledged = false;
                    self.motion_evidence.published = Some(receipt);
                }
                Ok(PortableReply::Downloaded(receipt)) => {
                    self.message = Some("New destination published after full length/SHA-256 verification and file/directory sync. Engine copy was not released".into());
                    self.export.published = Some(receipt);
                }
            }
        }
        self.remember_pinned_import_context();
        let control_idle = self
            .controller
            .as_ref()
            .is_some_and(|value| !value.control_busy());
        if control_idle {
            if let Some(action) = self.queued_control.take() {
                self.submit(action);
            }
        }
        if self.import.auto_continue
            && self.import.status.as_ref().is_some_and(|status| {
                self.controller
                    .as_ref()
                    .is_none_or(|controller| !controller.can_continue_import(&status.operation_id))
            })
        {
            self.import.auto_continue = false;
        }
        if self.import.auto_continue && !self.transfer_busy() && self.import_source_matches() {
            if let Some(status) = self.import.status.as_ref() {
                if !status.state.is_terminal() {
                    let action = PortableAction::ContinueImport {
                        operation_id: status.operation_id.clone(),
                        begin_request_id: self.import.ids.begin.clone(),
                        seal_request_id: self.import.ids.seal.clone(),
                    };
                    self.import.auto_continue = false;
                    if self.submit(action) {
                        self.import.upload_attempted = true;
                    }
                }
            }
        }
        if !self.control_busy() && Instant::now() >= self.next_poll {
            let import = self
                .import
                .status
                .as_ref()
                .filter(|status| !status.state.is_terminal())
                .map(|status| PortableAction::ImportStatus {
                    operation_id: status.operation_id.clone(),
                });
            let export = self
                .export
                .status
                .as_ref()
                .filter(|status| export_active(status.state))
                .map(|status| PortableAction::ExportStatus {
                    project_id: status.project_id.clone(),
                    operation_id: status.operation_id.clone(),
                });
            let action = if self.poll_import_first {
                import.or(export)
            } else {
                export.or(import)
            };
            self.poll_import_first = !self.poll_import_first;
            self.next_poll = Instant::now() + Duration::from_millis(750);
            if let Some(action) = action {
                self.submit(action);
            }
        }
        let active = self
            .controller
            .as_ref()
            .is_some_and(|value| value.control_busy() || value.transfer_busy())
            || self.queued_control.is_some()
            || self.import.auto_continue
            || self
                .import
                .status
                .as_ref()
                .is_some_and(|status| !status.state.is_terminal())
            || self
                .export
                .status
                .as_ref()
                .is_some_and(|status| export_active(status.state));
        if active {
            self.ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn import_source_matches(&self) -> bool {
        self.import.declaration.as_ref().is_some_and(|declaration| {
            self.import
                .status
                .as_ref()
                .is_none_or(|status| &status.package == declaration)
        })
    }
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        current: Option<(ProjectId, RevisionId)>,
        can_open: bool,
    ) -> Option<ProjectId> {
        self.poll();
        self.consent.bind(current.clone());
        ui.heading("Portable projects");
        ui.label(egui::RichText::new(PACKAGE_PRIVACY_WARNING).strong());
        ui.small("Packages are separate from generation jobs. Nothing here connects or actuates a device.");
        if let Some(error) = &self.init_error {
            ui.label(format!("Portable workers unavailable: {error}"));
            if ui.button("Retry worker initialization").clicked() {
                let wake = self.ctx.clone();
                match PortableController::with_wake(
                    self.endpoint.clone(),
                    self.bootstrap.clone(),
                    Arc::new(move || wake.request_repaint()),
                ) {
                    Ok(controller) => {
                        self.controller = Some(controller);
                        self.init_error = None;
                    }
                    Err(error) => {
                        self.init_error = Some(format!("{error:#}"));
                    }
                }
            }
            return None;
        }
        if let Some(session) = self
            .controller
            .as_ref()
            .and_then(PortableController::claimed_session)
        {
            ui.small(format!(
                "Client session {session}. Session identity is not proof of a project permission."
            ));
        }
        let mut open = None;
        egui::ScrollArea::vertical().id_salt("portable-project-panel").show(ui, |ui| {
            self.show_progress(ui);
            self.show_publication_recovery(ui);
            ui.separator();
            let idle = !self.control_busy() && !self.transfer_busy();
            if let Some(action) = self.neutral_bundle.show(ui, current.as_ref(), idle) { self.submit(action); }
            ui.separator();
            self.show_motion_evidence(ui,current.as_ref());
            ui.separator();
            self.show_export(ui, current.as_ref());
            ui.add_space(12.0);
            ui.separator();
            open = self.show_import(ui, can_open);
            if let Some(message) = &self.message {
                ui.separator();
                ui.label(message);
            }
            if let Some(failure) = &self.failure {
                ui.separator();
                ui.label(egui::RichText::new("Action did not complete normally").strong());
                ui.label(&failure.message);
                if let Some(publication) = &failure.publication {
                    ui.label(format!("Destination outcome: {:?}", publication.outcome));
                    copy_row(ui, "Destination", &publication.destination.display().to_string());
                    if let Some(path) = &publication.recovery_staging_path {
                        copy_row(ui, "Retained recovery file", &path.display().to_string());
                    }
                    ui.label("Do not overwrite or delete a possible published file. The engine copy is not automatically released.");
                }
                if ui.button("Dismiss message (does not change engine state)").clicked() {
                    self.failure = None;
                }
            }
        });
        open
    }
    fn show_publication_recovery(&mut self, ui: &mut egui::Ui) {
        let Some(record) = self
            .controller
            .as_ref()
            .and_then(PortableController::publication_recovery)
            .cloned()
        else {
            return;
        };
        ui.separator();
        ui.label(
            egui::RichText::new("Publication recovery requires explicit disposition").strong(),
        );
        ui.label(&record.message);
        ui.label(format!(
            "Destination outcome: {:?}",
            record.publication.outcome
        ));
        copy_row(ui, "Publication project", record.project_id.as_str());
        copy_row(ui, "Publication operation", record.operation_id.as_str());
        copy_row(ui, "Publication Begin request", record.request_id.as_str());
        copy_row(
            ui,
            "Destination",
            &record.publication.destination.display().to_string(),
        );
        if let Some(path) = &record.publication.recovery_staging_path {
            copy_row(ui, "Retained recovery file", &path.display().to_string());
        }
        ui.label("Do not overwrite or delete a possible published file. Further local publication is blocked until this outcome is handled. Acknowledgement changes no files or engine state.");
        ui.checkbox(
            &mut self.publication_disposition,
            "I have recorded and handled this publication outcome",
        );
        if ui
            .add_enabled(
                self.publication_disposition,
                egui::Button::new("Acknowledge publication recovery disposition"),
            )
            .clicked()
        {
            if let Some(controller) = &mut self.controller {
                match controller.acknowledge_publication_recovery(record.ticket) {
                    Ok(()) => self.publication_disposition = false,
                    Err(error) => self.failure = Some(PortableFailure::from_error(error)),
                }
            }
        }
    }
    fn show_progress(&mut self, ui: &mut egui::Ui) {
        let Some(controller) = self.controller.as_ref() else {
            return;
        };
        match controller.progress() {
            PortableProgress::Idle => {}
            PortableProgress::NeutralBundle => {
                ui.spinner();
                ui.label("Preparing and verifying selected-axis bundle…");
            }
            PortableProgress::Import {
                phase,
                hashed_bytes,
                accepted_bytes,
                verified_bytes,
                total_bytes,
            } => {
                ui.label(format!("Local import helper: {phase:?}"));
                meter(ui, "Source hashed", hashed_bytes, Some(total_bytes));
                meter(
                    ui,
                    "Upload acknowledged (not restart durability)",
                    accepted_bytes,
                    Some(total_bytes),
                );
                meter(ui, "Engine verification", verified_bytes, Some(total_bytes));
            }
            PortableProgress::Download {
                received_bytes,
                verification_bytes,
                total_bytes,
            } => {
                meter(
                    ui,
                    "Engine restart revalidation",
                    verification_bytes,
                    Some(total_bytes),
                );
                meter(
                    ui,
                    "Download to private staging",
                    received_bytes,
                    Some(total_bytes),
                );
            }
        }
        if controller.transfer_busy() && ui.button("Stop local transfer only").clicked() {
            controller.stop_local_transfer();
            self.import.auto_continue = false;
            self.message = Some("Local stop requested. Worker ownership remains until it drains; engine operations are not cancelled or released.".into());
        }
        if self.queued_control.is_some() {
            ui.label("Explicit engine control action queued; waiting for its actual outcome.");
        }
    }
    fn show_motion_evidence(
        &mut self,
        ui: &mut egui::Ui,
        current: Option<&(ProjectId, RevisionId)>,
    ) {
        self.motion_evidence.bind(current.cloned());
        ui.heading("Export motion with project evidence");
        ui.label("Publish the selected neutral axis, full unencrypted project and checked association in one new directory.");
        ui.small("Requires existing packaging permission; allow it below only when needed. Original media, prompts and private history may be included. Integrity checks do not qualify playback or learned output.");
        if let Some((project, revision)) = current {
            ui.label(format!("Project {project}, revision {revision:?}"));
        }
        ui.label("New output directory");
        if ui
            .text_edit_singleline(&mut self.motion_evidence.destination)
            .changed()
        {
            self.motion_evidence.acknowledged = false;
        }
        let before = self.motion_evidence.axis;
        egui::ComboBox::from_id_salt("motion-evidence-axis")
            .selected_text(before.as_str())
            .show_ui(ui, |ui| {
                for axis in Axis::ALL {
                    ui.selectable_value(&mut self.motion_evidence.axis, axis, axis.as_str());
                }
            });
        if before != self.motion_evidence.axis {
            self.motion_evidence.acknowledged = false;
        }
        ui.label("Retained staging directory for explicit recovery");
        if ui
            .text_edit_singleline(&mut self.motion_evidence.staging)
            .changed()
        {
            self.motion_evidence.acknowledged = false;
        }
        ui.checkbox(
            &mut self.motion_evidence.acknowledged,
            "I acknowledge the unencrypted private contents for this export or retained staging",
        );
        let idle = !self.control_busy() && !self.transfer_busy();
        if ui
            .add_enabled(
                idle && self.motion_evidence.acknowledged
                    && current.is_some()
                    && !self.motion_evidence.destination.trim().is_empty(),
                egui::Button::new("Export motion and full evidence"),
            )
            .clicked()
        {
            if let Some((project, revision)) = current {
                self.submit(PortableAction::ExportMotionEvidence {
                    project_id: project.clone(),
                    revision: *revision,
                    axis: self.motion_evidence.axis,
                    destination: PathBuf::from(&self.motion_evidence.destination),
                    consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
                });
                self.motion_evidence.acknowledged = false;
            }
        }
        if ui
            .add_enabled(
                idle && self.motion_evidence.acknowledged
                    && !self.motion_evidence.staging.trim().is_empty(),
                egui::Button::new("Resume retained paired export"),
            )
            .clicked()
        {
            self.submit(PortableAction::ResumeMotionEvidence {
                staging: PathBuf::from(&self.motion_evidence.staging),
                consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
            });
            self.motion_evidence.acknowledged = false;
        }
        ui.small("Stop local transfer retains recovery. A previously begun download requires lease reconciliation; it is never blindly retried. Progress counters cover package transfer, not the final association scan.");
        if let Some(receipt) = &self.motion_evidence.published {
            ui.label(format!(
                "Published project {}",
                receipt.association.origin_project_id
            ));
            ui.label(format!("Published {} at revision {:?}; selected axis {}. Full file/directory sync confirmed.",receipt.destination.display(),receipt.association.revision,receipt.association.axis.as_str()));
        }
    }
    fn show_export(&mut self, ui: &mut egui::Ui, current: Option<&(ProjectId, RevisionId)>) {
        ui.heading("Export a complete project");
        let idle = !self.control_busy() && !self.transfer_busy();
        let target = if self.export.attempted {
            self.export.target.as_ref()
        } else {
            current
        };
        if let Some((project, revision)) = target {
            ui.label(format!("Pinned target: {project}, revision {revision:?}"));
        } else {
            ui.label("Open a project to start an export. Recovery below can inspect an existing operation.");
        }
        copy_row(ui, "Start request", self.export.request.as_str());
        if let Some(inspected) = &self.export.inspected {
            ui.label("Read-only recovered export; unresolved Start remains selected");
            copy_row(ui, "Inspected project", inspected.project_id.as_str());
            copy_row(ui, "Inspected operation", inspected.operation_id.as_str());
            copy_row(ui, "Inspected Start request", inspected.request_id.as_str());
            ui.label(format!(
                "Inspected revision {:?}, state {:?}",
                inspected.requested_revision, inspected.state
            ));
            ui.small("Replay the original Start to resolve its outcome before switching export workflows.");
        }
        if let Some(previous) = &self.export.previous {
            ui.label("Prior export recovery context");
            if let Some((project, revision)) = &previous.target {
                ui.label(format!("Previous target: {project}, revision {revision:?}"));
            }
            copy_row(ui, "Previous Start request", previous.request.as_str());
            copy_row(
                ui,
                "Previous download Begin request",
                previous.download_request.as_str(),
            );
            if let Some(status) = &previous.status {
                copy_row(ui, "Previous operation", status.operation_id.as_str());
                ui.label(format!("Previous export state: {:?}", status.state));
            }
            if !previous.recovery_project.is_empty() {
                copy_row(ui, "Previous recovery project", &previous.recovery_project);
            }
            if !previous.recovery_operation.is_empty() {
                copy_row(
                    ui,
                    "Previous recovery operation",
                    &previous.recovery_operation,
                );
            }
            if !previous.destination.is_empty() {
                copy_row(ui, "Previous destination", &previous.destination);
            }
            if let Some(receipt) = &previous.published {
                copy_row(
                    ui,
                    "Previous published destination",
                    &receipt.destination.display().to_string(),
                );
                copy_row(
                    ui,
                    "Previous published operation",
                    receipt.operation_id.as_str(),
                );
            }
            ui.checkbox(
                &mut self.export.previous_disposition,
                "I have recorded the prior export recovery context",
            );
            if ui
                .add_enabled(
                    self.export.previous_disposition,
                    egui::Button::new("Acknowledge prior export recovery context"),
                )
                .clicked()
            {
                self.export.previous = None;
                self.export.previous_disposition = false;
            }
        }
        ui.checkbox(&mut self.consent.export, "I acknowledge the unencrypted private contents for this displayed project and revision");
        let label = if self.export.attempted && self.export.status.is_none() {
            "Replay exact export Start"
        } else {
            "Start export"
        };
        let may_start =
            target.is_some() && self.consent.export && idle && self.export.status.is_none();
        if ui
            .add_enabled(may_start, egui::Button::new(label))
            .clicked()
        {
            let (project_id, revision) = target.cloned().expect("enabled target");
            self.export.target = Some((project_id.clone(), revision));
            let action = PortableAction::StartExport {
                project_id,
                revision,
                request_id: self.export.request.clone(),
                consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
            };
            if self.submit(action) {
                self.export.attempted = true;
                self.consent.export = false;
            }
        }
        if let Some(status) = self.export.status.clone() {
            ui.label(format!("Engine export: {:?}", status.state));
            copy_row(ui, "Export operation", status.operation_id.as_str());
            if let Some(capture) = &status.capture {
                ui.label(format!(
                    "Captured revision {:?}, event cursor {}",
                    capture.captured_revision, capture.captured_event_cursor
                ));
                copy_row(ui, "Manifest SHA-256", &capture.manifest_sha256);
            }
            meter(
                ui,
                "Engine package bytes",
                status.progress.completed_bytes,
                status.progress.total_bytes,
            );
            ui.label(format!(
                "Objects verified: {} / {}",
                status.progress.completed_objects,
                status
                    .progress
                    .total_objects
                    .map_or_else(|| "unknown".into(), |value| value.to_string())
            ));
            if let Some(error) = &status.error {
                ui.label(format!("{:?}: {}", error.code, error.message));
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.control_busy(),
                        egui::Button::new("Refresh export status"),
                    )
                    .clicked()
                {
                    self.submit(PortableAction::ExportStatus {
                        project_id: status.project_id.clone(),
                        operation_id: status.operation_id.clone(),
                    });
                }
                if ui
                    .add_enabled(
                        export_active(status.state) && self.queued_control.is_none(),
                        egui::Button::new("Cancel engine export"),
                    )
                    .clicked()
                {
                    self.queue_control(PortableAction::CancelExport {
                        project_id: status.project_id.clone(),
                        operation_id: status.operation_id.clone(),
                    });
                }
            });
            if status.state == PackageExportOperationState::Ready {
                if let Some(artifact) = &status.artifact {
                    ui.label(format!(
                        "Verified engine package: {} bytes; {} declared objects",
                        artifact.byte_len, artifact.object_count
                    ));
                    copy_row(ui, "Package SHA-256", &artifact.sha256);
                }
                ui.label("New local destination (must not already exist)");
                ui.add_enabled(
                    idle,
                    egui::TextEdit::singleline(&mut self.export.destination)
                        .char_limit(4096)
                        .hint_text("/absolute/path/new-project.package"),
                );
                copy_row(
                    ui,
                    "Download Begin request",
                    self.export.download_request.as_str(),
                );
                ui.checkbox(
                    &mut self.consent.download,
                    format!(
                        "I acknowledge saving unencrypted private data from project {}",
                        status.project_id
                    ),
                );
                if ui
                    .add_enabled(
                        idle && self.consent.download
                            && !self.export.destination.trim().is_empty()
                            && self.controller.as_ref().is_some_and(|controller| {
                                controller.publication_recovery().is_none()
                            }),
                        egui::Button::new("Download to new file"),
                    )
                    .clicked()
                {
                    let action = PortableAction::DownloadExport {
                        project_id: status.project_id.clone(),
                        operation_id: status.operation_id.clone(),
                        destination: PathBuf::from(self.export.destination.trim()),
                        request_id: self.export.download_request.clone(),
                        consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
                    };
                    if self.submit(action) {
                        self.consent.download = false;
                    }
                }
                ui.small("The exact Begin ID recovers a lost lease acknowledgement. After a partial transfer was abandoned, explicitly create a fresh attempt; partial local files are not silently resumed.");
                if ui
                    .add_enabled(
                        idle,
                        egui::Button::new("Prepare fresh full-download attempt"),
                    )
                    .clicked()
                {
                    self.export.download_request = new_request_id();
                    self.consent.download = false;
                }
                ui.checkbox(&mut self.export.released_intentionally,
                    "I have a durable verified copy, or intentionally want to discard the engine package");
                if ui
                    .add_enabled(
                        idle && self.export.released_intentionally,
                        egui::Button::new("Release engine package"),
                    )
                    .clicked()
                {
                    self.submit(PortableAction::ReleaseExport {
                        project_id: status.project_id.clone(),
                        operation_id: status.operation_id.clone(),
                    });
                    self.export.released_intentionally = false;
                }
            }
        }
        let may_prepare = self
            .export
            .status
            .as_ref()
            .is_some_and(|status| !export_active(status.state))
            || (self.export.attempted
                && self.export.status.is_none()
                && self.export.start_rejected
                && !self.export.start_ambiguous);
        if may_prepare
            && ui
                .add_enabled(
                    idle && self.export.previous.is_none(),
                    egui::Button::new("Prepare another export request"),
                )
                .clicked()
        {
            let previous = std::mem::take(&mut self.export);
            self.export.target = current.cloned();
            self.export.previous = Some(Box::new(previous));
            self.consent.export = false;
            self.consent.download = false;
            self.consent.allow = false;
            self.message = Some("Fresh export draft prepared for the current project and revision. Prior recovery context is retained until explicitly acknowledged.".into());
        }
        if let Some(receipt) = &self.export.published {
            ui.label(egui::RichText::new("Local file published").strong());
            copy_row(
                ui,
                "Destination",
                &receipt.destination.display().to_string(),
            );
            copy_row(ui, "Operation for this file", receipt.operation_id.as_str());
            ui.small(receipt.verification);
            ui.small(format!("File/directory synced: {}; lease cleanup confirmed: {}; engine release requested: {}",
                receipt.file_and_directory_synced, receipt.lease_cleanup_confirmed, receipt.engine_release_requested));
        }
        ui.collapsing("Recover an export by its scoped IDs", |ui| {
            ui.add_enabled(
                idle,
                egui::TextEdit::singleline(&mut self.export.recovery_project)
                    .char_limit(256)
                    .hint_text("Project ID"),
            );
            ui.add_enabled(
                idle,
                egui::TextEdit::singleline(&mut self.export.recovery_operation)
                    .char_limit(256)
                    .hint_text("Export operation ID"),
            );
            if ui
                .add_enabled(idle, egui::Button::new("Query existing export"))
                .clicked()
            {
                let action = (|| -> Result<_> {
                    Ok(PortableAction::ExportStatus {
                        project_id: ProjectId::new(self.export.recovery_project.trim().to_owned())?,
                        operation_id: PackageOperationId::new(
                            self.export.recovery_operation.trim().to_owned(),
                        )?,
                    })
                })();
                match action {
                    Ok(action) => {
                        self.consent.download = false;
                        self.submit(action);
                    }
                    Err(error) => self.failure = Some(PortableFailure::from_error(error)),
                }
            }
        });
        ui.collapsing("Explicit permission for an existing project", |ui| {
            ui.label("This reads the separate OS-owner approval capability only when you click Allow. ManageGrants is still required. It grants PackageProject to this client's session, not another actor.");
            if let Some((project_id, _)) = current {
                copy_row(ui, "Approval project", project_id.as_str());
                ui.checkbox(&mut self.consent.allow, "I explicitly approve packaging private data for this project");
                if ui.add_enabled(!self.control_busy() && self.consent.allow, egui::Button::new("Allow packaging for this session")).clicked() {
                    self.submit(PortableAction::AllowPackaging {
                        project_id: project_id.clone(), consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
                    });
                    self.consent.allow = false;
                }
            } else { ui.label("Open the exact project before approving its packaging capability."); }
        });
    }
    fn show_import(&mut self, ui: &mut egui::Ui, can_open: bool) -> Option<ProjectId> {
        ui.heading("Import as a new project");
        ui.label(PACKAGE_IMPORT_NOTICE);
        ui.small("The selected regular file is read through a retained handle. No archived path, job or device is executed.");
        let idle = !self.transfer_busy() && !self.control_busy();
        if let Some(retained) = self.pinned_import.clone() {
            ui.label(egui::RichText::new("Retained import Start recovery context").strong());
            copy_row(ui, "Pinned Start request", retained.ids.start.as_str());
            copy_row(
                ui,
                "Pinned upload Begin request",
                retained.ids.begin.as_str(),
            );
            copy_row(ui, "Pinned Seal request", retained.ids.seal.as_str());
            copy_row(ui, "Pinned source", &retained.source);
            if let Some(declaration) = &retained.declaration {
                copy_row(ui, "Pinned source SHA-256", &declaration.sha256);
            }
            if ui
                .add_enabled(
                    idle,
                    egui::Button::new("Restore pinned import Start context"),
                )
                .clicked()
            {
                self.import = retained;
                self.import.auto_continue = false;
            }
        }
        let source_pinned = self
            .controller
            .as_ref()
            .is_some_and(PortableController::import_source_pinned);
        if source_pinned {
            ui.label("The submitted Start pins this source and declaration. Replay the exact Start or recover its terminal outcome before selecting another source.");
        }
        ui.label("Source package (regular file, not a symlink)");
        let changed = ui
            .add_enabled(
                idle && !source_pinned,
                egui::TextEdit::singleline(&mut self.import.source)
                    .char_limit(4096)
                    .hint_text("/absolute/path/project.package"),
            )
            .changed();
        if changed {
            self.import.declaration = None;
            self.import.notice = false;
        }
        if ui
            .add_enabled(
                idle && !source_pinned && !self.import.source.trim().is_empty(),
                egui::Button::new("Prepare and hash selected source"),
            )
            .clicked()
        {
            self.import.declaration = None;
            self.import.auto_continue = false;
            self.submit(PortableAction::PrepareImport {
                source: PathBuf::from(self.import.source.trim()),
            });
        }
        if let Some(declaration) = &self.import.declaration {
            ui.label(format!(
                "Prepared format {}, {} bytes",
                declaration.format_version, declaration.byte_len
            ));
            copy_row(ui, "Source SHA-256", &declaration.sha256);
            if !self.import_source_matches() {
                ui.label("This source does not match the recovered operation. Choose the exact declared package; no upload is enabled.");
            }
        }
        copy_row(ui, "Import Start request", self.import.ids.start.as_str());
        copy_row(ui, "Upload Begin request", self.import.ids.begin.as_str());
        copy_row(ui, "Seal request", self.import.ids.seal.as_str());
        if let Some(previous) = &self.import.previous_attempt {
            ui.small(format!("Prior attempt: {previous}"));
        }
        ui.checkbox(
            &mut self.import.notice,
            "I understand this creates a fresh project with inert archived provenance",
        );
        let start_label = if self.import.start_attempted {
            "Replay exact import Start"
        } else {
            "Import prepared package"
        };
        if ui
            .add_enabled(
                idle && self.import.notice
                    && self.import.declaration.is_some()
                    && self.import.status.is_none(),
                egui::Button::new(start_label),
            )
            .clicked()
            && self.submit(PortableAction::StartImport {
                request_id: self.import.ids.start.clone(),
            })
        {
            self.import.start_attempted = true;
            self.import.auto_continue = true;
        }
        let mut open = None;
        if let Some(status) = self.import.status.clone() {
            ui.label(format!("Engine import: {:?}", status.state));
            copy_row(ui, "Import operation", status.operation_id.as_str());
            meter(
                ui,
                "Engine received bytes",
                status.progress.received_bytes,
                Some(status.progress.total_bytes),
            );
            meter(
                ui,
                "Engine verified bytes",
                status.progress.verified_bytes,
                Some(status.progress.total_bytes),
            );
            if let Some(error) = &status.error {
                ui.label(format!("{:?}: {}", error.code, error.message));
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.control_busy(),
                        egui::Button::new("Refresh import status"),
                    )
                    .clicked()
                {
                    self.submit(PortableAction::ImportStatus {
                        operation_id: status.operation_id.clone(),
                    });
                }
                if ui
                    .add_enabled(
                        !status.state.is_terminal() && self.queued_control.is_none(),
                        egui::Button::new("Cancel engine import"),
                    )
                    .clicked()
                {
                    if let Some(controller) = &self.controller {
                        if controller.transfer.pending.is_some_and(|pending| {
                            matches!(
                                pending.kind,
                                PortableActionKind::PrepareImport
                                    | PortableActionKind::StartImport
                                    | PortableActionKind::ContinueImport
                            )
                        }) {
                            controller.stop_local_transfer();
                        }
                    }
                    self.import.auto_continue = false;
                    self.queue_control(PortableAction::CancelImport {
                        operation_id: status.operation_id.clone(),
                    });
                }
            });
            if !status.state.is_terminal() {
                if status.state == PackageImportOperationState::AwaitingUpload
                    && self.import.upload_attempted
                {
                    ui.label("The prior upload attempt ended. Prepare new Begin/Seal IDs before a deliberate fresh upload attempt.");
                    if ui
                        .add_enabled(idle, egui::Button::new("Prepare fresh upload attempt"))
                        .clicked()
                    {
                        self.import.previous_attempt = Some(format!(
                            "Begin {}, Seal {}",
                            self.import.ids.begin, self.import.ids.seal
                        ));
                        self.import.ids.begin = new_request_id();
                        self.import.ids.seal = new_request_id();
                        self.import.upload_attempted = false;
                    }
                }
                let ready_attempt = status.state != PackageImportOperationState::AwaitingUpload
                    || !self.import.upload_attempted;
                if ui
                    .add_enabled(
                        idle && self.import.notice
                            && self.import_source_matches()
                            && ready_attempt
                            && self.controller.as_ref().is_some_and(|controller| {
                                controller.can_continue_import(&status.operation_id)
                            }),
                        egui::Button::new("Continue existing import (not a new clone)"),
                    )
                    .clicked()
                    && self.submit(PortableAction::ContinueImport {
                        operation_id: status.operation_id.clone(),
                        begin_request_id: self.import.ids.begin.clone(),
                        seal_request_id: self.import.ids.seal.clone(),
                    })
                {
                    self.import.upload_attempted = true;
                }
            } else if status.state == PackageImportOperationState::Completed {
                if let Some(receipt) = &status.receipt {
                    ui.label(egui::RichText::new("Fresh project published by the engine").strong());
                    copy_row(ui, "New project", receipt.project_id.as_str());
                    copy_row(ui, "Origin map SHA-256", &receipt.origin_map.sha256);
                    let clicked = ui
                        .add_enabled(can_open, egui::Button::new("Open imported clone"))
                        .clicked();
                    open = explicit_open(clicked, can_open, Some(&receipt.project_id));
                    if !can_open {
                        ui.small("Wait for the current project command to finish before opening this clone.");
                    }
                }
            } else {
                ui.label("This operation is terminal. An engine restart does not resume its acknowledged upload prefix. A new clone requires a deliberate new Start.");
            }
            if idle
                && status.state.is_terminal()
                && ui.button("Prepare another import request").clicked()
            {
                self.message = Some(format!(
                    "Previous import operation {} remains recoverable by ID",
                    status.operation_id
                ));
                self.import = ImportView::default();
            }
        }
        ui.collapsing("Recover an import without creating another project", |ui| {
            ui.add_enabled(idle, egui::TextEdit::singleline(&mut self.import.recovery_operation).char_limit(256).hint_text("Import operation ID"));
            if ui.add_enabled(idle, egui::Button::new("Query existing import")).clicked() {
                match PackageImportOperationId::new(self.import.recovery_operation.trim().to_owned()) {
                    Ok(operation_id) => {
                        self.import.auto_continue = false;
                        self.submit(PortableAction::ImportStatus { operation_id });
                    }
                    Err(error) => self.failure = Some(PortableFailure::from_error(error.into())),
                }
            }
            ui.small("Same-session status recovery needs no source file. To continue an unfinished same-epoch upload, prepare the exact package again. Retain the displayed request IDs after ambiguous responses.");
        });
        open
    }
}
fn same_import_terminal_binding(prior: &PackageImportStatus, status: &PackageImportStatus) -> bool {
    let same_receipt = match (&prior.receipt, &status.receipt) {
        (None, None) => true,
        (Some(prior), Some(receipt)) => {
            prior.project_id == receipt.project_id
                && prior.revision == receipt.revision
                && prior.capture == receipt.capture
                && prior.package == receipt.package
                && prior.origin_map.artifact_id == receipt.origin_map.artifact_id
                && prior.origin_map.sha256 == receipt.origin_map.sha256
                && prior.origin_map.byte_len == receipt.origin_map.byte_len
        }
        _ => false,
    };
    prior.operation_id == status.operation_id
        && prior.request_id == status.request_id
        && prior.state == status.state
        && prior.package == status.package
        && prior.capture == status.capture
        && prior.error == status.error
        && same_receipt
}
fn export_active(state: PackageExportOperationState) -> bool {
    matches!(
        state,
        PackageExportOperationState::Queued
            | PackageExportOperationState::Capturing
            | PackageExportOperationState::Streaming
    )
}
fn explicit_open(clicked: bool, can_open: bool, project: Option<&ProjectId>) -> Option<ProjectId> {
    if clicked && can_open {
        project.cloned()
    } else {
        None
    }
}
fn copy_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        ui.monospace(value);
        if ui.small_button("Copy").clicked() {
            ui.ctx().copy_text(value.to_owned());
        }
    });
}
fn meter(ui: &mut egui::Ui, label: &str, completed: u64, total: Option<u64>) {
    if let Some(total) = total.filter(|value| *value > 0) {
        let fraction = ((completed as f64 / total as f64).clamp(0.0, 1.0)) as f32;
        ui.add(
            egui::ProgressBar::new(fraction).text(format!("{label}: {completed} / {total} bytes")),
        );
    } else {
        ui.label(format!(
            "{label}: {completed} bytes; total not yet available"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(value: &str) -> ProjectId {
        ProjectId::new(value.to_owned()).unwrap()
    }
    fn operation(value: &str) -> PackageImportOperationId {
        PackageImportOperationId::new(value.to_owned()).unwrap()
    }
    fn test_lane() -> (Lane, Receiver<Work>, SyncSender<PortableEvent>) {
        let (commands, input) = mpsc::sync_channel(1);
        let (output, events) = mpsc::sync_channel(1);
        (
            Lane {
                commands,
                events,
                pending: None,
            },
            input,
            output,
        )
    }
    fn event(ticket: u64) -> PortableEvent {
        PortableEvent {
            ticket: PortableTicket(ticket),
            kind: PortableActionKind::ImportStatus,
            result: Err(PortableFailure::from_error(anyhow!("fixture result"))),
        }
    }

    #[test]
    fn consent_is_not_reused_across_project_or_revision() {
        let mut consent = ConsentState::default();
        consent.bind(Some((project("first"), RevisionId::new(1))));
        consent.export = true;
        consent.allow = true;
        consent.download = true;
        consent.bind(Some((project("first"), RevisionId::new(1))));
        assert!(consent.export && consent.allow && consent.download);
        consent.bind(Some((project("first"), RevisionId::new(2))));
        assert!(!consent.export && !consent.allow && !consent.download);
        consent.export = true;
        consent.allow = true;
        consent.download = true;
        consent.bind(Some((project("second"), RevisionId::new(2))));
        assert!(!consent.export && !consent.allow && !consent.download);
    }

    #[test]
    fn terminal_completion_does_not_itself_open_a_clone() {
        let clone = project("fresh-clone");
        assert_eq!(explicit_open(false, true, Some(&clone)), None);
        assert_eq!(explicit_open(true, false, Some(&clone)), None);
        assert_eq!(explicit_open(true, true, Some(&clone)), Some(clone));
        assert_eq!(explicit_open(true, true, None), None);
    }

    #[test]
    fn failed_second_submission_preserves_first_request_and_lane_ownership() {
        let (mut lane, input, _output) = test_lane();
        let request_id = new_request_id();
        lane.submit(Work {
            ticket: PortableTicket(1),
            action: PortableAction::StartImport {
                request_id: request_id.clone(),
            },
            local: LocalControl::default(),
        })
        .unwrap();
        assert!(lane
            .submit(Work {
                ticket: PortableTicket(2),
                action: PortableAction::StartImport {
                    request_id: new_request_id()
                },
                local: LocalControl::default(),
            })
            .is_err());
        assert_eq!(lane.pending.unwrap().ticket, PortableTicket(1));
        match input.try_recv().unwrap().action {
            PortableAction::StartImport {
                request_id: received,
            } => assert_eq!(received, request_id),
            _ => panic!("action changed"),
        }
        assert!(matches!(input.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn bounded_terminal_mailbox_preserves_result_until_consumed() {
        let (mut lane, _input, output) = test_lane();
        lane.pending = Some(Pending {
            ticket: PortableTicket(1),
            kind: PortableActionKind::ImportStatus,
        });
        output.try_send(event(1)).unwrap();
        assert!(matches!(
            output.try_send(event(2)),
            Err(mpsc::TrySendError::Full(_))
        ));
        assert_eq!(lane.pending.unwrap().ticket, PortableTicket(1));
        assert_eq!(lane.poll().unwrap().ticket, PortableTicket(1));
        assert!(lane.pending.is_none());
    }

    #[test]
    fn stale_completion_never_clears_newer_pending_action() {
        let (mut lane, _input, output) = test_lane();
        lane.pending = Some(Pending {
            ticket: PortableTicket(2),
            kind: PortableActionKind::ImportStatus,
        });
        output.send(event(1)).unwrap();
        assert!(lane.poll().is_none());
        assert_eq!(lane.pending.unwrap().ticket, PortableTicket(2));
        output.send(event(2)).unwrap();
        assert_eq!(lane.poll().unwrap().ticket, PortableTicket(2));
        assert!(lane.pending.is_none());
    }

    #[test]
    fn local_stop_does_not_free_transfer_lane_or_block_control_lane() {
        let (control, control_input, _control_output) = test_lane();
        let (transfer, transfer_input, _transfer_output) = test_lane();
        let mut controller = PortableController {
            control,
            transfer,
            next_ticket: 1,
            local: LocalControl::default(),
            session: Arc::new(Mutex::new(None)),
            recovery: ControllerRecovery::default(),
        };
        controller
            .submit(PortableAction::PrepareImport {
                source: PathBuf::from("not-opened-by-this-state-test"),
            })
            .unwrap();
        controller.stop_local_transfer();
        assert!(controller.local.check().is_err());
        assert!(controller.transfer_busy());
        controller
            .submit(PortableAction::CancelImport {
                operation_id: operation("existing-operation"),
            })
            .unwrap();
        assert!(matches!(
            control_input.try_recv().unwrap().action,
            PortableAction::CancelImport { .. }
        ));
        assert!(matches!(
            transfer_input.try_recv().unwrap().action,
            PortableAction::PrepareImport { .. }
        ));
        assert!(controller.transfer_busy());
    }

    #[test]
    fn worker_principal_change_is_rejected_without_rebinding() {
        let pin = Arc::new(Mutex::new(None));
        let first = SessionId::new("first-session").unwrap();
        let second = SessionId::new("second-session").unwrap();
        pin_session(&pin, &first).unwrap();
        pin_session(&pin, &first).unwrap();
        assert!(pin_session(&pin, &second).is_err());
        assert_eq!(pin.lock().unwrap().as_ref(), Some(&first));
    }

    #[test]
    fn first_or_second_worker_spawn_failure_prevents_controller_initialization() {
        for index in [0, 1] {
            FAIL_SPAWN_AFTER.with(|flag| flag.set(Some(index)));
            let result = PortableController::new(
                PathBuf::from("unused-endpoint"),
                PathBuf::from("unused-bootstrap"),
            );
            match result {
                Err(error) => assert!(format!("{error:#}").contains("portable worker")),
                Ok(_) => panic!("controller initialized with a missing required worker"),
            }
        }
    }

    #[test]
    fn publication_uncertainty_retains_exact_recovery_path() {
        let destination = PathBuf::from("/selected/new-package");
        let recovery = PathBuf::from("/selected/private-staging");
        let error = PackagePublicationError {
            destination: destination.clone(),
            outcome: PackagePublicationOutcome::OutcomeUnknown,
            recovery_staging_path: Some(recovery.clone()),
            detail: "injected publication uncertainty".into(),
        };
        let failure =
            PortableFailure::from_error(anyhow::Error::new(error).context("download context"));
        let publication = failure.publication.unwrap();
        assert_eq!(publication.destination, destination);
        assert_eq!(
            publication.outcome,
            PackagePublicationOutcome::OutcomeUnknown
        );
        assert_eq!(publication.recovery_staging_path, Some(recovery));
    }

    #[test]
    fn start_begin_and_seal_identities_exist_before_submission() {
        let ids = ImportIds::default();
        assert_ne!(ids.start, ids.begin);
        assert_ne!(ids.start, ids.seal);
        assert_ne!(ids.begin, ids.seal);
        let start = ids.start.clone();
        let begin = ids.begin.clone();
        let seal = ids.seal.clone();
        let action = PortableAction::ContinueImport {
            operation_id: operation("existing-operation"),
            begin_request_id: begin.clone(),
            seal_request_id: seal.clone(),
        };
        match action {
            PortableAction::ContinueImport {
                begin_request_id,
                seal_request_id,
                ..
            } => {
                assert_eq!(begin_request_id, begin);
                assert_eq!(seal_request_id, seal);
            }
            _ => unreachable!(),
        }
        assert_eq!(ids.start, start);
    }

    fn p1d_panel() -> (
        PortablePanel,
        Receiver<Work>,
        SyncSender<PortableEvent>,
        Receiver<Work>,
        SyncSender<PortableEvent>,
    ) {
        let (control, control_input, control_output) = test_lane();
        let (transfer, transfer_input, transfer_output) = test_lane();
        let mut panel = PortablePanel::new(
            PathBuf::from("unused-endpoint"),
            PathBuf::from("unused-bootstrap"),
            egui::Context::default(),
        );
        panel.controller = Some(PortableController {
            control,
            transfer,
            next_ticket: 1,
            local: LocalControl::default(),
            session: Arc::new(Mutex::new(None)),
            recovery: ControllerRecovery::default(),
        });
        panel.next_poll = Instant::now() + Duration::from_secs(3600);
        (
            panel,
            control_input,
            control_output,
            transfer_input,
            transfer_output,
        )
    }

    fn p1d_status(id: &str, completed: bool) -> PackageImportStatus {
        let package = PackageUploadDeclaration {
            format_version: 1,
            sha256: "a".repeat(64),
            byte_len: 1024,
        };
        let capture = PackageCaptureBinding {
            project_id: project("archived-original"),
            captured_revision: RevisionId::new(1),
            captured_event_cursor: 1,
            manifest_sha256: "b".repeat(64),
        };
        let status = PackageImportStatus {
            operation_id: operation(id),
            request_id: RequestId::new(format!("start-{id}")).unwrap(),
            package: package.clone(),
            state: if completed {
                PackageImportOperationState::Completed
            } else {
                PackageImportOperationState::Validating
            },
            progress: PackageImportProgress {
                received_bytes: 1024,
                verified_bytes: if completed { 1024 } else { 0 },
                total_bytes: 1024,
                completed_objects: u64::from(completed),
                total_objects: Some(1),
            },
            capture: completed.then(|| capture.clone()),
            receipt: completed.then(|| PackageImportReceipt {
                project_id: project(&format!("clone-{id}")),
                revision: RevisionId::new(1),
                capture,
                package,
                origin_map: ArtifactIdentity {
                    artifact_id: ArtifactId::new("c".repeat(64)).unwrap(),
                    sha256: "c".repeat(64),
                    byte_len: 256,
                },
            }),
            error: None,
        };
        status.validate().unwrap();
        status
    }

    fn p1d_frame(
        panel: &mut PortablePanel,
        current: Option<(ProjectId, RevisionId)>,
        events: Vec<egui::Event>,
        export_only: bool,
    ) -> egui::FullOutput {
        let ctx = panel.ctx.clone();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 4000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if export_only {
                        panel.show_export(ui, current.as_ref());
                    } else {
                        panel.show(ui, current.clone(), true);
                    }
                });
            },
        )
    }

    fn p1d_text_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
        fn find(shape: &egui::Shape, needle: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.text().contains(needle) => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, needle)),
                _ => None,
            }
        }
        output
            .shapes
            .iter()
            .find_map(|shape| find(&shape.shape, needle))
    }

    fn p1d_click(
        panel: &mut PortablePanel,
        current: Option<(ProjectId, RevisionId)>,
        label: &str,
        export_only: bool,
    ) {
        let output = p1d_frame(panel, current.clone(), Vec::new(), export_only);
        let pos = p1d_text_position(&output, label)
            .unwrap_or_else(|| panic!("missing visible action: {label}"));
        p1d_frame(
            panel,
            current,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            export_only,
        );
    }

    fn p1d_export_failure(
        panel: &mut PortablePanel,
        output: &SyncSender<PortableEvent>,
        error: anyhow::Error,
    ) {
        let target = panel.export.target.clone().unwrap();
        assert!(panel.submit(PortableAction::StartExport {
            project_id: target.0,
            revision: target.1,
            request_id: panel.export.request.clone(),
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
        }));
        panel.export.attempted = true;
        let pending = panel.controller.as_ref().unwrap().control.pending.unwrap();
        output
            .send(PortableEvent {
                ticket: pending.ticket,
                kind: pending.kind,
                result: Err(PortableFailure::from_error(error)),
            })
            .unwrap();
        panel.poll();
    }

    #[test]
    fn p1d_rejected_export_offers_deliberate_current_revision_draft() {
        let (mut panel, _input, output, _transfer_input, _transfer_output) = p1d_panel();
        panel.export.target = Some((project("old-project"), RevisionId::new(1)));
        let prior_request = panel.export.request.clone();
        p1d_export_failure(
            &mut panel,
            &output,
            ProtocolError::new(ErrorCode::Forbidden, "grant denied").into(),
        );
        assert!(panel.export.attempted);
        let current = (project("new-project"), RevisionId::new(2));
        panel.consent.bind(Some(current.clone()));
        panel.consent.export = true;
        panel.consent.download = true;
        panel.consent.allow = true;
        p1d_click(
            &mut panel,
            Some(current.clone()),
            "Prepare another export request",
            true,
        );
        assert!(!panel.export.attempted);
        assert_ne!(panel.export.request, prior_request);
        assert!(!panel.consent.export && !panel.consent.download && !panel.consent.allow);
        let output = p1d_frame(&mut panel, Some(current), Vec::new(), true);
        assert!(p1d_text_position(&output, prior_request.as_str()).is_some());
        assert!(p1d_text_position(&output, "old-project").is_some());
        assert!(p1d_text_position(&output, "new-project").is_some());
    }

    #[test]
    fn p1d_ambiguous_export_replay_rejection_cannot_reset_prior_intent() {
        let (mut panel, input, output, _transfer_input, _transfer_output) = p1d_panel();
        panel.export.target = Some((project("original-project"), RevisionId::new(1)));
        let prior_request = panel.export.request.clone();
        p1d_export_failure(&mut panel, &output, anyhow!("lost Start acknowledgement"));
        input.try_recv().unwrap();
        p1d_export_failure(
            &mut panel,
            &output,
            ProtocolError::new(ErrorCode::Forbidden, "grant revoked").into(),
        );
        let output = p1d_frame(
            &mut panel,
            Some((project("different-project"), RevisionId::new(2))),
            Vec::new(),
            true,
        );
        assert!(p1d_text_position(&output, "Prepare another export request").is_none());
        assert_eq!(panel.export.request, prior_request);
        assert!(panel.export.attempted);
        assert_eq!(
            panel.export.target.as_ref().unwrap().0,
            project("original-project")
        );
    }

    fn p1d_deliver_export_snapshot(
        panel: &mut PortablePanel,
        input: &Receiver<Work>,
        output: &SyncSender<PortableEvent>,
        status: PackageExportStatus,
    ) {
        status.validate().unwrap();
        assert!(panel.submit(PortableAction::ExportStatus {
            project_id: status.project_id.clone(),
            operation_id: status.operation_id.clone(),
        }));
        let work = input.try_recv().unwrap();
        output
            .send(PortableEvent {
                ticket: work.ticket,
                kind: work.action.kind(),
                result: Ok(PortableReply::ExportStatus(status)),
            })
            .unwrap();
        panel.poll();
    }

    fn p1d_failed_export_snapshot() -> PackageExportStatus {
        PackageExportStatus {
            operation_id: PackageOperationId::new("inspected-export").unwrap(),
            project_id: project("inspected-project"),
            request_id: RequestId::new("inspected-start").unwrap(),
            requested_revision: RevisionId::new(9),
            state: PackageExportOperationState::Failed,
            capture: None,
            progress: PackageExportProgress::default(),
            artifact: None,
            error: Some(ProtocolError::invalid("fixture export failed")),
        }
    }

    #[test]
    fn p1d008_other_export_recovery_preserves_exact_replay_until_matching_resolution() {
        let (mut panel, input, output, _transfer_input, _transfer_output) = p1d_panel();
        let original = (project("original-export-project"), RevisionId::new(1));
        panel.export.target = Some(original.clone());
        panel.export.destination = "/selected/original-package".into();
        let request = panel.export.request.clone();
        let download = panel.export.download_request.clone();
        p1d_export_failure(&mut panel, &output, anyhow!("lost Start acknowledgement"));
        input.try_recv().unwrap();

        // Each component of the original binding is required, even if the
        // unrelated operation has already reached a terminal state.
        for mismatch in 0..3 {
            let mut other = p1d_failed_export_snapshot();
            other.request_id = request.clone();
            other.project_id = original.0.clone();
            other.requested_revision = original.1;
            match mismatch {
                0 => other.request_id = RequestId::new("different-start").unwrap(),
                1 => other.project_id = project("different-project"),
                _ => other.requested_revision = RevisionId::new(2),
            }
            let inspected_request = other.request_id.clone();
            p1d_deliver_export_snapshot(&mut panel, &input, &output, other);
            assert_eq!(panel.export.request, request);
            assert_eq!(panel.export.target, Some(original.clone()));
            assert_eq!(panel.export.download_request, download);
            assert_eq!(panel.export.destination, "/selected/original-package");
            assert!(panel.export.status.is_none());
            assert!(panel.export.previous.is_none());
            assert!(panel.export.start_ambiguous);
            assert_eq!(
                panel.export.inspected.as_ref().unwrap().request_id,
                inspected_request
            );
            let frame = p1d_frame(
                &mut panel,
                Some((project("current-project"), RevisionId::new(3))),
                Vec::new(),
                true,
            );
            assert!(p1d_text_position(&frame, "Read-only recovered export").is_some());
            assert!(p1d_text_position(&frame, "Replay exact export Start").is_some());
            assert!(p1d_text_position(&frame, "Prepare another export request").is_none());
        }

        panel.consent.export = true;
        p1d_click(
            &mut panel,
            Some((project("current-project"), RevisionId::new(3))),
            "Replay exact export Start",
            true,
        );
        let replay = input.try_recv().unwrap();
        match &replay.action {
            PortableAction::StartExport {
                project_id,
                revision,
                request_id,
                ..
            } => {
                assert_eq!(project_id, &original.0);
                assert_eq!(*revision, original.1);
                assert_eq!(request_id, &request);
            }
            _ => panic!("recovery changed the replay action"),
        }
        let mut resolved = p1d_failed_export_snapshot();
        resolved.project_id = original.0.clone();
        resolved.requested_revision = original.1;
        resolved.request_id = request.clone();
        resolved.validate().unwrap();
        output
            .send(PortableEvent {
                ticket: replay.ticket,
                kind: replay.action.kind(),
                result: Ok(PortableReply::ExportStatus(resolved)),
            })
            .unwrap();
        panel.poll();
        assert!(panel.export.inspected.is_none());
        assert_eq!(panel.export.status.as_ref().unwrap().request_id, request);
        p1d_click(
            &mut panel,
            Some((project("current-project"), RevisionId::new(3))),
            "Prepare another export request",
            true,
        );
        assert_eq!(panel.export.previous.as_ref().unwrap().request, request);
        assert_eq!(
            panel.export.previous.as_ref().unwrap().target,
            Some(original)
        );
    }

    #[test]
    fn p1d008_export_recovery_without_pending_start_still_adopts_operation() {
        let (mut panel, input, output, _transfer_input, _transfer_output) = p1d_panel();
        let status = p1d_failed_export_snapshot();
        let request = status.request_id.clone();
        p1d_deliver_export_snapshot(&mut panel, &input, &output, status);
        assert_eq!(panel.export.request, request);
        assert!(panel.export.status.is_some());
        assert!(panel.export.inspected.is_none());
    }

    #[test]
    fn p1d_terminal_import_absorbs_late_snapshot_and_rejects_changed_receipt() {
        let (mut panel, input, output, _transfer_input, _transfer_output) = p1d_panel();
        let completed = p1d_status("first-import", true);
        panel.import.status = Some(completed.clone());
        for status in [p1d_status("first-import", false), {
            let mut contradictory = completed.clone();
            contradictory.receipt.as_mut().unwrap().project_id = project("different-clone");
            contradictory
        }] {
            assert!(panel.submit(PortableAction::ImportStatus {
                operation_id: operation("first-import")
            }));
            let work = input.try_recv().unwrap();
            output
                .send(PortableEvent {
                    ticket: work.ticket,
                    kind: work.action.kind(),
                    result: Ok(PortableReply::ImportStatus(status)),
                })
                .unwrap();
            panel.poll();
            let retained = panel.import.status.as_ref().unwrap();
            assert_eq!(retained.state, PackageImportOperationState::Completed);
            assert_eq!(
                retained.receipt.as_ref().unwrap().project_id,
                project("clone-first-import")
            );
        }
        assert!(panel
            .failure
            .as_ref()
            .unwrap()
            .message
            .contains("Contradictory terminal"));
        assert!(panel.submit(PortableAction::ImportStatus {
            operation_id: operation("explicit-recovery")
        }));
        let work = input.try_recv().unwrap();
        output
            .send(PortableEvent {
                ticket: work.ticket,
                kind: work.action.kind(),
                result: Ok(PortableReply::ImportStatus(p1d_status(
                    "explicit-recovery",
                    false,
                ))),
            })
            .unwrap();
        panel.poll();
        assert_eq!(
            panel.import.status.as_ref().unwrap().operation_id,
            operation("explicit-recovery")
        );
    }

    fn p1d_download_action(destination: &str) -> PortableAction {
        PortableAction::DownloadExport {
            project_id: project("publication-project"),
            operation_id: PackageOperationId::new("publication-operation").unwrap(),
            destination: PathBuf::from(destination),
            request_id: RequestId::new("publication-begin").unwrap(),
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
        }
    }

    fn p1d_uncertain_publication(
        panel: &mut PortablePanel,
        input: &Receiver<Work>,
        output: &SyncSender<PortableEvent>,
    ) -> PortableTicket {
        assert!(panel.submit(p1d_download_action("/selected/new-package")));
        let work = input.try_recv().unwrap();
        output
            .send(PortableEvent {
                ticket: work.ticket,
                kind: work.action.kind(),
                result: Err(PortableFailure::from_error(
                    PackagePublicationError {
                        destination: PathBuf::from("/selected/new-package"),
                        outcome: PackagePublicationOutcome::OutcomeUnknown,
                        recovery_staging_path: Some(PathBuf::from(
                            "/selected/retained-private-staging",
                        )),
                        detail: "publication acknowledgement uncertain".into(),
                    }
                    .into(),
                )),
            })
            .unwrap();
        panel.poll();
        work.ticket
    }

    #[test]
    fn p1d_publication_recovery_survives_unrelated_error_and_message_dismissal() {
        let (mut panel, input, output, transfer_input, transfer_output) = p1d_panel();
        p1d_uncertain_publication(&mut panel, &transfer_input, &transfer_output);
        assert!(panel.submit(PortableAction::ExportStatus {
            project_id: project("unrelated-project"),
            operation_id: PackageOperationId::new("unrelated-operation").unwrap(),
        }));
        let work = input.try_recv().unwrap();
        output
            .send(PortableEvent {
                ticket: work.ticket,
                kind: work.action.kind(),
                result: Err(PortableFailure::from_error(anyhow!(
                    "unrelated status failure"
                ))),
            })
            .unwrap();
        panel.poll();
        p1d_click(
            &mut panel,
            None,
            "Dismiss message (does not change engine state)",
            false,
        );
        let output = p1d_frame(&mut panel, None, Vec::new(), false);
        for value in [
            "/selected/retained-private-staging",
            "/selected/new-package",
            "publication-operation",
            "publication-begin",
            "OutcomeUnknown",
        ] {
            assert!(
                p1d_text_position(&output, value).is_some(),
                "lost recovery context: {value}"
            );
        }
    }

    #[test]
    fn p1d_publication_recovery_blocks_new_download_until_explicit_disposition() {
        let (mut panel, _input, _output, transfer_input, transfer_output) = p1d_panel();
        p1d_uncertain_publication(&mut panel, &transfer_input, &transfer_output);
        assert!(panel
            .controller
            .as_mut()
            .unwrap()
            .submit(p1d_download_action("/selected/another-package"))
            .is_err());
        assert!(matches!(
            transfer_input.try_recv(),
            Err(TryRecvError::Empty)
        ));
        p1d_click(
            &mut panel,
            None,
            "I have recorded and handled this publication outcome",
            false,
        );
        p1d_click(
            &mut panel,
            None,
            "Acknowledge publication recovery disposition",
            false,
        );
        assert!(panel
            .controller
            .as_mut()
            .unwrap()
            .submit(p1d_download_action("/selected/another-package"))
            .is_ok());
    }

    #[cfg(not(any(unix, windows)))]
    #[test]
    fn retained_package_input_refuses_unsupported_platform_without_effects() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("selected.package");
        let bytes = b"original source remains untouched";
        std::fs::write(&source, bytes).unwrap();
        let control = PackageImportControl::default();
        let error = PreparedPackageImport::open(&source, &control)
            .err()
            .expect("unsupported platform must refuse retained package input");
        assert!(error
            .to_string()
            .contains("not implemented on this platform"));
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert_eq!(control.phase(), PackageImportPhase::Idle);
        assert_eq!(control.hashed_bytes(), 0);
        assert_eq!(control.accepted_bytes(), 0);
        assert_eq!(control.verified_bytes(), 0);
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn p1d_ambiguous_import_start_blocks_replacing_real_prepared_source() {
        fn wait_for_reply(controller: &mut PortableController) -> PortableEvent {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(event) = controller.poll().into_iter().next() {
                    return event;
                }
                assert!(Instant::now() < deadline, "portable worker did not answer");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.package");
        let replacement = temp.path().join("replacement.package");
        // Valid header probes only: no archive validation or engine success is faked.
        fn header_probe_bytes(length: usize, fill: u8) -> Vec<u8> {
            let mut bytes = vec![fill; length];
            bytes[..PACKAGE_IMPORT_HEADER_BYTES].fill(0);
            bytes[..8].copy_from_slice(b"PULSPKG\0");
            bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
            bytes[12..20]
                .copy_from_slice(&((length - PACKAGE_IMPORT_HEADER_BYTES) as u64).to_le_bytes());
            bytes
        }
        std::fs::write(&first, header_probe_bytes(256, b'a')).unwrap();
        std::fs::write(&replacement, header_probe_bytes(512, b'b')).unwrap();
        let mut controller = PortableController::new(
            temp.path().join("unavailable.sock"),
            temp.path().join("unavailable-bootstrap"),
        )
        .unwrap();
        controller
            .submit(PortableAction::PrepareImport { source: first })
            .unwrap();
        let declaration = match wait_for_reply(&mut controller).result.unwrap() {
            PortableReply::ImportPrepared { declaration } => declaration,
            other => panic!("unexpected prepare reply: {other:?}"),
        };
        assert_eq!(declaration.byte_len, 256);
        let start = new_request_id();
        controller
            .submit(PortableAction::StartImport {
                request_id: start.clone(),
            })
            .unwrap();
        assert!(wait_for_reply(&mut controller).result.is_err());
        assert!(
            controller
                .submit(PortableAction::PrepareImport {
                    source: replacement
                })
                .is_err(),
            "ambiguous Start allowed replacing its retained source"
        );
        assert!(
            controller
                .submit(PortableAction::StartImport {
                    request_id: new_request_id()
                })
                .is_err(),
            "ambiguous Start allowed a new request identity"
        );
        controller
            .submit(PortableAction::StartImport { request_id: start })
            .unwrap();
        assert!(wait_for_reply(&mut controller).result.is_err());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn p1d_other_import_recovery_preserves_and_restores_real_pinned_source() {
        fn drain_transfer(panel: &mut PortablePanel) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                panel.poll();
                if !panel.transfer_busy() {
                    return;
                }
                assert!(Instant::now() < deadline, "portable worker did not answer");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("pinned-source.package");
        let mut bytes = vec![b'a'; 1024];
        bytes[..PACKAGE_IMPORT_HEADER_BYTES].fill(0);
        bytes[..8].copy_from_slice(b"PULSPKG\0");
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        bytes[12..20].copy_from_slice(&8u64.to_le_bytes());
        std::fs::write(&source, bytes).unwrap();
        let mut panel = PortablePanel::new(
            temp.path().join("unavailable.sock"),
            temp.path().join("unavailable-bootstrap"),
            egui::Context::default(),
        );
        // Keep the real retained-handle transfer worker. Only inject control snapshots.
        let (control, input, output) = test_lane();
        panel.controller.as_mut().unwrap().control = control;
        panel.next_poll = Instant::now() + Duration::from_secs(3600);
        panel.import.source = source.display().to_string();
        assert!(panel.submit(PortableAction::PrepareImport {
            source: source.clone()
        }));
        drain_transfer(&mut panel);
        let declaration = panel.import.declaration.clone().unwrap();
        let ids = panel.import.ids.clone();
        panel.import.notice = true;
        p1d_click(&mut panel, None, "Import prepared package", false);
        drain_transfer(&mut panel);
        assert!(panel.failure.is_some());
        for completed in [false, true] {
            let mut other = p1d_status("other-import", completed);
            other.package = declaration.clone();
            if let Some(receipt) = &mut other.receipt {
                receipt.package = declaration.clone();
            }
            other.validate().unwrap();
            assert!(panel.submit(PortableAction::ImportStatus {
                operation_id: other.operation_id.clone(),
            }));
            let work = input.try_recv().unwrap();
            output
                .send(PortableEvent {
                    ticket: work.ticket,
                    kind: work.action.kind(),
                    result: Ok(PortableReply::ImportStatus(other.clone())),
                })
                .unwrap();
            panel.poll();
            assert_eq!(
                panel.import.status.as_ref().unwrap().operation_id,
                other.operation_id
            );
            assert!(
                panel
                    .controller
                    .as_mut()
                    .unwrap()
                    .submit(PortableAction::ContinueImport {
                        operation_id: other.operation_id,
                        begin_request_id: new_request_id(),
                        seal_request_id: new_request_id(),
                    })
                    .is_err(),
                "another operation could consume the pinned source"
            );
            assert!(!panel.transfer_busy());
            if completed {
                p1d_click(&mut panel, None, "Prepare another import request", false);
                assert!(panel.import.declaration.is_none());
            }
            let frame = p1d_frame(&mut panel, None, Vec::new(), false);
            assert!(p1d_text_position(&frame, ids.start.as_str()).is_some());
            p1d_click(
                &mut panel,
                None,
                "Restore pinned import Start context",
                false,
            );
            assert_eq!(panel.import.ids.start, ids.start);
            assert_eq!(panel.import.ids.begin, ids.begin);
            assert_eq!(panel.import.ids.seal, ids.seal);
            assert_eq!(panel.import.declaration.as_ref(), Some(&declaration));
            assert_eq!(panel.import.source, source.display().to_string());
            assert!(panel.import.status.is_none());
            p1d_click(&mut panel, None, "Replay exact import Start", false);
            drain_transfer(&mut panel);
            assert!(panel.failure.is_some());
            let pin = panel
                .controller
                .as_ref()
                .unwrap()
                .recovery
                .import_start
                .as_ref()
                .unwrap();
            assert_eq!(pin.request_id, ids.start);
            assert_eq!(pin.package, declaration);
        }
    }
}

#[cfg(test)]
mod motion_evidence_tests {
    use super::*;
    #[test]
    fn neutral_bundle_actions_share_stop_control_and_preserve_typed_recovery() {
        let actions = [
            PortableAction::ExportNeutralBundle {
                project_id: ProjectId::new("p").unwrap(),
                revision: RevisionId::new(4),
                destination: PathBuf::from(crate::test_host_path("output")),
                axes: Axis::ALL.to_vec(),
            },
            PortableAction::ResumeNeutralBundle {
                staging: PathBuf::from(crate::test_host_path("stage")),
            },
        ];
        for action in actions {
            assert!(action.is_transfer());
            let control = LocalControl::for_action(&action);
            assert!(matches!(
                control.progress(),
                PortableProgress::NeutralBundle
            ));
            control.stop();
            assert!(control.check().is_err());
            assert!(control.download.as_ref().unwrap().check().is_err());
        }
        let error = crate::neutral_bundle::NeutralBundleError {
            destination: PathBuf::from(crate::test_host_path("output")),
            outcome: PackagePublicationOutcome::OutcomeUnknown,
            recovery_staging_path: PathBuf::from(crate::test_host_path("stage")),
            detail: "unknown publication".into(),
        };
        let failure = PortableFailure::from_error(anyhow::Error::new(error));
        let publication = failure.publication.unwrap();
        assert_eq!(
            publication.outcome,
            PackagePublicationOutcome::OutcomeUnknown
        );
        assert_eq!(
            publication.recovery_staging_path,
            Some(PathBuf::from(crate::test_host_path("stage")))
        );
    }
    #[test]
    fn paired_actions_use_transfer_lane_and_consent_tracks_revision() {
        let action = PortableAction::ExportMotionEvidence {
            project_id: ProjectId::new("p").unwrap(),
            revision: RevisionId::new(0),
            axis: Axis::Stroke,
            destination: PathBuf::from("/new/output"),
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
        };
        assert!(action.is_transfer());
        let control = LocalControl::for_action(&action);
        assert!(control.download.is_some());
        control.stop();
        assert!(control.check().is_err());
        assert!(control.download.unwrap().check().is_err());
        assert!(PortableAction::ResumeMotionEvidence {
            staging: PathBuf::from("/retained"),
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data()
        }
        .is_transfer());
        let mut view = MotionEvidenceView::default();
        view.bind(Some((ProjectId::new("p").unwrap(), RevisionId::new(0))));
        view.acknowledged = true;
        view.bind(Some((ProjectId::new("p").unwrap(), RevisionId::new(1))));
        assert!(!view.acknowledged);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod native_motion_evidence_tests {
    use super::*;
    #[test]
    #[ignore = "requires owned native Engine and committed six-axis fixture"]
    fn native_neutral_bundle_transfer_controller() {
        let state =
            PathBuf::from(std::env::var_os("PULSAR_NEUTRAL_BUNDLE_STATE").expect("owned state"));
        let project =
            ProjectId::new(std::env::var("PULSAR_NEUTRAL_BUNDLE_PROJECT").expect("owned project"))
                .unwrap();
        let output =
            PathBuf::from(std::env::var_os("PULSAR_NEUTRAL_BUNDLE_OUTPUT").expect("owned output"));
        let mut client =
            SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))
                .unwrap();
        let before = crate::get_snapshot(&mut client, &project).unwrap();
        assert_eq!(before.program.tracks().len(), 6);
        let ctx = egui::Context::default();
        let mut panel = PortablePanel::new(
            state.join("engine.sock"),
            state.join("pairing.token"),
            ctx.clone(),
        );
        let target = (project.clone(), before.revision);
        panel.neutral_bundle.destination = output.to_string_lossy().into_owned();
        let drawn = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                panel.neutral_bundle.show(ui, Some(&target), true);
            });
        });
        assert!(!drawn.shapes.is_empty());
        assert!(panel.submit(PortableAction::ExportNeutralBundle {
            project_id: project.clone(),
            revision: before.revision,
            destination: output.clone(),
            axes: Axis::ALL.to_vec(),
        }));
        let deadline = Instant::now() + Duration::from_secs(60);
        while panel.neutral_bundle.published.is_none() {
            panel.poll();
            assert!(panel.failure.is_none(), "{:?}", panel.failure);
            assert!(Instant::now() < deadline, "neutral transfer expired");
            std::thread::sleep(Duration::from_millis(10));
        }
        let receipt = panel.neutral_bundle.published.as_ref().unwrap();
        assert_eq!(receipt.destination, output);
        assert!(receipt.file_and_directory_synced && !receipt.qualified);
        assert!(!panel.transfer_busy());
        assert!(matches!(
            panel.controller.as_ref().unwrap().progress(),
            PortableProgress::Idle
        ));
        let after = crate::get_snapshot(&mut client, &project).unwrap();
        assert_eq!(before.revision, after.revision);
        assert_eq!(before.program, after.program);
    }
    #[test]
    #[ignore = "requires owned native Engine and committed six-axis recovery fixture"]
    fn native_neutral_bundle_conflict_and_resume_controller() {
        let state =
            PathBuf::from(std::env::var_os("PULSAR_NEUTRAL_BUNDLE_STATE").expect("owned state"));
        let project =
            ProjectId::new(std::env::var("PULSAR_NEUTRAL_BUNDLE_PROJECT").expect("owned project"))
                .unwrap();
        let output =
            PathBuf::from(std::env::var_os("PULSAR_NEUTRAL_BUNDLE_OUTPUT").expect("owned output"));
        let observation = PathBuf::from(
            std::env::var_os("PULSAR_NEUTRAL_BUNDLE_OBSERVATION").expect("owned observation"),
        );
        let parent = output.parent().unwrap();
        assert!(!output.exists());
        let stages = || -> std::collections::BTreeSet<PathBuf> {
            std::fs::read_dir(parent)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".pulsar-neutral-bundle-")
                })
                .collect()
        };
        let initial_stages = stages();
        let mut client =
            SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))
                .unwrap();
        let before = crate::get_snapshot(&mut client, &project).unwrap();
        assert_eq!(before.program.tracks().len(), 6);
        let ctx = egui::Context::default();
        let mut panel = PortablePanel::new(
            state.join("engine.sock"),
            state.join("pairing.token"),
            ctx.clone(),
        );
        assert!(panel.submit(PortableAction::ExportNeutralBundle {
            project_id: project.clone(),
            revision: before.revision,
            destination: output.clone(),
            axes: Axis::ALL.to_vec(),
        }));
        let deadline = Instant::now() + Duration::from_secs(60);
        let stage = loop {
            let new: Vec<_> = stages().difference(&initial_stages).cloned().collect();
            if !new.is_empty() {
                assert_eq!(new.len(), 1);
                break new[0].clone();
            }
            panel.poll();
            assert!(panel.failure.is_none(), "{:?}", panel.failure);
            assert!(
                panel.transfer_busy() && Instant::now() < deadline,
                "stage barrier missed"
            );
            std::thread::sleep(Duration::from_micros(100));
        };
        std::fs::create_dir(&output).unwrap();
        let sentinel = b"preserve this owned conflicting directory";
        std::fs::write(output.join("sentinel"), sentinel).unwrap();
        while panel.transfer_busy() {
            panel.poll();
            assert!(Instant::now() < deadline, "conflict transfer expired");
            std::thread::sleep(Duration::from_millis(1));
        }
        let failure = panel.failure.as_ref().expect("actual conflict failure");
        let publication = failure.publication.as_ref().unwrap();
        assert_eq!(publication.outcome, PackagePublicationOutcome::NotPublished);
        assert_eq!(publication.recovery_staging_path.as_ref(), Some(&stage));
        assert_eq!(panel.neutral_bundle.staging, stage.to_string_lossy());
        assert_eq!(std::fs::read(output.join("sentinel")).unwrap(), sentinel);
        let intent = std::fs::read(stage.join(".pulsar-bundle.json")).unwrap();
        let mut leaves = std::collections::BTreeMap::new();
        for axis in ["stroke", "surge", "sway", "yaw", "roll", "pitch"] {
            let name = format!("{axis}.funscript");
            leaves.insert(name.clone(), std::fs::read(stage.join(name)).unwrap());
        }
        // Preserve the owned blocker, then submit the exact retained intent.
        let preserved = output.with_extension("preserved-conflict");
        assert!(!preserved.exists());
        std::fs::rename(&output, &preserved).unwrap();
        panel.failure = None;
        assert!(panel.submit(PortableAction::ResumeNeutralBundle {
            staging: stage.clone()
        }));
        while panel.neutral_bundle.published.is_none() {
            panel.poll();
            assert!(panel.failure.is_none(), "{:?}", panel.failure);
            assert!(Instant::now() < deadline, "resume transfer expired");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!stage.exists());
        assert_eq!(
            std::fs::read(output.join(".pulsar-bundle.json")).unwrap(),
            intent
        );
        for (name, bytes) in leaves {
            assert_eq!(std::fs::read(output.join(name)).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(preserved.join("sentinel")).unwrap(), sentinel);
        assert!(panel.neutral_bundle.staging.is_empty());
        let after = crate::get_snapshot(&mut client, &project).unwrap();
        assert_eq!(before.revision, after.revision);
        assert_eq!(before.program, after.program);
        std::fs::write(observation, serde_json::to_vec_pretty(&serde_json::json!({"qualified":false,"rendered_gui":false,"actual_conflict":true,"retained_intent_unchanged":true,"master_unchanged":true,"preserved_conflict":preserved,"published":output})).unwrap()).unwrap();
    }
    #[test]
    #[ignore = "owned native Engine and authored six-axis project supplied by the paired export harness"]
    fn native_paired_export_transfer_controller() {
        let state =
            PathBuf::from(std::env::var_os("PULSAR_MOTION_EVIDENCE_STATE").expect("owned state"));
        let project =
            ProjectId::new(std::env::var("PULSAR_MOTION_EVIDENCE_PROJECT").expect("owned project"))
                .unwrap();
        let output =
            PathBuf::from(std::env::var_os("PULSAR_MOTION_EVIDENCE_OUTPUT").expect("owned output"));
        let mut client =
            SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))
                .unwrap();
        let before = crate::get_snapshot(&mut client, &project).unwrap();
        assert_eq!(before.program.tracks().len(), 6);
        let ctx = egui::Context::default();
        let mut panel = PortablePanel::new(
            state.join("engine.sock"),
            state.join("pairing.token"),
            ctx.clone(),
        );
        let target = (project.clone(), before.revision);
        panel.motion_evidence.bind(Some(target.clone()));
        panel.motion_evidence.destination = output.to_string_lossy().into_owned();
        panel.motion_evidence.acknowledged = true;
        let constructed = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| panel.show_motion_evidence(ui, Some(&target)));
        });
        assert!(!constructed.shapes.is_empty());
        assert!(panel.submit(PortableAction::ExportMotionEvidence {
            project_id: project.clone(),
            revision: before.revision,
            axis: Axis::Stroke,
            destination: output.clone(),
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data()
        }));
        let deadline = Instant::now() + Duration::from_secs(120);
        while panel.motion_evidence.published.is_none() {
            panel.poll();
            assert!(panel.failure.is_none(), "{:?}", panel.failure);
            assert!(
                Instant::now() < deadline,
                "paired transfer controller expired"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let receipt = panel.motion_evidence.published.as_ref().unwrap();
        assert_eq!(receipt.destination, output);
        assert!(
            !receipt.association.execution_authority
                && !receipt.association.qualified
                && receipt.file_and_directory_synced
        );
        assert!(!panel.transfer_busy());
        let after = crate::get_snapshot(&mut client, &project).unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.program, before.program);
        let checked = crate::motion_evidence::verify_motion_evidence_files(
            &output.join("motion.funscript"),
            &output.join("project.pulsar"),
            &project,
            before.revision,
            Axis::Stroke,
        )
        .unwrap();
        assert_eq!(checked.package_sha256, receipt.association.package_sha256);
        let observation=serde_json::to_vec_pretty(&serde_json::json!({"qualified":false,"rendered_gui":false,"headless_construction":true,"actual_transfer_lane":true,"master_unchanged":true,"association":receipt.association})).unwrap();
        let drawn = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| panel.show_motion_evidence(ui, Some(&target)));
        });
        assert!(!drawn.shapes.is_empty());
        std::fs::write(output.join("controller-observation.json"), observation).unwrap();
    }
}
