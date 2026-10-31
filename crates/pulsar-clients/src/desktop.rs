//! Pulsar's non-authoritative desktop presentation. No decoder, inference
//! runtime, database, plugin host, or physical-device driver is linked here.

use crate::{
    absolute_path,
    editor::{self, EditGesture, EditorPoint},
    portable::PortablePanel,
    preview::{self, DecodedFrame, PreviewState},
    session::{EngineApi, SessionClient},
};
use eframe::egui::{self, Color32, RichText, Stroke};
#[path = "candidate_evidence.rs"]
mod candidate_evidence;
#[path = "inference_fallback_panel.rs"]
mod inference_fallback_panel;
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Studio,
    Cinema,
    Generator,
    ModelEvidence,
    Browser,
    Library,
    References,
    Doctor,
    Portable,
    Devices,
    Automation,
    Stash,
    Evidence,
    Plugins,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GeneratorInputMode {
    Video,
    Text,
    Patterns,
    Image,
    Audio,
    Funscript,
}
impl GeneratorInputMode {
    fn label(self) -> &'static str {
        match self {
            Self::Video => "Video motion",
            Self::Text => "Constrained pattern text",
            Self::Patterns => "Explicit axis patterns",
            Self::Image => "Still image + pattern prompt",
            Self::Audio => "Audio-derived motion",
            Self::Funscript => "Funscript candidate import",
        }
    }
}

fn select_media_source(
    sources: &[pulsar_protocol::SourceSummary],
    preferred: Option<&SourceVersionId>,
) -> Option<SourceVersionId> {
    sources
        .iter()
        .find(|source| {
            source.kind == pulsar_protocol::SourceKind::Media
                && preferred == Some(&source.source_version)
        })
        .or_else(|| {
            sources
                .iter()
                .find(|source| source.kind == pulsar_protocol::SourceKind::Media)
        })
        .map(|source| source.source_version.clone())
}

/// Snapshot-driven placement/source selection starts at the declared valid
/// anchor. Ordinary refreshes preserve the user's playhead and preview.
fn snapshot_preview_seek(
    current: f64,
    placement: Option<&SourcePlacementSpec>,
    placement_changed: bool,
    source_changed: bool,
) -> Option<f64> {
    if !placement_changed && !source_changed {
        return None;
    }
    Some(
        placement
            .map(|p| p.project_anchor.as_nanos() as f64 / 1e9)
            .unwrap_or(if source_changed { 0.0 } else { current }),
    )
}

fn independent_audio_rate(numerator: &str, denominator: &str) -> anyhow::Result<(i64, u32)> {
    let numerator = numerator.parse::<i64>().map_err(|_| {
        anyhow::anyhow!("Audio clock rate numerator must be a positive 64-bit integer")
    })?;
    let denominator = denominator.parse::<u32>().map_err(|_| {
        anyhow::anyhow!("Audio clock rate denominator must be a nonzero 32-bit integer")
    })?;
    AudioClockRate::new(numerator, denominator)?;
    Ok((numerator, denominator))
}

