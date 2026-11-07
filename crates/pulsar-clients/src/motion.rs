//! Client-local hydrated motion. No partial artifact is visible to callers.
use anyhow::{bail, ensure, Context, Result};
use pulsar_protocol as wire;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use wire::*;

const CACHE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TRANSFER_LIFETIME: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Serialize)]
pub struct ProjectSnapshot {
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub name: String,
    pub motion: MotionDescriptor,
    pub program: Arc<MotionProgram>,
    pub sources: Vec<SourceSummary>,
    pub placements: Vec<SourcePlacement>,
}
#[derive(Clone, Debug, Serialize)]
pub struct CandidateSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_interval: Option<TimeRange>,
    pub candidate_id: CandidateId,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub motion: MotionDescriptor,
    pub program: Arc<MotionProgram>,
    pub job_id: Option<JobId>,
    pub review: Vec<ReviewFlag>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ResponseBody {
    MediaProbe(Box<wire::MediaProbeReport>),
    NativeImagePreparation(Box<wire::NativeImagePreparationStatus>),
    NativeImagePreparationLimits(Box<wire::NativeImagePreparationLimits>),
    AssistantStartResolution(Box<pulsar_protocol::AssistantStartResolution>),
    AssistantEditRecipes(Vec<pulsar_protocol::AssistantEditRecipe>),
    AssistantTargets(Box<pulsar_protocol::AssistantTargetCatalog>),
    AssistantProfiles(Vec<pulsar_protocol::AssistantProfile>),
    AssistantImage(Box<pulsar_protocol::AssistantImageContext>),
    Assistant(Box<pulsar_protocol::AssistantReport>),
    AssistantAttempts(Vec<pulsar_protocol::AssistantReport>),
    WasmEditorInfo(pulsar_protocol::WasmEditorInfo),
    WasmEdit(Box<pulsar_protocol::WasmEditReport>),
    WasmStartResolution(Box<pulsar_protocol::WasmStartResolution>),
    WasmEditCatalog(pulsar_protocol::WasmEditCatalog),
    WasmArchivePage(pulsar_protocol::WasmArchivePage),
    WasmEditArchived(Box<pulsar_protocol::WasmEditReport>),
    WasmEditRemoved(Box<pulsar_protocol::WasmEditReport>),
    LiveCapture(Box<pulsar_protocol::LiveCaptureReport>),
    BrowserAnalysisSuccessor(Box<pulsar_protocol::BrowserAnalysisSuccessorReport>),
    BrowserAnalysisTransfer(Box<pulsar_protocol::BrowserAnalysisTransferReport>),
    LiveRecordingPage(Box<pulsar_protocol::LiveRecordingPage>),
    LiveCudaEvidenceRemoval(pulsar_protocol::LiveCudaEvidenceRemoval),
    LiveCudaEvidencePage(pulsar_protocol::LiveCudaEvidencePage),
    RetainedLiveCudaEvidence(Box<pulsar_protocol::RetainedLiveCudaEvidence>),
    LiveCaptureLimits(pulsar_protocol::LiveCaptureLimits),
    BrowserMediaCatalog(Box<pulsar_protocol::BrowserMediaCatalog>),
    BrowserMediaPreparation(Box<pulsar_protocol::BrowserMediaPreparationReport>),
    BrowserTiming(Box<pulsar_protocol::BrowserTimingReport>),
    BrowserTimingProbe(Box<pulsar_protocol::BrowserTimingProbe>),
    HandyHspSourceQualification(Box<pulsar_protocol::HandyHspSourceQualificationReport>),
    HandyHspSourceQualificationSaved(Box<pulsar_protocol::HandyHspSourceQualificationReceipt>),
    HandyBrowserStartQualification(Box<pulsar_protocol::HandyBrowserStartQualificationReport>),
    HandyBrowserStartQualificationSaved(
        Box<pulsar_protocol::HandyBrowserStartQualificationReceipt>,
    ),
    HandyBrowserPlayback(Box<pulsar_protocol::HandyBrowserPlaybackReport>),
    MotionRetimeReceipt(Box<pulsar_protocol::MotionRetimeReceipt>),
    ArchivedMotionRetimeReceipt(Box<pulsar_protocol::ArchivedMotionRetimeReceipt>),
    ArchivedGeneration(Box<pulsar_protocol::ArchivedGenerationReport>),
    CandidateImageOrigin(Box<pulsar_protocol::CandidateImageOriginReport>),
    CandidateImageOriginPixels(Box<pulsar_protocol::CandidateImageOriginPixels>),
    ModelPackSelection(Box<pulsar_protocol::ModelPackSelectionReport>),
    ModelPackWithdrawal(Box<pulsar_protocol::ModelPackWithdrawalReport>),
    ModelPack(pulsar_protocol::ModelPackReport),
    ModelCatalog(pulsar_protocol::ModelCatalogReport),
    ModelPackCatalog(Box<pulsar_protocol::ModelPackCatalogReport>),
    ModelPackStaging(pulsar_protocol::ModelPackStagingReport),
    ModelPackDiscard(pulsar_protocol::ModelPackDiscardReport),
    HandyInspection(pulsar_protocol::HandyInspection),
    HandyStrokeScript(Box<pulsar_protocol::HandyStrokeScript>),
    HandyHosting(Box<pulsar_protocol::HandyHostingReport>),
    HandySetupInspection(Box<pulsar_protocol::HandySetupInspection>),
    HandyProgramCorrelation(Box<pulsar_protocol::HandyProgramCorrelation>),
    HandyPlaybackPreparation(Box<pulsar_protocol::HandyPlaybackPreparation>),
    HandyController(Box<pulsar_protocol::HandyControllerReceipt>),
    HandyServerClock(Box<pulsar_protocol::HandyServerClockObservation>),
    HandyDeviceTiming(Box<pulsar_protocol::HandyDeviceTimingInspection>),
    HandyClockSync(Box<pulsar_protocol::HandyClockSyncStatus>),
    HandyPlayback(Box<pulsar_protocol::HandyPlaybackStatus>),
    HandyHspLive(Box<pulsar_protocol::HandyHspLiveStatus>),
    HandyHspContinuity(Box<pulsar_protocol::HandyHspContinuityStatus>),
    HandyHspPreparation(Box<pulsar_protocol::HandyHspPreparationStatus>),
    HandyHsp(Box<pulsar_protocol::HandyHspStatus>),
    HandyStop(Box<pulsar_protocol::HandyStopStatus>),
    HandyContinuousTiming(Box<pulsar_protocol::HandyContinuousTimingObservation>),
    HandyPlayerGuard(Box<pulsar_protocol::HandyPlayerGuardReport>),
    HandyPlayerChallenge(Box<pulsar_protocol::HandyPlayerChallenge>),
    HandyRecoveryPlan(Box<pulsar_protocol::HandyRecoveryPlan>),
    HandyRecovery(Box<pulsar_protocol::HandyRecoveryStatus>),
    HandyPlaybackClock(Box<pulsar_protocol::HandyPlaybackClockObservation>),
    HandyPlaybackRetirement(Box<pulsar_protocol::HandyPlaybackRetirement>),
    HandySetupStatus(Box<pulsar_protocol::HandySetupStatus>),
    HandySetupCancellation(Box<pulsar_protocol::HandySetupCancellation>),
    HandyQualificationContext(Box<pulsar_protocol::HandyQualificationContext>),
    HandyQualification(Box<pulsar_protocol::HandyQualificationReport>),
    HandyQualificationSaved(Box<pulsar_protocol::HandyQualificationReceipt>),
    HandySession(Box<pulsar_protocol::HandyConnectionSession>),
    HandySessions(Box<pulsar_protocol::HandyConnectionCatalog>),
    HandyRegistrationSaved(Box<pulsar_protocol::HandyRegistrationReceipt>),
    HandyRegistrationResolution(Box<pulsar_protocol::HandyRegistrationResolution>),
    HandyRegistrations(Box<pulsar_protocol::HandyRegistrationCatalog>),
    HandyRegisteredInspection(Box<pulsar_protocol::HandyRegisteredInspection>),
    StashDiscovery(StashDiscovery),
    StashPlan(StashPlan),
    StashScan(Box<StashScanReceipt>),
    StashTag(Box<StashTagReceipt>),
    StashBatch(StashBatchReport),
    LibraryBatch(LibraryBatchReport),
    LibraryBatchCatalog(LibraryBatchCatalog),
    LibraryBatchReleased(LibraryBatchReleaseReceipt),
    StashSourceCaptured(Box<StashSourceCapture>),
    StashScriptExported(Box<StashScriptExport>),
    StashPublication(Box<StashPublicationObservation>),
    InferenceCatalog(InferenceCatalog),
    InferenceEnvironment(Box<pulsar_protocol::InferenceEnvironmentReport>),
    InferenceEquivalenceStatus(Box<pulsar_protocol::InferenceEquivalenceStatus>),
    InferenceEquivalenceList(Box<pulsar_protocol::InferenceEquivalencePage>),
    InferenceEquivalenceSaved(Box<pulsar_protocol::InferenceEquivalenceReceipt>),
    DeviceMotionExported {
        path: PathBuf,
        revision: RevisionId,
        gain: f64,
        profile_sha256: String,
        master_sha256: String,
    },
    PackageImport(Box<pulsar_protocol::PackageImportStatus>),
    PackageUpload(pulsar_protocol::PackageUploadLease),
    PackageUploadAbandoned {
        lease_id: pulsar_protocol::PackageUploadId,
    },

    PackageExport(Box<PackageExportStatus>),
    PackageDownload(Box<PackageDownloadLease>),
    PackageDownloadPending(PackageDownloadPending),
    PackageDownloadAbandoned {
        lease_id: PackageDownloadId,
    },
    Project(Box<ProjectSnapshot>),
    Job(JobSnapshot),
    JobAttempts(Box<JobAttemptPage>),
    Candidate(Box<CandidateSnapshot>),
    Diagnostics(DiagnosticsReport),
    ReviewStatus(pulsar_protocol::ReviewStatus),
    Transfer(TransferLease),
    TransferAbandoned {
        lease_id: TransferId,
    },
    Source {
        source_version: SourceVersionId,
    },
    Exported {
        path: PathBuf,
        revision: RevisionId,
    },
    AxisExported {
        path: PathBuf,
        revision: RevisionId,
        axis: Axis,
    },
    ModelPackPreview(Box<pulsar_protocol::ModelPackPreviewResult>),
    Preview(Box<PreviewResult>),
    Capabilities(Capabilities),
    Events(EventPage),
    Paired {
        session: SessionId,
        auth_token: String,
    },
    Ack,
}
impl std::fmt::Debug for ResponseBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MediaProbe(_) => "MediaProbe",
            Self::AssistantStartResolution(_) => "AssistantStartResolution",
            Self::AssistantEditRecipes(_) => "AssistantEditRecipes",
            Self::AssistantTargets(_) => "AssistantTargets",
            Self::AssistantProfiles(_) => "AssistantProfiles",
            Self::AssistantImage(_) => "AssistantImage",
            Self::Assistant(_) => "Assistant",
            Self::AssistantAttempts(_) => "AssistantAttempts",
            Self::WasmEditorInfo(_) => "WasmEditorInfo",
            Self::WasmEdit(_) => "WasmEdit",
            Self::WasmStartResolution(_) => "WasmStartResolution",
            Self::WasmEditCatalog(_) => "WasmEditCatalog",
            Self::WasmArchivePage(_) => "WasmArchivePage",
            Self::WasmEditArchived(_) => "WasmEditArchived",
            Self::WasmEditRemoved(_) => "WasmEditRemoved",
            Self::StashScriptExported(_) => "StashScriptExported",
            Self::StashPublication(_) => "StashPublication",
            Self::StashSourceCaptured(_) => "StashSourceCaptured",
            Self::StashBatch(_) => "StashBatch",
            Self::LibraryBatchReleased(_) => "LibraryBatchReleased",
            Self::LibraryBatch(_) => "LibraryBatch",
            Self::LibraryBatchCatalog(_) => "LibraryBatchCatalog",
            Self::StashTag(_) => "StashTag",
            Self::StashScan(_) => "StashScan",
            Self::StashPlan(_) => "StashPlan",
            Self::MotionRetimeReceipt(_) => "MotionRetimeReceipt",
            Self::ArchivedGeneration(_) => "ArchivedGeneration",
            Self::CandidateImageOrigin(_) => "CandidateImageOrigin",
            Self::CandidateImageOriginPixels(_) => "CandidateImageOriginPixels",
            Self::ArchivedMotionRetimeReceipt(_) => "ArchivedMotionRetimeReceipt",
            Self::ModelPackSelection(_) => "ModelPackSelection",
            Self::ModelPackWithdrawal(_) => "ModelPackWithdrawal",
            Self::ModelPack(_) => "ModelPack",
            Self::ModelPackCatalog(_) => "ModelPackCatalog",
            Self::ModelCatalog(_) => "ModelCatalog",
            Self::ModelPackStaging(_) => "ModelPackStaging",
            Self::ModelPackDiscard(_) => "ModelPackDiscard",
            Self::LiveCapture(_) => "LiveCapture",
            Self::LiveRecordingPage(_) => "LiveRecordingPage",
            Self::LiveCudaEvidenceRemoval(_) => "LiveCudaEvidenceRemoval",
            Self::LiveCudaEvidencePage(_) => "LiveCudaEvidencePage",
            Self::RetainedLiveCudaEvidence(_) => "RetainedLiveCudaEvidence",
            Self::LiveCaptureLimits(_) => "LiveCaptureLimits",
            Self::BrowserMediaCatalog(_) => "BrowserMediaCatalog",
            Self::BrowserMediaPreparation(_) => "BrowserMediaPreparation",
            Self::BrowserAnalysisSuccessor(_) => "BrowserAnalysisSuccessor",
            Self::BrowserAnalysisTransfer(_) => "BrowserAnalysisTransfer",
            Self::BrowserTiming(_) => "BrowserTiming",
            Self::BrowserTimingProbe(_) => "BrowserTimingProbe",
            Self::HandyHspSourceQualification(_) => "HandyHspSourceQualification",
            Self::HandyBrowserStartQualification(_) => "HandyBrowserStartQualification",
            Self::HandyHspSourceQualificationSaved(_) => "HandyHspSourceQualificationSaved",
            Self::HandyBrowserStartQualificationSaved(_) => "HandyBrowserStartQualificationSaved",
            Self::HandyBrowserPlayback(_) => "HandyBrowserPlayback",
            Self::HandySession(_) => "HandySession",
            Self::HandySessions(_) => "HandySessions",
            Self::HandySetupInspection(_) => "HandySetupInspection",
            Self::HandyProgramCorrelation(_) => "HandyProgramCorrelation",
            Self::HandyPlaybackPreparation(_) => "HandyPlaybackPreparation",
            Self::HandyPlaybackClock(_) => "HandyPlaybackClock",
            Self::HandyClockSync(_) => "HandyClockSync",
            Self::HandyPlayback(_) => "HandyPlayback",
            Self::HandyHspLive(_) => "HandyHspLive",
            Self::HandyHspContinuity(_) => "HandyHspContinuity",
            Self::HandyHspPreparation(_) => "HandyHspPreparation",
            Self::HandyHsp(_) => "HandyHsp",
            Self::HandyStop(_) => "HandyStop",
            Self::HandyContinuousTiming(_) => "HandyContinuousTiming",
            Self::HandyPlayerGuard(_) => "HandyPlayerGuard",
            Self::HandyPlayerChallenge(_) => "HandyPlayerChallenge",
            Self::HandyRecoveryPlan(_) => "HandyRecoveryPlan",
            Self::HandyRecovery(_) => "HandyRecovery",
            Self::HandyDeviceTiming(_) => "HandyDeviceTiming",
            Self::HandyServerClock(_) => "HandyServerClock",
            Self::HandyController(_) => "HandyController",
            Self::HandyPlaybackRetirement(_) => "HandyPlaybackRetirement",
            Self::HandySetupStatus(_) => "HandySetupStatus",
            Self::HandySetupCancellation(_) => "HandySetupCancellation",
            Self::HandyQualificationContext(_) => "HandyQualificationContext",
            Self::HandyQualification(_) => "HandyQualification",
            Self::HandyQualificationSaved(_) => "HandyQualificationSaved",
            Self::HandyHosting(_) => "HandyHosting",
            Self::HandyStrokeScript(_) => "HandyStrokeScript",
            Self::HandyInspection(_) => "HandyInspection",
            Self::HandyRegistrationSaved(_) => "HandyRegistrationSaved",
            Self::HandyRegistrationResolution(_) => "HandyRegistrationResolution",
            Self::HandyRegistrations(_) => "HandyRegistrations",
            Self::HandyRegisteredInspection(_) => "HandyRegisteredInspection",
            Self::StashDiscovery(_) => "StashDiscovery",
            Self::Project(_) => "Project",
            Self::Job(_) => "Job",
            Self::JobAttempts(_) => "JobAttempts",
            Self::Candidate(_) => "Candidate",
            Self::ReviewStatus(_) => "ReviewStatus",
            Self::Diagnostics(_) => "Diagnostics",
            Self::Transfer(_) => "Transfer",
            Self::TransferAbandoned { .. } => "TransferAbandoned",
            Self::Source { .. } => "Source",
            Self::Exported { .. } => "Exported",
            Self::AxisExported { .. } => "AxisExported",
            Self::ModelPackPreview(_) => "ModelPackPreview",
            Self::Preview(_) => "Preview",
            Self::Capabilities(_) => "Capabilities",
            Self::InferenceCatalog(_) => "InferenceCatalog",
            Self::InferenceEnvironment(_) => "InferenceEnvironment",
            Self::InferenceEquivalenceStatus(_) => "InferenceEquivalenceStatus",
            Self::InferenceEquivalenceList(_) => "InferenceEquivalenceList",
            Self::InferenceEquivalenceSaved(_) => "InferenceEquivalenceSaved",
            Self::DeviceMotionExported { .. } => "DeviceMotionExported",
            Self::Events(_) => "Events",
            Self::Paired { .. } => "Paired { credentials: [REDACTED] }",
            Self::PackageImport(_) => "PackageImport",
            Self::PackageUpload(_) => "PackageUpload",
            Self::PackageUploadAbandoned { .. } => "PackageUploadAbandoned",
            Self::NativeImagePreparation(_) => "NativeImagePreparation",
            Self::NativeImagePreparationLimits(_) => "NativeImagePreparationLimits",
            Self::Ack => "Ack",
            Self::PackageExport(_) => "PackageExport",
            Self::PackageDownload(_) => "PackageDownload",
            Self::PackageDownloadPending(_) => "PackageDownloadPending",
            Self::PackageDownloadAbandoned { .. } => "PackageDownloadAbandoned",
        })
    }
}