fn independent_audio_selection(
    sources: &[SourceSummary],
    primary: Option<&SourceVersionId>,
    selected: Option<&SourceVersionId>,
    anchors: [(i64, u32); 2],
    rate: (i64, u32),
) -> anyhow::Result<IndependentAudio> {
    let source_version = selected
        .filter(|selected| Some(*selected) != primary)
        .filter(|selected| {
            sources.iter().any(|source| {
                &source.source_version == *selected && source.kind == SourceKind::Media
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Select a distinct imported media source for audio"))?
        .clone();
    Ok(IndependentAudio {
        source_version,
        alignment: AudioAlignment {
            video_anchor: SourceTimestamp::new(anchors[0].0, anchors[0].1)?,
            audio_anchor: SourceTimestamp::new(anchors[1].0, anchors[1].1)?,
            rate: {
                let checked = AudioClockRate::new(rate.0, rate.1)?;
                (rate.0 != i64::from(rate.1)).then_some(checked)
            },
        },
    })
}

fn diagnostics_match_view(
    report: &DiagnosticsReport,
    project: Option<(&ProjectId, RevisionId)>,
    candidate: Option<&CandidateId>,
) -> bool {
    project.is_some_and(|(id, revision)| id == &report.project_id && revision == report.revision)
        && (report.candidate_id.is_none() || report.candidate_id.as_ref() == candidate)
}

fn accept_diagnostics_report(
    current: &mut Option<DiagnosticsReport>,
    report: DiagnosticsReport,
    project: Option<(&ProjectId, RevisionId)>,
    candidate: Option<&CandidateId>,
) -> bool {
    if !diagnostics_match_view(&report, project, candidate) {
        return false;
    }
    *current = Some(report);
    true
}

fn execute_work(client: &mut impl EngineApi, work: RpcWork) -> anyhow::Result<ResponseBody> {
    match work.command {
        RpcCommand::Identified(id, command) => {
            client.execute_identified(id, command, work.project, work.revision)
        }
        RpcCommand::Wire(command) => client.execute(command, work.project, work.revision),
        RpcCommand::HandyInspect(_) | RpcCommand::HandyRegistration(_) => {
            anyhow::bail!("Handy inspection requires the trusted session-bound owner directory")
        }
        RpcCommand::FallbackGeneration(_) => {
            anyhow::bail!("Fallback generation requires the captured authenticated session")
        }
        RpcCommand::Equivalence(_) => {
            anyhow::bail!("Evidence review requires the trusted session-bound owner directory")
        }
        RpcCommand::DismissPackRecovery(_) => {
            anyhow::bail!("Recovery dismissal requires the session-bound local store")
        }
        RpcCommand::Repair(request) => {
            if work.project.as_ref() != Some(&request.project)
                || work.revision != Some(request.revision)
            {
                anyhow::bail!("Repair work differs from its captured project/revision");
            }
            crate::repair_panel::propose(client, request)
                .map(|candidate| ResponseBody::Candidate(Box::new(candidate)))
        }
        RpcCommand::Edit(proposal) => {
            let project = work
                .project
                .ok_or_else(|| anyhow::anyhow!("Edit draft lacks project identity"))?;
            let revision = work
                .revision
                .ok_or_else(|| anyhow::anyhow!("Edit draft lacks base revision"))?;
            let candidate = client.upload_edit(
                project.clone(),
                revision,
                &proposal.program,
                &proposal.label,
            )?;
            let candidate_id = candidate.candidate_id;
            client.execute(Command::CommitCandidate { candidate_id: candidate_id.clone() },
                Some(project), Some(revision))
                .map_err(|error| {
                    let message = format!("Edit candidate {candidate_id} exists; commit was not confirmed. Explicit review/rebase is required before a new commit. Cause: {error}");
                    error.context(message)
                })
        }
    }
}

fn execute_recoverable_work(
    client: &mut SessionClient,
    directory: &std::path::Path,
    work: RpcWork,
) -> anyhow::Result<ResponseBody> {
    if let RpcCommand::FallbackGeneration(intent) = &work.command {
        let session = inference_fallback_panel::SessionBinding::current(client);
        return intent.execute(client, &session, work.project.as_ref(), work.revision);
    }
    if let RpcCommand::Equivalence(action) = &work.command {
        anyhow::ensure!(
            work.project.is_none() && work.revision.is_none(),
            "Evidence review is global owner scope"
        );
        return crate::equivalence_panel::execute(directory, client, action);
    }
    if let RpcCommand::HandyRegistration(action) = &work.command {
        anyhow::ensure!(
            work.project.is_none() && work.revision.is_none(),
            "Handy registration is not project-scoped"
        );
        return crate::handy_registration_recovery::execute(directory, client, action);
    }
    if let RpcCommand::HandyInspect(inspection) = &work.command {
        anyhow::ensure!(
            work.project.is_none() && work.revision.is_none(),
            "Handy inspection must not carry project edit scope"
        );
        return crate::handy_panel::inspect(client, directory, inspection.clone())
            .map(ResponseBody::HandyInspection);
    }
    let binding = client.recovery_credential_binding();
    if let RpcCommand::DismissPackRecovery(id) = &work.command {
        crate::pack_recovery::dismiss(directory, client.session_id(), &binding, id)?;
        return Ok(ResponseBody::Ack);
    }
    if let RpcCommand::Identified(id, command) = &work.command {
        if matches!(
            command,
            Command::ImportModelPack { .. }
                | Command::SelectModelPack { .. }
                | Command::WithdrawModelPackApproval { .. }
                | Command::RefreshModelCatalog { .. }
                | Command::SelectCatalogModelPack { .. }
        ) {
            return crate::pack_recovery::execute(directory, client, id.clone(), command.clone());
        }
    }
    execute_work(client, work)
}

enum RpcCommand {
    Identified(RequestId, Command),
    Wire(Command),
    Edit(EditProposal),
    Repair(crate::repair_panel::RepairRequest),
    HandyInspect(pulsar_protocol::HandyInspectionRequest),
    HandyRegistration(Box<crate::handy_registration_recovery::Action>),
    DismissPackRecovery(RequestId),
    Equivalence(Box<crate::equivalence_panel::Action>),
    FallbackGeneration(Box<inference_fallback_panel::Intent>),
}

struct RpcWork {
    command: RpcCommand,
    project: Option<ProjectId>,
    revision: Option<RevisionId>,
}
struct RpcReply {
    fallback_session: Option<inference_fallback_panel::SessionBinding>,
    fallback_binding: Option<inference_fallback_panel::Binding>,
    equivalence_action: Option<Box<crate::equivalence_panel::Action>>,
    equivalence_scope: Option<crate::equivalence_panel::Scope>,
    equivalence_recovery: Option<Result<Vec<crate::inference_equivalence_recovery::Entry>, String>>,
    handy_action: Option<Box<crate::handy_registration_recovery::Action>>,
    handy_recovery: Option<Result<Vec<crate::handy_registration_recovery::RecoveryEntry>, String>>,
    response: Result<ResponseBody, String>,
    repair_binding: Option<(ProjectId, RevisionId)>,
    frame: Option<Result<DecodedFrame, String>>,
    previous_frame: Option<Result<DecodedFrame, String>>,
    recovery: Result<Vec<crate::pack_recovery::PackRecoveryEntry>, String>,
}

/// This contains view state and disposable edit drafts, never authoritative
/// project mutation or device authority. Each window has an independent clock.
pub struct PulsarDesktop {
    sender: SyncSender<RpcWork>,
    receiver: Receiver<RpcReply>,
    busy: bool,
    tab: Tab,
    browser_live_panel: crate::browser_live_panel::BrowserLivePanel,
    browser_close_pending: bool,
    project: Option<ProjectSnapshot>,
    capabilities: Option<Capabilities>,
    wasm_panel: crate::wasm_panel::WasmPanel,
    native_preparation_panel: crate::native_preparation_panel::NativePreparationPanel,
    assistant_panel: crate::assistant_panel::AssistantPanel,
    inference_catalog: Option<InferenceCatalog>,
    job: Option<JobSnapshot>,
    job_attempts: Option<JobAttemptPage>,
    candidate: Option<CandidateSnapshot>,
    source: Option<SourceVersionId>,
    selected_placement: Option<SourcePlacementId>,
    placement_times: [f64; 4],
    placement_rate: (i32, u32),
    placement_save_pending: bool,
    retime_from: String,
    retime_axes: Vec<Axis>,
    retime_status_revision: u64,
    retime_receipt: Option<(String, String)>,
    archived_generation: Option<pulsar_protocol::ArchivedGenerationReport>,
    candidate_evidence: candidate_evidence::SummaryCache,
    candidate_image_origin: Option<CandidateImageOriginReport>,
    candidate_image_pixels: Option<(CandidateImageOriginPixels, egui::TextureHandle)>,
    pattern_reviews: crate::pattern_review::PatternReviews,
    pending_image_record: Option<String>,
    preview: PreviewState,
    texture: Option<egui::TextureHandle>,
    previous_texture: Option<egui::TextureHandle>,
    preview_pending: bool,
    request_generation: u64,
    seek_generation: u64,
    playhead_seconds: f64,
    preview_playing: bool,
    last_tick: Instant,
    last_preview: Instant,
    last_job_poll: Instant,
    project_name: String,
    open_project_id: String,
    source_path: String,
    export_path: String,
    model_path: String,
    pack_panel: crate::pack_panel::PackPanel,
    equivalence_panel: crate::equivalence_panel::EquivalencePanel,
    bundled_models: crate::bundled_models::BundledModelPanel,
    assistant_settings: crate::assistant_settings::AssistantSettingsPanel,
    evidence_panel: crate::evidence_panel::EvidencePanel,
    use_selected_pack: bool,
    fallback_panel: inference_fallback_panel::FallbackPanel,
    fallback_session: Option<inference_fallback_panel::SessionBinding>,
    inference_profile_path: String,
    dynamics_profile_path: String,
    inference_profile: Option<DeclaredInferenceProfile>,
    annotated_view_path: String,
    annotated_view: Option<AnnotatedGenerationView>,
    author_target_mode: bool,
    target_author: crate::target_view_authoring::TargetViewAuthor,
    authored_target_id: String,
    video_view: Option<VideoView>,
    model_classes: u32,
    model_width: u32,
    model_height: u32,
    use_model: bool,
    use_detector_score_threshold: bool,
    detector_score_threshold: f32,
    use_analysis_dimensions: bool,
    analysis_dimensions: [u32; 2],
    use_generation_interval: bool,
    generation_interval_ns: [i64; 2],
    use_target_seed: bool,
    target_seed_frame: u64,
    target_seed_point: [f64; 2],
    target_seed_filter_class: bool,
    target_seed_class: u32,
    target_seed_binding: Option<(SourceVersionId, TransformId)>,
    pick_target_mode: bool,
    target_picker: crate::target_picker::TargetPicker,
    preset: String,
    use_mixed_audio: bool,
    use_independent_audio: bool,
    independent_audio_source: Option<SourceVersionId>,
    audio_alignment_anchors: [(i64, u32); 2],
    audio_clock_rate: (String, String),
    creative_audio: bool,
    status: String,
    startup_warning: Option<String>,
    new_position: f64,
    gesture: Option<EditGesture>,
    timeline_axis: Axis,
    generation_mode: GeneratorInputMode,
    pattern_prompt: String,
    audio_panel: crate::audio_panel::AudioPanel,
    script_import_path: String,
    repair_panel: crate::repair_panel::RepairPanel,
    handy_panel: crate::handy_panel::HandyPanel,
    handy_registration_panel: crate::handy_registration_panel::HandyRegistrationPanel,
    diagnostics: Option<DiagnosticsReport>,
    segment_speed_limit: f64,
    review_status: Option<ReviewStatus>,
    review_note: String,
    review_partial: bool,
    review_interval_ns: (i64, i64),
    review_retry: Option<(RequestId, Command, ProjectId, RevisionId)>,
    merge_axes: Vec<Axis>,
    merge_range_enabled: bool,
    merge_start_seconds: f64,
    merge_end_seconds: f64,
    portable: PortablePanel,
    owner_directory: PathBuf,
    stash_endpoint: String,
    stash_credential_env: String,
    stash_discovery: Option<StashDiscovery>,
    stash_mappings_path: String,
    stash_annotated_views_path: String,
    stash_source_plans_path: String,
    stash_plan: Option<StashPlan>,
    stash_capture: Option<StashSourceCapture>,
    stash_export: Option<StashScriptExport>,
    stash_source_time: bool,
    stash_use_archived: bool,
    stash_archived_namespace: String,
    stash_require_selected_source: bool,
    stash_publication_request: String,
    stash_publication_report: Option<String>,
    stash_scan_request: String,
    stash_scan: Option<StashScanReceipt>,
    stash_tag_name: String,
    stash_create_tag: bool,
    stash_tag_request: String,
    stash_tag_resume_request: String,
    stash_tag: Option<StashTagReceipt>,
    library_panel: crate::library_panel::LibraryPanel,
    reference_panel: crate::reference_panel::ReferencePanel,
    batch_endpoint: PathBuf,
    batch_bootstrap: PathBuf,
    stash_batch: Option<Receiver<Result<crate::stash_flow::BatchReport, String>>>,
    stash_batch_report: Option<String>,
    stash_batch_id: String,
    stash_batch_offset: u32,
    stash_batch_has_more: bool,
    stash_batch_item_count: u32,
    stash_batch_scan: bool,
    stash_batch_tag_requested: bool,
}

impl PulsarDesktop {
    pub(crate) fn configure_assistant_host(&mut self, service: crate::AssistantHostService) {
        self.assistant_settings.configure(service);
    }
    pub(crate) fn configure_bundled_discovery(&mut self, discover: crate::BundledModelDiscovery) {
        self.bundled_models.configure(discover);
    }
    pub(crate) fn set_startup_warning(&mut self, warning: Option<String>) {
        // Bound display storage independently of error-chain or filesystem path size.
        self.startup_warning = warning.map(|message| message.chars().take(4096).collect());
    }

    pub fn new(
        context: &eframe::CreationContext<'_>,
        endpoint: PathBuf,
        bootstrap: PathBuf,
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(17, 21, 29);
        visuals.window_fill = Color32::from_rgb(21, 27, 37);
        visuals.selection.bg_fill = Color32::from_rgb(0, 99, 113);
        visuals.selection.stroke.color = Color32::from_rgb(69, 222, 230);
        context.egui_ctx.set_visuals(visuals);
        let (sender, requests) = mpsc::sync_channel::<RpcWork>(8);
        let (replies, receiver) = mpsc::sync_channel(8);
        let repaint = context.egui_ctx.clone();
        let native_preparation_panel = crate::native_preparation_panel::NativePreparationPanel::new(
            endpoint.clone(),
            bootstrap.clone(),
            context.egui_ctx.clone(),
        );
        let assistant_panel = crate::assistant_panel::AssistantPanel::new(
            endpoint.clone(),
            bootstrap.clone(),
            context.egui_ctx.clone(),
        );
        let wasm_panel = crate::wasm_panel::WasmPanel::new(
            endpoint.clone(),
            bootstrap.clone(),
            context.egui_ctx.clone(),
        );
        let evidence_panel = crate::evidence_panel::EvidencePanel::new(
            endpoint.clone(),
            bootstrap.clone(),
            context.egui_ctx.clone(),
        );
        let portable = PortablePanel::new(
            endpoint.clone(),
            bootstrap.clone(),
            context.egui_ctx.clone(),
        );
        let browser_live_panel =
            crate::browser_live_panel::BrowserLivePanel::new(endpoint.clone(), bootstrap.clone());
        let owner_directory = bootstrap
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        let batch_endpoint = endpoint.clone();
        let batch_bootstrap = bootstrap.clone();
        let recovery_directory = owner_directory.clone();
        std::thread::spawn(move || {
            let mut client: Option<SessionClient> = None;
            while let Ok(work) = requests.recv() {
                let equivalence_action = match &work.command {
                    RpcCommand::Equivalence(action) => Some(action.clone()),
                    _ => None,
                };
                let handy_action = match &work.command {
                    RpcCommand::HandyRegistration(action) => Some(action.clone()),
                    _ => None,
                };
                let fallback_binding = match &work.command {
                    RpcCommand::FallbackGeneration(intent) => Some(intent.binding().clone()),
                    _ => None,
                };
                let repair_binding = matches!(&work.command, RpcCommand::Repair(_))
                    .then(|| work.project.clone().zip(work.revision))
                    .flatten();
                let response = (|| -> anyhow::Result<ResponseBody> {
                    if client.is_none() {
                        client = Some(SessionClient::connect(&endpoint, &bootstrap)?);
                    }
                    let client = client.as_mut().expect("client established");
                    execute_recoverable_work(client, &recovery_directory, work)
                })()
                .map_err(|error| error.to_string());
                let frame = match &response {
                    Ok(ResponseBody::CandidateImageOriginPixels(pixels)) => {
                        Some(crate::image_origin_pixels::decode(pixels).map_err(|e| e.to_string()))
                    }
                    Ok(ResponseBody::Preview(result)) => result.frame.as_ref().map(|frame| {
                        preview::decode_frame(frame).map_err(|error| error.to_string())
                    }),
                    Ok(ResponseBody::ModelPackPreview(result)) => {
                        result.preview.frame.as_ref().map(|frame| {
                            preview::decode_frame(frame).map_err(|error| error.to_string())
                        })
                    }
                    _ => None,
                };
                let recovery = client
                    .as_ref()
                    .ok_or_else(|| "Session is not connected".to_owned())
                    .and_then(|client| {
                        crate::pack_recovery::load(
                            &recovery_directory,
                            client.session_id(),
                            &client.recovery_credential_binding(),
                        )
                        .map_err(|e| e.to_string())
                    });
                let equivalence_scope = equivalence_action.as_ref().and_then(|_| {
                    client
                        .as_ref()
                        .map(crate::equivalence_panel::Scope::current)
                });
                let equivalence_recovery = equivalence_action.as_ref().map(|_| {
                    client
                        .as_ref()
                        .ok_or_else(|| "Session is not connected".to_owned())
                        .and_then(|client| {
                            crate::inference_equivalence_recovery::load(
                                &recovery_directory,
                                client.session_id(),
                                &client.recovery_credential_binding(),
                            )
                            .map_err(|error| error.to_string())
                        })
                });
                let handy_recovery = handy_action.as_ref().map(|_| {
                    client
                        .as_ref()
                        .ok_or_else(|| "Session is not connected".to_string())
                        .and_then(|client| {
                            crate::handy_registration_recovery::load(
                                &recovery_directory,
                                client.session_id(),
                                &client.recovery_credential_binding(),
                            )
                            .map_err(|error| error.to_string())
                        })
                });
                let previous_frame = match &response {
                    Ok(ResponseBody::Preview(result)) => result.annotated.as_ref(),
                    Ok(ResponseBody::ModelPackPreview(result)) => result.preview.annotated.as_ref(),
                    _ => None,
                }
                .and_then(|e| e.previous.as_ref())
                .map(|p| preview::decode_frame(&p.frame).map_err(|e| e.to_string()));
                if replies
                    .send(RpcReply {
                        fallback_session: client
                            .as_ref()
                            .map(inference_fallback_panel::SessionBinding::current),
                        fallback_binding,
                        equivalence_action,
                        equivalence_scope,
                        equivalence_recovery,
                        handy_action,
                        handy_recovery,
                        response,
                        repair_binding,
                        frame,
                        previous_frame,
                        recovery,
                    })
                    .is_err()
                {
                    break;
                }
                repaint.request_repaint();
            }
        });
        let mut app = Self {
            sender,
            receiver,
            busy: false,
            tab: Tab::Studio,
            browser_live_panel,
            browser_close_pending: false,
            project: None,
            capabilities: None,
            inference_catalog: None,
            owner_directory,
            stash_endpoint: "http://localhost:9999/graphql".into(),
            stash_credential_env: String::new(),
            stash_discovery: None,
            stash_mappings_path: String::new(),
            stash_annotated_views_path: String::new(),
            stash_source_plans_path: String::new(),
            stash_plan: None,
            stash_capture: None,
            stash_export: None,
            stash_source_time: false,
            stash_use_archived: false,
            stash_archived_namespace: String::new(),
            stash_require_selected_source: false,
            stash_publication_request: String::new(),
            stash_publication_report: None,
            stash_scan_request: String::new(),
            stash_scan: None,
            stash_tag_name: "Pulsar-Generated".into(),
            stash_create_tag: false,
            stash_tag_request: String::new(),
            stash_tag_resume_request: String::new(),
            stash_tag: None,
            library_panel: crate::library_panel::LibraryPanel::new(
                batch_endpoint.clone(),
                batch_bootstrap.clone(),
            ),
            reference_panel: Default::default(),
            batch_endpoint,
            batch_bootstrap,
            stash_batch: None,
            stash_batch_report: None,
            stash_batch_id: String::new(),
            stash_batch_offset: 0,
            stash_batch_has_more: false,
            stash_batch_item_count: 0,
            stash_batch_scan: false,
            stash_batch_tag_requested: false,
            job: None,
            job_attempts: None,
            candidate: None,
            source: None,
            selected_placement: None,
            placement_times: [0.0, 1.0, 0.0, 0.0],
            placement_rate: (1, 1),
            placement_save_pending: false,
            retime_from: String::new(),
            retime_axes: vec![Axis::Stroke],
            retime_status_revision: 0,
            retime_receipt: None,
            archived_generation: None,
            candidate_evidence: candidate_evidence::SummaryCache::default(),
            candidate_image_origin: None,
            candidate_image_pixels: None,
            pattern_reviews: Default::default(),
            pending_image_record: None,
            preview: PreviewState::default(),
            texture: None,
            previous_texture: None,
            preview_pending: false,
            request_generation: 0,
            seek_generation: 0,
            playhead_seconds: 0.0,
            preview_playing: false,
            last_tick: Instant::now(),
            last_preview: Instant::now(),
            last_job_poll: Instant::now(),
            project_name: "Untitled motion".into(),
            open_project_id: String::new(),
            source_path: String::new(),
            export_path: String::new(),
            model_path: String::new(),
            pack_panel: Default::default(),
            equivalence_panel: Default::default(),
            bundled_models: Default::default(),
            assistant_settings: Default::default(),
            evidence_panel,
            wasm_panel,
            native_preparation_panel,
            assistant_panel,
            use_selected_pack: false,
            fallback_panel: Default::default(),
            fallback_session: None,
            inference_profile_path: String::new(),
            dynamics_profile_path: String::new(),
            inference_profile: None,
            annotated_view_path: String::new(),
            annotated_view: None,
            author_target_mode: false,
            target_author: Default::default(),
            authored_target_id: "selected-target".into(),
            video_view: None,
            model_classes: 1,
            model_width: 640,
            model_height: 640,
            use_model: false,
            use_detector_score_threshold: false,
            detector_score_threshold: 0.25,
            use_analysis_dimensions: false,
            analysis_dimensions: [320, 180],
            use_generation_interval: false,
            generation_interval_ns: [0, 1_000_000_000],
            use_target_seed: false,
            target_seed_frame: 0,
            target_seed_point: [0.5, 0.5],
            target_seed_filter_class: false,
            target_seed_class: 0,
            target_seed_binding: None,
            pick_target_mode: false,
            target_picker: Default::default(),
            preset: "default".into(),
            use_mixed_audio: false,
            use_independent_audio: false,
            independent_audio_source: None,
            audio_alignment_anchors: [(0, 1), (0, 1)],
            audio_clock_rate: ("1".into(), "1".into()),
            creative_audio: false,
            status: "Connecting to the per-user engine...".into(),
            startup_warning: None,
            new_position: 0.5,
            gesture: None,
            timeline_axis: Axis::Stroke,
            generation_mode: GeneratorInputMode::Video,
            pattern_prompt:
                "pattern=sine axis=stroke duration=5s frequency=1hz amplitude=0.2 offset=0.5".into(),
            audio_panel: Default::default(),
            script_import_path: String::new(),
            repair_panel: Default::default(),
            handy_panel: Default::default(),
            handy_registration_panel: Default::default(),
            diagnostics: None,
            segment_speed_limit: 450.0,
            review_status: None,
            review_note: String::new(),
            review_partial: false,
            review_interval_ns: (0, 1),
            review_retry: None,
            merge_axes: vec![Axis::Stroke],
            merge_range_enabled: false,
            merge_start_seconds: 0.0,
            merge_end_seconds: 5.0,
            portable,
        };
        app.send(Command::Capabilities, None, None);
        app
    }

    fn send(
        &mut self,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> bool {
        self.enqueue(RpcCommand::Wire(command), project, revision)
    }

    fn send_edit(
        &mut self,
        proposal: EditProposal,
        project: ProjectId,
        revision: RevisionId,
    ) -> bool {
        self.enqueue(RpcCommand::Edit(proposal), Some(project), Some(revision))
    }

    fn enqueue(
        &mut self,
        command: RpcCommand,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> bool {
        if self.busy {
            self.status = "An engine request is pending; no duplicate was submitted.".into();
            return false;
        }
        match self.sender.try_send(RpcWork {
            command,
            project,
            revision,
        }) {
            Ok(()) => {
                self.busy = true;
                true
            }
            Err(error) => {
                self.status = format!("Client request queue unavailable: {error}");
                false
            }
        }
    }

    fn project_command(&mut self, command: Command, revision_required: bool) -> bool {
        let Some(project) = &self.project else {
            self.status = "Create or open a project first.".into();
            return false;
        };
        self.send(
            command,
            Some(project.project_id.clone()),
            revision_required.then_some(project.revision),
        )
    }

    fn poll(&mut self, context: &egui::Context) {
        if let Some(receiver) = &self.stash_batch {
            match receiver.try_recv() {
                Ok(result) => {
                    self.stash_batch = None;
                    match result {
                        Ok(report) => {
                            self.stash_batch_offset = 0;
                            self.stash_batch_has_more = false;
                            self.stash_batch_item_count = report.item_count;
                            self.status=if report.has_failures() {"Stash batch has failed or unknown outcomes. Reconcile retained IDs before retrying."} else {"Stash batch finished. Published scripts remain unqualified."}.into();
                            self.stash_batch_report = Some(
                                serde_json::to_string_pretty(&report)
                                    .unwrap_or_else(|e| e.to_string()),
                            );
                        }
                        Err(error) => self.status = error,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.stash_batch = None;
                    self.status="Stash controller disconnected; engine jobs may still be running. Reconcile outcomes before retrying.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        while let Ok(reply) = self.receiver.try_recv() {
            self.busy = false;
            self.fallback_session = reply.fallback_session;
            let fallback_current = self.fallback_binding();
            self.fallback_panel.sync(fallback_current.clone());
            if reply
                .fallback_binding
                .as_ref()
                .is_some_and(|binding| Some(binding) != fallback_current.as_ref())
            {
                self.status = match &reply.response {
                    Ok(ResponseBody::Job(job)) => format!("Job {} was submitted for the earlier selection in project {}. Open that project to inspect it.", job.job_id, job.project_id),
                    Err(error) => format!("Fallback submission for the earlier selection was not confirmed: {error}"),
                    _ => "Fallback response belongs to an earlier selection; inspect its original project explicitly.".into(),
                };
                continue;
            }
            match reply.recovery {
                Ok(entries) => self.pack_panel.restore_recovery(entries),
                Err(error) => {
                    self.pack_panel.message = format!("Recovery storage unavailable: {error}")
                }
            }
            if let Some(action) = &reply.equivalence_action {
                let missing = Err("Evidence recovery observation is absent".to_owned());
                self.equivalence_panel.finish(
                    action,
                    reply.equivalence_scope,
                    &reply.response,
                    reply.equivalence_recovery.as_ref().unwrap_or(&missing),
                );
                self.status = self.equivalence_panel.message.clone();
                continue;
            }
            if let Some(action) = &reply.handy_action {
                let missing = Err("Handy recovery observation is absent".to_string());
                self.handy_registration_panel.finish(
                    action,
                    &reply.response,
                    reply.handy_recovery.as_ref().unwrap_or(&missing),
                );
                self.status = self.handy_registration_panel.status().to_string();
                continue;
            }
            match reply.response {
                Err(error) => {
                    self.pack_panel.failed();
                    self.placement_save_pending = false;
                    self.status = error;
                }
                Ok(ResponseBody::Capabilities(capabilities)) => {
                    self.startup_warning = None;
                    self.status =
                        "Engine connected. Qualification status is shown explicitly.".into();
                    self.capabilities = Some(capabilities);
                }
                Ok(ResponseBody::ModelCatalog(report)) => {
                    match self.pack_panel.accept_catalog(report) {
                        Ok(refresh) => {
                            self.status = self.pack_panel.message.clone();
                            if refresh {
                                self.submit_pack_action(
                                    crate::pack_panel::PackAction::CatalogStatus,
                                );
                            }
                        }
                        Err(error) => {
                            self.pack_panel.failed();
                            self.status = error.to_string();
                        }
                    }
                }
                Ok(ResponseBody::ModelPackCatalog(report)) => {
                    match self.pack_panel.accept_association(*report) {
                        Ok(()) => self.status = self.pack_panel.message.clone(),
                        Err(error) => {
                            self.pack_panel.failed();
                            self.status = error.to_string();
                        }
                    }
                }
                Ok(ResponseBody::ModelPackWithdrawal(report)) => {
                    match self.pack_panel.accept_withdrawal(*report) {
                        Ok(refresh) => {
                            self.status = self.pack_panel.message.clone();
                            self.seek(self.playhead_seconds);
                            if refresh {
                                self.submit_pack_action(crate::pack_panel::PackAction::Refresh);
                            }
                        }
                        Err(error) => {
                            self.pack_panel.failed();
                            self.status = error.to_string();
                        }
                    }
                }
                Ok(ResponseBody::ModelPackSelection(report)) => {
                    match self.pack_panel.accept_selection(*report) {
                        Ok(refresh) => {
                            self.status = self.pack_panel.message.clone();
                            self.seek(self.playhead_seconds);
                            if refresh {
                                self.submit_pack_action(crate::pack_panel::PackAction::Refresh);
                            }
                        }
                        Err(error) => {
                            self.pack_panel.failed();
                            self.status = error.to_string();
                        }
                    }
                }
                Ok(ResponseBody::ModelPack(report)) => {
                    match self.pack_panel.accept_integrity(report) {
                        Ok(refresh) => {
                            self.status = self.pack_panel.message.clone();
                            if refresh {
                                self.submit_pack_action(crate::pack_panel::PackAction::Refresh);
                            }
                        }
                        Err(error) => {
                            self.pack_panel.failed();
                            self.status = error.to_string();
                        }
                    }
                }
                Ok(ResponseBody::InferenceCatalog(catalog)) => {
                    self.status = catalog.limitation.clone();
                    self.inference_catalog = Some(catalog);
                }
                Ok(ResponseBody::StashPlan(plan)) => {
                    self.status="Stash local-file plan refreshed. Ready files still require an immutable source snapshot before generation.".into();
                    self.stash_discovery = Some(plan.discovery.clone());
                    self.stash_plan = Some(plan);
                }
                Ok(ResponseBody::StashDiscovery(discovery)) => {
                    self.stash_plan = None;
                    self.status=format!("Stash returned {} missing-script scenes; pagination {}. Local files and scripts still require validation.",discovery.scenes.len(),if discovery.pagination_complete {"complete"} else {"limited"});
                    self.stash_discovery = Some(discovery);
                }
                Ok(ResponseBody::Project(project)) => {
                    let prior_source = self.source.clone();
                    let changed_project = self
                        .project
                        .as_ref()
                        .is_none_or(|old| old.project_id != project.project_id);
                    self.status = format!(
                        "Durable project revision {} acknowledged.",
                        project.revision.value()
                    );
                    self.open_project_id = project.project_id.to_string();
                    if changed_project {
                        self.source = select_media_source(&project.sources, None);
                        self.stash_capture = None;
                        self.stash_export = None;
                        self.candidate = None;
                        self.job = None;
                        self.job_attempts = None;
                        self.gesture = None;
                        self.seek(0.0);
                    } else if self.source.is_none() {
                        self.source = select_media_source(&project.sources, None);
                    }
                    self.diagnostics = None;
                    self.review_status = None;
                    if changed_project {
                        self.review_retry = None;
                    }
                    self.source = select_media_source(&project.sources, self.source.as_ref());
                    let prior_placement = self.selected_placement.clone();
                    if changed_project {
                        self.selected_placement = None;
                        self.retime_from.clear();
                        self.retime_status_revision = project.revision.value();
                        self.retime_receipt = None;
                    }
                    if self.placement_save_pending {
                        self.selected_placement = project
                            .placements
                            .iter()
                            .find(|p| p.created_revision == project.revision)
                            .map(|p| p.placement_id.clone());
                        self.placement_save_pending = false;
                    }
                    if self
                        .selected_placement
                        .as_ref()
                        .is_some_and(|id| !project.placements.iter().any(|p| &p.placement_id == id))
                    {
                        self.selected_placement = None;
                    }
                    if let Some(placed) = self
                        .selected_placement
                        .as_ref()
                        .and_then(|id| project.placements.iter().find(|p| &p.placement_id == id))
                    {
                        self.source = Some(placed.spec.source_version.clone());
                    }
                    let seek = snapshot_preview_seek(
                        self.playhead_seconds,
                        self.selected_placement
                            .as_ref()
                            .and_then(|id| {
                                project.placements.iter().find(|p| &p.placement_id == id)
                            })
                            .map(|p| &p.spec),
                        prior_placement != self.selected_placement,
                        changed_project || prior_source != self.source,
                    );
                    self.project = Some(*project);
                    if let Some(seconds) = seek {
                        self.seek(seconds);
                    }
                    self.project_command(
                        Command::Diagnostics {
                            candidate_id: None,
                            maximum_speed_percent_per_second: None,
                        },
                        false,
                    );
                    self.project_command(Command::ReviewStatus, false);
                    if self.preview.displayed().is_none() {
                        self.preview_pending = self.has_media_source();
                    }
                    if !self.has_media_source() {
                        self.preview_playing = false;
                        self.preview_pending = false;
                        self.preview.clear();
                        self.texture = None;
                        self.previous_texture = None;
                    }
                }
                Ok(ResponseBody::StashSourceCaptured(receipt)) => {
                    self.source = Some(receipt.source_version.clone());
                    self.seek(0.0);
                    self.status=format!("Stash source captured for scene {}. Script destination remains unpublished: {}",receipt.scene_id,receipt.script_destination.display());
                    self.stash_capture = Some(*receipt);
                    self.stash_export = None;
                    self.project_command(Command::GetSnapshot, false);
                }
                Ok(ResponseBody::StashBatch(report)) => {
                    self.stash_batch_id = report.batch_request_id.to_string();
                    self.stash_batch_offset = report.offset;
                    self.stash_batch_has_more = report.has_more;
                    self.stash_batch_item_count = report.item_count;
                    self.status = format!(
                        "Engine batch {}: {}. Showing {} of {} items.",
                        report.batch_request_id,
                        report.state,
                        report.items.len(),
                        report.item_count
                    );
                    self.stash_batch_report = Some(
                        serde_json::to_string_pretty(&report).unwrap_or_else(|e| e.to_string()),
                    );
                }
                Ok(ResponseBody::StashTag(receipt)) => {
                    self.status=format!("Stash tag {:?}; existing scene tags were not replaced. Script qualification remains unchanged.",receipt.state);
                    self.stash_tag_request = receipt.request_id.to_string();
                    self.stash_tag = Some(*receipt);
                }
                Ok(ResponseBody::StashScan(receipt)) => {
                    self.status=format!("Stash scan {:?}. Scan completion and attachment are separate observations; scripts remain unqualified.",receipt.state);
                    self.stash_scan_request = receipt.request_id.to_string();
                    self.stash_scan = Some(*receipt);
                }
                Ok(ResponseBody::StashPublication(observation)) => {
                    self.status=format!("Publication {:?}; proof {:?}; source/script {:?}. No dispatch was performed.",observation.dispatch,observation.proof_observation,observation.source_and_script_observation);
                    self.stash_publication_report = Some(
                        serde_json::to_string_pretty(&observation)
                            .unwrap_or_else(|e| e.to_string()),
                    );
                }
                Ok(ResponseBody::StashScriptExported(receipt)) => {
                    self.status=format!("Unqualified script published without overwrite: {}. Unresolved reviews: {}",receipt.path.display(),receipt.unresolved_review_count);
                    self.stash_export = Some(*receipt);
                }
                Ok(ResponseBody::Source { source_version }) => {
                    self.stash_capture = None;
                    self.stash_export = None;
                    self.source = Some(source_version);
                    self.seek(0.0);
                    self.status = "Immutable source imported. Refreshing project revision.".into();
                    self.project_command(Command::GetSnapshot, false);
                }
                Ok(ResponseBody::Job(job)) => {
                    if self.job_attempts.as_ref().is_some_and(|page| {
                        page.job_id != job.job_id || page.current_attempt != job.attempt_id
                    }) {
                        self.job_attempts = None;
                    }
                    self.status = job
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("Job {}: {}", job.job_id, job.state));
                    self.job = Some(job);
                }
                Ok(ResponseBody::JobAttempts(page)) => {
                    if page.validate().is_ok()
                        && self
                            .project
                            .as_ref()
                            .is_some_and(|p| p.project_id == page.project_id)
                        && self.job.as_ref().is_some_and(|j| {
                            j.job_id == page.job_id && j.attempt_id == page.current_attempt
                        })
                    {
                        self.job_attempts = Some(*page);
                    } else {
                        self.job_attempts = None;
                        self.status="Attempt history changed or belongs to another selection; refresh the job before inspecting it.".into();
                    }
                }
                Ok(ResponseBody::Candidate(candidate)) => {
                    if let Some(binding) = &reply.repair_binding {
                        if !crate::repair_panel::reply_matches(
                            self.project.as_ref().map(|p| (&p.project_id, p.revision)),
                            binding,
                        ) {
                            self.status=format!("Repair candidate {} belongs to captured project {} revision {}; current view changed. Inspect it explicitly before further action.",candidate.candidate_id,binding.0,binding.1);
                            continue;
                        }
                    }
                    self.status =
                        "Candidate is available for review; project remains unchanged.".into();
                    self.diagnostics = None;
                    self.candidate = Some(*candidate);
                }
                Ok(ResponseBody::Preview(result)) => {
                    self.display_preview(*result, reply.frame, reply.previous_frame, context)
                }
                Ok(ResponseBody::ModelPackPreview(report)) => {
                    if self.use_model
                        && self.use_selected_pack
                        && self.pack_panel.matches_preview(&report)
                    {
                        self.display_preview(
                            report.preview,
                            reply.frame,
                            reply.previous_frame,
                            context,
                        );
                    } else {
                        self.status =
                            "Discarded preview for a different pack selection, model or profile."
                                .into();
                    }
                }
                Ok(ResponseBody::MotionRetimeReceipt(receipt)) => {
                    if self
                        .project
                        .as_ref()
                        .is_some_and(|p| p.project_id == receipt.project_id)
                        && receipt.revision.value() == self.retime_status_revision
                    {
                        self.retime_receipt = Some((
                            format!(
                                "Local retime: revision {} → {}; recorded editor {}.",
                                receipt.base_revision.value(),
                                receipt.revision.value(),
                                receipt.actor
                            ),
                            serde_json::to_string_pretty(&receipt)
                                .unwrap_or_else(|e| e.to_string()),
                        ));
                        self.status = "Verified local retiming facts.".into();
                    } else {
                        self.status =
                            "Discarded retiming facts for another project or revision.".into();
                    }
                }
                Ok(ResponseBody::CandidateImageOriginPixels(pixels)) => {
                    let selected = self.pending_image_record.take();
                    self.candidate_image_pixels = None;
                    if pixels.validate().is_ok()
                        && selected.as_deref() == Some(pixels.record.sha256.as_str())
                        && self
                            .project
                            .as_ref()
                            .is_some_and(|p| p.project_id == pixels.report.project_id)
                        && self
                            .candidate
                            .as_ref()
                            .is_some_and(|c| image_origin_scope_matches(&pixels.report, c))
                    {
                        match reply.frame {
                            Some(Ok(frame)) => {
                                let texture = context.load_texture(
                                    "pulsar-retained-conditioning-image",
                                    egui::ColorImage::from_rgba_unmultiplied(
                                        [frame.width, frame.height],
                                        &frame.rgba,
                                    ),
                                    egui::TextureOptions::LINEAR,
                                );
                                self.candidate_image_origin = Some(pixels.report.clone());
                                self.candidate_image_pixels = Some((*pixels, texture));
                                self.status = "Showing exact retained conditioning pixels.".into();
                            }
                            Some(Err(error)) => {
                                self.status = format!("Retained image refused: {error}")
                            }
                            None => self.status = "Retained image pixels were not decoded.".into(),
                        }
                    } else {
                        self.status =
                            "Discarded retained pixels for another candidate or origin.".into();
                    }
                }
                Ok(ResponseBody::CandidateImageOrigin(report)) => {
                    if report.validate().is_ok()
                        && self
                            .project
                            .as_ref()
                            .is_some_and(|p| p.project_id == report.project_id)
                        && self
                            .candidate
                            .as_ref()
                            .is_some_and(|c| image_origin_scope_matches(&report, c))
                    {
                        self.candidate_image_origin = Some(*report);
                        self.status = "Verified retained image generation evidence.".into();
                    } else {
                        self.status =
                            "Discarded image origin evidence for another candidate or motion."
                                .into();
                    }
                }
                Ok(ResponseBody::ArchivedGeneration(report)) => {
                    if self
                        .project
                        .as_ref()
                        .is_some_and(|p| p.project_id == report.project_id)
                        && self
                            .candidate
                            .as_ref()
                            .is_some_and(|c| c.candidate_id == report.candidate_id)
                        && report.master_derivation.as_ref().is_none_or(|m| {
                            self.project.as_ref().is_some_and(|p| {
                                p.revision == m.revision
                                    && p.motion.program.sha256 == m.master.sha256
                            })
                        })
                    {
                        self.archived_generation = Some(*report);
                        self.status = "Verified historical generation facts.".into();
                    } else {
                        self.status =
                            "Discarded generation facts for another project or candidate.".into();
                    }
                }
                Ok(ResponseBody::ArchivedMotionRetimeReceipt(receipt)) => {
                    if self
                        .project
                        .as_ref()
                        .is_some_and(|p| p.project_id == receipt.project_id)
                        && receipt.revision.value() == self.retime_status_revision
                    {
                        self.retime_receipt=Some((format!("Imported retime: revision {} → {}; original editor {}. Attribution is historical; no local edit authority transfers.",receipt.base_revision.value(),receipt.revision.value(),receipt.origin_receipt.actor),serde_json::to_string_pretty(&receipt).unwrap_or_else(|e|e.to_string())));
                        self.status =
                            "Verified imported retiming facts and destination correspondence."
                                .into();
                    } else {
                        self.status =
                            "Discarded imported facts for another project or revision.".into();
                    }
                }
                Ok(ResponseBody::ReviewStatus(report)) => {
                    if self.project.as_ref().is_some_and(|p| {
                        p.project_id == report.project_id && p.revision == report.revision
                    }) && report.validate().is_ok()
                    {
                        self.review_status = Some(report);
                    } else {
                        self.status = "Discarded stale review status".into();
                    }
                }
                Ok(ResponseBody::Diagnostics(report)) => {
                    let accepted = accept_diagnostics_report(
                        &mut self.diagnostics,
                        report,
                        self.project
                            .as_ref()
                            .map(|project| (&project.project_id, project.revision)),
                        self.candidate
                            .as_ref()
                            .map(|candidate| &candidate.candidate_id),
                    );
                    if accepted {
                        let report = self
                            .diagnostics
                            .as_ref()
                            .expect("accepted diagnostics are retained");
                        self.status = format!(
                            "Read-only diagnostics: {} issue(s) at revision {}.",
                            report.issues.len(),
                            report.revision.value()
                        );
                    } else {
                        self.status =
                            "Discarded diagnostics for a stale project, revision, or candidate."
                                .into();
                    }
                }
                Ok(ResponseBody::AxisExported {
                    path,
                    revision,
                    axis,
                }) => {
                    self.status = format!(
                        "Exported {:?} revision {}: {}",
                        axis,
                        revision.value(),
                        path.display()
                    );
                }
                Ok(ResponseBody::Exported { path, revision }) => {
                    self.status =
                        format!("Exported revision {}: {}", revision.value(), path.display());
                }
                Ok(ResponseBody::HandyInspection(report)) => {
                    self.status = match self.handy_panel.accept(report) {
                        Ok(()) => {
                            "Handy observation received; playback remains unqualified.".into()
                        }
                        Err(error) => error.to_string(),
                    };
                }
                Ok(ResponseBody::DeviceMotionExported {
                    path,
                    revision,
                    gain,
                    ..
                }) => {
                    self.status=format!("Exported device-motion proposal for revision {} (amplitude gain {:.4}): {}. Physical playback is not admitted or qualified.",revision.value(),gain,path.display());
                }
                Ok(_) => {
                    self.status = "Engine request acknowledged.".into();
                }
            }
        }
    }

    fn display_preview(
        &mut self,
        result: PreviewResult,
        frame: Option<Result<DecodedFrame, String>>,
        previous_frame: Option<Result<DecodedFrame, String>>,
        context: &egui::Context,
    ) {
        if !self.preview.accept(result) {
            return;
        }
        let pixels = (|| -> anyhow::Result<_> {
            let previous_required = self
                .preview
                .displayed()
                .and_then(|p| p.annotated.as_ref())
                .is_some_and(|e| e.previous.is_some());
            let current = frame.transpose().map_err(anyhow::Error::msg)?;
            let previous = previous_frame.transpose().map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                previous_required == previous.is_some(),
                "Annotated predecessor pixels are missing or unexpected"
            );
            Ok((current, previous))
        })();
        match pixels {
            Ok((current, previous)) => {
                let texture = |name, frame: DecodedFrame| {
                    context.load_texture(
                        name,
                        egui::ColorImage::from_rgba_unmultiplied(
                            [frame.width, frame.height],
                            &frame.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                };
                self.texture = current.map(|f| texture("pulsar-source-frame", f));
                self.previous_texture = previous.map(|f| texture("pulsar-previous-target", f));
            }
            Err(error) => {
                self.texture = None;
                self.previous_texture = None;
                self.preview.clear();
                self.status = error.to_string();
            }
        }
    }

    fn seek(&mut self, seconds: f64) {
        self.target_author.clear();
        self.target_picker.clear();
        self.playhead_seconds = seconds.clamp(
            if self.selected_placement.is_some() {
                -86_400.0
            } else {
                0.0
            },
            86_400.0,
        );
        self.seek_generation = self.seek_generation.saturating_add(1);
        self.request_generation = self.request_generation.saturating_add(1);
        self.texture = None;
        self.previous_texture = None;
        self.preview.clear();
        self.preview_pending = self.has_media_source();
    }

    fn submit_pack_action(&mut self, action: crate::pack_panel::PackAction) {
        if matches!(action, crate::pack_panel::PackAction::Dismiss) {
            match self.pack_panel.dismissable_id() {
                Ok(id) => {
                    self.enqueue(RpcCommand::DismissPackRecovery(id), None, None);
                }
                Err(error) => self.status = error.to_string(),
            };
            return;
        }
        let submission = (|| -> anyhow::Result<_> {
            let proof = crate::project_package::read_local_owner_proof(
                &self.owner_directory,
                "model-pack-owner.token",
            )?;
            self.pack_panel.prepare(action, proof)
        })();
        match submission {
            Ok(submission) => {
                let command = if let Some(id) = &submission.id {
                    RpcCommand::Identified(id.clone(), submission.command.clone())
                } else {
                    RpcCommand::Wire(submission.command.clone())
                };
                if self.enqueue(command, None, None) {
                    self.pack_panel.submitted(&submission);
                }
            }
            Err(error) => self.status = error.to_string(),
        }
    }
    fn fallback_binding(&self) -> Option<inference_fallback_panel::Binding> {
        if self.generation_mode != GeneratorInputMode::Video
            || !self.use_model
            || !self.has_media_source()
        {
            return None;
        }
        let profile = self.generation_profile()?;
        profile.validate().ok()?;
        let choice = if self.use_selected_pack {
            Some(self.pack_panel.generation_choice().ok()?)
        } else {
            if self.model_path.trim().is_empty() {
                return None;
            }
            None
        };
        let project = self.project.as_ref()?;
        Some(inference_fallback_panel::Binding {
            session: self.fallback_session.clone()?,
            project: project.project_id.clone(),
            revision: project.revision,
            source: self.source.clone()?,
            model_key: serde_json::to_vec(&(
                self.use_selected_pack,
                &self.model_path,
                choice,
                profile,
            ))
            .ok()?,
        })
    }

    fn generation_profile(&self) -> Option<DeclaredInferenceProfile> {
        if !self.use_model {
            return None;
        }
        if self.use_selected_pack {
            self.pack_panel.profile()
        } else {
            self.inference_profile.clone()
        }
    }
    fn model_contract(&self) -> Option<ModelInputContract> {
        if let Some(profile) = self.generation_profile() {
            return Some(profile.preprocessing.input);
        }
        if self.use_model && self.use_selected_pack {
            return None;
        }
        self.use_model.then_some(ModelInputContract {
            width: self.model_width,
            height: self.model_height,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: self.model_classes,
            decoder: DetectorDecoder::YoloChannelMajor,
        })
    }

    fn generation_analysis_dimensions(&self) -> (u32, u32) {
        let input = self.model_contract();
        let custom = self.use_analysis_dimensions
            && !input
                .as_ref()
                .is_some_and(|input| input.decoder == DetectorDecoder::TemporalScriptPosition);
        crate::inference::generation_analysis_dimensions(
            input.as_ref(),
            custom.then_some(self.analysis_dimensions[0]),
            custom.then_some(self.analysis_dimensions[1]),
        )
    }

    fn generation_detector_score_threshold(&self) -> Option<f32> {
        selected_detector_score_threshold(
            self.use_detector_score_threshold,
            self.detector_score_threshold,
            self.model_contract().as_ref(),
        )
    }

    fn selected_model(&self) -> anyhow::Result<Option<PathBuf>> {
        if !self.use_model {
            return Ok(None);
        }
        if self.use_selected_pack {
            self.pack_panel.generation_choice()?;
            return Ok(None);
        }
        if self.model_path.trim().is_empty() {
            anyhow::bail!("Select a model path or disable neural analysis");
        }
        Ok(Some(absolute_path(std::path::Path::new(&self.model_path))?))
    }

    fn request_preview(&mut self) {
        if !self.has_media_source() {
            self.preview_pending = false;
            self.preview_playing = false;
            self.preview.clear();
            self.texture = None;
            self.previous_texture = None;
            return;
        }
        if self.busy {
            return;
        }
        let (Some(source), Some(project)) = (self.source.clone(), self.project.as_ref()) else {
            return;
        };
        let placement = project
            .placements
            .iter()
            .find(|p| Some(&p.placement_id) == self.selected_placement.as_ref())
            .cloned();
        let source = placement
            .as_ref()
            .map(|p| p.spec.source_version.clone())
            .unwrap_or(source);
        let mut context = FrameContext {
            source_version: source.clone(),
            source_placement: placement
                .as_ref()
                .map(|p| p.placement_id.clone())
                .unwrap_or_else(|| {
                    SourcePlacementId::new(format!("primary:{source}"))
                        .expect("bounded source identifier")
                }),
            // For time-based seeks, the engine replaces this placeholder with
            // the decoder's actual ordinal. It is never shown as a source frame.
            frame: FrameId::new(0),
            transform: match video_view_transform(self.video_view.as_ref()) {
                Ok(value) => value,
                Err(error) => {
                    self.status = error.to_string();
                    self.preview_pending = false;
                    return;
                }
            },
            seek_generation: self.seek_generation,
            request_generation: self.request_generation,
        };
        let source_time = match SourceTimestamp::new(
            (self.playhead_seconds * 1_000_000_000.0).round() as i64,
            1_000_000_000,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.status = error.to_string();
                self.preview_pending = false;
                return;
            }
        };
        let plain_reference_preview = self.tab == Tab::References;
        let preview_model = self.use_model && !self.author_target_mode && !plain_reference_preview;
        let mut model_path = match if self.author_target_mode || plain_reference_preview {
            Ok(None)
        } else {
            self.selected_model()
        } {
            Ok(value) => value,
            Err(error) => {
                self.status = error.to_string();
                self.preview_pending = false;
                return;
            }
        };
        let project_id = project.project_id.clone();
        let profile = self.generation_profile();
        let annotated_profile = preview_model
            && profile.as_ref().is_some_and(|p| {
                matches!(
                    p.preprocessing.resize,
                    InferenceResize::AnnotatedTemporalView
                )
            });
        if annotated_profile || (preview_model && self.annotated_view.is_some()) {
            let prepared = (|| -> anyhow::Result<_> {
                let profile=profile.ok_or_else(||anyhow::anyhow!("Annotated target preview requires an explicit annotated CPU or CUDA profile"))?;
                let spec = AnnotatedPreviewSpec {
                    view: self.annotated_view.clone().ok_or_else(|| {
                        anyhow::anyhow!("Load a source-bound target view for annotated preview")
                    })?,
                    source_sample_stride: 1,
                };
                spec.validate_profile(&profile)?;
                context.transform = annotated_preview_transform(&spec.view)?;
                let model = if self.use_selected_pack {
                    AnnotatedPreviewModel::Pack {
                        choice: self.pack_panel.generation_choice()?,
                    }
                } else {
                    AnnotatedPreviewModel::File {
                        model_path: model_path
                            .clone()
                            .ok_or_else(|| anyhow::anyhow!("Annotated preview model is absent"))?,
                        profile: (profile.clone()).into(),
                    }
                };
                let placed = placement.clone().map(|placement| PlacedPreviewRequest {
                    placement,
                    project_time: ProjectTime::from_nanos(source_time.numerator()),
                });
                let time = if let Some(request) = &placed {
                    ModelPackPreviewTime::Placement {
                        placement_id: request.placement.placement_id.clone(),
                        project_time: request.project_time,
                    }
                } else {
                    ModelPackPreviewTime::Source {
                        source_time,
                        time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    }
                };
                self.preview
                    .begin_annotated(context, time, placed, spec, model, profile)
            })();
            self.preview_pending = false;
            match prepared {
                Ok(command) => {
                    self.last_preview = Instant::now();
                    self.send(command, Some(project_id), None);
                }
                Err(error) => {
                    self.preview.clear();
                    self.texture = None;
                    self.previous_texture = None;
                    self.preview_playing = false;
                    self.status = error.to_string();
                }
            }
            return;
        }
        let mut model_input = if self.author_target_mode || plain_reference_preview {
            None
        } else {
            self.model_contract()
        };
        let mut inference_profile = preview_model
            .then(|| self.inference_profile.clone())
            .flatten();
        let pack_detector = preview_model
            && self.use_selected_pack
            && model_input
                .as_ref()
                .is_some_and(|c| c.decoder == DetectorDecoder::YoloChannelMajor);
        let pack_choice = if pack_detector {
            match self.pack_panel.generation_choice() {
                Ok(choice) => Some(choice),
                Err(error) => {
                    self.status = error.to_string();
                    self.preview_pending = false;
                    return;
                }
            }
        } else {
            None
        };
        if self.use_selected_pack
            || model_input
                .as_ref()
                .is_some_and(|c| c.decoder == DetectorDecoder::TemporalScriptPosition)
        {
            model_path = None;
            model_input = None;
            inference_profile = None;
        }
        if let Some(placement) = placement {
            let request = PlacedPreviewRequest {
                placement,
                project_time: ProjectTime::from_nanos(source_time.numerator()),
            };
            if let Err(error) = request.validate(&context) {
                self.status = error.to_string();
                self.preview_pending = false;
                return;
            }
            self.preview.begin_placed(context.clone(), request.clone());
            self.preview_pending = false;
            self.last_preview = Instant::now();
            let command = if let Some(choice) = pack_choice {
                Command::PreviewModelPackFrame {
                    context,
                    time: ModelPackPreviewTime::Placement {
                        placement_id: request.placement.placement_id,
                        project_time: request.project_time,
                    },
                    video_view: self.video_view.clone(),
                    choice,
                }
            } else {
                Command::PreviewPlacedFrame {
                    placement_id: request.placement.placement_id,
                    project_time: request.project_time,
                    context,
                    video_view: self.video_view.clone(),
                    model_path,
                    model_input,
                    inference_profile,
                }
            };
            self.send(command, Some(project_id), None);
            return;
        }
        self.preview.begin_timed(
            context.clone(),
            source_time,
            PreviewTimeBasis::ElapsedFromFirstFrame,
        );
        self.preview_pending = false;
        self.last_preview = Instant::now();
        self.send(
            if let Some(choice) = pack_choice {
                Command::PreviewModelPackFrame {
                    context,
                    time: ModelPackPreviewTime::Source {
                        source_time,
                        time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    },
                    video_view: self.video_view.clone(),
                    choice,
                }
            } else {
                Command::PreviewFrame {
                    time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    video_view: self.video_view.clone(),
                    context,
                    source_time,
                    model_path,
                    model_input,
                    inference_profile,
                }
            },
            Some(project_id),
            None,
        );
    }

    fn generator(&mut self, ui: &mut egui::Ui) {
        ui.heading("Generator");
        egui::ComboBox::from_label("Input modality")
            .selected_text(self.generation_mode.label())
            .show_ui(ui, |ui| {
                for mode in [
                    GeneratorInputMode::Video,
                    GeneratorInputMode::Text,
                    GeneratorInputMode::Patterns,
                    GeneratorInputMode::Image,
                    GeneratorInputMode::Audio,
                    GeneratorInputMode::Funscript,
                ] {
                    ui.selectable_value(&mut self.generation_mode, mode, mode.label());
                }
            });
        ui.label("Immutable input -> isolated worker -> review candidate -> atomic commit.");
        match self.generation_mode {
            GeneratorInputMode::Video => self.video_generator(ui),
            GeneratorInputMode::Patterns => {
                ui.label("One strict pattern line per axis (one through six axes)");
                ui.text_edit_multiline(&mut self.pattern_prompt);
                ui.small("Use the same duration for every axis. Axes: stroke, sway, surge, roll, pitch, yaw.");
                ui.colored_label(Color32::YELLOW,"Explicit synthetic patterns; no observed motion or free-form AI interpretation.");
                if ui
                    .add_enabled(
                        !self.busy && self.project.is_some(),
                        egui::Button::new("Generate axis-pattern candidate"),
                    )
                    .clicked()
                {
                    match crate::patterns::build_pattern_input(&self.pattern_prompt)
                        .and_then(crate::patterns::candidate_command)
                    {
                        Ok(command) => {
                            self.project_command(command, true);
                        }
                        Err(error) => self.status = format!("{error:#}"),
                    }
                }
            }
            GeneratorInputMode::Text | GeneratorInputMode::Image => {
                ui.label("Strict pattern syntax");
                ui.text_edit_multiline(&mut self.pattern_prompt);
                ui.small("Required: pattern, axis, duration, frequency, amplitude, offset. Patterns: sine, triangle, hold, pulse.");
                ui.colored_label(Color32::YELLOW, "Deterministic preset syntax, not free-form language understanding or a local AI assistant.");
                if self.generation_mode == GeneratorInputMode::Image {
                    ui.label("Import/select a still image in Project. Prompt supplies motion; image content is not claimed to infer motion.");
                }
                let enabled = !self.busy
                    && self.project.is_some()
                    && (self.generation_mode == GeneratorInputMode::Text
                        || self.has_media_source());
                if ui
                    .add_enabled(enabled, egui::Button::new("Synthesize pattern candidate"))
                    .clicked()
                {
                    if self.pattern_prompt.trim().is_empty() {
                        self.status = "A nonempty constrained pattern prompt is required.".into();
                    } else {
                        let input = if self.generation_mode == GeneratorInputMode::Text {
                            GenerationInput::Text {
                                prompt: self.pattern_prompt.clone(),
                            }
                        } else {
                            GenerationInput::Image {
                                source_version: self.source.clone().unwrap(),
                                prompt: self.pattern_prompt.clone(),
                            }
                        };
                        self.project_command(Command::GenerateInput { input }, true);
                    }
                }
            }
            GeneratorInputMode::Audio => {
                let source = self
                    .has_media_source()
                    .then(|| self.source.clone())
                    .flatten();
                if let Some(input) =
                    self.audio_panel
                        .ui(ui, source, !self.busy && self.project.is_some())
                {
                    match input {
                        Ok(input) => {
                            self.project_command(Command::GenerateInput { input }, true);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            GeneratorInputMode::Funscript => {
                ui.label("Funscript path");
                ui.text_edit_singleline(&mut self.script_import_path);
                ui.label("Import proposes an engine-owned candidate. Original script and committed timeline remain unchanged.");
                if ui
                    .add_enabled(
                        !self.busy && self.project.is_some(),
                        egui::Button::new("Import script candidate"),
                    )
                    .clicked()
                {
                    if self.script_import_path.trim().is_empty() {
                        self.status = "Enter a funscript path.".into();
                    } else {
                        match absolute_path(std::path::Path::new(&self.script_import_path)) {
                            Ok(path) => {
                                self.project_command(Command::ImportFunscript { path }, true);
                            }
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                }
            }
        }
        if let Some(job) = self.job.clone() {
            ui.separator();
            ui.label(format!("Job {} / attempt {}", job.job_id, job.attempt_id));
            ui.label(format!("State: {}", job.state));
            if job.state == "fallback_pending" && job.error.is_none() {
                ui.label("Checking the selected equivalent backends. This job remains active and can be cancelled.");
            }
            if ui
                .add_enabled(
                    !self.busy
                        && self
                            .capabilities
                            .as_ref()
                            .is_some_and(|c| c.operations.iter().any(|o| o == "job_attempts")),
                    egui::Button::new("Inspect attempt history"),
                )
                .clicked()
            {
                self.project_command(
                    Command::JobAttempts {
                        job_id: job.job_id.clone(),
                        after_attempt: None,
                        limit: 8,
                    },
                    false,
                );
            }
            if let Some(page) = self
                .job_attempts
                .clone()
                .filter(|p| p.job_id == job.job_id && p.current_attempt == job.attempt_id)
            {
                ui.collapsing("Attempt declarations and completion observations",|ui| {
                    ui.small("Attempt-ID order; declarations are not proof of executed provider placement. Missing completion or retirement is unknown. This history does not authorize fallback.");
                    for attempt in &page.attempts {
                        ui.label(format!("{} · {}",attempt.attempt_id,attempt.state));
                        if let Some(profile)=&attempt.declared_profile {
                            ui.small(format!("Declared {:?} · profile {}",profile.provider,attempt.declared_profile_sha256.as_deref().unwrap_or("unavailable")));
                        } else {ui.small("No explicit profile retained; historical defaults are not inferred.");}
                        if let Some(outcome)=&attempt.outcome {
                            if let Some(error)=&outcome.validation_error {ui.label(format!("{:?}: {}{}",error.code,error.message,if error.message_truncated {" [truncated; full message digest retained]"}else{""}));}
                            ui.small(if outcome.worker_retirement.is_some(){"Worker-tree retirement observed."}else{"Worker-tree retirement not established by this record."});
                            if let Some(absence)=outcome.worker_retirement.as_ref().and_then(|r|r.provider_unavailable.as_ref()) {ui.label(format!("{:?} was absent from the selected runtime's provider list.",absence.provider));}
                        } else {ui.small("No completion observation retained.");}
                        ui.separator();
                    }
                    if let Some(after)=page.next_after {
                        if ui.add_enabled(!self.busy,egui::Button::new("Next attempt page")).clicked() {
                            self.project_command(Command::JobAttempts {job_id:job.job_id.clone(),after_attempt:Some(after),limit:8},false);
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Refresh job"))
                    .clicked()
                {
                    self.project_command(
                        Command::JobStatus {
                            job_id: job.job_id.clone(),
                        },
                        false,
                    );
                }
                if ui
                    .add_enabled(
                        !self.busy && job.candidate_id.is_none(),
                        egui::Button::new("Cancel job"),
                    )
                    .clicked()
                {
                    self.project_command(
                        Command::CancelJob {
                            job_id: job.job_id.clone(),
                        },
                        false,
                    );
                }
                if let Some(candidate_id) = job.candidate_id {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Review candidate"))
                        .clicked()
                    {
                        self.project_command(Command::GetCandidate { candidate_id }, false);
                    }
                }
            });
        }
        if let Some(candidate) = self.candidate.clone() {
            ui.separator();
            let stale = self
                .project
                .as_ref()
                .is_some_and(|project| project.revision != candidate.base_revision);
            self.candidate_controls(ui, &candidate, stale);
        }
        ui.separator();
        ui.label("Free-form local AI, visual-semantic image interpretation, trained multimodal inference and physical qualification remain explicit implementation/research gaps.");
    }

    fn video_generator(&mut self, ui: &mut egui::Ui) {
        if self.selected_placement.is_some() {
            ui.colored_label(Color32::YELLOW,"Generation uses the selected placement's project timing. Existing motion changes only through an explicit candidate commit.");
        }
        self.video_view_controls(ui);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Inspect inference support"))
            .clicked()
        {
            self.send(Command::InferenceCatalog, None, None);
        }
        if let Some(catalog) = &self.inference_catalog {
            ui.label(&catalog.limitation);
            ui.label(format!(
                "Worker available: {}; runtime installed: {}; qualified profiles: {}",
                catalog.worker_available,
                catalog.runtime.is_some(),
                catalog.qualified_profiles.len()
            ));
        }
        egui::ComboBox::from_label("Stroke preset")
            .selected_text(&self.preset)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.preset, "default".into(), "Default");
                ui.selectable_value(&mut self.preset, "fast".into(), "Fast");
                let available = self
                    .model_contract()
                    .is_some_and(|input| input.decoder != DetectorDecoder::TemporalScriptPosition);
                ui.add_enabled_ui(available, |ui| {
                    ui.selectable_value(&mut self.preset, "maximum".into(), "Maximum");
                });
            });
        if self.preset == "maximum" {
            ui.label("Maximum uses denser visual tracking. Select a detector model; a quality improvement is not yet qualified.");
        }
        if ui
            .checkbox(
                &mut self.use_model,
                "Use declared model analysis (unqualified)",
            )
            .changed()
        {
            self.seek(self.playhead_seconds);
        }
        if let Some(pack) = self.bundled_models.ui(ui, self.busy) {
            if let Err(error) = self.pack_panel.prepare_bundled_import(&pack) {
                self.status = error.to_string();
            }
        }
        if let Some(action) = self.pack_panel.ui(ui, self.busy) {
            self.submit_pack_action(action);
        }
        if self.use_model {
            let previous = self.use_selected_pack;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.use_selected_pack, false, "Model file");
                ui.selectable_value(
                    &mut self.use_selected_pack,
                    true,
                    "Owner-selected offline pack",
                );
            });
            if previous != self.use_selected_pack {
                self.seek(self.playhead_seconds);
            }
            if self.use_selected_pack {
                if let Some(profile) = self.pack_panel.profile() {
                    ui.label(format!(
                        "Selected {:?} profile, {} threads; {} × {}, {} classes.",
                        profile.provider,
                        profile.cpu_threads,
                        profile.preprocessing.input.width,
                        profile.preprocessing.input.height,
                        profile.preprocessing.input.class_count
                    ));
                } else {
                    ui.label("Refresh and explicitly approve a pack before generation.");
                }
                ui.label("Annotated CPU or CUDA packs use the loaded source-bound target view for frame-pair preview. Detector packs use the selected video view.");
            } else {
                ui.label("Model path");
                if ui.text_edit_singleline(&mut self.model_path).changed() {
                    self.seek(self.playhead_seconds);
                }
                ui.label("Optional inference profile JSON");
                ui.text_edit_singleline(&mut self.inference_profile_path);
                ui.horizontal(|ui| {
                if ui.button("Load profile").clicked() {
                    match crate::inference::read_profile(std::path::Path::new(&self.inference_profile_path)) {
                        Ok(profile) => {
                            self.inference_profile = Some(profile);
                            self.status = "Profile loaded. The engine checks exact model and runtime bytes before execution.".into();
                            self.seek(self.playhead_seconds);
                        }
                        Err(error) => self.status = format!("Profile was not changed: {error:#}"),
                    }
                }
                if ui.button("Use default CPU profile").clicked() {
                    self.inference_profile = None;
                    self.seek(self.playhead_seconds);
                }
            });
                if let Some(profile) = &self.inference_profile {
                    ui.label(format!(
                        "Initial profile: {:?}, {} threads; {} × {}, {} classes.",
                        profile.provider,
                        profile.cpu_threads,
                        profile.preprocessing.input.width,
                        profile.preprocessing.input.height,
                        profile.preprocessing.input.class_count
                    ));
                }
                ui.add_enabled_ui(self.inference_profile.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Input");
                        ui.add(egui::DragValue::new(&mut self.model_width).range(1..=4096));
                        ui.add(egui::DragValue::new(&mut self.model_height).range(1..=4096));
                        ui.label("Classes");
                        ui.add(egui::DragValue::new(&mut self.model_classes).range(1..=65536));
                    });
                });
                ui.label(format!(
                    "Declared model decoder: {:?}. No format guessing.",
                    self.model_contract().map(|c| c.decoder)
                ));
                if self
                    .model_contract()
                    .is_some_and(|c| c.decoder == DetectorDecoder::TemporalScriptPosition)
                {
                    ui.label("Annotated CPU or CUDA profiles preview the declared target frame pair. Other script-position profiles show pixels only. Estimates remain unqualified.");
                }
                ui.colored_label(Color32::YELLOW, "Custom model output is unqualified. Runtime must be installed and pinned by the engine.");
            }
        }
        let temporal_analysis = self
            .model_contract()
            .is_some_and(|input| input.decoder == DetectorDecoder::TemporalScriptPosition);
        ui.add_enabled_ui(!temporal_analysis, |ui| {
            ui.checkbox(
                &mut self.use_analysis_dimensions,
                "Custom decoded analysis size",
            );
            ui.add_enabled_ui(self.use_analysis_dimensions, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut self.analysis_dimensions[0]).range(1..=16384));
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut self.analysis_dimensions[1]).range(1..=16384));
                });
            });
        });
        let (analysis_width, analysis_height) = self.generation_analysis_dimensions();
        ui.label(format!("Generation analysis limit: {analysis_width} × {analysis_height}. Flat video keeps its aspect ratio. Preview size is independent."));
        if temporal_analysis {
            ui.label("This temporal model requires fixed 64 × 64 analysis.");
        }
        let detector_available = self
            .model_contract()
            .is_some_and(|c| c.decoder != DetectorDecoder::TemporalScriptPosition);
        ui.add_enabled_ui(detector_available, |ui| {
            ui.checkbox(
                &mut self.use_detector_score_threshold,
                "Custom minimum generation detector score",
            );
            ui.add_enabled(
                self.use_detector_score_threshold,
                egui::Slider::new(&mut self.detector_score_threshold, 0.0..=1.0)
                    .text("Minimum score"),
            );
        });
        ui.label(
            "Generation detector scores are uncalibrated. The default and preview cutoff is 0.25.",
        );
        ui.label(
            "Annotated target view (optional, requires an annotated temporal CPU or CUDA profile)",
        );
        ui.text_edit_singleline(&mut self.annotated_view_path);
        ui.horizontal(|ui| {
            if ui.button("Load target view").clicked() {
                match crate::inference::read_annotated_view(std::path::Path::new(&self.annotated_view_path)) {
                    Ok(view)=>{self.annotated_view=Some(view);self.seek(self.playhead_seconds);self.status="Target view loaded. Preview and generation check its source content binding.".into();},
                    Err(error)=>self.status=format!("Target view was not changed: {error:#}"),
                }
            }
            if ui.button("Clear target view").clicked() {self.annotated_view=None;self.seek(self.playhead_seconds);}
        });
        if let Some(view) = &self.annotated_view {
            ui.label(format!(
                "Target {}; region {:?}; source {}",
                view.declaration.target_id, view.declaration.target_region, view.source_sha256
            ));
        }
        let can_author = self.has_media_source() && self.video_view.is_some();
        if ui
            .add_enabled(
                can_author,
                egui::Checkbox::new(&mut self.author_target_mode, "Draw target view in preview"),
            )
            .changed()
        {
            self.preview_playing = false;
            if self.author_target_mode {
                self.pick_target_mode = false;
            }
            self.seek(self.playhead_seconds);
        }
        if self.author_target_mode {
            ui.small("Declare the eye/projection, then drag inside the paused preview. This fixed rectangle declares geometry only; it does not check tracking or script timing.");
            ui.horizontal(|ui| {
                ui.label("Target ID");
                ui.text_edit_singleline(&mut self.authored_target_id);
            });
            let draft = self.target_author.current_draft(
                self.preview.displayed(),
                self.video_view.as_ref(),
                &self.authored_target_id,
            );
            if let Ok(view) = &draft {
                ui.label(format!(
                    "Draft region {:?}; analysis limits {:?}",
                    view.declaration.target_region, view.declaration.analysis_dimensions
                ));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(draft.is_ok(), egui::Button::new("Use drawn target view"))
                    .clicked()
                {
                    if let Ok(view) = &draft {
                        self.annotated_view = Some(view.clone());
                        self.author_target_mode = false;
                        self.seek(self.playhead_seconds);
                        self.status =
                            "Target view selected. Model and source admission still apply.".into();
                    }
                }
                if ui
                    .add_enabled(
                        draft.is_ok() && !self.annotated_view_path.trim().is_empty(),
                        egui::Button::new("Save new drawn target view"),
                    )
                    .clicked()
                {
                    if let Ok(view) = &draft {
                        self.status = match crate::target_view_authoring::save_new(
                            std::path::Path::new(&self.annotated_view_path),
                            view,
                        ) {
                            Ok(path) => format!("Target view saved: {}", path.display()),
                            Err(error) => format!("Target view save failed: {error:#}"),
                        };
                    }
                }
            });
        }
        ui.checkbox(
            &mut self.use_generation_interval,
            "Reanalyse a project-time interval",
        );
        if self.use_generation_interval {
            ui.horizontal(|ui| {
                for (index, label) in ["Start ns (inclusive)", "End ns (exclusive)"]
                    .into_iter()
                    .enumerate()
                {
                    ui.label(label);
                    ui.add(
                        egui::DragValue::new(&mut self.generation_interval_ns[index])
                            .range(0..=i64::MAX),
                    );
                }
            });
            ui.label("Only interval samples reach perception. Source decoding still verifies the container; clocks and source stride are unchanged. Apply preserves outside motion and protected stencils.");
        }
        let can_pick = self.has_media_source()
            && self.use_model
            && self
                .model_contract()
                .is_some_and(|c| c.decoder != DetectorDecoder::TemporalScriptPosition);
        if ui
            .add_enabled(
                can_pick || self.pick_target_mode,
                egui::Checkbox::new(
                    &mut self.pick_target_mode,
                    "Pick observed target in preview",
                ),
            )
            .changed()
        {
            self.preview_playing = false;
            if self.pick_target_mode {
                self.author_target_mode = false;
            }
            self.seek(self.playhead_seconds);
        }
        if self.pick_target_mode {
            ui.small("Click one observed box in the paused preview, then explicitly use the draft. Overlapping boxes and missing detections refuse. Generation re-detects at the original frame; identity and quality remain unqualified.");
            let picked = self
                .target_picker
                .current_draft(self.preview.displayed(), self.video_view.as_ref());
            if let Ok(picked) = &picked {
                ui.label(format!(
                    "Picked source frame {}; normalized point {:.4}, {:.4}",
                    picked.seed.source_frame_index, picked.seed.x, picked.seed.y
                ));
            }
            if ui
                .add_enabled(
                    picked.is_ok(),
                    egui::Button::new("Use picked tracking target"),
                )
                .clicked()
            {
                if let Ok(picked) = picked {
                    self.target_seed_frame = picked.seed.source_frame_index;
                    self.target_seed_point = [picked.seed.x, picked.seed.y];
                    // Preview observations carry no detector class; never invent one.
                    self.target_seed_filter_class = false;
                    self.target_seed_binding = Some((
                        picked.context().source_version.clone(),
                        picked.context().transform.clone(),
                    ));
                    self.use_target_seed = true;
                    self.pick_target_mode = false;
                    self.target_picker.clear();
                    self.status = "Target seed selected. Preview has no class metadata; add a class filter explicitly if needed. Generation admission and re-detection still apply.".into();
                }
            }
        }
        ui.checkbox(
            &mut self.use_target_seed,
            "Select a tracking target explicitly",
        );
        if self.use_target_seed {
            ui.label("At this original source-frame ordinal, select the unique observed box containing the analysis-view point. Missing identity remains a gap; no automatic target switching.");
            ui.horizontal(|ui| {
                ui.label("Source frame");
                ui.add(egui::DragValue::new(&mut self.target_seed_frame));
            });
            ui.horizontal(|ui| {
                for (index, label) in ["Normalized x", "Normalized y"].into_iter().enumerate() {
                    ui.label(label);
                    ui.add(
                        egui::DragValue::new(&mut self.target_seed_point[index])
                            .range(0.0..=0.999999),
                    );
                }
            });
            ui.checkbox(&mut self.target_seed_filter_class, "Require detector class");
            if self.target_seed_filter_class {
                ui.add(egui::DragValue::new(&mut self.target_seed_class));
            }
            if ui
                .add_enabled(
                    self.source.is_some(),
                    egui::Button::new("Bind target declaration to current source and view"),
                )
                .clicked()
            {
                match video_view_transform(self.video_view.as_ref()) {
                    Ok(transform) => {
                        self.target_seed_binding =
                            self.source.clone().map(|source| (source, transform));
                        self.status="Target declaration bound. Generation still requires a detector and one observed matching box.".into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            if !self.use_model
                || self
                    .model_contract()
                    .is_none_or(|model| model.decoder == DetectorDecoder::TemporalScriptPosition)
            {
                ui.colored_label(Color32::YELLOW,"An indexed-video detector is required. Script-position and global-motion recipes cannot select an entity.");
            }
        }
        ui.checkbox(
            &mut self.use_mixed_audio,
            "Use audio alongside video motion",
        );
        if self.use_mixed_audio {
            ui.checkbox(
                &mut self.use_independent_audio,
                "Use a separately imported audio source",
            );
            if self.use_independent_audio {
                if let Some(project) = &self.project {
                    let selected = project
                        .sources
                        .iter()
                        .find(|source| {
                            Some(&source.source_version) == self.independent_audio_source.as_ref()
                        })
                        .map(|source| source.label.as_str())
                        .unwrap_or("Select imported audio media");
                    egui::ComboBox::from_id_salt("independent-audio-source")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for source in project.sources.iter().filter(|source| {
                                source.kind == SourceKind::Media
                                    && Some(&source.source_version) != self.source.as_ref()
                            }) {
                                ui.selectable_value(
                                    &mut self.independent_audio_source,
                                    Some(source.source_version.clone()),
                                    &source.label,
                                );
                            }
                        });
                }
                for (i, label) in [
                    "Video clock anchor (seconds)",
                    "Audio clock anchor (seconds)",
                ]
                .into_iter()
                .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        ui.add(egui::DragValue::new(&mut self.audio_alignment_anchors[i].0));
                        ui.label("/");
                        ui.add(
                            egui::DragValue::new(&mut self.audio_alignment_anchors[i].1)
                                .range(1..=u32::MAX),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Audio seconds per video second");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.audio_clock_rate.0)
                            .char_limit(20)
                            .desired_width(100.0),
                    );
                    ui.label("/");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.audio_clock_rate.1)
                            .char_limit(10)
                            .desired_width(80.0),
                    );
                });
                ui.label("Declare corresponding anchors and a positive clock rate. Alignment is not measured; unsupported audio times remain gaps.");
            } else {
                ui.label(
                    "Audio uses the current video container and its original stream timestamps.",
                );
            }
            ui.checkbox(
                &mut self.creative_audio,
                "Creative: replace visual motion with audio RMS",
            );
            ui.label("Faithful preserves available visual samples. Audio assistance is synthesized; RMS maps to 0.5–0.75 without normalization.");
        }
        let fallback_binding = self.fallback_binding();
        self.fallback_panel.sync(fallback_binding);
        let fallback_supported = self.capabilities.as_ref().is_some_and(|capabilities| {
            capabilities
                .operations
                .iter()
                .any(|operation| operation == "generate_with_inference_fallback")
        });
        self.fallback_panel.ui(ui, self.busy, fallback_supported);
        if ui
            .add_enabled(
                !self.busy && self.has_media_source(),
                egui::Button::new("Generate stroke candidate"),
            )
            .clicked()
        {
            let selection = self.selected_model().and_then(|model| {
                let audio = if self.use_mixed_audio && self.use_independent_audio {
                    Some(independent_audio_selection(&self.project.as_ref().expect("media project").sources,self.source.as_ref(),self.independent_audio_source.as_ref(),self.audio_alignment_anchors,independent_audio_rate(&self.audio_clock_rate.0,&self.audio_clock_rate.1)?)?)
                } else {None};
                let target = if self.use_target_seed {
                    let transform=video_view_transform(self.video_view.as_ref())?;
                    if !self.target_seed_binding.as_ref().is_some_and(|(source,view)|Some(source)==self.source.as_ref() && view==&transform) {
                        return Err(ProtocolError::invalid("Bind the target declaration to the current source and view before generation").into());
                    }
                    Some(AnalysisTargetSeed {source_frame_index:self.target_seed_frame,x:self.target_seed_point[0],y:self.target_seed_point[1],class_id:self.target_seed_filter_class.then_some(self.target_seed_class)})
                } else {None};Ok((model,audio,target))
            });
            match selection {
                Ok((model_path, independent, target_seed)) => {
                    let project_interval = if self.use_generation_interval {
                        match TimeRange::new(
                            ProjectTime::from_nanos(self.generation_interval_ns[0]),
                            ProjectTime::from_nanos(self.generation_interval_ns[1]),
                        ) {
                            Ok(interval) => Some(interval),
                            Err(error) => {
                                self.status = error.to_string();
                                return;
                            }
                        }
                    } else {
                        None
                    };
                    let settings = GenerationSettings {
                        detector_score_threshold: self.generation_detector_score_threshold(),
                        annotated_view: self.annotated_view.clone(),
                        project_interval,
                        target_seed,
                        analysis_width: self.generation_analysis_dimensions().0,
                        analysis_height: self.generation_analysis_dimensions().1,
                        mixed_audio: self.use_mixed_audio.then(|| MixedAudioSettings {
                            independent,
                            intent: if self.creative_audio {
                                MultimodalIntent::CreativeAudio
                            } else {
                                MultimodalIntent::Faithful
                            },
                            mapping: AudioMapping::new(
                                Axis::Stroke,
                                0.25,
                                NormalizedPosition::new(0.5).expect("constant"),
                            )
                            .expect("constant"),
                        }),
                        source_placement: self
                            .project
                            .as_ref()
                            .and_then(|p| {
                                p.placements.iter().find(|p| {
                                    Some(&p.placement_id) == self.selected_placement.as_ref()
                                })
                            })
                            .cloned(),
                        video_view: self.video_view.clone(),
                        inference_profile: self.generation_profile(),
                        model_input: self.model_contract(),
                        ..Default::default()
                    };
                    match settings.validate_execution_preset(&self.preset, self.use_model) {
                        Ok(()) => {
                            let command = if self.use_model && self.use_selected_pack {
                                match self.pack_panel.generation_command(
                                    self.source.clone().unwrap(),
                                    self.preset.clone(),
                                    settings,
                                ) {
                                    Ok(command) => command,
                                    Err(error) => {
                                        self.status = error.to_string();
                                        return;
                                    }
                                }
                            } else {
                                Command::Generate {
                                    source_version: self.source.clone().unwrap(),
                                    preset: self.preset.clone(),
                                    settings: Some(settings),
                                    model_path,
                                }
                            };
                            if self.fallback_panel.enabled() {
                                match self.fallback_panel.prepare(command) {
                                    Ok(intent) => {
                                        let binding = intent.binding().clone();
                                        if self.enqueue(
                                            RpcCommand::FallbackGeneration(Box::new(intent)),
                                            Some(binding.project),
                                            Some(binding.revision),
                                        ) {
                                            self.fallback_panel.submitted();
                                            self.status = "Reading the selected policy and submitting the captured generation request.".into();
                                        }
                                    }
                                    Err(error) => self.status = error.to_string(),
                                }
                            } else {
                                self.project_command(command, true);
                            }
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    fn video_view_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.video_view.clone();
        crate::video_view_panel::show(ui, &mut self.video_view);
        if before != self.video_view {
            self.seek(self.playhead_seconds);
        }
    }

    fn candidate_controls(
        &mut self,
        ui: &mut egui::Ui,
        candidate: &CandidateSnapshot,
        stale: bool,
    ) {
        ui.label(format!(
            "Candidate {} / base revision {}",
            candidate.candidate_id,
            candidate.base_revision.value()
        ));
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Verify image generation origin"),
            )
            .clicked()
        {
            self.candidate_image_origin = None;
            self.candidate_image_pixels = None;
            self.pending_image_record = None;
            self.project_command(
                Command::CandidateImageOriginStatus {
                    candidate_id: candidate.candidate_id.clone(),
                },
                false,
            );
        }
        let mut show_image = None;
        if let Some(report) = self
            .candidate_image_origin
            .as_ref()
            .filter(|r| image_origin_scope_matches(r, candidate))
        {
            ui.label("Verified image-conditioned synthesis. The retained approval grants no new execution authority and does not establish observed temporal motion or model qualification.");
            for origin in &report.origins {
                let image = origin
                    .approval
                    .report
                    .image
                    .as_ref()
                    .expect("validated image origin");
                ui.label(format!(
                    "Original project {} / revision {} / candidate {}",
                    origin.approval.report.project_id,
                    origin.approval.report.base_revision.value(),
                    origin.candidate_id
                ));
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Show conditioning image"))
                    .clicked()
                {
                    show_image = Some(origin.record.sha256.clone());
                }
                ui.monospace(format!(
                    "Source SHA-256: {}\nPixels SHA-256: {}\nApproval record SHA-256: {}",
                    image.source.sha256, image.rgb.sha256, origin.record.sha256
                ));
                if let Some(AssistantProposal::SynthesizePatterns { patterns }) =
                    &origin.approval.report.proposal
                {
                    ui.push_id(&origin.record.sha256, |ui| {
                        ui.collapsing("Review retained generation patterns", |ui| {
                            ui.small("These are the historical approved patterns. Later candidate or master edits remain separate.");
                            self.pattern_reviews.show(ui, patterns);
                        });
                    });
                }
            }
            ui.collapsing("Retained image approval and generation details", |ui| {
                ui.monospace(
                    serde_json::to_string_pretty(report).unwrap_or_else(|e| e.to_string()),
                );
            });
        }
        if let Some(record_sha256) = show_image {
            self.pending_image_record = Some(record_sha256.clone());
            self.candidate_image_pixels = None;
            self.project_command(
                Command::CandidateImageOriginPixels {
                    candidate_id: candidate.candidate_id.clone(),
                    record_sha256,
                },
                false,
            );
        }
        if let Some((pixels, texture)) =
            self.candidate_image_pixels.as_ref().filter(|(pixels, _)| {
                image_origin_scope_matches(&pixels.report, candidate)
                    && self
                        .candidate_image_origin
                        .as_ref()
                        .is_some_and(|r| r.origins.iter().any(|o| o.record == pixels.record))
            })
        {
            ui.label(format!(
                "Retained normalized conditioning image: {} × {}",
                pixels.frame.width, pixels.frame.height
            ));
            let size = texture.size_vec2();
            let scale = (ui.available_width().min(320.0) / size.x)
                .min(320.0 / size.y)
                .min(1.0);
            ui.image((texture.id(), size * scale));
            ui.small("These are the stored pixels supplied to the image-conditioned proposal. A still image does not establish temporal motion.");
        }
        if candidate.job_id.is_none()
            && ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Verify archived generation origin"),
                )
                .clicked()
        {
            self.archived_generation = None;
            self.project_command(
                Command::ArchivedGenerationStatus {
                    candidate_id: candidate.candidate_id.clone(),
                    master_revision: None,
                },
                false,
            );
        }
        if candidate.job_id.is_none()
            && ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Verify current master ancestry"),
                )
                .clicked()
        {
            self.archived_generation = None;
            let master_revision = self.project.as_ref().map(|p| p.revision);
            self.project_command(
                Command::ArchivedGenerationStatus {
                    candidate_id: candidate.candidate_id.clone(),
                    master_revision,
                },
                false,
            );
        }
        if let Some(report) = self.archived_generation.as_ref().filter(|r| {
            r.project_id == candidate.project_id
                && r.candidate_id == candidate.candidate_id
                && r.master_derivation.as_ref().is_none_or(|m| {
                    self.project.as_ref().is_some_and(|p| {
                        p.revision == m.revision && p.motion.program.sha256 == m.master.sha256
                    })
                })
        }) {
            ui.label(format!("{} verified historical generation anchors. No live job or publication authority transfers.",report.origins.len()));
            if let Some(master) = &report.master_derivation {
                ui.label(format!("Current master revision {} has checked ancestry and {} recorded retiming steps. Publication still requires fresh source/destination authority and clock proof migration.",master.revision.value(),master.retiming_revisions.len()));
            }
            ui.collapsing("Historical generation details", |ui| {
                ui.monospace(
                    serde_json::to_string_pretty(report).unwrap_or_else(|e| e.to_string()),
                );
            });
        }
        ui.label("Amber curve is proposed motion; review bands identify localized uncertainty. It is not committed.");
        let rows = self.candidate_evidence.rows(&candidate.program);
        if rows.iter().any(|row| row.synthesized > 0) {
            ui.label("This candidate contains synthesized actions. Those actions do not establish observed temporal motion.");
        }
        if rows.iter().any(|row| row.gaps > 0) {
            ui.label(
                "Unresolved evidence gaps remain. Standard export requires explicit resolution.",
            );
        }
        ui.collapsing("Motion evidence by axis", |ui| {
            ui.small("Counts describe action labels and gap intervals, not durations or model qualification. These labels alone do not identify an image origin.");
            if rows.is_empty() {
                ui.label("No motion axes supplied.");
            } else {
                egui::Grid::new("candidate-motion-evidence").striped(true).show(ui, |ui| {
                    for heading in ["Axis", "Observed", "Predicted", "Inferred", "Synthesized", "Gaps"] {
                        ui.label(heading);
                    }
                    ui.end_row();
                    for row in rows {
                        ui.label(row.axis.as_str());
                        for count in [row.observed, row.predicted, row.inferred, row.synthesized, row.gaps] {
                            ui.label(count.to_string());
                        }
                        ui.end_row();
                    }
                });
            }
        });
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .id_salt("candidate-review-flags")
            .show(ui, |ui| {
                for flag in candidate.review.iter().take(256) {
                    ui.label(format!(
                        "{} / {:?} / {:.3}..{:.3}s / {:?}",
                        flag.reason.as_str(),
                        flag.axis,
                        flag.range.start().as_nanos() as f64 / 1e9,
                        flag.range.end().as_nanos() as f64 / 1e9,
                        flag.evidence
                    ));
                    if let Some(confidence) = &flag.confidence {
                        ui.small(format!(
                            "Confidence {:.3} ({:?})",
                            confidence.value(),
                            confidence.calibration()
                        ));
                    }
                }
                if candidate.review.len() > 256 {
                    ui.label(format!(
                        "{} additional flags retained by the engine.",
                        candidate.review.len() - 256
                    ));
                }
                if candidate.review.is_empty() {
                    ui.label("No flags supplied. This is not a guarantee of correct motion.");
                }
            });
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Inspect candidate and protected regions"),
            )
            .clicked()
        {
            self.project_command(
                Command::Diagnostics {
                    candidate_id: Some(candidate.candidate_id.clone()),
                    maximum_speed_percent_per_second: Some(self.segment_speed_limit),
                },
                false,
            );
        }
        if stale {
            ui.colored_label(
                Color32::YELLOW,
                "Project changed. Explicit rebase and fresh review are required.",
            );
            if ui
                .add_enabled(!self.busy, egui::Button::new("Explicitly rebase candidate"))
                .clicked()
            {
                self.project_command(
                    Command::RebaseCandidate {
                        candidate_id: candidate.candidate_id.clone(),
                    },
                    true,
                );
            }
        }
        ui.horizontal_wrapped(|ui| {
            for axis in [
                Axis::Stroke,
                Axis::Sway,
                Axis::Surge,
                Axis::Roll,
                Axis::Pitch,
                Axis::Yaw,
            ] {
                let mut selected = self.merge_axes.contains(&axis);
                if ui.checkbox(&mut selected, format!("{axis:?}")).changed() {
                    if selected {
                        self.merge_axes.push(axis);
                    } else {
                        self.merge_axes.retain(|value| *value != axis);
                    }
                }
            }
        });
        ui.checkbox(
            &mut self.merge_range_enabled,
            "Merge only a project-time range",
        );
        if self.merge_range_enabled {
            ui.horizontal(|ui| {
                ui.label("Start");
                ui.add(
                    egui::DragValue::new(&mut self.merge_start_seconds)
                        .range(0.0..=86_400.0)
                        .suffix(" s"),
                );
                ui.label("End");
                ui.add(
                    egui::DragValue::new(&mut self.merge_end_seconds)
                        .range(0.0..=86_400.0)
                        .suffix(" s"),
                );
            });
        }
        if ui
            .add_enabled(
                !self.busy && !stale && !self.merge_axes.is_empty(),
                egui::Button::new("Merge selected axes / preserve protection"),
            )
            .clicked()
        {
            let range = if self.merge_range_enabled {
                crate::modality::range(Some(self.merge_start_seconds), Some(self.merge_end_seconds))
            } else {
                Ok(candidate.project_interval)
            };
            match range {
                Ok(range) => {
                    self.project_command(
                        Command::MergeCandidate {
                            candidate_id: candidate.candidate_id.clone(),
                            axes: self.merge_axes.clone(),
                            range,
                        },
                        true,
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && !stale,
                egui::Button::new("Apply all candidate axes / preserve protected edits"),
            )
            .clicked()
        {
            let axes = candidate
                .program
                .tracks()
                .iter()
                .map(|track| track.axis())
                .collect();
            self.project_command(
                Command::MergeCandidate {
                    candidate_id: candidate.candidate_id.clone(),
                    axes,
                    range: candidate.project_interval,
                },
                true,
            );
        }
        if let Some(interval) = candidate.project_interval {
            ui.label(format!("Native interval candidate: {}..{} ns. Whole replacement of an existing master is refused; applying uses this retained range.",interval.start().as_nanos(),interval.end().as_nanos()));
        }
        if ui
            .add_enabled(
                !self.busy
                    && !stale
                    && (candidate.project_interval.is_none()
                        || self
                            .project
                            .as_ref()
                            .is_some_and(|p| p.program.tracks().is_empty())),
                egui::Button::new("Commit whole reviewed candidate"),
            )
            .clicked()
        {
            self.project_command(
                Command::CommitCandidate {
                    candidate_id: candidate.candidate_id.clone(),
                },
                true,
            );
        }
        if ui.button("Dismiss candidate preview").clicked() {
            self.candidate = None;
            self.diagnostics = None;
        }
        self.diagnostics_view(ui);
    }

    fn doctor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Evidence and protection review");
        ui.label("Diagnostics are read-only. Explicit unlock changes protection; it does not repair motion or qualify hardware.");
        ui.label("Average normalized segment slope; gaps are skipped. Peak velocity, jitter repair and device safety are not measured.");
        ui.horizontal(|ui| {
            ui.label("Maximum percent/second");
            ui.add(
                egui::DragValue::new(&mut self.segment_speed_limit)
                    .range(0.001..=100_000_000_000.0),
            );
        });
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Inspect committed revision"),
            )
            .clicked()
        {
            self.project_command(
                Command::Diagnostics {
                    candidate_id: None,
                    maximum_speed_percent_per_second: Some(self.segment_speed_limit),
                },
                false,
            );
        }
        self.diagnostics_view(ui);
        ui.separator();
        let master = self
            .project
            .as_ref()
            .is_some_and(|p| !p.program.tracks().is_empty());
        if let Some(action) = self.repair_panel.ui(
            ui,
            &mut self.script_import_path,
            self.project.is_some(),
            master,
            self.busy,
        ) {
            match action {
                crate::repair_panel::RepairAction::Import(path) => match absolute_path(&path) {
                    Ok(path) => {
                        self.project_command(Command::ImportFunscriptForRepair { path }, true);
                    }
                    Err(error) => self.status = error.to_string(),
                },
                crate::repair_panel::RepairAction::Propose(options) => {
                    if let Some(project) = self.project.clone() {
                        let request = crate::repair_panel::RepairRequest {
                            project: project.project_id.clone(),
                            revision: project.revision,
                            program: project.program,
                            options,
                        };
                        self.enqueue(
                            RpcCommand::Repair(request),
                            Some(project.project_id),
                            Some(project.revision),
                        );
                    }
                }
            }
        }
    }

    fn diagnostics_current(&self) -> Option<&DiagnosticsReport> {
        self.diagnostics.as_ref().filter(|report| {
            diagnostics_match_view(
                report,
                self.project
                    .as_ref()
                    .map(|project| (&project.project_id, project.revision)),
                self.candidate
                    .as_ref()
                    .map(|candidate| &candidate.candidate_id),
            )
        })
    }

    fn diagnostics_view(&mut self, ui: &mut egui::Ui) {
        let Some(report) = self.diagnostics_current().cloned() else {
            return;
        };
        if report.candidate_id.is_some()
            && self
                .candidate
                .as_ref()
                .map(|candidate| &candidate.candidate_id)
                != report.candidate_id.as_ref()
        {
            ui.label("Diagnostics refer to a different candidate; request fresh diagnostics.");
            return;
        }
        ui.separator();
        ui.label(format!(
            "Diagnostics revision {} / {} protected regions",
            report.revision.value(),
            report.protected_regions.len()
        ));
        if let Some(scan) = &report.segment_speed {
            ui.label(format!("Average segment slope limit {} percent/second: {} violations / {} visited edges; {} unavailable edges skipped{}.",
                scan.maximum_percent_per_second, scan.violations.len(), scan.scanned_segments, scan.unavailable_segments,
                if scan.truncated { "; truncated prefix" } else { "" }));
            for violation in &scan.violations {
                ui.label(format!(
                    "{:?} {}..{} ns: {:.6} percent/second",
                    violation.axis,
                    violation.range.start().as_nanos(),
                    violation.range.end().as_nanos(),
                    violation.average_percent_per_second
                ));
            }
        }
        ui.label("Manual commits protect changed interpolation spans automatically. Unlocking is an explicit, undoable policy change.");
        let mut unlock = None;
        for (index, region) in report.protected_regions.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Protected {:?}: {:.3}..{:.3}s",
                    region.axis,
                    region.range.start().as_nanos() as f64 / 1e9,
                    region.range.end().as_nanos() as f64 / 1e9
                ));
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new("Explicitly unlock this region"),
                    )
                    .clicked()
                {
                    unlock = Some(index);
                }
            });
        }
        if let Some(index) = unlock {
            let mut regions = report.protected_regions.clone();
            regions.remove(index);
            self.project_command(Command::SetProtectedRegions { regions }, true);
        }
        for issue in report.issues.iter().take(256) {
            let span = issue
                .range
                .as_ref()
                .map(|range| {
                    format!(
                        "{:.3}..{:.3}s",
                        range.start().as_nanos() as f64 / 1e9,
                        range.end().as_nanos() as f64 / 1e9
                    )
                })
                .unwrap_or_default();
            ui.label(format!(
                "{} {:?} {}: {}",
                issue.code, issue.axis, span, issue.message
            ));
        }
        if report.issues.is_empty() {
            ui.label("No issues reported by these checks. Neural accuracy and safety remain unqualified.");
        }
        if self.candidate.is_none() {
            self.review_decisions_view(ui);
        }
    }
    fn review_decisions_view(&mut self, ui: &mut egui::Ui) {
        let Some(report) = self.review_status.clone().filter(|r| {
            self.project
                .as_ref()
                .is_some_and(|p| p.project_id == r.project_id && p.revision == r.revision)
        }) else {
            return;
        };
        ui.separator();
        ui.label(
            "Explicit review decisions retain the original warnings. Edits require review again.",
        );
        if report.unknown {
            ui.label(
                "Review ancestry remains unknown; decisions do not establish missing evidence.",
            );
        }
        ui.label("Review rationale (required)");
        ui.text_edit_singleline(&mut self.review_note);
        ui.checkbox(&mut self.review_partial, "Review a selected subinterval");
        if self.review_partial {
            ui.horizontal(|ui| {
                ui.label("Start ns");
                ui.add(egui::DragValue::new(&mut self.review_interval_ns.0));
                ui.label("End ns (exclusive)");
                ui.add(egui::DragValue::new(&mut self.review_interval_ns.1));
            });
        }
        let mut selected = None;
        for entry in report.entries.iter().take(256) {
            let unresolved = entry
                .unresolved_ranges()
                .unwrap_or_else(|_| vec![entry.flag.range]);
            let resolved = unresolved.is_empty();
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} {:?}: {:?} {}..{} ns",
                    entry.flag.reason.as_str(),
                    if resolved { "resolved" } else { "open" },
                    entry.flag.axis,
                    entry.flag.range.start().as_nanos(),
                    entry.flag.range.end().as_nanos()
                ));
                let range = TimeRange::new(
                    ProjectTime::from_nanos(self.review_interval_ns.0),
                    ProjectTime::from_nanos(self.review_interval_ns.1),
                )
                .ok()
                .filter(|r| {
                    r.start() >= entry.flag.range.start() && r.end() <= entry.flag.range.end()
                });
                if self.review_partial {
                    for (label, disposition) in [
                        ("Resolve selected interval", ReviewDisposition::Resolved),
                        ("Reopen selected interval", ReviewDisposition::Reopened),
                    ] {
                        if ui
                            .add_enabled(
                                !self.busy
                                    && range.is_some()
                                    && validate_review_intent(&entry.review_id, &self.review_note)
                                        .is_ok(),
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            selected = Some((entry.review_id.clone(), disposition, range));
                        }
                    }
                } else if ui
                    .add_enabled(
                        !self.busy
                            && validate_review_intent(&entry.review_id, &self.review_note).is_ok(),
                        egui::Button::new(if resolved {
                            "Reopen whole warning"
                        } else {
                            "Resolve whole warning"
                        }),
                    )
                    .clicked()
                {
                    selected = Some((
                        entry.review_id.clone(),
                        if resolved {
                            ReviewDisposition::Reopened
                        } else {
                            ReviewDisposition::Resolved
                        },
                        None,
                    ));
                }
            });
            if !entry.decision_history.is_empty() {
                ui.label(format!("Unreviewed ranges: {:?}", unresolved));
                for d in &entry.decision_history {
                    ui.label(format!(
                        "{} {:?} at revision {}: {}",
                        d.actor, d.range, d.recorded_revision, d.note
                    ));
                }
            }
            if let Some(d) = &entry.decision {
                ui.label(format!(
                    "Recorded actor {} at revision {}: {}",
                    d.actor, d.recorded_revision, d.note
                ));
            }
        }
        if report.entries.len() > 256 {
            ui.label("Additional warnings are available through CLI review-status.");
        }
        if let Some((id, disposition, range)) = selected {
            let request_id = crate::session::new_request_id();
            let command = if let Some(range) = range {
                Command::SetReviewIntervalDisposition {
                    review_id: id,
                    range,
                    disposition,
                    note: self.review_note.clone(),
                }
            } else {
                Command::SetReviewDisposition {
                    review_id: id,
                    disposition,
                    note: self.review_note.clone(),
                }
            };
            self.review_retry = Some((
                request_id.clone(),
                command.clone(),
                report.project_id.clone(),
                report.revision,
            ));
            self.review_send(request_id, command, report.project_id, report.revision);
        }
        if let Some((id, command, project, revision)) = self.review_retry.clone() {
            ui.label(format!("Last review request: {id}"));
            if ui
                .add_enabled(!self.busy, egui::Button::new("Retry exact review request"))
                .clicked()
            {
                self.review_send(id, command, project, revision);
            }
        }
    }
    fn review_send(
        &mut self,
        id: RequestId,
        command: Command,
        project: ProjectId,
        revision: RevisionId,
    ) {
        if self
            .sender
            .try_send(RpcWork {
                command: RpcCommand::Identified(id, command),
                project: Some(project),
                revision: Some(revision),
            })
            .is_ok()
        {
            self.busy = true;
        } else {
            self.status = "Review queue is full; exact request retained for retry".into();
        }
    }

    fn paint_review_spans(&self, painter: &egui::Painter, rect: egui::Rect, duration: f64) {
        let paint = |range: &TimeRange, color: Color32| {
            let start = (range.start().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
            let end = (range.end().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
            if end > start {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left() + start * rect.width(), rect.top()),
                        egui::pos2(rect.left() + end * rect.width(), rect.bottom()),
                    ),
                    0.0,
                    color,
                );
            }
        };
        if let Some(candidate) = &self.candidate {
            for flag in &candidate.review {
                if flag.axis.is_none() || flag.axis == Some(self.timeline_axis) {
                    paint(
                        &flag.range,
                        Color32::from_rgba_unmultiplied(235, 159, 43, 30),
                    );
                }
            }
        }
        if let Some(report) = self.diagnostics_current() {
            for region in &report.protected_regions {
                if region.axis.is_none() || region.axis == Some(self.timeline_axis) {
                    paint(
                        &region.range,
                        Color32::from_rgba_unmultiplied(158, 169, 188, 60),
                    );
                }
            }
        }
    }

    fn has_media_source(&self) -> bool {
        self.project.as_ref().is_some_and(|project| {
            project.sources.iter().any(|source| {
                source.kind == pulsar_protocol::SourceKind::Media
                    && self.source.as_ref() == Some(&source.source_version)
            })
        })
    }

    fn project_panel(&mut self, ui: &mut egui::Ui) {
        if let Some((count, revision)) = self
            .diagnostics_current()
            .map(|report| (report.issues.len(), report.revision.value()))
        {
            ui.heading("Review status");
            if count > 0 {
                ui.colored_label(
                    Color32::YELLOW,
                    format!("{count} diagnostic issue(s) at revision {revision}"),
                );
            } else {
                ui.label(format!(
                    "No issues from current checks at revision {revision}."
                ));
                ui.small("This is not accuracy or safety qualification.");
            }
            if ui.button("Open read-only diagnostics").clicked() {
                self.tab = Tab::Doctor;
            }
            ui.separator();
        }
        ui.heading("Project");
        ui.text_edit_singleline(&mut self.project_name);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Create project"))
            .clicked()
        {
            self.send(
                Command::CreateProject {
                    name: self.project_name.clone(),
                },
                None,
                None,
            );
        }
        ui.label("Existing project ID");
        ui.text_edit_singleline(&mut self.open_project_id);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Open project"))
            .clicked()
        {
            match ProjectId::new(self.open_project_id.trim()) {
                Ok(project_id) => {
                    self.send(Command::OpenProject { project_id }, None, None);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(project) = &self.project {
            ui.separator();
            ui.label(RichText::new(&project.name).strong());
            ui.label(format!("Revision {}", project.revision.value()));
            ui.label(format!("{} motion axes", project.program.tracks().len()));
            let sources = project.sources.clone();
            for source in sources {
                if source.kind != pulsar_protocol::SourceKind::Media {
                    let kind = match source.kind {
                        pulsar_protocol::SourceKind::GenerationInput => "generation input",
                        pulsar_protocol::SourceKind::Funscript => "funscript",
                        pulsar_protocol::SourceKind::LiveRecording => "accepted live recording",
                        pulsar_protocol::SourceKind::Media => unreachable!(),
                    };
                    ui.add_enabled(
                        false,
                        egui::Label::new(format!("{} [{}; not previewable]", source.label, kind)),
                    );
                    continue;
                }
                if ui
                    .selectable_label(
                        self.source.as_ref() == Some(&source.source_version),
                        &source.label,
                    )
                    .clicked()
                {
                    self.source = Some(source.source_version);
                    self.selected_placement = None;
                    self.seek(0.0);
                }
            }
        }
        self.placement_controls(ui);
        ui.separator();
        ui.label("Media path");
        ui.text_edit_singleline(&mut self.source_path);
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Import immutable source"),
            )
            .clicked()
        {
            if self.source_path.trim().is_empty() {
                self.status = "Enter a source path.".into();
            } else {
                match absolute_path(std::path::Path::new(&self.source_path)) {
                    Ok(path) => {
                        self.project_command(Command::ImportSource { path }, true);
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        }
        ui.separator();
        ui.label(format!("Export {:?} .funscript path", self.timeline_axis));
        ui.text_edit_singleline(&mut self.export_path);
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Export committed revision"),
            )
            .clicked()
        {
            if self.export_path.trim().is_empty() {
                self.status = "Enter an export path.".into();
            } else {
                match absolute_path(std::path::Path::new(&self.export_path)) {
                    Ok(path) => {
                        self.project_command(
                            Command::ExportAxis {
                                path,
                                axis: self.timeline_axis,
                            },
                            true,
                        );
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        }
        let edge_count = self
            .project
            .as_ref()
            .map(|project| editor::unavailable_edge_count(&project.program, self.timeline_axis))
            .unwrap_or(0);
        if edge_count > 0 {
            ui.collapsing("Unavailable edge time", |ui| {
                ui.label(format!("{} unavailable intervals lie outside this axis's first and last keyframes.", edge_count));
                ui.small("Trim excludes that unsupported time. Keyframe times, positions and evidence stay unchanged; internal gaps still block export. Undo restores the excluded intervals.");
                if ui.add_enabled(!self.busy, egui::Button::new("Trim unavailable edge time on selected axis")).clicked() {
                    if let Some(project) = &self.project {
                        match editor::trim_unavailable_edges(&project.program, self.timeline_axis) {
                            Ok(proposal) => { self.send_edit(proposal, project.project_id.clone(), project.revision); }
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                }
            });
        }
        ui.label("Device dynamics profile JSON path");
        ui.text_edit_singleline(&mut self.dynamics_profile_path);
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Export device-motion proposal JSON"),
            )
            .clicked()
        {
            let result = (|| -> anyhow::Result<Command> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.dynamics_profile_path,
                ))?;
                let profile: DynamicsProfile = serde_json::from_slice(&bytes)?;
                profile.validate()?;
                anyhow::ensure!(!self.export_path.trim().is_empty(), "Enter an export path.");
                Ok(Command::ExportDeviceMotion {
                    path: absolute_path(std::path::Path::new(&self.export_path))?,
                    profile,
                })
            })();
            match result {
                Ok(command) => {
                    self.project_command(command, true);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.small("Proposal only. Requires the specified quintic interpolation and stopped entry; it does not qualify or arm a device.");
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Undo"),
                )
                .clicked()
            {
                self.project_command(Command::Undo, true);
            }
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Redo"),
                )
                .clicked()
            {
                self.project_command(Command::Redo, true);
            }
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Refresh"),
                )
                .clicked()
            {
                self.project_command(Command::GetSnapshot, false);
            }
        });
    }

    fn placement_controls(&mut self, ui: &mut egui::Ui) {
        let placements = self
            .project
            .as_ref()
            .map(|p| p.placements.clone())
            .unwrap_or_default();
        ui.collapsing("Source placements",|ui|{
            ui.label("Align media in project time. Existing motion is preserved and flagged for review.");
            if ui.selectable_label(self.selected_placement.is_none(),"Original clip clock").clicked(){self.selected_placement=None;self.seek(0.0);}
            for p in placements {
                if ui.selectable_label(self.selected_placement.as_ref()==Some(&p.placement_id),format!("{} / speed {}/{}",p.placement_id,p.spec.rate_numerator,p.spec.rate_denominator)).clicked(){
                    self.source=Some(p.spec.source_version.clone());if self.retime_from.is_empty(){self.retime_from=p.placement_id.to_string();}self.selected_placement=Some(p.placement_id);
                    self.placement_times=[p.spec.source_start.numerator() as f64/f64::from(p.spec.source_start.denominator()),p.spec.source_end.numerator() as f64/f64::from(p.spec.source_end.denominator()),p.spec.source_anchor.numerator() as f64/f64::from(p.spec.source_anchor.denominator()),p.spec.project_anchor.as_nanos() as f64/1e9];
                    self.placement_rate=(p.spec.rate_numerator,p.spec.rate_denominator);self.seek(self.placement_times[3]);
                }
            }
            for (index,label) in ["Trim start","Trim end","Source anchor","Project anchor"].iter().enumerate(){ui.horizontal(|ui|{ui.label(*label);ui.add(egui::DragValue::new(&mut self.placement_times[index]).range(if index==3{-86400.0..=86400.0}else{0.0..=86400.0}).suffix(" s"));});}
            ui.horizontal(|ui|{ui.label("Speed");ui.add(egui::DragValue::new(&mut self.placement_rate.0).range(-1000..=1000));ui.label("/");ui.add(egui::DragValue::new(&mut self.placement_rate.1).range(1..=1000));});
            if ui.add_enabled(!self.busy && self.has_media_source(),egui::Button::new(if self.selected_placement.is_some(){"Replace selected placement"}else{"Add placement"})).clicked(){
                let build=||->anyhow::Result<SourcePlacementSpec>{
                    let source_version=self.source.clone().ok_or_else(||anyhow::anyhow!("Select media first"))?;
                    let rational=|seconds:f64|->anyhow::Result<SourceTimestamp>{anyhow::ensure!(seconds.is_finite(),"Time must be finite");Ok(SourceTimestamp::new((seconds*1e9).round() as i64,1_000_000_000)?)};
                    let spec=SourcePlacementSpec{source_version,source_start:rational(self.placement_times[0])?,source_end:rational(self.placement_times[1])?,source_anchor:rational(self.placement_times[2])?,project_anchor:ProjectTime::from_nanos((self.placement_times[3]*1e9).round() as i64),rate_numerator:self.placement_rate.0,rate_denominator:self.placement_rate.1};spec.validate()?;Ok(spec)
                };
                match build(){Ok(spec)=>{self.placement_save_pending=self.project_command(Command::SetSourcePlacement{spec,replace:self.selected_placement.clone()},true);},Err(error)=>self.status=error.to_string()}
            }
            if let Some(placement_id)=self.selected_placement.clone(){if ui.add_enabled(!self.busy,egui::Button::new("Remove placement")).clicked(){self.project_command(Command::RemoveSourcePlacement{placement_id},true);}}
            ui.separator();
            ui.label("Retime existing motion");
            ui.label("Treat selected motion as the old clock and map it to the selected placement. Positions stay unchanged; protected spans and trim loss refuse. Undo restores the previous motion.");
            ui.horizontal(|ui|{ui.label("Old placement ID");ui.add(egui::TextEdit::singleline(&mut self.retime_from).char_limit(128));});
            let available=self.project.as_ref().map(|p|p.program.tracks().iter().map(|t|t.axis()).collect::<Vec<_>>()).unwrap_or_default();
            self.retime_axes.retain(|axis|available.contains(axis));
            ui.horizontal_wrapped(|ui|{for axis in available {let mut selected=self.retime_axes.contains(&axis);if ui.checkbox(&mut selected,axis.as_str()).changed(){self.retime_axes.retain(|a|*a!=axis);if selected{self.retime_axes.push(axis);}}}});
            if ui.add_enabled(!self.busy && !self.retime_axes.is_empty() && self.selected_placement.is_some() && !self.retime_from.is_empty(),egui::Button::new("Retime selected motion")).clicked(){
                match SourcePlacementId::new(self.retime_from.clone()) {Ok(from_placement)=>{let to_placement=self.selected_placement.clone().expect("enabled with selected placement");self.project_command(Command::RetimeMotion {from_placement,to_placement,axes:self.retime_axes.clone()},true);},Err(error)=>self.status=error.to_string()}
            }
            ui.collapsing("Retiming receipts",|ui|{
                ui.label("Inspect a recorded edit by revision. Imported attribution remains historical.");
                ui.horizontal(|ui|{
                    ui.label("Revision");
                    if ui.add_enabled(!self.busy,egui::DragValue::new(&mut self.retime_status_revision)).changed(){self.retime_receipt=None;}
                    for (label,archival) in [("Local receipt",false),("Imported receipt",true)] {
                        if ui.add_enabled(!self.busy && self.project.is_some(),egui::Button::new(label)).clicked(){
                            self.retime_receipt=None;let revision=RevisionId::new(self.retime_status_revision);
                            self.project_command(if archival {Command::ArchivedMotionRetimeStatus {revision}} else {Command::MotionRetimeStatus {revision}},false);
                        }
                    }
                });
                if let Some((summary,details))=&self.retime_receipt {
                    ui.label(summary);ui.collapsing("Receipt details",|ui|{egui::ScrollArea::vertical().max_height(220.0).show(ui,|ui|{ui.monospace(details);});});
                }
            });

        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        if !self.has_media_source() {
            ui.label("No previewable media source. Generated patterns and scripts remain available on the timeline.");
        }
        ui.add_enabled_ui(self.has_media_source(), |ui| {

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.author_target_mode && !self.pick_target_mode, egui::Button::new(if self.preview_playing {
                    "Pause preview"
                } else {
                    "Play preview"
                }))
                .clicked()
            {
                self.preview_playing = !self.preview_playing;
                self.last_tick = Instant::now();
            }
            let response = ui.add(
                egui::DragValue::new(&mut self.playhead_seconds)
                    .speed(0.04)
                    .range(if self.selected_placement.is_some(){-86_400.0..=86_400.0}else{0.0..=86_400.0})
                    .suffix(" s"),
            );
            if response.changed() {
                self.seek(self.playhead_seconds);
            }
            if ui
                .add_enabled(
                    !self.busy && self.has_media_source(),
                    egui::Button::new("Analyze displayed time"),
                )
                .clicked()
            {
                self.seek(self.playhead_seconds);
            }
        });
        ui.small("Independent preview. The declared eye and viewport apply to displayed pixels. Audio and device playback remain separate.");
        let available = ui.available_size();
        let annotated = self.preview.displayed().is_some_and(|result| result.annotated.is_some());
        let metadata_height = if annotated { 140.0 } else { 90.0 };
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(available.x.max(1.0), (available.y - metadata_height).max(80.0)),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(8, 11, 17));
        if let Some(texture) = &self.texture {
            let draw_image = |texture: &egui::TextureHandle, viewport: egui::Rect| {
                let image_size = texture.size_vec2();
                let scale = (viewport.width() / image_size.x).min(viewport.height() / image_size.y);
                let image_rect = egui::Rect::from_center_size(viewport.center(), image_size * scale);
                painter.image(
                    texture.id(), image_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                image_rect
            };
            let current_viewport = if let Some(previous) = &self.previous_texture {
                let middle = rect.center().x;
                let previous_viewport = egui::Rect::from_min_max(rect.min, egui::pos2(middle - 4.0, rect.bottom()));
                let current_viewport = egui::Rect::from_min_max(egui::pos2(middle + 4.0, rect.top()), rect.max);
                draw_image(previous, previous_viewport);
                painter.text(previous_viewport.left_top() + egui::vec2(6.0, 6.0), egui::Align2::LEFT_TOP,
                    "Previous target", egui::FontId::proportional(14.0), Color32::WHITE);
                current_viewport
            } else { rect };
            let image_rect = draw_image(texture, current_viewport);
            if self.previous_texture.is_some() {
                painter.text(current_viewport.left_top() + egui::vec2(6.0, 6.0), egui::Align2::LEFT_TOP,
                    "Current target", egui::FontId::proportional(14.0), Color32::WHITE);
            }
            if let Some(result) = self.preview.displayed() {
                if self.pick_target_mode {
                    if let Err(error) = self.target_picker.widget(ui, image_rect, result, self.video_view.as_ref()) {
                        self.status = format!("Target pick unavailable: {error:#}");
                    }
                }
                if self.author_target_mode {
                    if let Some(view) = &self.video_view {
                        if let Err(error) = self.target_author.widget(ui, image_rect, result, view) {
                            self.status = format!("Target drawing unavailable: {error:#}");
                        }
                    } else { self.target_author.clear(); }
                }
                if let (Some(map), Some(cursor)) = (&result.coordinate_map, ui.ctx().pointer_hover_pos()) {
                    if image_rect.contains(cursor) {
                        let point = ViewPoint {
                            x: f64::from((cursor.x - image_rect.left()) / image_rect.width()),
                            y: f64::from((cursor.y - image_rect.top()) / image_rect.height()),
                        };
                        if let Ok(source) = map.to_source(point) {
                            painter.text(image_rect.left_top(), egui::Align2::LEFT_TOP,
                                format!("Source position: {:.4}, {:.4}", source.x, source.y),
                                egui::FontId::monospace(12.0), Color32::WHITE);
                        }
                    }
                }
                for observation in &result.observations {
                    if !observation.matches_frame(&result.context) {
                        continue;
                    }
                    let bounds = observation.bounds();
                    // The worker publishes normalized displayed-view coordinates.
                    // Unmapped model/crop/viewport pixels are never guessed.
                    if bounds.space() != CoordinateSpace::ProjectionNormalized {
                        continue;
                    }
                    let min = egui::pos2(
                        image_rect.left() + bounds.x_min() as f32 * image_rect.width(),
                        image_rect.top() + bounds.y_min() as f32 * image_rect.height(),
                    );
                    let max = egui::pos2(
                        image_rect.left() + bounds.x_max() as f32 * image_rect.width(),
                        image_rect.top() + bounds.y_max() as f32 * image_rect.height(),
                    );
                    let color = if observation.evidence() == EvidenceKind::Observed {
                        Color32::from_rgb(61, 223, 229)
                    } else {
                        Color32::from_rgb(255, 188, 79)
                    };
                    painter.rect_stroke(
                        egui::Rect::from_min_max(min, max),
                        1.0,
                        Stroke::new(2.0_f32, color),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        min,
                        egui::Align2::LEFT_BOTTOM,
                        format!("{:?}", observation.evidence()),
                        egui::FontId::monospace(12.0),
                        color,
                    );
                }
            }
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if self.busy && self.has_media_source() {
                    "Waiting for a matching source frame"
                } else {
                    "Import media, then request a preview"
                },
                egui::FontId::proportional(18.0),
                Color32::from_rgb(151, 166, 185),
            );
        }
        egui::ScrollArea::vertical().id_salt("preview-metadata")
            .max_height(metadata_height).show(ui, |ui| {
        if let Some(result) = self.preview.displayed() {
            ui.label(format!(
                "Decoded source frame {} / source time {:?}",
                result.source_frame_index, result.source_time
            ));
            if let Some(elapsed) = result.elapsed_source_time { ui.label(format!("Elapsed clip time {:.6} s", elapsed.numerator() as f64/f64::from(elapsed.denominator()))); }
            if let Some(placed)=&result.placed {ui.label(format!("Actual project time {:.6} s",placed.actual_project_time.numerator() as f64/placed.actual_project_time.denominator() as f64));}
            if let Some(evidence)=&result.annotated {
                ui.label(format!("Target {} · original source stride {} · unqualified estimate",
                    evidence.spec.view.declaration.target_id,evidence.spec.source_sample_stride));
                if let Some(position)=evidence.position {ui.label(format!("Target position: {:.6}",position.value()));}
                else {ui.colored_label(Color32::YELLOW,"No supported frame-pair estimate at this sample.");}
                if let Some(previous)=&evidence.previous {
                    ui.label(format!("Previous target: source frame {} / PTS {:?}; current target: source frame {}. Pair follows original source order, including reverse placements.",previous.source_frame_index,previous.source_time,result.source_frame_index));
                }
                ui.small(format!("Model {} · profile {} · runtime {}",evidence.model.sha256,evidence.profile.sha256,evidence.runtime.sha256));
            }
            match &result.analysis {
                AnalysisStatus::Available => {
                    if result.annotated.is_none() {ui.label(format!("{} matching observations. Empty means no detection, not a fixed analysis box.", result.observations.len()));}
                }
                AnalysisStatus::Pending => {
                    ui.label("Analysis pending. No stale evidence shown.");
                }
                AnalysisStatus::Unavailable { reason } => {
                    ui.colored_label(Color32::YELLOW, format!("Analysis unavailable: {reason}"));
                }
            }
        }
        });
            });
    }

    fn timeline_duration_seconds(&self) -> f64 {
        self.project
            .iter()
            .map(|project| &project.program)
            .chain(self.candidate.iter().map(|candidate| &candidate.program))
            .flat_map(|program| program.tracks())
            .flat_map(|track| track.actions())
            .map(|action| action.time().as_nanos() as f64 / 1e9)
            .fold(10.0, f64::max)
            .max(self.playhead_seconds + 1.0)
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Timeline axis");
            for axis in [
                Axis::Stroke,
                Axis::Sway,
                Axis::Surge,
                Axis::Roll,
                Axis::Pitch,
                Axis::Yaw,
            ] {
                if ui
                    .selectable_value(&mut self.timeline_axis, axis, format!("{axis:?}"))
                    .changed()
                {
                    self.gesture = None;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{:?} / NEUTRAL MASTER TIMELINE",
                    self.timeline_axis
                ))
                .strong(),
            );
            ui.add(egui::Slider::new(&mut self.new_position, 0.0..=1.0).text("New position"));
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Add at playhead"),
                )
                .clicked()
            {
                if let Some(project) = &self.project {
                    let mut points = editor::axis_points(&project.program, self.timeline_axis);
                    points.push(EditorPoint {
                        nanos: (self.playhead_seconds * 1_000_000_000.0).round() as i64,
                        position: self.new_position,
                    });
                    match editor::replace_axis(&project.program, self.timeline_axis, &points) {
                        Ok(program) => {
                            self.send_edit(
                                EditProposal {
                                    program,
                                    label: format!("Add {:?} keyframe", self.timeline_axis),
                                },
                                project.project_id.clone(),
                                project.revision,
                            );
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
        });
        let committed = self
            .project
            .as_ref()
            .map(|project| editor::axis_points(&project.program, self.timeline_axis))
            .unwrap_or_default();
        let points = self
            .gesture
            .as_ref()
            .map(|gesture| gesture.points.clone())
            .unwrap_or_else(|| committed.clone());
        let candidate = self
            .candidate
            .as_ref()
            .map(|candidate| editor::axis_points(&candidate.program, self.timeline_axis))
            .unwrap_or_default();
        let duration = self.timeline_duration_seconds();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 138.0),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, Color32::from_rgb(10, 15, 23));
        self.paint_review_spans(&painter, rect, duration);
        for index in 0..=4 {
            let y = rect.top() + rect.height() * index as f32 / 4.0;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0_f32, Color32::from_rgb(30, 42, 56)),
            );
        }
        let to_screen = |point: &EditorPoint| {
            egui::pos2(
                rect.left() + (point.nanos as f64 / 1e9 / duration) as f32 * rect.width(),
                rect.bottom() - point.position as f32 * rect.height(),
            )
        };
        let committed_gaps = self
            .project
            .as_ref()
            .and_then(|project| project.program.track(self.timeline_axis))
            .map(|track| track.gaps())
            .unwrap_or_default();
        let candidate_gaps = self
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.program.track(self.timeline_axis))
            .map(|track| track.gaps())
            .unwrap_or_default();
        for (line, gaps, color) in [
            (&candidate, candidate_gaps, Color32::from_rgb(221, 160, 54)),
            (&points, committed_gaps, Color32::from_rgb(57, 214, 227)),
        ] {
            for gap in gaps {
                let left = (gap.start().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
                let right = (gap.end().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left() + left * rect.width(), rect.top()),
                        egui::pos2(rect.left() + right * rect.width(), rect.bottom()),
                    ),
                    0.0,
                    Color32::from_rgba_unmultiplied(190, 80, 96, 38),
                );
            }
            editor::available_segments(line, gaps, |segment| {
                painter.line_segment(
                    [to_screen(&segment[0]), to_screen(&segment[1])],
                    Stroke::new(1.7_f32, color),
                );
            });
            for point in line {
                if editor::point_is_available(point.nanos, gaps) {
                    painter.circle_filled(to_screen(point), 3.3, color);
                }
            }
        }
        if !committed_gaps.is_empty() || !candidate_gaps.is_empty() {
            painter.text(
                rect.left_top() + egui::vec2(5.0, 4.0),
                egui::Align2::LEFT_TOP,
                "Shaded gaps: motion unavailable; curves are clipped, not filled.",
                egui::FontId::proportional(11.0),
                Color32::from_rgb(240, 173, 185),
            );
        }
        let cursor_x = rect.left() + (self.playhead_seconds / duration) as f32 * rect.width();
        painter.line_segment(
            [
                egui::pos2(cursor_x, rect.top()),
                egui::pos2(cursor_x, rect.bottom()),
            ],
            Stroke::new(1.0_f32, Color32::WHITE),
        );
        if response.drag_started() && !self.busy {
            if let (Some(pointer), Some(project)) =
                (response.interact_pointer_pos(), self.project.as_ref())
            {
                let nearest = committed
                    .iter()
                    .enumerate()
                    .map(|(i, point)| (i, pointer.distance(to_screen(point))))
                    .filter(|(_, distance)| *distance <= 14.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((selected, _)) = nearest {
                    self.gesture = Some(EditGesture {
                        project_id: project.project_id.clone(),
                        base_revision: project.revision,
                        base_program: project.program.clone(),
                        axis: self.timeline_axis,
                        points: committed,
                        selected,
                    });
                }
            }
        }
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let seconds =
                    ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * duration;
                if let Some(gesture) = &mut self.gesture {
                    gesture.points[gesture.selected] = EditorPoint {
                        nanos: (seconds * 1e9).round() as i64,
                        position: ((rect.bottom() - pointer.y) / rect.height()).clamp(0.0, 1.0)
                            as f64,
                    };
                } else {
                    self.seek(seconds);
                }
            }
        }
        if response.drag_stopped() {
            if let Some(gesture) = self.gesture.take() {
                match gesture.finish() {
                    Ok((command, project, revision)) => {
                        self.send_edit(command, project, revision);
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        }
        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                self.seek(
                    ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * duration,
                );
            }
        }
        ui.small("Drag a point: one atomic edit on release. Click elsewhere: independent preview seek. White = playhead; cyan = committed/draft gesture; amber = candidate.");
    }

    fn capability_panel(&mut self, ui: &mut egui::Ui) {
        self.native_preparation_panel.show(
            ui,
            self.project
                .as_ref()
                .map(|p| (p.project_id.clone(), p.revision)),
        );
        if let Some(action) = self.handy_registration_panel.ui(ui, self.busy) {
            if !self.enqueue(
                RpcCommand::HandyRegistration(Box::new(action.clone())),
                None,
                None,
            ) {
                self.handy_registration_panel.finish(
                    &action,
                    &Err(self.status.clone()),
                    &Err("Action was not queued; refresh after reconnect".into()),
                );
            }
        }
        ui.separator();
        if let Some(inspection) = self.handy_panel.ui(ui, self.busy) {
            self.enqueue(RpcCommand::HandyInspect(inspection), None, None);
        }
        ui.separator();
        ui.heading("Capabilities and qualification");
        if let Some(capabilities) = &self.capabilities {
            ui.label(format!("Protocol {}", capabilities.protocol_version));
            for backend in &capabilities.backends {
                ui.label(format!(
                    "{}: {} / {:?}",
                    backend.name,
                    if backend.available {
                        "available"
                    } else {
                        "unavailable"
                    },
                    backend.qualification
                ));
                if let Some(reason) = &backend.reason {
                    ui.small(reason);
                }
            }
            ui.separator();
            ui.label("Engine operations");
            for operation in &capabilities.operations {
                ui.monospace(operation);
            }
        }
        ui.separator();
        ui.colored_label(Color32::YELLOW, "Physical playback is disabled here. Preview motion is not device admission or physical stopping evidence.");
        ui.add_enabled(
            false,
            egui::Button::new("Arm physical playback: qualification required"),
        );
        ui.label("Doctor offers read-only diagnostics and explicit neutral repair candidates. Handy connectivity/identity inspection is explicit and read-only. Broader hardware validation and native extensions remain migration gaps. Stash discovery, explicit batch generation and linking are available in the Stash tab.");
        self.assistant_panel.show(
            ui,
            self.project
                .as_ref()
                .map(|p| (p.project_id.clone(), p.revision)),
        );
        if let Some(candidate) = self.assistant_panel.take_edit_candidate() {
            if self.project.as_ref().is_some_and(|p| {
                p.project_id == candidate.project_id && p.revision == candidate.base_revision
            }) {
                self.status =
                    "Assistant edit candidate is ready for review and protected commit.".into();
                self.diagnostics = None;
                self.candidate = Some(candidate);
            }
        }
        if let Some(job) = self.assistant_panel.take_generation_job() {
            if self.project.as_ref().is_some_and(|project| {
                project.project_id == job.project_id && project.revision == job.base_revision
            }) {
                self.status = format!(
                    "Assistant-approved generation job {}: {}",
                    job.job_id, job.state
                );
                self.job = Some(job);
                self.last_job_poll = Instant::now();
            }
        }
        if ui
            .add_enabled(!self.busy, egui::Button::new("Refresh capabilities"))
            .clicked()
        {
            self.send(Command::Capabilities, None, None);
        }
    }
}