/// A local draft, never a wire command. Evidence is discarded by the values codec.
#[derive(Clone, Debug)]
pub struct EditProposal {
    pub program: MotionProgram,
    pub label: String,
}

pub(crate) trait BulkIo {
    fn upload(&mut self, offset: u64, bytes: &[u8]) -> Result<u64>;
    fn download(&mut self, offset: u64, byte_len: u32) -> Result<Vec<u8>>;
}
impl BulkIo for wire::BulkClient {
    fn upload(&mut self, offset: u64, bytes: &[u8]) -> Result<u64> {
        Ok(wire::BulkClient::upload(self, offset, bytes)?)
    }
    fn download(&mut self, offset: u64, byte_len: u32) -> Result<Vec<u8>> {
        Ok(wire::BulkClient::download(self, offset, byte_len)?)
    }
}
/// Original edit-upload identity. Unknown outcomes retain this state for exact retry.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetainedEditUpload {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub begin_request: RequestId,
    pub finish_request: RequestId,
    pub byte_len: u64,
    pub sha256: String,
    pub label: String,
    pub lease: Option<TransferLease>,
    pub finalization_attempted: bool,
}
impl RetainedEditUpload {
    pub(crate) fn new(
        project: ProjectId,
        revision: RevisionId,
        program: &MotionProgram,
        label: String,
    ) -> Result<Self> {
        let bytes = bounded_json(&EditValuesProgram::from_program_values(program)?)?;
        ensure!(
            !label.is_empty() && label.len() <= 1024,
            "edit label length refused"
        );
        Ok(Self {
            project,
            revision,
            begin_request: crate::session::new_request_id(),
            finish_request: crate::session::new_request_id(),
            byte_len: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            label,
            lease: None,
            finalization_attempted: false,
        })
    }
}
pub(crate) trait MotionIo {
    fn control_identified(
        &mut self,
        _request: RequestId,
        _command: Command,
        _project: ProjectId,
        _revision: Option<RevisionId>,
    ) -> Result<wire::ResponseBody> {
        bail!("this motion transport cannot preserve original request identity")
    }

    fn control(
        &mut self,
        command: Command,
        project: ProjectId,
        revision: Option<RevisionId>,
    ) -> Result<wire::ResponseBody>;
    fn open_bulk(&mut self, lease: &TransferLease, deadline: Instant) -> Result<Box<dyn BulkIo>>;
    fn bulk_endpoint(&self) -> &Path;
}

#[derive(Default)]
pub(crate) struct MotionCache {
    entries: VecDeque<(ProgramDescriptor, Arc<MotionProgram>)>,
    byte_len: u64,
}
impl MotionCache {
    pub(crate) fn hydrate(
        &mut self,
        response: wire::ResponseBody,
        expected_project: Option<&ProjectId>,
        expected_candidate: Option<&CandidateId>,
        io: &mut impl MotionIo,
    ) -> Result<ResponseBody> {
        Ok(match response {
            wire::ResponseBody::MediaProbe(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|p| p == &value.project_id),
                    "Media probe project mismatch"
                );
                ResponseBody::MediaProbe(value)
            }
            wire::ResponseBody::NativeImagePreparation(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|p| p == &value.report.intent.project),
                    "Native preparation project mismatch"
                );
                ResponseBody::NativeImagePreparation(value)
            }
            wire::ResponseBody::NativeImagePreparationLimits(value) => {
                value.validate()?;
                ResponseBody::NativeImagePreparationLimits(value)
            }
            wire::ResponseBody::AssistantStartResolution(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Assistant resolution project differs"
                );
                ResponseBody::AssistantStartResolution(value)
            }
            wire::ResponseBody::AssistantEditRecipes(value) => {
                anyhow::ensure!(value.len() <= 12, "recipe catalog exceeds bounds");
                for recipe in &value {
                    recipe.validate()?;
                }
                ResponseBody::AssistantEditRecipes(value)
            }
            wire::ResponseBody::AssistantTargets(value) => {
                value.validate().map_err(anyhow::Error::from)?;
                ResponseBody::AssistantTargets(value)
            }
            wire::ResponseBody::AssistantImage(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Prepared image project differs"
                );
                ResponseBody::AssistantImage(value)
            }
            wire::ResponseBody::AssistantProfiles(value) => {
                ensure!(value.len() <= 8, "Assistant profile catalog exceeds bound");
                for profile in &value {
                    profile.validate()?;
                }
                ResponseBody::AssistantProfiles(value)
            }
            wire::ResponseBody::Assistant(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|p| p == &value.project_id),
                    "Assistant project differs"
                );
                ResponseBody::Assistant(value)
            }
            wire::ResponseBody::AssistantAttempts(value) => {
                ensure!(
                    value.len() <= 128,
                    "Assistant attempt catalog exceeds bound"
                );
                for report in &value {
                    report.validate()?;
                    ensure!(
                        expected_project.is_none_or(|p| p == &report.project_id),
                        "Assistant catalog project differs"
                    );
                }
                ResponseBody::AssistantAttempts(value)
            }
            wire::ResponseBody::WasmEditorInfo(value) => ResponseBody::WasmEditorInfo(value),
            wire::ResponseBody::WasmStartResolution(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|p| p == &value.project_id),
                    "WASM start resolution project differs"
                );
                ResponseBody::WasmStartResolution(value)
            }
            wire::ResponseBody::WasmEdit(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|p| p == &value.project_id),
                    "WASM edit response project mismatch"
                );
                ResponseBody::WasmEdit(value)
            }
            wire::ResponseBody::WasmArchivePage(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|p| p == &value.project_id),
                    "WASM archive history project mismatch"
                );
                ResponseBody::WasmArchivePage(value)
            }
            wire::ResponseBody::WasmEditCatalog(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|p| p == &value.project_id),
                    "WASM catalog project mismatch"
                );
                ResponseBody::WasmEditCatalog(value)
            }
            wire::ResponseBody::WasmEditArchived(value) => {
                value.validate()?;
                ResponseBody::WasmEditArchived(value)
            }
            wire::ResponseBody::WasmEditRemoved(value) => {
                value.validate()?;
                ensure!(
                    value.state.is_unsuccessful_terminal()
                        && expected_project.is_none_or(|p| p == &value.project_id),
                    "WASM removal response mismatch"
                );
                ResponseBody::WasmEditRemoved(value)
            }
            wire::ResponseBody::Project(snapshot) => {
                snapshot.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &snapshot.project_id),
                    "Project response identity mismatch"
                );
                let binding = MotionBinding::ProjectRevision {
                    project_id: snapshot.project_id.clone(),
                    revision: snapshot.revision,
                };
                let program = self.program(&snapshot.motion, &binding, io)?;
                ResponseBody::Project(
                    (ProjectSnapshot {
                        project_id: snapshot.project_id,
                        revision: snapshot.revision,
                        name: snapshot.name,
                        sources: snapshot.sources,
                        placements: snapshot.placements,
                        motion: snapshot.motion,
                        program,
                    })
                    .into(),
                )
            }
            wire::ResponseBody::Candidate(snapshot) => {
                snapshot.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &snapshot.project_id),
                    "Candidate project identity mismatch"
                );
                ensure!(
                    expected_candidate.is_none_or(|id| id == &snapshot.candidate_id),
                    "Candidate response identity mismatch"
                );
                let binding = MotionBinding::Candidate {
                    project_id: snapshot.project_id.clone(),
                    candidate_id: snapshot.candidate_id.clone(),
                    base_revision: snapshot.base_revision,
                };
                let program = self
                    .program(&snapshot.motion, &binding, io)
                    .with_context(|| {
                        format!(
                            "Candidate {} exists, but its motion was not hydrated",
                            snapshot.candidate_id
                        )
                    })?;
                ResponseBody::Candidate(
                    (CandidateSnapshot {
                        project_interval: snapshot.project_interval,
                        candidate_id: snapshot.candidate_id,
                        project_id: snapshot.project_id,
                        base_revision: snapshot.base_revision,
                        job_id: snapshot.job_id,
                        review: snapshot.review,
                        motion: snapshot.motion,
                        program,
                    })
                    .into(),
                )
            }
            wire::ResponseBody::PackageExport(value) => ResponseBody::PackageExport(value),
            wire::ResponseBody::PackageDownload(value) => ResponseBody::PackageDownload(value),
            wire::ResponseBody::PackageDownloadPending(value) => {
                ResponseBody::PackageDownloadPending(value)
            }
            wire::ResponseBody::PackageDownloadAbandoned { lease_id } => {
                ResponseBody::PackageDownloadAbandoned { lease_id }
            }
            wire::ResponseBody::PackageImport(value) => ResponseBody::PackageImport(value),
            wire::ResponseBody::PackageUpload(value) => ResponseBody::PackageUpload(value),
            wire::ResponseBody::PackageUploadAbandoned { lease_id } => {
                ResponseBody::PackageUploadAbandoned { lease_id }
            }
            wire::ResponseBody::Job(value) => ResponseBody::Job(value),
            wire::ResponseBody::JobAttempts(value) => ResponseBody::JobAttempts(value),
            wire::ResponseBody::ReviewStatus(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Review response project mismatch"
                );
                ResponseBody::ReviewStatus(value)
            }
            wire::ResponseBody::Diagnostics(value) => ResponseBody::Diagnostics(value),
            wire::ResponseBody::Transfer(value) => ResponseBody::Transfer(value),
            wire::ResponseBody::TransferAbandoned { lease_id } => {
                ResponseBody::TransferAbandoned { lease_id }
            }
            wire::ResponseBody::Source { source_version } => {
                ResponseBody::Source { source_version }
            }
            wire::ResponseBody::Exported { path, revision } => {
                ResponseBody::Exported { path, revision }
            }
            wire::ResponseBody::AxisExported {
                path,
                revision,
                axis,
            } => ResponseBody::AxisExported {
                path,
                revision,
                axis,
            },
            wire::ResponseBody::ModelPackPreview(value) => {
                value.validate()?;
                ResponseBody::ModelPackPreview(value)
            }
            wire::ResponseBody::Preview(value) => ResponseBody::Preview(value),
            wire::ResponseBody::Capabilities(value) => ResponseBody::Capabilities(value),
            wire::ResponseBody::LiveRecordingPage(value) => {
                value.validate()?;
                ResponseBody::LiveRecordingPage(value)
            }
            wire::ResponseBody::LiveCapture(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.identity.project),
                    "Capture response project mismatch"
                );
                ResponseBody::LiveCapture(value)
            }
            wire::ResponseBody::LiveCudaEvidenceRemoval(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Retained CUDA removal project mismatch"
                );
                ResponseBody::LiveCudaEvidenceRemoval(value)
            }
            wire::ResponseBody::LiveCudaEvidencePage(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project),
                    "Retained CUDA listing project mismatch"
                );
                ResponseBody::LiveCudaEvidencePage(value)
            }
            wire::ResponseBody::RetainedLiveCudaEvidence(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.identity.project),
                    "Retained CUDA evidence project mismatch"
                );
                ResponseBody::RetainedLiveCudaEvidence(value)
            }
            wire::ResponseBody::LiveCaptureLimits(value) => ResponseBody::LiveCaptureLimits(value),
            wire::ResponseBody::BrowserMediaCatalog(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Browser media catalog project mismatch"
                );
                ResponseBody::BrowserMediaCatalog(value)
            }
            wire::ResponseBody::BrowserAnalysisSuccessor(value) => {
                value.validate()?;
                ensure!(
                    expected_project == Some(&value.predecessor.project),
                    "Browser analysis successor project differs"
                );
                ResponseBody::BrowserAnalysisSuccessor(value)
            }
            wire::ResponseBody::BrowserAnalysisTransfer(value) => {
                value.validate()?;
                ensure!(
                    expected_project == Some(&value.predecessor.project),
                    "Browser analysis transfer project differs"
                );
                ResponseBody::BrowserAnalysisTransfer(value)
            }
            wire::ResponseBody::BrowserMediaPreparation(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Browser media preparation project mismatch"
                );
                ResponseBody::BrowserMediaPreparation(value)
            }
            wire::ResponseBody::BrowserTiming(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.binding.project_id),
                    "Browser timing project mismatch"
                );
                ResponseBody::BrowserTiming(value)
            }
            wire::ResponseBody::BrowserTimingProbe(value) => {
                value.validate()?;
                ResponseBody::BrowserTimingProbe(value)
            }
            wire::ResponseBody::HandyHspSourceQualification(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.manifest.subject.project_id),
                    "Browser dossier project mismatch"
                );
                ResponseBody::HandyHspSourceQualification(value)
            }
            wire::ResponseBody::HandyHspSourceQualificationSaved(value) => {
                value.validate()?;
                ensure!(
                    expected_project
                        .is_none_or(|id| id == &value.report.manifest.subject.project_id),
                    "Browser dossier receipt project mismatch"
                );
                ResponseBody::HandyHspSourceQualificationSaved(value)
            }
            wire::ResponseBody::HandyBrowserStartQualification(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.manifest.subject.project_id),
                    "Browser dossier project mismatch"
                );
                ResponseBody::HandyBrowserStartQualification(value)
            }
            wire::ResponseBody::HandyBrowserStartQualificationSaved(value) => {
                value.validate()?;
                ensure!(
                    expected_project
                        .is_none_or(|id| id == &value.report.manifest.subject.project_id),
                    "Browser dossier receipt project mismatch"
                );
                ResponseBody::HandyBrowserStartQualificationSaved(value)
            }
            wire::ResponseBody::HandyBrowserPlayback(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Browser playback project mismatch"
                );
                ResponseBody::HandyBrowserPlayback(value)
            }
            wire::ResponseBody::InferenceCatalog(value) => ResponseBody::InferenceCatalog(value),
            wire::ResponseBody::InferenceEnvironment(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none() && expected_candidate.is_none(),
                    "Inference environment observation requires global scope"
                );
                ResponseBody::InferenceEnvironment(value)
            }
            wire::ResponseBody::InferenceEquivalenceStatus(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none(),
                    "Inference equivalence status requires global scope"
                );
                ResponseBody::InferenceEquivalenceStatus(value)
            }
            wire::ResponseBody::InferenceEquivalenceList(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none(),
                    "Inference equivalence list requires global scope"
                );
                ResponseBody::InferenceEquivalenceList(value)
            }
            wire::ResponseBody::InferenceEquivalenceSaved(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none(),
                    "Inference equivalence receipt requires global scope"
                );
                ResponseBody::InferenceEquivalenceSaved(value)
            }
            wire::ResponseBody::StashScriptExported(value) => {
                ResponseBody::StashScriptExported(value)
            }
            wire::ResponseBody::StashPublication(value) => ResponseBody::StashPublication(value),
            wire::ResponseBody::StashSourceCaptured(value) => {
                ResponseBody::StashSourceCaptured(value)
            }
            wire::ResponseBody::StashBatch(value) => ResponseBody::StashBatch(value),
            wire::ResponseBody::LibraryBatchReleased(value) => {
                ResponseBody::LibraryBatchReleased(value)
            }
            wire::ResponseBody::LibraryBatch(value) => ResponseBody::LibraryBatch(value),
            wire::ResponseBody::LibraryBatchCatalog(value) => {
                ResponseBody::LibraryBatchCatalog(value)
            }
            wire::ResponseBody::StashTag(value) => ResponseBody::StashTag(value),
            wire::ResponseBody::StashScan(value) => ResponseBody::StashScan(value),
            wire::ResponseBody::StashPlan(value) => ResponseBody::StashPlan(value),
            wire::ResponseBody::CandidateImageOriginPixels(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.report.project_id),
                    "Image pixels response project mismatch"
                );
                ensure!(
                    expected_candidate.is_none_or(|id| id == &value.report.candidate_id),
                    "Image pixels response candidate mismatch"
                );
                ResponseBody::CandidateImageOriginPixels(value)
            }
            wire::ResponseBody::CandidateImageOrigin(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Image origin response project mismatch"
                );
                ensure!(
                    expected_candidate.is_none_or(|id| id == &value.candidate_id),
                    "Image origin response candidate mismatch"
                );
                ResponseBody::CandidateImageOrigin(value)
            }
            wire::ResponseBody::ArchivedGeneration(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Archived generation response project mismatch"
                );
                ResponseBody::ArchivedGeneration(value)
            }
            wire::ResponseBody::ArchivedMotionRetimeReceipt(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_none_or(|id| id == &value.project_id),
                    "Archival retiming response project mismatch"
                );
                ResponseBody::ArchivedMotionRetimeReceipt(value)
            }
            wire::ResponseBody::MotionRetimeReceipt(value) => {
                value.validate()?;
                ResponseBody::MotionRetimeReceipt(value)
            }
            wire::ResponseBody::ModelPackCatalog(value) => {
                value.validate()?;
                ResponseBody::ModelPackCatalog(value)
            }
            wire::ResponseBody::ModelCatalog(value) => {
                value.validate()?;
                ResponseBody::ModelCatalog(value)
            }
            wire::ResponseBody::ModelPackDiscard(value) => {
                value.validate()?;
                ResponseBody::ModelPackDiscard(value)
            }
            wire::ResponseBody::ModelPackStaging(value) => {
                value.validate()?;
                ResponseBody::ModelPackStaging(value)
            }
            wire::ResponseBody::ModelPackWithdrawal(value) => {
                value.validate()?;
                ResponseBody::ModelPackWithdrawal(value)
            }
            wire::ResponseBody::ModelPackSelection(value) => {
                value.validate()?;
                ResponseBody::ModelPackSelection(value)
            }
            wire::ResponseBody::ModelPack(value) => {
                value.validate()?;
                ResponseBody::ModelPack(value)
            }
            wire::ResponseBody::HandyRegistrationResolution(value) => {
                value.validate()?;
                ResponseBody::HandyRegistrationResolution(value)
            }
            wire::ResponseBody::HandyRegistrationSaved(value) => {
                value.validate()?;
                ResponseBody::HandyRegistrationSaved(value)
            }
            wire::ResponseBody::HandyRegistrations(value) => {
                value.validate()?;
                ResponseBody::HandyRegistrations(value)
            }
            wire::ResponseBody::HandyRegisteredInspection(value) => {
                value.validate()?;
                ResponseBody::HandyRegisteredInspection(value)
            }
            wire::ResponseBody::HandySession(value) => {
                value.validate()?;
                ResponseBody::HandySession(value)
            }
            wire::ResponseBody::HandySessions(value) => {
                value.validate()?;
                ResponseBody::HandySessions(value)
            }
            wire::ResponseBody::HandySetupCancellation(value) => {
                value.validate()?;
                ResponseBody::HandySetupCancellation(value)
            }
            wire::ResponseBody::HandySetupStatus(value) => {
                value.validate()?;
                ResponseBody::HandySetupStatus(value)
            }
            wire::ResponseBody::HandyPlaybackClock(value) => {
                value.validate()?;
                ResponseBody::HandyPlaybackClock(value)
            }
            wire::ResponseBody::HandyClockSync(value) => {
                value.validate()?;
                ResponseBody::HandyClockSync(value)
            }
            wire::ResponseBody::HandyRecoveryPlan(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Handy recovery plan project differs"
                );
                ResponseBody::HandyRecoveryPlan(value)
            }
            wire::ResponseBody::HandyRecovery(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Handy recovery history project differs"
                );
                ResponseBody::HandyRecovery(value)
            }
            wire::ResponseBody::HandyContinuousTiming(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|project| project == &value.controller.project_id),
                    "continuous timing project differs"
                );
                ResponseBody::HandyContinuousTiming(value)
            }
            wire::ResponseBody::HandyPlayerGuard(value) => {
                value.validate()?;
                ensure!(
                    expected_project
                        .is_some_and(|project| project == &value.binding.controller.project_id),
                    "Handy player guard project differs"
                );
                ResponseBody::HandyPlayerGuard(value)
            }
            wire::ResponseBody::HandyPlayerChallenge(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some(),
                    "Handy player challenge requires project scope"
                );
                ResponseBody::HandyPlayerChallenge(value)
            }
            wire::ResponseBody::HandyHspLive(value) => {
                value.validate()?;
                ensure!(
                    expected_project == Some(&value.project_id),
                    "Handy HSP live project differs"
                );
                ResponseBody::HandyHspLive(value)
            }
            wire::ResponseBody::HandyHspContinuity(value) => {
                value.validate()?;
                ensure!(
                    expected_project == Some(&value.project_id),
                    "Handy HSP continuity project differs"
                );
                ResponseBody::HandyHspContinuity(value)
            }
            wire::ResponseBody::HandyHspPreparation(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Handy HSP preparation project differs"
                );
                ResponseBody::HandyHspPreparation(value)
            }
            wire::ResponseBody::HandyHsp(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Handy HSP stream project differs"
                );
                ResponseBody::HandyHsp(value)
            }
            wire::ResponseBody::HandyStop(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Handy stop history project differs"
                );
                ResponseBody::HandyStop(value)
            }
            wire::ResponseBody::HandyPlayback(value) => {
                value.validate()?;
                ensure!(
                    expected_project.is_some_and(|p| p == &value.project_id),
                    "Handy playback history project differs"
                );
                ResponseBody::HandyPlayback(value)
            }
            wire::ResponseBody::HandyDeviceTiming(value) => {
                value.validate()?;
                ResponseBody::HandyDeviceTiming(value)
            }
            wire::ResponseBody::HandyServerClock(value) => {
                value.validate()?;
                ResponseBody::HandyServerClock(value)
            }
            wire::ResponseBody::HandyController(value) => {
                value.validate()?;
                ResponseBody::HandyController(value)
            }
            wire::ResponseBody::HandyPlaybackPreparation(value) => {
                value.validate()?;
                ResponseBody::HandyPlaybackPreparation(value)
            }
            wire::ResponseBody::HandyPlaybackRetirement(value) => {
                value.validate()?;
                ResponseBody::HandyPlaybackRetirement(value)
            }
            wire::ResponseBody::HandyProgramCorrelation(value) => {
                value.validate()?;
                ResponseBody::HandyProgramCorrelation(value)
            }
            wire::ResponseBody::HandySetupInspection(value) => {
                value.validate()?;
                ResponseBody::HandySetupInspection(value)
            }
            wire::ResponseBody::HandyQualificationContext(value) => {
                value.validate()?;
                ResponseBody::HandyQualificationContext(value)
            }
            wire::ResponseBody::HandyQualification(value) => {
                value.validate()?;
                ResponseBody::HandyQualification(value)
            }
            wire::ResponseBody::HandyQualificationSaved(value) => {
                value.validate()?;
                ResponseBody::HandyQualificationSaved(value)
            }
            wire::ResponseBody::HandyHosting(value) => {
                value.validate()?;
                ResponseBody::HandyHosting(value)
            }
            wire::ResponseBody::HandyStrokeScript(value) => {
                value.validate()?;
                ResponseBody::HandyStrokeScript(value)
            }
            wire::ResponseBody::HandyInspection(value) => {
                value.validate()?;
                ResponseBody::HandyInspection(value)
            }
            wire::ResponseBody::StashDiscovery(value) => ResponseBody::StashDiscovery(value),
            wire::ResponseBody::DeviceMotionExported {
                path,
                revision,
                gain,
                profile_sha256,
                master_sha256,
            } => ResponseBody::DeviceMotionExported {
                path,
                revision,
                gain,
                profile_sha256,
                master_sha256,
            },
            wire::ResponseBody::Events(value) => ResponseBody::Events(value),
            wire::ResponseBody::Paired {
                session,
                auth_token,
            } => ResponseBody::Paired {
                session,
                auth_token,
            },
            wire::ResponseBody::Ack => ResponseBody::Ack,
        })
    }

    fn program(
        &mut self,
        descriptor: &MotionDescriptor,
        binding: &MotionBinding,
        io: &mut impl MotionIo,
    ) -> Result<Arc<MotionProgram>> {
        descriptor.validate()?;
        ensure!(
            &descriptor.binding == binding,
            "Motion descriptor authority binding mismatch"
        );
        if let Some((_, program)) = self
            .entries
            .iter()
            .find(|(key, _)| key == &descriptor.program)
        {
            return Ok(program.clone());
        }
        let started = Instant::now();
        let lease = match io.control(
            Command::BeginMotionDownload {
                locator: binding.locator(),
                offset: 0,
                byte_len: descriptor.program.byte_len,
            },
            binding.project_id().clone(),
            None,
        )? {
            wire::ResponseBody::Transfer(lease) => lease,
            _ => bail!("Unexpected motion download response"),
        };
        let result = (|| {
            validate_lease(
                &lease,
                io.bulk_endpoint(),
                binding.project_id(),
                TransferDirection::Download,
                descriptor.program.byte_len,
                &descriptor.program.sha256,
            )?;
            let deadline = transfer_deadline(&lease, started)?;
            let mut bulk = io.open_bulk(&lease, deadline)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(usize::try_from(descriptor.program.byte_len)?)
                .context("Cannot reserve bounded motion download memory")?;
            while (bytes.len() as u64) < descriptor.program.byte_len {
                ensure!(
                    Instant::now() < deadline,
                    "Motion download lease deadline elapsed"
                );
                let offset = bytes.len() as u64;
                let count = (descriptor.program.byte_len - offset)
                    .min(MAX_ARTIFACT_CHUNK_BYTES as u64) as u32;
                let chunk = bulk.download(offset, count)?;
                ensure!(
                    chunk.len() == count as usize,
                    "Truncated or oversized motion chunk"
                );
                bytes.extend_from_slice(&chunk);
            }
            ensure!(
                Instant::now() < deadline,
                "Motion download lease deadline elapsed"
            );
            validate_program_bytes(&descriptor.program, &bytes).map(Arc::new)
        })();
        if result.is_err() {
            let _ = io.control(
                Command::AbandonTransfer {
                    lease_id: lease.lease_id.clone(),
                },
                binding.project_id().clone(),
                None,
            );
        }
        let program = result?;
        while self.entries.len() >= 8 || self.byte_len + descriptor.program.byte_len > CACHE_BYTES {
            if let Some((old, _)) = self.entries.pop_front() {
                self.byte_len -= old.byte_len;
            } else {
                break;
            }
        }
        self.byte_len += descriptor.program.byte_len;
        self.entries
            .push_back((descriptor.program.clone(), program.clone()));
        Ok(program)
    }

    pub(crate) fn upload_edit_retained(
        &mut self,
        intent: &mut RetainedEditUpload,
        program: &MotionProgram,
        io: &mut impl MotionIo,
        checkpoint: &mut impl FnMut(&RetainedEditUpload) -> Result<()>,
    ) -> Result<CandidateSnapshot> {
        let bytes = bounded_json(&EditValuesProgram::from_program_values(program)?)?;
        ensure!(
            bytes.len() as u64 == intent.byte_len
                && format!("{:x}", Sha256::digest(&bytes)) == intent.sha256,
            "retained edit bytes changed; original upload identity cannot be replaced"
        );
        checkpoint(intent)?;
        if !intent.finalization_attempted {
            if intent.lease.is_none() {
                let started = Instant::now();
                let wire::ResponseBody::Transfer(lease) = io.control_identified(
                    intent.begin_request.clone(),
                    Command::BeginEditUpload {
                        byte_len: intent.byte_len,
                        sha256: intent.sha256.clone(),
                        label: intent.label.clone(),
                    },
                    intent.project.clone(),
                    Some(intent.revision),
                )?
                else {
                    bail!("retained edit admission response differs");
                };
                // A replay can already have accepted bytes. Validate identity before opening bulk.
                lease.validate()?;
                let mut initial = lease.clone();
                initial.accepted_prefix = 0;
                validate_lease(
                    &initial,
                    io.bulk_endpoint(),
                    &intent.project,
                    TransferDirection::Upload,
                    intent.byte_len,
                    &intent.sha256,
                )?;
                transfer_deadline(&lease, started)?;
                intent.lease = Some(lease);
                checkpoint(intent)?;
            }
            let original = intent.lease.as_ref().context("retained lease absent")?;
            let started = Instant::now();
            let wire::ResponseBody::Transfer(lease) = io.control(
                Command::TransferStatus {
                    lease_id: original.lease_id.clone(),
                },
                intent.project.clone(),
                None,
            )?
            else {
                bail!("retained upload status differs");
            };
            lease.validate()?;
            let mut initial = lease.clone();
            initial.accepted_prefix = 0;
            validate_lease(
                &initial,
                io.bulk_endpoint(),
                &intent.project,
                TransferDirection::Upload,
                intent.byte_len,
                &intent.sha256,
            )?;
            ensure!(
                lease.lease_id == original.lease_id && lease.engine_epoch == original.engine_epoch,
                "retained upload lease or Engine boot changed"
            );
            let deadline = transfer_deadline(&lease, started)?;
            let offset = usize::try_from(lease.accepted_prefix)?;
            intent.lease = Some(lease.clone());
            checkpoint(intent)?;
            if offset < bytes.len() {
                let mut bulk = io.open_bulk(&lease, deadline)?;
                let mut accepted = offset;
                for chunk in bytes[offset..].chunks(MAX_ARTIFACT_CHUNK_BYTES) {
                    ensure!(Instant::now() < deadline, "retained upload lease expired");
                    let next = bulk.upload(accepted as u64, chunk)?;
                    accepted += chunk.len();
                    ensure!(
                        next == accepted as u64,
                        "retained upload acknowledgement differs"
                    );
                }
            }
            // Set before dispatch: retry finalization directly, including after Engine restart.
            intent.finalization_attempted = true;
            checkpoint(intent)?;
        }
        let lease = intent
            .lease
            .as_ref()
            .context("retained finalization lease absent")?;
        let response = io.control_identified(
            intent.finish_request.clone(),
            Command::FinishEditUpload {
                lease_id: lease.lease_id.clone(),
            },
            intent.project.clone(),
            Some(intent.revision),
        )?;
        let wire::ResponseBody::Candidate(candidate) = &response else {
            bail!("retained edit finalization response differs");
        };
        ensure!(
            candidate.project_id == intent.project && candidate.base_revision == intent.revision,
            "retained edit candidate scope differs"
        );
        match self.hydrate(response, Some(&intent.project), None, io)? {
            ResponseBody::Candidate(c) => Ok(*c),
            _ => unreachable!(),
        }
    }

    pub(crate) fn upload_edit(
        &mut self,
        project: ProjectId,
        revision: RevisionId,
        program: &MotionProgram,
        label: &str,
        io: &mut impl MotionIo,
    ) -> Result<CandidateSnapshot> {
        let values = EditValuesProgram::from_program_values(program)?;
        let bytes = bounded_json(&values)?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let started = Instant::now();
        let lease = match io.control(
            Command::BeginEditUpload {
                byte_len: bytes.len() as u64,
                sha256: sha256.clone(),
                label: label.into(),
            },
            project.clone(),
            Some(revision),
        )? {
            wire::ResponseBody::Transfer(lease) => lease,
            _ => bail!("Unexpected edit upload admission response"),
        };
        let uploaded = (|| -> Result<()> {
            validate_lease(
                &lease,
                io.bulk_endpoint(),
                &project,
                TransferDirection::Upload,
                bytes.len() as u64,
                &sha256,
            )?;
            let deadline = transfer_deadline(&lease, started)?;
            let mut bulk = io.open_bulk(&lease, deadline)?;
            let mut offset = 0_usize;
            for chunk in bytes.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
                ensure!(
                    Instant::now() < deadline,
                    "Edit upload lease deadline elapsed"
                );
                let accepted = bulk.upload(offset as u64, chunk)?;
                offset += chunk.len();
                ensure!(
                    accepted == offset as u64,
                    "Edit upload acknowledgement mismatch"
                );
            }
            ensure!(
                Instant::now() < deadline,
                "Edit upload lease deadline elapsed"
            );
            Ok(())
        })();
        if let Err(error) = uploaded {
            let _ = io.control(
                Command::AbandonTransfer {
                    lease_id: lease.lease_id.clone(),
                },
                project,
                None,
            );
            return Err(error.context("Edit upload was not finalized"));
        }
        let response = io.control(Command::FinishEditUpload { lease_id: lease.lease_id.clone() }, project.clone(), Some(revision))
            .with_context(|| format!("Edit finalization for lease {} was not confirmed; inspect TransferStatus before retrying", lease.lease_id))?;
        match &response {
            wire::ResponseBody::Candidate(candidate) => {
                ensure!(
                    candidate.project_id == project && candidate.base_revision == revision,
                    "Finalized candidate did not retain the admitted project/base revision"
                );
            }
            _ => bail!("Unexpected edit finalization response; no commit was requested"),
        }
        match self.hydrate(response, Some(&project), None, io)? {
            ResponseBody::Candidate(candidate) => Ok(*candidate),
            _ => unreachable!(),
        }
    }
}