impl PulsarDesktop {
    fn stash_tag_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Exact tag name");
        ui.text_edit_singleline(&mut self.stash_tag_name);
        ui.checkbox(&mut self.stash_create_tag, "Create the tag if absent");
        ui.label("Tag request ID (retain for reconciliation)");
        ui.text_edit_singleline(&mut self.stash_tag_request);
        let can_add = !self.busy
            && self.stash_scan.as_ref().is_some_and(|scan| {
                scan.state == StashScanState::VerifiedLinked
                    && self
                        .project
                        .as_ref()
                        .is_some_and(|p| p.project_id == scan.project_id)
            });
        if ui
            .add_enabled(
                can_add,
                egui::Button::new("Add tag to verified attached scene"),
            )
            .clicked()
        {
            let work = (|| -> anyhow::Result<_> {
                let scan = self
                    .stash_scan
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Reconcile the scene attachment first"))?;
                let project = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Open the publication project"))?;
                let id = if self.stash_tag_request.trim().is_empty() {
                    crate::session::new_request_id()
                } else {
                    RequestId::new(self.stash_tag_request.trim())?
                };
                let command = Command::StashAddTag {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scan_request_id: scan.request_id.clone(),
                    tag_name: self.stash_tag_name.clone(),
                    create: self.stash_create_tag,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                };
                Ok((id, command, project.project_id.clone(), project.revision))
            })();
            match work {
                Ok((id, command, project, revision)) => {
                    self.stash_tag_request = id.to_string();
                    self.enqueue(
                        RpcCommand::Identified(id, command),
                        Some(project),
                        Some(revision),
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && self.project.is_some() && !self.stash_tag_request.trim().is_empty(),
                egui::Button::new("Reconcile recorded tag creation or ADD"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                Ok(Command::StashTagStatus {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    tag_request_id: RequestId::new(self.stash_tag_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.label("Resume request ID (separate from the original tag request)");
        ui.text_edit_singleline(&mut self.stash_tag_resume_request);
        let can_resume = !self.busy
            && self.project.is_some()
            && self
                .stash_tag
                .as_ref()
                .is_some_and(|tag| tag.state == StashTagState::Resolved && !tag.add_dispatched);
        if ui
            .add_enabled(
                can_resume,
                egui::Button::new("Attach resolved tag whose ADD was never dispatched"),
            )
            .clicked()
        {
            let work = (|| -> anyhow::Result<_> {
                let project = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Open the publication project"))?;
                let id = if self.stash_tag_resume_request.trim().is_empty() {
                    crate::session::new_request_id()
                } else {
                    RequestId::new(self.stash_tag_resume_request.trim())?
                };
                let command = Command::StashResumeTag {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    tag_request_id: RequestId::new(self.stash_tag_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                };
                Ok((id, command, project.project_id.clone(), project.revision))
            })();
            match work {
                Ok((id, command, project, revision)) => {
                    self.stash_tag_resume_request = id.to_string();
                    self.enqueue(
                        RpcCommand::Identified(id, command),
                        Some(project),
                        Some(revision),
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(tag) = &self.stash_tag {
            ui.label(format!(
                "Tag: {:?}; ID: {}; ADD dispatched: {}",
                tag.state,
                tag.tag_id.as_deref().unwrap_or("unknown"),
                tag.add_dispatched
            ));
        }
        ui.small("Tag updates use ADD and preserve existing tags. Creation and attachment are separate outcomes; unknown effects require reconciliation rather than another dispatch.");
    }
    fn stash_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stash library");
        ui.label("GraphQL endpoint");
        ui.text_edit_singleline(&mut self.stash_endpoint);
        ui.label("Credential environment variable name (optional)");
        ui.text_edit_singleline(&mut self.stash_credential_env);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Find scenes missing scripts"))
            .clicked()
        {
            match crate::project_package::read_local_owner_proof(
                &self.owner_directory,
                "stash-owner.token",
            ) {
                Ok(proof) => {
                    let connection = StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    };
                    self.send(
                        Command::StashListMissing {
                            connection,
                            limit: 100,
                            owner_proof: proof,
                        },
                        None,
                        None,
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.label("Folder mappings JSON file");
        ui.text_edit_singleline(&mut self.stash_mappings_path);
        ui.label("Per-source placement/interval plans JSON file (optional)");
        ui.text_edit_singleline(&mut self.stash_source_plans_path);
        ui.label("Per-source annotated views JSON file (optional)");
        ui.text_edit_singleline(&mut self.stash_annotated_views_path);
        ui.checkbox(
            &mut self.stash_require_selected_source,
            "Require capture to match selected project source",
        );
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Check mapped files before generation"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.stash_mappings_path,
                ))?;
                let mappings: Vec<StashRootMapping> = serde_json::from_slice(&bytes)?;
                let proof = crate::project_package::read_local_owner_proof(
                    &self.owner_directory,
                    "stash-owner.token",
                )?;
                Ok(Command::StashPlanMissing {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    limit: 100,
                    mappings,
                    owner_proof: proof,
                })
            })();
            match command {
                Ok(command) => {
                    self.send(command, None, None);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.separator();
        ui.colored_label(Color32::YELLOW,"Generation uses the Generator model and preset settings. Scripts remain unqualified; unresolved motion gaps block publication.");
        ui.checkbox(&mut self.stash_batch_scan, "Scan successful publications");
        ui.checkbox(
            &mut self.stash_batch_tag_requested,
            "Apply the exact tag below after verified attachment",
        );
        ui.label("Engine batch ID (retain for recovery)");
        ui.text_edit_singleline(&mut self.stash_batch_id);
        if ui
            .add_enabled(
                !self.busy && self.stash_batch.is_none(),
                egui::Button::new("Generate and publish missing scripts"),
            )
            .clicked()
        {
            let config = (|| -> anyhow::Result<crate::stash_flow::BatchConfig> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.stash_mappings_path,
                ))?;
                Ok(crate::stash_flow::BatchConfig {
                    source_plans: if self.stash_source_plans_path.trim().is_empty() {
                        vec![]
                    } else {
                        crate::inference::read_stash_source_plans(std::path::Path::new(
                            &self.stash_source_plans_path,
                        ))?
                    },
                    annotated_views: if self.stash_annotated_views_path.trim().is_empty() {
                        vec![]
                    } else {
                        crate::inference::read_stash_annotated_views(std::path::Path::new(
                            &self.stash_annotated_views_path,
                        ))?
                    },
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    mappings: serde_json::from_slice(&bytes)?,
                    limit: 100,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                    preset: self.preset.clone(),
                    model_path: self.selected_model()?,
                    model_pack: if self.use_model && self.use_selected_pack {
                        Some(self.pack_panel.generation_choice()?)
                    } else {
                        None
                    },
                    scan: self.stash_batch_scan || self.stash_batch_tag_requested,
                    tag: self.stash_batch_tag_requested.then(|| StashBatchTag {
                        name: self.stash_tag_name.clone(),
                        create: self.stash_create_tag,
                    }),
                    request_id: Some(if self.stash_batch_id.trim().is_empty() {
                        crate::session::new_request_id()
                    } else {
                        RequestId::new(self.stash_batch_id.trim())?
                    }),
                    detach: false,
                    settings: GenerationSettings {
                        detector_score_threshold: self.generation_detector_score_threshold(),
                        analysis_width: self.generation_analysis_dimensions().0,
                        analysis_height: self.generation_analysis_dimensions().1,
                        video_view: self.video_view.clone(),
                        inference_profile: self
                            .use_model
                            .then(|| self.generation_profile())
                            .flatten(),
                        model_input: self.model_contract(),
                        ..Default::default()
                    },
                })
            })();
            match config {
                Err(error) => self.status = error.to_string(),
                Ok(config) => {
                    self.stash_batch_id = config
                        .request_id
                        .as_ref()
                        .expect("batch ID set before submission")
                        .to_string();
                    let endpoint = self.batch_endpoint.clone();
                    let bootstrap = self.batch_bootstrap.clone();
                    let repaint = ui.ctx().clone();
                    let (sender, receiver) = mpsc::sync_channel(1);
                    self.stash_batch = Some(receiver);
                    self.stash_batch_report = None;
                    self.status="Stash generation running in separate projects. Closing this window does not cancel engine-owned jobs.".into();
                    std::thread::spawn(move || {
                        let result = (|| -> anyhow::Result<_> {
                            let mut client = SessionClient::connect(&endpoint, &bootstrap)?;
                            crate::stash_flow::generate_missing(&mut client, &config)
                        })()
                        .map_err(|error| error.to_string());
                        let _ = sender.send(result);
                        repaint.request_repaint();
                    });
                }
            }
        }
        if self.stash_batch.is_some() {
            ui.spinner();
            ui.label("Engine batch running. Closing this window does not cancel scheduling or engine-owned jobs.");
        }
        ui.horizontal(|ui| {
            for (label, kind) in [
                ("Read batch status", 0),
                ("Resume recorded batch", 1),
                ("Cancel future batch effects", 2),
            ] {
                if ui
                    .add_enabled(
                        !self.busy && !self.stash_batch_id.trim().is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    let command = (|| -> anyhow::Result<Command> {
                        let id = RequestId::new(self.stash_batch_id.trim())?;
                        let proof = crate::project_package::read_local_owner_proof(
                            &self.owner_directory,
                            "stash-owner.token",
                        )?;
                        Ok(match kind {
                            0 => Command::StashBatchStatus {
                                batch_request_id: id,
                                offset: self.stash_batch_offset,
                                owner_proof: proof,
                            },
                            1 => Command::StashResumeBatch {
                                batch_request_id: id,
                                owner_proof: proof,
                            },
                            _ => Command::StashCancelBatch {
                                batch_request_id: id,
                                owner_proof: proof,
                            },
                        })
                    })();
                    match command {
                        Ok(command) => {
                            self.send(command, None, None);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            for (label, offset, enabled) in [
                (
                    "Previous five items",
                    self.stash_batch_offset.saturating_sub(5),
                    self.stash_batch_offset > 0,
                ),
                (
                    "Next five items",
                    self.stash_batch_offset.saturating_add(5),
                    self.stash_batch_has_more,
                ),
            ] {
                if ui
                    .add_enabled(!self.busy && enabled, egui::Button::new(label))
                    .clicked()
                {
                    let command = (|| -> anyhow::Result<Command> {
                        Ok(Command::StashBatchStatus {
                            batch_request_id: RequestId::new(self.stash_batch_id.trim())?,
                            offset,
                            owner_proof: crate::project_package::read_local_owner_proof(
                                &self.owner_directory,
                                "stash-owner.token",
                            )?,
                        })
                    })();
                    match command {
                        Ok(command) => {
                            self.send(command, None, None);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            ui.small(format!(
                "{} selected items; page starts at {}",
                self.stash_batch_item_count, self.stash_batch_offset
            ));
        });
        if let Some(report) = &self.stash_batch_report {
            ui.collapsing("Batch outcomes and reconciliation IDs", |ui| {
                ui.monospace(report);
            });
        }
        ui.label("Publication request ID (retain for uncertain outcomes)");
        ui.text_edit_singleline(&mut self.stash_publication_request);
        ui.checkbox(
            &mut self.stash_source_time,
            "Map placed motion to original source time for publication",
        );
        ui.checkbox(
            &mut self.stash_use_archived,
            "Use verified archived generation (no live job)",
        );
        if self.stash_use_archived {
            if let Some(report) = self.archived_generation.as_ref().filter(|r| {
                self.project
                    .as_ref()
                    .is_some_and(|p| p.project_id == r.project_id)
                    && self
                        .candidate
                        .as_ref()
                        .is_some_and(|c| c.candidate_id == r.candidate_id)
            }) {
                egui::ComboBox::from_id_salt("stash-archived-generation")
                    .selected_text(if self.stash_archived_namespace.is_empty() {
                        "Select original namespace"
                    } else {
                        &self.stash_archived_namespace
                    })
                    .show_ui(ui, |ui| {
                        for origin in &report.origins {
                            ui.selectable_value(
                                &mut self.stash_archived_namespace,
                                origin.origin_manifest.sha256.clone(),
                                format!(
                                    "{} / historical job {}",
                                    origin.origin_manifest.sha256, origin.job_id
                                ),
                            );
                        }
                    });
            }
            ui.label("Verify current master ancestry in the candidate panel, select its original namespace, and freshly bind the current Stash source before publication.");
        }
        if ui
            .add_enabled(
                !self.busy && self.stash_capture.is_some() && self.candidate.is_some(),
                egui::Button::new("Publish current committed motion"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                let capture = self
                    .stash_capture
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Capture a Stash source first"))?;
                let candidate = self
                    .candidate
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Select a generated candidate"))?;
                let (job_id, archived_origin) = if self.stash_use_archived {
                    let report = self.archived_generation.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "Verify archived generation and current master ancestry first"
                        )
                    })?;
                    let project = self
                        .project
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Select project"))?;
                    anyhow::ensure!(
                        report.project_id == project.project_id
                            && report.candidate_id == candidate.candidate_id
                            && report
                                .master_derivation
                                .as_ref()
                                .is_some_and(|m| m.revision == project.revision
                                    && m.master.sha256 == project.motion.program.sha256),
                        "Archived ancestry report is absent or stale"
                    );
                    let origin = report
                        .origins
                        .iter()
                        .find(|o| o.origin_manifest.sha256 == self.stash_archived_namespace)
                        .ok_or_else(|| anyhow::anyhow!("Select an inspected original namespace"))?;
                    (origin.job_id.clone(), Some(origin.origin_manifest.clone()))
                } else {
                    (candidate.job_id.clone().ok_or_else(||anyhow::anyhow!("Stash publication requires a generation job or explicitly selected archival evidence"))?,None)
                };
                Ok(Command::StashExportScript {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    source_version: capture.source_version.clone(),
                    scene_id: capture.scene_id.clone(),
                    file_id: capture.file_id.clone(),
                    candidate_id: candidate.candidate_id.clone(),
                    job_id,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                    source_time: self.stash_source_time,
                    archived_origin,
                })
            })();
            match command {
                Ok(command) => {
                    let id = if self.stash_publication_request.trim().is_empty() {
                        Ok(crate::session::new_request_id())
                    } else {
                        RequestId::new(self.stash_publication_request.trim())
                    };
                    match (id, self.project.as_ref()) {
                        (Ok(id), Some(p)) => {
                            let project = p.project_id.clone();
                            let revision = p.revision;
                            self.stash_publication_request = id.to_string();
                            self.enqueue(
                                RpcCommand::Identified(id, command),
                                Some(project),
                                Some(revision),
                            );
                        }
                        (Err(error), _) => self.status = error.to_string(),
                        _ => self.status = "Open a project first".into(),
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && !self.stash_publication_request.trim().is_empty(),
                egui::Button::new("Observe publication status"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<_> {
                Ok(Command::StashPublicationStatus {
                    publication_request_id: RequestId::new(self.stash_publication_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(report) = &self.stash_publication_report {
            ui.collapsing("Publication observation", |ui| {
                ui.monospace(report);
            });
        }
        if let Some(export) = &self.stash_export {
            ui.monospace(export.path.display().to_string());
        }
        ui.separator();
        ui.label("Scan request ID (retain for reconciliation)");
        ui.text_edit_singleline(&mut self.stash_scan_request);
        if ui
            .add_enabled(
                !self.busy
                    && self
                        .stash_export
                        .as_ref()
                        .is_some_and(|e| e.publication_request_id.is_some()),
                egui::Button::new("Submit targeted scan for current publication"),
            )
            .clicked()
        {
            let work = (|| -> anyhow::Result<_> {
                let publication = self
                    .stash_export
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Publish a script first"))?;
                let snapshot = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Open the publication project"))?;
                if snapshot.project_id != publication.project_id {
                    anyhow::bail!("Open the publication project before scanning");
                }
                let id = if self.stash_scan_request.trim().is_empty() {
                    crate::session::new_request_id()
                } else {
                    RequestId::new(self.stash_scan_request.trim())?
                };
                let command = Command::StashSubmitScan {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    publication_request_id: publication.publication_request_id.clone().ok_or_else(
                        || anyhow::anyhow!("Legacy publication lacks its request ID"),
                    )?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                };
                Ok((id, command, snapshot.project_id.clone(), snapshot.revision))
            })();
            match work {
                Ok((id, command, project, revision)) => {
                    self.stash_scan_request = id.to_string();
                    self.enqueue(
                        RpcCommand::Identified(id, command),
                        Some(project),
                        Some(revision),
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && self.project.is_some() && !self.stash_scan_request.trim().is_empty(),
                egui::Button::new("Reconcile recorded scan and attachment"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                Ok(Command::StashScanStatus {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scan_request_id: RequestId::new(self.stash_scan_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && self.project.is_some() && !self.stash_scan_request.trim().is_empty(),
                egui::Button::new("Observe attachment without a scan job ID"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                Ok(Command::StashReconcileAttachment {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scan_request_id: RequestId::new(self.stash_scan_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(scan) = &self.stash_scan {
            ui.label(format!(
                "Scan: {:?}; remote job: {}",
                scan.state,
                scan.remote_job_id.as_deref().unwrap_or("unknown")
            ));
            if let Some(observation) = &scan.attachment {
                ui.label(format!(
                    "Attachment observation: {:?} at {} ms UTC; remote script hash unverified",
                    observation.state, observation.observed_at_unix_ms
                ));
            }
        }
        ui.small("Submit records dispatch before contacting Stash. Unknown acknowledgements are never automatically resubmitted. Reconciliation needs the same authenticated session and publication project.");

        self.stash_tag_controls(ui);
        ui.small("Mappings are a JSON array of server_root and local_root pairs. Existing scripts and symlink paths block automatic generation.");
        let mut selected_capture = None;
        if let Some(plan) = &self.stash_plan {
            for file in &plan.files {
                ui.label(format!("Scene {}: {:?}", file.scene_id, file.status));
                if let Some(source) = &file.local_source {
                    ui.monospace(source.display().to_string());
                }
                if file.status == StashFileStatus::ReadyForSnapshot
                    && ui
                        .add_enabled(
                            !self.busy && self.project.is_some(),
                            egui::Button::new("Capture source in current project"),
                        )
                        .clicked()
                {
                    selected_capture = file
                        .file_id
                        .as_ref()
                        .map(|id| (file.scene_id.clone(), id.clone()));
                }
            }
        }
        if let Some((scene_id, file_id)) = selected_capture {
            let command = (|| -> anyhow::Result<Command> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.stash_mappings_path,
                ))?;
                let mappings = serde_json::from_slice(&bytes)?;
                let owner_proof = crate::project_package::read_local_owner_proof(
                    &self.owner_directory,
                    "stash-owner.token",
                )?;
                Ok(Command::StashImportSource {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scene_id,
                    file_id,
                    expected_server_path: None,
                    expected_source_version: if self.stash_require_selected_source {
                        Some(self.source.clone().ok_or_else(|| {
                            anyhow::anyhow!("Select the existing project source to rebind")
                        })?)
                    } else {
                        None
                    },
                    allow_multifile_selection: None,
                    mappings,
                    owner_proof,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, true);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.small("Discovery sends a metadata query to the selected server. Credentials must already be in the engine environment. Scripts are not generated, overwritten or scanned by discovery.");
        if let Some(discovery) = &self.stash_discovery {
            ui.label(format!(
                "{} scenes returned; server count {} (not a transaction snapshot).",
                discovery.scenes.len(),
                discovery.reported_count
            ));
            for scene in &discovery.scenes {
                ui.separator();
                ui.label(format!("{}: {}", scene.id, scene.title));
                if scene.files.len() != 1 {
                    ui.colored_label(
                        Color32::YELLOW,
                        "Ambiguous or missing video files: explicit file selection is required.",
                    );
                }
                for file in &scene.files {
                    ui.monospace(&file.path);
                }
            }
        }
    }
}

impl eframe::App for PulsarDesktop {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.reference_panel.poll();
        self.browser_live_panel.poll(
            self.project
                .as_ref()
                .map(|p| (p.project_id.clone(), p.revision)),
        );
        if context.input(|input| input.viewport().close_requested()) {
            self.browser_close_pending = true;
        }
        if self.browser_close_pending {
            self.browser_live_panel.stop();
            if self.browser_live_panel.busy() || self.reference_panel.busy() {
                self.tab = if self.reference_panel.busy() {
                    Tab::References
                } else {
                    Tab::Browser
                };
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else {
                context.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        if self.browser_live_panel.busy() || self.reference_panel.busy() {
            context.request_repaint_after(Duration::from_millis(50));
        }
        self.library_panel.poll();
        if self.library_panel.busy() {
            context.request_repaint_after(Duration::from_millis(50));
        }
        self.assistant_settings.poll();
        self.portable.poll();
        self.poll(context);
        let fallback_binding = self.fallback_binding();
        self.fallback_panel.sync(fallback_binding);
        self.evidence_panel.poll(
            self.project
                .as_ref()
                .map(|p| (p.project_id.clone(), p.revision)),
        );
        let now = Instant::now();
        if self.preview_playing {
            let seconds = self.playhead_seconds + now.duration_since(self.last_tick).as_secs_f64();
            if now.duration_since(self.last_preview) >= Duration::from_millis(150) {
                self.seek(seconds);
            } else {
                self.playhead_seconds = seconds;
            }
            context.request_repaint_after(Duration::from_millis(30));
        }
        self.last_tick = now;
        egui::TopBottomPanel::top("pulsar-header").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("PULSAR")
                        .size(24.0)
                        .strong()
                        .color(Color32::from_rgb(62, 217, 226)),
                );
                ui.label("Local motion workstation");
                if self.busy {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("Development build / unqualified output");
                });
            });
        });
        egui::TopBottomPanel::bottom("pulsar-status").show(context, |ui| {
            if let Some(warning) = &self.startup_warning {
                ui.colored_label(Color32::YELLOW, format!("Engine startup failed: {warning}"));
                ui.label("Inspect the engine before retrying. Reconnect checks the existing engine; it does not start another process.");
            }
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if ui.add_enabled(!self.busy, egui::Button::new("Reconnect")).clicked() {
                    self.send(Command::Capabilities, None, None);
                }
            });
        });
        egui::SidePanel::left("pulsar-project")
            .default_width(245.0)
            .min_width(200.0)
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (tab, label) in [
                            (Tab::Studio, "Studio"),
                            (Tab::Cinema, "Cinema"),
                            (Tab::Generator, "Generator"),
                            (Tab::ModelEvidence, "Model evidence"),
                            (Tab::Browser, "Browser live"),
                            (Tab::Library, "Library batches"),
                            (Tab::References, "Reference sets"),
                            (Tab::Doctor, "Doctor"),
                            (Tab::Portable, "Portable projects"),
                            (Tab::Devices, "Devices"),
                            (Tab::Automation, "Automation"),
                            (Tab::Stash, "Stash"),
                            (Tab::Evidence, "Capture evidence"),
                            (Tab::Plugins, "WASM editor"),
                            (Tab::Settings, "Settings"),
                        ] {
                            ui.selectable_value(&mut self.tab, tab, label);
                        }
                    });
                    ui.separator();
                    self.project_panel(ui);
                });
            });
        egui::TopBottomPanel::bottom("pulsar-timeline")
            .resizable(true)
            .default_height(205.0)
            .show(context, |ui| {
                if self.tab == Tab::References {
                    if let Some(seconds) = self
                        .reference_panel
                        .comparison(ui, self.preview.displayed())
                    {
                        self.preview_playing = false;
                        self.seek(seconds);
                    }
                } else {
                    self.timeline(ui);
                }
            });
        egui::CentralPanel::default().show(context, |ui| match self.tab {
            Tab::Settings => {
                egui::ScrollArea::vertical().show(ui, |ui| self.assistant_settings.ui(ui));
            }
            Tab::Studio | Tab::Cinema => self.canvas(ui),
            Tab::References => {
                let view = self
                    .target_author
                    .current_draft(
                        self.preview.displayed(),
                        self.video_view.as_ref(),
                        &self.authored_target_id,
                    )
                    .ok()
                    .or_else(|| self.annotated_view.clone())
                    .filter(|view| {
                        self.source
                            .as_ref()
                            .is_some_and(|source| source.to_string() == view.source_sha256)
                    });
                let mut selected = None;
                ui.columns(2, |columns| {
                    egui::ScrollArea::vertical()
                        .id_salt("reference-set-fields")
                        .show(&mut columns[0], |ui| {
                            selected = self.reference_panel.show(
                                ui,
                                view,
                                !self.busy && self.project.is_some(),
                            );
                        });
                    self.canvas(&mut columns[1]);
                });
                if let Some(selected) = selected {
                    self.preview_playing = false;
                    self.author_target_mode = false;
                    self.pick_target_mode = false;
                    self.selected_placement = None;
                    self.video_view = Some(selected.view);
                    let imported = self.project.as_ref().is_some_and(|project| {
                        project.sources.iter().any(|source| {
                            source.kind == SourceKind::Media
                                && source.source_version == selected.source
                        })
                    });
                    if imported {
                        self.source = Some(selected.source);
                        self.seek(0.0);
                    } else {
                        // Clear the prior frame immediately; the import response supplies
                        // the actual content identity, which comparison checks again.
                        self.source = None;
                        self.seek(0.0);
                        self.project_command(
                            Command::ImportSource {
                                path: selected.path,
                            },
                            true,
                        );
                    }
                }
            }
            Tab::ModelEvidence => {
                let mut action = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    action = self.equivalence_panel.ui(ui, self.busy);
                });
                if let Some(action) = action {
                    if !self.enqueue(
                        RpcCommand::Equivalence(Box::new(action.clone())),
                        None,
                        None,
                    ) {
                        self.equivalence_panel.enqueue_failed(&action);
                    }
                }
            }
            Tab::Generator => {
                egui::ScrollArea::vertical().show(ui, |ui| self.generator(ui));
            }
            Tab::Browser => {
                let current = self
                    .project
                    .as_ref()
                    .map(|p| (p.project_id.clone(), p.revision));
                let generator = if self.use_model
                    && !self.use_selected_pack
                    && !self.model_path.trim().is_empty()
                {
                    self.inference_profile.clone().and_then(|profile| {
                        absolute_path(std::path::Path::new(self.model_path.trim()))
                            .ok()
                            .map(|model_path| LiveCaptureInference {
                                model_path,
                                profile,
                            })
                    })
                } else {
                    None
                };
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let pack = self
                        .pack_panel
                        .generation_choice()
                        .ok()
                        .zip(self.pack_panel.profile());
                    self.browser_live_panel.show(ui, current, generator, pack)
                });
            }
            Tab::Library => {
                let selected = egui::ScrollArea::vertical()
                    .show(ui, |ui| self.library_panel.show(ui))
                    .inner;
                if let Some(project_id) = selected {
                    self.send(Command::OpenProject { project_id }, None, None);
                }
            }
            Tab::Plugins => {
                let current = self
                    .project
                    .as_ref()
                    .map(|project| (project.project_id.clone(), project.revision));
                let selected = egui::ScrollArea::vertical()
                    .show(ui, |ui| self.wasm_panel.show(ui, current))
                    .inner;
                if let Some(candidate_id) = selected {
                    self.project_command(Command::GetCandidate { candidate_id }, false);
                }
            }
            Tab::Evidence => {
                egui::ScrollArea::vertical().show(ui, |ui| self.evidence_panel.show(ui));
            }
            Tab::Stash => {
                egui::ScrollArea::vertical().show(ui, |ui| self.stash_panel(ui));
            }
            Tab::Doctor => {
                egui::ScrollArea::vertical().show(ui, |ui| self.doctor(ui));
            }
            Tab::Portable => {
                let current = self
                    .project
                    .as_ref()
                    .map(|project| (project.project_id.clone(), project.revision));
                let selected = egui::ScrollArea::vertical()
                    .show(ui, |ui| self.portable.show(ui, current, !self.busy))
                    .inner;
                if let Some(project_id) = selected {
                    self.send(Command::OpenProject { project_id }, None, None);
                }
            }
            _ => {
                egui::ScrollArea::vertical().show(ui, |ui| self.capability_panel(ui));
            }
        });
        if !self.busy && self.preview_pending {
            self.request_preview();
        }
        if !self.busy && now.duration_since(self.last_job_poll) >= Duration::from_millis(750) {
            if let Some(job) = self.job.as_ref() {
                if job.candidate_id.is_none()
                    && job.error.is_none()
                    && !matches!(
                        job.state.to_ascii_lowercase().as_str(),
                        "cancelled" | "canceled" | "failed" | "interrupted"
                    )
                {
                    let command = Command::JobStatus {
                        job_id: job.job_id.clone(),
                    };
                    self.last_job_poll = now;
                    self.project_command(command, false);
                }
            }
        }
        if self.busy || self.job.is_some() {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }
}

fn selected_detector_score_threshold(
    custom: bool,
    score: f32,
    contract: Option<&ModelInputContract>,
) -> Option<f32> {
    (custom
        && contract.is_some_and(|input| input.decoder != DetectorDecoder::TemporalScriptPosition))
    .then_some(score)
}

#[cfg(test)]
mod detector_score_tests {
    use super::*;
    #[test]
    fn generation_and_batch_score_choice_requires_an_applicable_detector() {
        let detector = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        assert_eq!(
            selected_detector_score_threshold(true, 0.75, Some(&detector)),
            Some(0.75)
        );
        assert_eq!(
            selected_detector_score_threshold(false, 0.75, Some(&detector)),
            None
        );
        assert_eq!(selected_detector_score_threshold(true, 0.75, None), None);
        let temporal = ModelInputContract {
            channels: 6,
            decoder: DetectorDecoder::TemporalScriptPosition,
            ..detector.clone()
        };
        assert_eq!(
            selected_detector_score_threshold(true, 0.75, Some(&temporal)),
            None
        );
        let corrupt = GenerationSettings {
            model_input: Some(detector.clone()),
            detector_score_threshold: selected_detector_score_threshold(
                true,
                f32::NAN,
                Some(&detector),
            ),
            ..Default::default()
        };
        assert!(corrupt.validate().is_err());
    }
}

#[cfg(test)]
mod source_kind_tests {
    use super::{independent_audio_rate, independent_audio_selection, select_media_source};
    use pulsar_protocol::{SourceKind, SourceSummary, SourceVersionId};

    fn source(id: &str, label: &str, kind: SourceKind) -> SourceSummary {
        SourceSummary {
            source_version: SourceVersionId::new(id).unwrap(),
            label: label.into(),
            kind,
        }
    }

    #[test]
    fn independent_audio_selection_refuses_stale_same_and_nonmedia_sources() {
        let sources = vec![
            source("video", "Video", SourceKind::Media),
            source("audio", "Audio", SourceKind::Media),
            source("script", "Audio.wav", SourceKind::Funscript),
        ];
        let primary = Some(&sources[0].source_version);
        let selected = independent_audio_selection(
            &sources,
            primary,
            Some(&sources[1].source_version),
            [(0, 1), (1, 8)],
            (1, 1),
        )
        .unwrap();
        assert_eq!(selected.source_version, sources[1].source_version);
        assert_eq!(
            selected.alignment.audio_anchor,
            pulsar_protocol::SourceTimestamp::new(1, 8).unwrap()
        );
        for candidate in [None, primary, Some(&sources[2].source_version)] {
            assert!(independent_audio_selection(
                &sources,
                primary,
                candidate,
                [(0, 1), (0, 1)],
                (1, 1)
            )
            .is_err());
        }
        assert!(independent_audio_selection(
            &sources[..1],
            primary,
            Some(&sources[1].source_version),
            [(0, 1), (0, 1)],
            (1, 1)
        )
        .is_err());
        assert!(independent_audio_selection(
            &sources,
            primary,
            Some(&sources[1].source_version),
            [(0, 0), (0, 1)],
            (1, 1)
        )
        .is_err());
    }

    #[test]
    fn audio_rate_text_preserves_full_integer_precision() {
        assert_eq!(
            independent_audio_rate("9223372036854775807", "4294967295").unwrap(),
            (i64::MAX, u32::MAX)
        );
        assert_eq!(
            independent_audio_rate("9007199254740993", "1").unwrap(),
            (9_007_199_254_740_993, 1)
        );
        for (n, d) in [
            ("9223372036854775808", "1"),
            ("1", "4294967296"),
            ("0", "1"),
            ("-1", "1"),
            ("1", "0"),
            ("1.5", "1"),
            ("", "1"),
        ] {
            assert!(independent_audio_rate(n, d).is_err());
        }
    }

    #[test]
    fn independent_audio_selection_retains_rate_and_refuses_invalid_rates() {
        let sources = vec![
            source("video", "Video", SourceKind::Media),
            source("audio", "Audio", SourceKind::Media),
        ];
        let select = |rate| {
            independent_audio_selection(
                &sources,
                Some(&sources[0].source_version),
                Some(&sources[1].source_version),
                [(0, 1), (1, 8)],
                rate,
            )
        };
        assert!(select((2, 2)).unwrap().alignment.rate.is_none());
        let selected = select((1001, 1000)).unwrap();
        assert_eq!(selected.alignment.rate.unwrap().numerator(), 1001);
        assert_eq!(selected.alignment.rate.unwrap().denominator(), 1000);
        for rate in [(0, 1), (-1, 1), (1, 0)] {
            assert!(select(rate).is_err());
        }
    }

    #[test]
    fn synthetic_and_script_sources_never_become_media_from_their_labels() {
        let sources = vec![
            source("text", "movie.mp4", SourceKind::GenerationInput),
            source("script", "image.png", SourceKind::Funscript),
        ];
        assert_eq!(select_media_source(&sources, None), None);
        for source in &sources {
            assert_eq!(
                select_media_source(&sources, Some(&source.source_version)),
                None
            );
        }
    }

    #[test]
    fn media_selection_skips_nonmedia_and_preserves_an_existing_media_choice() {
        let sources = vec![
            source("text", "Generation input", SourceKind::GenerationInput),
            source("media-one", "script.funscript", SourceKind::Media),
            source("script", "movie.mp4", SourceKind::Funscript),
            source("media-two", "Generation input", SourceKind::Media),
        ];
        assert_eq!(
            select_media_source(&sources, None),
            Some(sources[1].source_version.clone())
        );
        assert_eq!(
            select_media_source(&sources, Some(&sources[3].source_version)),
            Some(sources[3].source_version.clone())
        );
        assert_eq!(
            select_media_source(&sources, Some(&sources[0].source_version)),
            Some(sources[1].source_version.clone())
        );
    }

    #[test]
    fn removed_or_reclassified_media_is_not_retained_after_snapshot_refresh() {
        let selected = SourceVersionId::new("old-media").unwrap();
        assert_eq!(select_media_source(&[], Some(&selected)), None);
        let sources = vec![source(
            "old-media",
            "movie.mp4",
            SourceKind::GenerationInput,
        )];
        assert_eq!(select_media_source(&sources, Some(&selected)), None);
    }
}

#[cfg(test)]
mod diagnostics_freshness_tests {
    use super::accept_diagnostics_report;
    use pulsar_protocol::{CandidateId, DiagnosticsReport, ProjectId, RevisionId};

    fn report(project: &str, revision: u64, candidate: Option<&str>) -> DiagnosticsReport {
        DiagnosticsReport {
            project_id: ProjectId::new(project).unwrap(),
            revision: RevisionId::new(revision),
            candidate_id: candidate.map(|id| CandidateId::new(id).unwrap()),
            base_revision: candidate.map(|_| RevisionId::new(revision)),
            protected_regions: Vec::new(),
            segment_speed: None,
            issues: Vec::new(),
        }
    }

    #[test]
    fn late_old_project_and_revision_reports_do_not_replace_current_diagnostics() {
        let project = ProjectId::new("current-project").unwrap();
        let view = Some((&project, RevisionId::new(7)));
        let mut current = None;
        assert!(accept_diagnostics_report(
            &mut current,
            report("current-project", 7, None),
            view,
            None
        ));
        assert!(!accept_diagnostics_report(
            &mut current,
            report("old-project", 7, None),
            view,
            None
        ));
        assert!(!accept_diagnostics_report(
            &mut current,
            report("current-project", 6, None),
            view,
            None
        ));
        let retained = current.unwrap();
        assert_eq!(retained.project_id, project);
        assert_eq!(retained.revision, RevisionId::new(7));
    }

    #[test]
    fn candidate_diagnostics_require_the_visible_candidate_identity() {
        let project = ProjectId::new("project").unwrap();
        let candidate = CandidateId::new("visible-candidate").unwrap();
        let view = Some((&project, RevisionId::new(7)));
        let mut current = None;
        assert!(!accept_diagnostics_report(
            &mut current,
            report("project", 7, Some("old-candidate")),
            view,
            Some(&candidate)
        ));
        assert!(!accept_diagnostics_report(
            &mut current,
            report("project", 7, Some("visible-candidate")),
            view,
            None
        ));
        assert!(accept_diagnostics_report(
            &mut current,
            report("project", 7, Some("visible-candidate")),
            view,
            Some(&candidate)
        ));
        assert_eq!(
            current.as_ref().unwrap().candidate_id.as_ref(),
            Some(&candidate)
        );
    }

    #[test]
    fn diagnostics_for_an_unopened_project_are_never_accepted() {
        let mut current = None;
        assert!(!accept_diagnostics_report(
            &mut current,
            report("project", 7, None),
            None,
            None
        ));
        assert!(current.is_none());
    }
}

use crate::motion::{CandidateSnapshot, EditProposal, ProjectSnapshot, ResponseBody};

#[cfg(test)]
mod edit_transport_tests {
    use super::*;
    use crate::motion::{fixture_descriptor, EditProposal};

    struct EditApi {
        calls: Vec<&'static str>,
        conflict: bool,
    }
    impl EngineApi for EditApi {
        fn upload_edit(
            &mut self,
            project: ProjectId,
            revision: RevisionId,
            program: &MotionProgram,
            _: &str,
        ) -> anyhow::Result<CandidateSnapshot> {
            assert_eq!(project.as_str(), "project");
            assert_eq!(revision.value(), 4);
            self.calls.push("upload");
            let candidate_id = CandidateId::new("edit-candidate").unwrap();
            Ok(CandidateSnapshot {
                project_interval: None,
                candidate_id: candidate_id.clone(),
                project_id: project.clone(),
                base_revision: revision,
                motion: fixture_descriptor(
                    program,
                    MotionBinding::Candidate {
                        project_id: project,
                        candidate_id,
                        base_revision: revision,
                    },
                ),
                program: program.clone().into(),
                job_id: None,
                review: Vec::new(),
            })
        }
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> anyhow::Result<ResponseBody> {
            assert!(
                matches!(command, Command::CommitCandidate { candidate_id } if candidate_id.as_str() == "edit-candidate")
            );
            assert_eq!(project.unwrap().as_str(), "project");
            assert_eq!(revision, Some(RevisionId::new(4)));
            self.calls.push("commit");
            if self.conflict {
                Err(ProtocolError::new(ErrorCode::RevisionConflict, "stale base").into())
            } else {
                Ok(ResponseBody::Ack)
            }
        }
    }
    fn draft() -> RpcWork {
        RpcWork {
            command: RpcCommand::Edit(EditProposal {
                program: MotionProgram::default(),
                label: "Edit".into(),
            }),
            project: Some(ProjectId::new("project").unwrap()),
            revision: Some(RevisionId::new(4)),
        }
    }
    fn repair_work() -> RpcWork {
        let program = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            [(0, 0.0), (20, 1.0)]
                .into_iter()
                .map(|(t, p)| {
                    MotionAction::new(
                        ProjectTime::from_nanos(t * 1_000_000),
                        NormalizedPosition::new(p).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()])
        .unwrap();
        RpcWork {
            command: RpcCommand::Repair(crate::repair_panel::RepairRequest {
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(4),
                program: std::sync::Arc::new(program),
                options: ScriptRepairOptions::new(450.0, 20, 2).unwrap(),
            }),
            project: Some(ProjectId::new("project").unwrap()),
            revision: Some(RevisionId::new(4)),
        }
    }
    #[test]
    fn repair_rpc_runs_as_background_candidate_work_without_commit() {
        let parent = std::thread::current().id();
        let (result, calls) = std::thread::spawn(move || {
            assert_ne!(std::thread::current().id(), parent);
            let mut api = EditApi {
                calls: vec![],
                conflict: false,
            };
            let result = execute_work(&mut api, repair_work());
            (result, api.calls)
        })
        .join()
        .unwrap();
        assert!(matches!(result.unwrap(), ResponseBody::Candidate(_)));
        assert_eq!(calls, vec!["upload"]);
    }
    #[test]
    fn repair_rpc_refuses_missing_or_rebound_context_before_effects() {
        let mut api = EditApi {
            calls: vec![],
            conflict: false,
        };
        for (project, revision) in [
            (None, Some(RevisionId::new(4))),
            (
                Some(ProjectId::new("other").unwrap()),
                Some(RevisionId::new(4)),
            ),
            (Some(ProjectId::new("project").unwrap()), None),
        ] {
            let mut work = repair_work();
            work.project = project;
            work.revision = revision;
            assert!(execute_work(&mut api, work).is_err());
        }
        assert!(api.calls.is_empty());
    }
    #[test]
    fn gui_draft_uses_shared_upload_then_normal_pinned_candidate_commit() {
        let mut api = EditApi {
            calls: Vec::new(),
            conflict: false,
        };
        assert!(matches!(
            execute_work(&mut api, draft()).unwrap(),
            ResponseBody::Ack
        ));
        assert_eq!(api.calls, vec!["upload", "commit"]);
    }
    #[test]
    fn stale_gui_commit_retains_candidate_identity_and_is_never_replayed() {
        let mut api = EditApi {
            calls: Vec::new(),
            conflict: true,
        };
        let error = execute_work(&mut api, draft()).unwrap_err();
        assert!(error.to_string().contains("edit-candidate"));
        assert!(error.to_string().contains("stale base"));
        assert_eq!(
            error.downcast_ref::<ProtocolError>().unwrap().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(api.calls, vec!["upload", "commit"]);
    }
}

#[cfg(test)]
mod placement_snapshot_seek_tests {
    use super::*;
    fn placement(anchor: i64, rate: i32) -> SourcePlacementSpec {
        SourcePlacementSpec {
            source_version: SourceVersionId::new("source").unwrap(),
            source_start: SourceTimestamp::new(1, 2).unwrap(),
            source_end: SourceTimestamp::new(7, 2).unwrap(),
            source_anchor: SourceTimestamp::new(1, 2).unwrap(),
            project_anchor: ProjectTime::from_nanos(anchor),
            rate_numerator: rate,
            rate_denominator: 1,
        }
    }
    #[test]
    fn saved_placement_previews_valid_anchor_instead_of_out_of_trim_old_playhead() {
        for (anchor, rate) in [(1_000_000_000, 2), (-2_000_000_000, -2)] {
            let spec = placement(anchor, rate);
            spec.validate().unwrap();
            assert!(spec.source_at_project(ProjectTime::from_nanos(0)).is_err());
            for source_changed in [false, true] {
                let seconds =
                    snapshot_preview_seek(0.0, Some(&spec), true, source_changed).unwrap();
                assert_eq!(seconds, anchor as f64 / 1e9);
                assert_eq!(
                    spec.source_at_project(ProjectTime::from_nanos((seconds * 1e9).round() as i64))
                        .unwrap(),
                    ExactSeconds::from_source(spec.source_anchor)
                );
            }
        }
    }
    #[test]
    fn refresh_preserves_seek_and_original_clip_selection_preserves_reset_semantics() {
        let spec = placement(1_000_000_000, 2);
        assert_eq!(snapshot_preview_seek(1.25, Some(&spec), false, false), None);
        assert_eq!(
            snapshot_preview_seek(1.25, Some(&spec), false, true),
            Some(1.0)
        );
        assert_eq!(snapshot_preview_seek(1.25, None, false, true), Some(0.0));
        assert_eq!(snapshot_preview_seek(1.25, None, true, false), Some(1.25));
        assert_eq!(snapshot_preview_seek(1.25, None, false, false), None);
    }
}

fn image_origin_scope_matches(
    report: &CandidateImageOriginReport,
    candidate: &CandidateSnapshot,
) -> bool {
    report.project_id == candidate.project_id
        && report.candidate_id == candidate.candidate_id
        && report.base_revision == candidate.base_revision
        && report.motion == candidate.motion
}

#[cfg(test)]
mod image_origin_scope_tests {
    use super::*;
    #[test]
    fn candidate_origin_presentation_rejects_changed_namespace_revision_and_motion() {
        let program = MotionProgram::new(vec![]).unwrap();
        let project_id = ProjectId::new("project").unwrap();
        let candidate_id = CandidateId::new("candidate").unwrap();
        let base_revision = RevisionId::new(4);
        let motion = crate::motion::fixture_descriptor(
            &program,
            MotionBinding::Candidate {
                project_id: project_id.clone(),
                candidate_id: candidate_id.clone(),
                base_revision,
            },
        );
        let candidate = CandidateSnapshot {
            project_interval: None,
            project_id: project_id.clone(),
            candidate_id: candidate_id.clone(),
            base_revision,
            motion: motion.clone(),
            program: program.into(),
            job_id: None,
            review: vec![],
        };
        // Scope comparison is independent of the typed evidence validator;
        // native Engine tests exercise valid retained records and mutation refusals.
        let report = CandidateImageOriginReport {
            project_id,
            candidate_id,
            base_revision,
            motion,
            origins: vec![],
            observed_temporal_motion: false,
            execution_authority: false,
            qualified: false,
        };
        assert!(image_origin_scope_matches(&report, &candidate));
        assert!(report.validate().is_err());
        let mut changed = report.clone();
        changed.project_id = ProjectId::new("other").unwrap();
        assert!(!image_origin_scope_matches(&changed, &candidate));
        let mut changed = report.clone();
        changed.candidate_id = CandidateId::new("other").unwrap();
        assert!(!image_origin_scope_matches(&changed, &candidate));
        let mut changed = report.clone();
        changed.base_revision = RevisionId::new(5);
        assert!(!image_origin_scope_matches(&changed, &candidate));
        let mut changed = report;
        changed.motion.program.sha256 = "0".repeat(64);
        assert!(!image_origin_scope_matches(&changed, &candidate));
    }
}