pub(crate) fn validate_lease(
    lease: &TransferLease,
    endpoint: &Path,
    project: &ProjectId,
    direction: TransferDirection,
    byte_len: u64,
    sha256: &str,
) -> Result<()> {
    lease.validate()?;
    ensure!(
        lease.bulk_endpoint == endpoint,
        "Bulk endpoint does not match authenticated engine endpoint"
    );
    ensure!(
        &lease.project_id == project && lease.direction == direction,
        "Transfer lease authority/direction mismatch"
    );
    ensure!(
        lease.sha256 == sha256
            && lease.total_byte_len == byte_len
            && lease.offset == 0
            && lease.byte_len == byte_len
            && lease.accepted_prefix == 0,
        "Transfer lease does not match complete motion artifact"
    );
    ensure!(
        lease.expires_after_ms <= MAX_TRANSFER_LIFETIME.as_millis() as u64,
        "Transfer lease lifetime exceeds client bound"
    );
    Ok(())
}

pub(crate) fn validate_handshake(expected: &TransferLease, actual: &TransferLease) -> Result<()> {
    actual.validate()?;
    ensure!(
        actual.lease_id == expected.lease_id
            && actual.engine_epoch == expected.engine_epoch
            && actual.project_id == expected.project_id
            && actual.direction == expected.direction
            && actual.sha256 == expected.sha256
            && actual.total_byte_len == expected.total_byte_len
            && actual.offset == expected.offset
            && actual.byte_len == expected.byte_len
            && actual.accepted_prefix == expected.accepted_prefix
            && actual.bulk_endpoint == expected.bulk_endpoint
            && actual.expires_after_ms <= expected.expires_after_ms,
        "Bulk handshake receipt changed the admitted lease"
    );
    Ok(())
}
fn transfer_deadline(lease: &TransferLease, started: Instant) -> Result<Instant> {
    started
        .checked_add(Duration::from_millis(lease.expires_after_ms))
        .context("Transfer deadline overflow")
}
fn validate_program_bytes(descriptor: &ProgramDescriptor, bytes: &[u8]) -> Result<MotionProgram> {
    descriptor.validate()?;
    ensure!(
        bytes.len() as u64 == descriptor.byte_len,
        "Motion artifact length mismatch"
    );
    ensure!(
        format!("{:x}", Sha256::digest(bytes)) == descriptor.sha256,
        "Motion artifact SHA-256 mismatch"
    );
    let program: MotionProgram =
        serde_json::from_slice(bytes).context("Motion artifact failed checked decoding")?;
    ensure!(
        program.tracks().len() == descriptor.axes.len(),
        "Motion axis summary mismatch"
    );
    for track in program.tracks() {
        let summary = descriptor
            .axes
            .iter()
            .find(|axis| axis.axis == track.axis())
            .context("Motion axis missing from descriptor")?;
        ensure!(
            summary.action_count == track.actions().len() as u64
                && summary.gap_count == track.gaps().len() as u64,
            "Motion action/gap summary mismatch"
        );
    }
    Ok(program)
}
struct BoundedJson(Vec<u8>);
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > MAX_MOTION_BYTES.saturating_sub(self.0.len() as u64) {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "motion upload exceeds 64 MiB admission",
            ));
        }
        let needed = self.0.len() + bytes.len();
        if needed > self.0.capacity() {
            let capacity = needed.next_power_of_two().min(MAX_MOTION_BYTES as usize);
            self.0
                .try_reserve_exact(capacity - self.0.len())
                .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn bounded_json(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut output = BoundedJson(Vec::new());
    serde_json::to_writer(&mut output, value)?;
    validate_motion_length(output.0.len() as u64)?;
    Ok(output.0)
}

#[cfg(test)]
pub(crate) fn fixture_descriptor(
    program: &MotionProgram,
    binding: MotionBinding,
) -> MotionDescriptor {
    let bytes = serde_json::to_vec(program).unwrap();
    MotionDescriptor {
        program: ProgramDescriptor {
            artifact_id: ArtifactId::new("fixture-motion").unwrap(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            byte_len: bytes.len() as u64,
            codec: MotionCodec::MotionProgramJsonV1,
            axes: program
                .tracks()
                .iter()
                .map(|track| AxisSummary {
                    axis: track.axis(),
                    action_count: track.actions().len() as u64,
                    gap_count: track.gaps().len() as u64,
                })
                .collect(),
        },
        binding,
    }
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    fn project() -> ProjectId {
        ProjectId::new("project").unwrap()
    }
    fn program() -> MotionProgram {
        MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![
                MotionAction::new(
                    ProjectTime::from_nanos(0),
                    NormalizedPosition::new(0.2).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(100),
                    NormalizedPosition::new(0.8).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap(),
            ],
        )
        .unwrap()])
        .unwrap()
    }
    fn binding() -> MotionBinding {
        MotionBinding::ProjectRevision {
            project_id: project(),
            revision: RevisionId::new(1),
        }
    }
    struct MockBulk {
        bytes: Vec<u8>,
        uploaded: Rc<RefCell<Vec<u8>>>,
        corrupt: bool,
        truncated: bool,
        bad_ack: bool,
    }
    impl BulkIo for MockBulk {
        fn upload(&mut self, offset: u64, bytes: &[u8]) -> Result<u64> {
            self.uploaded.borrow_mut().extend_from_slice(bytes);
            Ok(offset + bytes.len() as u64 + u64::from(self.bad_ack))
        }
        fn download(&mut self, offset: u64, len: u32) -> Result<Vec<u8>> {
            let mut bytes = self.bytes[offset as usize..offset as usize + len as usize].to_vec();
            if self.corrupt {
                bytes[0] ^= 1;
            }
            if self.truncated {
                bytes.pop();
            }
            Ok(bytes)
        }
    }
    struct MockIo {
        program: MotionProgram,
        commands: Vec<Command>,
        opens: usize,
        endpoint: PathBuf,
        uploaded: Rc<RefCell<Vec<u8>>>,
        corrupt: bool,
        truncated: bool,
        bad_ack: bool,
        redirect: bool,
    }
    impl MockIo {
        fn new() -> Self {
            Self {
                program: program(),
                commands: Vec::new(),
                opens: 0,
                endpoint: PathBuf::from("/private/bulk.sock"),
                uploaded: Rc::new(RefCell::new(Vec::new())),
                corrupt: false,
                truncated: false,
                bad_ack: false,
                redirect: false,
            }
        }
        fn lease(&self, direction: TransferDirection, len: u64, hash: String) -> TransferLease {
            TransferLease {
                lease_id: TransferId::new("lease").unwrap(),
                engine_epoch: "epoch".into(),
                project_id: project(),
                direction,
                sha256: hash,
                total_byte_len: len,
                offset: 0,
                byte_len: len,
                accepted_prefix: 0,
                expires_after_ms: 300_000,
                bulk_endpoint: if self.redirect {
                    PathBuf::from("/attacker/bulk.sock")
                } else {
                    self.endpoint.clone()
                },
            }
        }
        fn descriptor(&self) -> MotionDescriptor {
            fixture_descriptor(&self.program, binding())
        }
    }
    impl MotionIo for MockIo {
        fn control(
            &mut self,
            command: Command,
            scope: ProjectId,
            revision: Option<RevisionId>,
        ) -> Result<wire::ResponseBody> {
            assert_eq!(scope, project());
            self.commands.push(command.clone());
            Ok(match command {
                Command::BeginMotionDownload {
                    offset, byte_len, ..
                } => {
                    assert_eq!(offset, 0);
                    wire::ResponseBody::Transfer(self.lease(
                        TransferDirection::Download,
                        byte_len,
                        self.descriptor().program.sha256,
                    ))
                }
                Command::BeginEditUpload {
                    byte_len, sha256, ..
                } => {
                    assert_eq!(revision, Some(RevisionId::new(1)));
                    wire::ResponseBody::Transfer(self.lease(
                        TransferDirection::Upload,
                        byte_len,
                        sha256,
                    ))
                }
                Command::FinishEditUpload { .. } => {
                    let id = CandidateId::new("candidate").unwrap();
                    wire::ResponseBody::Candidate(
                        (wire::CandidateSnapshot {
                            project_interval: None,
                            candidate_id: id.clone(),
                            project_id: project(),
                            base_revision: RevisionId::new(1),
                            motion: fixture_descriptor(
                                &self.program,
                                MotionBinding::Candidate {
                                    project_id: project(),
                                    candidate_id: id,
                                    base_revision: RevisionId::new(1),
                                },
                            ),
                            job_id: None,
                            review: Vec::new(),
                        })
                        .into(),
                    )
                }
                Command::AbandonTransfer { lease_id } => {
                    wire::ResponseBody::TransferAbandoned { lease_id }
                }
                _ => bail!("Unexpected mock transfer command"),
            })
        }
        fn open_bulk(&mut self, _: &TransferLease, _: Instant) -> Result<Box<dyn BulkIo>> {
            self.opens += 1;
            Ok(Box::new(MockBulk {
                bytes: serde_json::to_vec(&self.program)?,
                uploaded: self.uploaded.clone(),
                corrupt: self.corrupt,
                truncated: self.truncated,
                bad_ack: self.bad_ack,
            }))
        }
        fn bulk_endpoint(&self) -> &Path {
            &self.endpoint
        }
    }

    struct RetainedIo {
        inner: MockIo,
        lease: Option<TransferLease>,
        ids: Vec<(RequestId, Command)>,
        lose_begin: bool,
        lose_finish: bool,
        invalid_prefix: bool,
    }
    impl RetainedIo {
        fn new() -> Self {
            Self {
                inner: MockIo::new(),
                lease: None,
                ids: vec![],
                lose_begin: false,
                lose_finish: false,
                invalid_prefix: false,
            }
        }
    }
    impl MotionIo for RetainedIo {
        fn control_identified(
            &mut self,
            id: RequestId,
            cmd: Command,
            scope: ProjectId,
            revision: Option<RevisionId>,
        ) -> Result<wire::ResponseBody> {
            self.ids.push((id, cmd.clone()));
            let response = self.inner.control(cmd.clone(), scope, revision)?;
            if let wire::ResponseBody::Transfer(lease) = &response {
                self.lease = Some(lease.clone());
            }
            if matches!(cmd, Command::BeginEditUpload { .. }) && self.lose_begin {
                self.lose_begin = false;
                bail!("lost Begin acknowledgement");
            }
            if matches!(cmd, Command::FinishEditUpload { .. }) && self.lose_finish {
                self.lose_finish = false;
                bail!("lost Finish acknowledgement");
            }
            Ok(response)
        }
        fn control(
            &mut self,
            cmd: Command,
            scope: ProjectId,
            revision: Option<RevisionId>,
        ) -> Result<wire::ResponseBody> {
            if matches!(cmd, Command::TransferStatus { .. }) {
                let mut lease = self.lease.clone().unwrap();
                lease.accepted_prefix = if self.invalid_prefix {
                    lease.byte_len + 1
                } else {
                    self.inner.uploaded.borrow().len() as u64
                };
                return Ok(wire::ResponseBody::Transfer(lease));
            }
            self.inner.control(cmd, scope, revision)
        }
        fn open_bulk(
            &mut self,
            lease: &TransferLease,
            deadline: Instant,
        ) -> Result<Box<dyn BulkIo>> {
            self.inner.open_bulk(lease, deadline)
        }
        fn bulk_endpoint(&self) -> &Path {
            self.inner.bulk_endpoint()
        }
    }
    #[test]
    fn environment_hydration_validates_global_report_without_transfer_io() {
        let report = crate::inference_equivalence::environment_tests::report;
        let wire_report = || wire::ResponseBody::InferenceEnvironment(Box::new(report()));
        let mut io = MockIo::new();
        let mut cache = MotionCache::default();
        let hydrated = cache.hydrate(wire_report(), None, None, &mut io).unwrap();
        assert!(matches!(&hydrated, ResponseBody::InferenceEnvironment(_)));
        assert_eq!(format!("{hydrated:?}"), "InferenceEnvironment");
        assert!(cache
            .hydrate(wire_report(), Some(&project()), None, &mut io)
            .is_err());
        assert!(cache
            .hydrate(
                wire_report(),
                None,
                Some(&CandidateId::new("candidate").unwrap()),
                &mut io
            )
            .is_err());
        let mut invalid = report();
        invalid.profile_sha256 = "0".repeat(64);
        assert!(cache
            .hydrate(
                wire::ResponseBody::InferenceEnvironment(Box::new(invalid)),
                None,
                None,
                &mut io
            )
            .is_err());
        let mut invalid = report();
        invalid.runtime_dependency_closure_verified = true;
        assert!(cache
            .hydrate(
                wire::ResponseBody::InferenceEnvironment(Box::new(invalid)),
                None,
                None,
                &mut io
            )
            .is_err());
        assert!(io.commands.is_empty());
        assert_eq!(io.opens, 0);
    }

    #[test]
    fn equivalence_page_hydration_validates_global_observation_without_transfer_io() {
        let page = |report_only| {
            wire::ResponseBody::InferenceEquivalenceList(Box::new(InferenceEquivalencePage {
                entries: vec![],
                next_after: None,
                report_only,
            }))
        };
        let mut io = MockIo::new();
        let mut cache = MotionCache::default();
        let hydrated = cache.hydrate(page(true), None, None, &mut io).unwrap();
        assert!(matches!(
            &hydrated,
            ResponseBody::InferenceEquivalenceList(_)
        ));
        assert_eq!(format!("{hydrated:?}"), "InferenceEquivalenceList");
        let project_error = cache
            .hydrate(page(true), Some(&project()), None, &mut io)
            .unwrap_err();
        assert!(project_error.to_string().contains("requires global scope"));
        assert!(cache.hydrate(page(false), None, None, &mut io).is_err());
        assert_eq!(io.opens, 0);
        assert!(io.commands.is_empty());
    }

    #[test]
    fn retained_upload_recovers_lost_begin_chunk_and_finish_without_replacement() {
        for loss in 0..3 {
            let p = program();
            let mut io = RetainedIo::new();
            let mut cache = MotionCache::default();
            let mut intent =
                RetainedEditUpload::new(project(), RevisionId::new(1), &p, "retained edit".into())
                    .unwrap();
            match loss {
                0 => io.lose_begin = true,
                1 => io.inner.bad_ack = true,
                _ => io.lose_finish = true,
            }
            assert!(cache
                .upload_edit_retained(&mut intent, &p, &mut io, &mut |_| Ok(()))
                .is_err());
            io.inner.bad_ack = false;
            let c = cache
                .upload_edit_retained(&mut intent, &p, &mut io, &mut |_| Ok(()))
                .unwrap();
            assert_eq!(c.program.as_ref(), &p);
            assert_eq!(
                io.inner.uploaded.borrow().as_slice(),
                bounded_json(&EditValuesProgram::from_program_values(&p).unwrap()).unwrap()
            );
            for (id, cmd) in &io.ids {
                assert_eq!(
                    id,
                    if matches!(cmd, Command::BeginEditUpload { .. }) {
                        &intent.begin_request
                    } else {
                        &intent.finish_request
                    }
                );
            }
            assert!(!io
                .inner
                .commands
                .iter()
                .any(|c| matches!(c, Command::AbandonTransfer { .. })));
            let ids = io.ids.len();
            let mut changed = intent.clone();
            changed.sha256 = "b".repeat(64);
            assert!(cache
                .upload_edit_retained(&mut changed, &p, &mut io, &mut |_| Ok(()))
                .is_err());
            assert_eq!(io.ids.len(), ids);
        }
    }
    #[test]
    fn checkpoint_failure_prevents_admission_bulk_or_finalization() {
        for cut in 0..3 {
            let p = program();
            let mut io = RetainedIo::new();
            let mut intent = RetainedEditUpload::new(
                project(),
                RevisionId::new(1),
                &p,
                "checkpoint edit".into(),
            )
            .unwrap();
            let mut cache = MotionCache::default();
            let result = cache.upload_edit_retained(&mut intent, &p, &mut io, &mut |state| {
                if (cut == 0 && state.lease.is_none())
                    || (cut == 1 && state.lease.is_some())
                    || (cut == 2 && state.finalization_attempted)
                {
                    bail!("checkpoint publication failed");
                }
                Ok(())
            });
            assert!(result.is_err());
            assert!(!io
                .ids
                .iter()
                .any(|(_, c)| matches!(c, Command::FinishEditUpload { .. })));
            if cut == 0 {
                assert!(io.ids.is_empty());
            }
            if cut < 2 {
                assert_eq!(io.inner.opens, 0);
            }
        }
    }

    #[test]
    fn retained_upload_rejects_invalid_progress_before_bulk() {
        let p = program();
        let mut io = RetainedIo::new();
        io.invalid_prefix = true;
        let mut intent =
            RetainedEditUpload::new(project(), RevisionId::new(1), &p, "retained edit".into())
                .unwrap();
        assert!(MotionCache::default()
            .upload_edit_retained(&mut intent, &p, &mut io, &mut |_| Ok(()))
            .is_err());
        assert_eq!(io.inner.opens, 0);
        assert!(!intent.finalization_attempted);
    }

    #[test]
    fn complete_digest_verified_program_is_cached_and_reused() {
        let mut io = MockIo::new();
        let descriptor = io.descriptor();
        let mut cache = MotionCache::default();
        let first = cache.program(&descriptor, &binding(), &mut io).unwrap();
        let second = cache.program(&descriptor, &binding(), &mut io).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(io.opens, 1);
    }
    #[test]
    fn descriptor_wrong_authority_rejects_before_transfer_or_cache_lookup() {
        let mut io = MockIo::new();
        let descriptor = io.descriptor();
        let mut cache = MotionCache::default();
        cache.program(&descriptor, &binding(), &mut io).unwrap();
        let mut bad = descriptor.clone();
        bad.binding = MotionBinding::ProjectRevision {
            project_id: ProjectId::new("other").unwrap(),
            revision: RevisionId::new(1),
        };
        assert!(cache.program(&bad, &binding(), &mut io).is_err());
        assert_eq!(io.opens, 1);
    }
    #[test]
    fn bad_hash_truncated_data_and_false_summaries_are_never_cached() {
        for fault in 0..3 {
            let mut io = MockIo::new();
            let mut descriptor = io.descriptor();
            let mut cache = MotionCache::default();
            if fault == 0 {
                io.corrupt = true;
            }
            if fault == 1 {
                io.truncated = true;
            }
            if fault == 2 {
                descriptor.program.axes[0].action_count += 1;
            }
            assert!(cache.program(&descriptor, &binding(), &mut io).is_err());
            assert!(cache.entries.is_empty());
            assert!(io
                .commands
                .iter()
                .any(|command| matches!(command, Command::AbandonTransfer { .. })));
        }
    }
    #[test]
    fn redirected_bulk_endpoint_never_receives_authentication() {
        let mut io = MockIo::new();
        io.redirect = true;
        let descriptor = io.descriptor();
        let mut cache = MotionCache::default();
        assert!(cache.program(&descriptor, &binding(), &mut io).is_err());
        assert_eq!(io.opens, 0);
    }
    #[test]
    fn handshake_cannot_change_grant_binding_epoch_or_expiry() {
        let io = MockIo::new();
        let descriptor = io.descriptor();
        let lease = io.lease(
            TransferDirection::Download,
            descriptor.program.byte_len,
            descriptor.program.sha256,
        );
        for mutation in 0..4 {
            let mut reply = lease.clone();
            match mutation {
                0 => reply.engine_epoch = "other-epoch".into(),
                1 => reply.project_id = ProjectId::new("other").unwrap(),
                2 => reply.expires_after_ms += 1,
                _ => reply.accepted_prefix = 1,
            }
            assert!(validate_handshake(&lease, &reply).is_err());
        }
    }
    #[test]
    fn bad_upload_ack_never_finalizes_and_wire_values_contain_no_evidence() {
        let mut io = MockIo::new();
        io.bad_ack = true;
        let mut cache = MotionCache::default();
        assert!(cache
            .upload_edit(project(), RevisionId::new(1), &program(), "Edit", &mut io)
            .is_err());
        assert!(!io
            .commands
            .iter()
            .any(|command| matches!(command, Command::FinishEditUpload { .. })));
        let text = String::from_utf8(io.uploaded.borrow().clone()).unwrap();
        assert!(!text.contains("evidence"));
        assert!(!text.contains("observed"));
        assert!(!text.contains("provenance"));
        let _: EditValuesProgram = serde_json::from_str(&text).unwrap();
    }
    #[test]
    fn successful_shared_edit_upload_returns_candidate_without_a_commit() {
        let mut io = MockIo::new();
        let mut cache = MotionCache::default();
        let candidate = cache
            .upload_edit(project(), RevisionId::new(1), &program(), "Edit", &mut io)
            .unwrap();
        assert_eq!(candidate.base_revision, RevisionId::new(1));
        assert_eq!(candidate.program.tracks()[0].actions().len(), 2);
        assert!(!io
            .commands
            .iter()
            .any(|command| matches!(command, Command::CommitCandidate { .. })));
    }
    #[test]
    fn cache_has_an_entry_count_bound_even_for_tiny_programs() {
        let mut io = MockIo::new();
        let mut cache = MotionCache::default();
        for index in 0..12 {
            let mut descriptor = io.descriptor();
            descriptor.program.artifact_id = ArtifactId::new(format!("artifact-{index}")).unwrap();
            cache.program(&descriptor, &binding(), &mut io).unwrap();
        }
        assert!(
            cache.entries.len() <= 8,
            "byte bounds alone do not bound cache metadata"
        );
    }
    #[test]
    fn archive_history_hydration_refuses_foreign_scope_without_motion_transfer() {
        let mut io = MockIo::new();
        let mut cache = MotionCache::default();
        let mut page = wire::WasmArchivePage {
            project_id: project(),
            after: None,
            limit: 32,
            operations: vec![],
            next: None,
        };
        cache
            .hydrate(
                wire::ResponseBody::WasmArchivePage(page.clone()),
                Some(&project()),
                None,
                &mut io,
            )
            .unwrap();
        page.project_id = ProjectId::new("foreign").unwrap();
        assert!(cache
            .hydrate(
                wire::ResponseBody::WasmArchivePage(page),
                Some(&project()),
                None,
                &mut io
            )
            .is_err());
        assert_eq!(io.opens, 0);
        assert!(io.commands.is_empty());
    }
}
