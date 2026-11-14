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
#[path = "../candidate_evidence.rs"]
mod candidate_evidence;
#[path = "../inference_fallback_panel.rs"]
mod inference_fallback_panel;
use crate::motion::{CandidateSnapshot, EditProposal, ProjectSnapshot, ResponseBody};
use pulsar_protocol::*;
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Player,
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
    Roleplay,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratorInputMode {
    Video,
    Text,
    Patterns,
    Image,
    Audio,
    Funscript,
}
impl GeneratorInputMode {
    pub(super) fn label(self) -> &'static str {
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

mod generator_ui;
mod panels_ui;
mod rpc;
mod stash_ui;
#[cfg(test)]
mod tests;
use rpc::*;

/// This contains view state and disposable edit drafts, never authoritative
/// project mutation or device authority. Each window has an independent clock.
pub struct PulsarDesktop {
    sender: SyncSender<RpcWork>,
    receiver: Receiver<RpcReply>,
    pub(super) busy: bool,
    pub(super) tab: Tab,
    pub(super) browser_live_panel: crate::browser_live_panel::BrowserLivePanel,
    pub(super) browser_close_pending: bool,
    pub(super) project: Option<ProjectSnapshot>,
    pub(super) capabilities: Option<Capabilities>,
    pub(super) wasm_panel: crate::wasm_panel::WasmPanel,
    pub(super) native_preparation_panel: crate::native_preparation_panel::NativePreparationPanel,
    pub(super) assistant_panel: crate::assistant_panel::AssistantPanel,
    pub(super) inference_catalog: Option<InferenceCatalog>,
    pub(super) job: Option<JobSnapshot>,
    pub(super) job_attempts: Option<JobAttemptPage>,
    pub(super) candidate: Option<CandidateSnapshot>,
    pub(super) open_candidate_id: String,
    pub(super) source: Option<SourceVersionId>,
    pub(super) selected_placement: Option<SourcePlacementId>,
    pub(super) placement_times: [f64; 4],
    pub(super) placement_rate: (i32, u32),
    pub(super) placement_save_pending: bool,
    pub(super) retime_from: String,
    pub(super) retime_axes: Vec<Axis>,
    pub(super) retime_status_revision: u64,
    pub(super) retime_receipt: Option<(String, String)>,
    pub(super) archived_generation: Option<pulsar_protocol::ArchivedGenerationReport>,
    candidate_evidence: candidate_evidence::SummaryCache,
    pub(super) candidate_image_origin: Option<CandidateImageOriginReport>,
    pub(super) candidate_image_pixels: Option<(CandidateImageOriginPixels, egui::TextureHandle)>,
    pub(super) pattern_reviews: crate::pattern_review::PatternReviews,
    pub(super) pending_image_record: Option<String>,
    pub(super) preview: PreviewState,
    pub(super) texture: Option<egui::TextureHandle>,
    pub(super) previous_texture: Option<egui::TextureHandle>,
    pub(super) preview_pending: bool,
    pub(super) request_generation: u64,
    pub(super) seek_generation: u64,
    pub(super) playhead_seconds: f64,
    pub(super) preview_playing: bool,
    pub(super) preview_source_end: Option<(SourceVersionId, f64)>,
    pub(super) last_tick: Instant,
    pub(super) last_preview: Instant,
    pub(super) last_job_poll: Instant,
    pub(super) project_name: String,
    pub(super) open_project_id: String,
    pub(super) consent_path: String,
    pub(super) consent_summary: Option<ConsentSummary>,
    pub(super) source_path: String,
    pub(super) export_path: String,
    pub(super) model_path: String,
    pub(super) pack_panel: crate::pack_panel::PackPanel,
    pub(super) equivalence_panel: crate::equivalence_panel::EquivalencePanel,
    pub(super) bundled_models: crate::bundled_models::BundledModelPanel,
    pub(super) assistant_settings: crate::assistant_settings::AssistantSettingsPanel,
    pub(super) roleplay_panel: crate::roleplay_panel::RoleplayPanel,
    pub(super) evidence_panel: crate::evidence_panel::EvidencePanel,
    pub(super) use_selected_pack: bool,
    fallback_panel: inference_fallback_panel::FallbackPanel,
    fallback_session: Option<inference_fallback_panel::SessionBinding>,
    pub(super) inference_profile_path: String,
    pub(super) dynamics_profile_path: String,
    pub(super) inference_profile: Option<DeclaredInferenceProfile>,
    pub(super) annotated_view_path: String,
    pub(super) annotated_view: Option<AnnotatedGenerationView>,
    pub(super) author_target_mode: bool,
    pub(super) target_author: crate::target_view_authoring::TargetViewAuthor,
    pub(super) authored_target_id: String,
    pub(super) video_view: Option<VideoView>,
    pub(super) model_classes: u32,
    pub(super) model_width: u32,
    pub(super) model_height: u32,
    pub(super) use_model: bool,
    pub(super) use_detector_score_threshold: bool,
    pub(super) detector_score_threshold: f32,
    pub(super) use_analysis_dimensions: bool,
    pub(super) analysis_dimensions: [u32; 2],
    pub(super) use_generation_interval: bool,
    pub(super) generation_interval_ns: [i64; 2],
    pub(super) use_target_seed: bool,
    pub(super) target_seed_frame: u64,
    pub(super) target_seed_point: [f64; 2],
    pub(super) target_seed_filter_class: bool,
    pub(super) target_seed_class: u32,
    pub(super) target_seed_binding: Option<(SourceVersionId, TransformId)>,
    pub(super) pick_target_mode: bool,
    pub(super) target_picker: crate::target_picker::TargetPicker,
    pub(super) preset: String,
    pub(super) use_mixed_audio: bool,
    pub(super) use_independent_audio: bool,
    pub(super) independent_audio_source: Option<SourceVersionId>,
    pub(super) audio_alignment_anchors: [(i64, u32); 2],
    pub(super) audio_clock_rate: (String, String),
    pub(super) creative_audio: bool,
    pub(super) status: String,
    pub(super) startup_warning: Option<String>,
    pub(super) new_position: f64,
    pub(super) gesture: Option<EditGesture>,
    pub(super) timeline_axis: Axis,
    pub(super) generation_mode: GeneratorInputMode,
    pub(super) pattern_prompt: String,
    pub(super) audio_panel: crate::audio_panel::AudioPanel,
    pub(super) script_import_path: String,
    pub(super) repair_panel: crate::repair_panel::RepairPanel,
    pub(super) desktop_player: crate::desktop_player_panel::PlayerPanel,
    pub(super) handy_panel: crate::handy_panel::HandyPanel,
    pub(super) handy_registration_panel: crate::handy_registration_panel::HandyRegistrationPanel,
    pub(super) diagnostics: Option<DiagnosticsReport>,
    pub(super) segment_speed_limit: f64,
    pub(super) review_status: Option<ReviewStatus>,
    pub(super) review_note: String,
    pub(super) review_partial: bool,
    pub(super) review_interval_ns: (i64, i64),
    pub(super) review_retry: Option<(RequestId, Command, ProjectId, RevisionId)>,
    pub(super) merge_axes: Vec<Axis>,
    pub(super) merge_range_enabled: bool,
    pub(super) merge_start_seconds: f64,
    pub(super) merge_end_seconds: f64,
    pub(super) portable: PortablePanel,
    pub(super) owner_directory: PathBuf,
    pub(super) stash_endpoint: String,
    pub(super) stash_credential_env: String,
    pub(super) stash_discovery: Option<StashDiscovery>,
    pub(super) stash_mappings_path: String,
    pub(super) stash_annotated_views_path: String,
    pub(super) stash_source_plans_path: String,
    pub(super) stash_plan: Option<StashPlan>,
    pub(super) stash_capture: Option<StashSourceCapture>,
    pub(super) stash_export: Option<StashScriptExport>,
    pub(super) stash_source_time: bool,
    pub(super) stash_use_archived: bool,
    pub(super) stash_archived_namespace: String,
    pub(super) stash_require_selected_source: bool,
    pub(super) stash_publication_request: String,
    pub(super) stash_publication_report: Option<String>,
    pub(super) stash_scan_request: String,
    pub(super) stash_scan: Option<StashScanReceipt>,
    pub(super) stash_tag_name: String,
    pub(super) stash_create_tag: bool,
    pub(super) stash_tag_request: String,
    pub(super) stash_tag_resume_request: String,
    pub(super) stash_tag: Option<StashTagReceipt>,
    pub(super) library_panel: crate::library_panel::LibraryPanel,
    pub(super) reference_panel: crate::reference_panel::ReferencePanel,
    pub(super) batch_endpoint: PathBuf,
    pub(super) batch_bootstrap: PathBuf,
    pub(super) stash_batch: Option<Receiver<Result<crate::stash_flow::BatchReport, String>>>,
    pub(super) stash_batch_report: Option<String>,
    pub(super) stash_batch_id: String,
    pub(super) stash_batch_offset: u32,
    pub(super) stash_batch_has_more: bool,
    pub(super) stash_batch_item_count: u32,
    pub(super) stash_batch_scan: bool,
    pub(super) stash_batch_tag_requested: bool,
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
        let desktop_player =
            crate::desktop_player_panel::PlayerPanel::new(endpoint.clone(), bootstrap.clone());
        let roleplay_panel = crate::roleplay_panel::RoleplayPanel::new(
            endpoint.clone(),
            bootstrap.clone(),
            context.egui_ctx.clone(),
        );
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
            tab: Tab::Player,
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
            open_candidate_id: String::new(),
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
            preview_source_end: None,
            last_tick: Instant::now(),
            last_preview: Instant::now(),
            last_job_poll: Instant::now(),
            project_name: "Untitled motion".into(),
            open_project_id: String::new(),
            consent_path: String::new(),
            consent_summary: None,
            source_path: String::new(),
            export_path: String::new(),
            model_path: String::new(),
            pack_panel: Default::default(),
            equivalence_panel: Default::default(),
            bundled_models: Default::default(),
            assistant_settings: Default::default(),
            roleplay_panel,
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
            desktop_player,
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

    pub(super) fn send(
        &mut self,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> bool {
        self.enqueue(RpcCommand::Wire(command), project, revision)
    }

    pub(super) fn send_edit(
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

    pub(super) fn project_command(&mut self, command: Command, revision_required: bool) -> bool {
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

    pub(super) fn poll(&mut self, context: &egui::Context) {
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

    pub(super) fn display_preview(
        &mut self,
        result: PreviewResult,
        frame: Option<Result<DecodedFrame, String>>,
        previous_frame: Option<Result<DecodedFrame, String>>,
        context: &egui::Context,
    ) {
        if !self.preview.accept(result) {
            return;
        }
        if let Some(result) = self.preview.displayed() {
            if let Some(end) = result.last_source_elapsed_time {
                let seconds = end.numerator() as f64 / f64::from(end.denominator());
                self.preview_source_end = Some((result.context.source_version.clone(), seconds));
                if self.preview_playing
                    && self.selected_placement.is_none()
                    && result.elapsed_source_time == Some(end)
                {
                    self.preview_playing = false;
                    self.preview_pending = false;
                    self.playhead_seconds = seconds;
                    self.status = "Preview reached the last source frame.".into();
                }
            }
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

    pub(super) fn preview_placement(&self) -> Option<&SourcePlacementSpec> {
        let selected = self.selected_placement.as_ref()?;
        self.project
            .as_ref()?
            .placements
            .iter()
            .find(|p| &p.placement_id == selected)
            .map(|p| &p.spec)
    }

    pub(super) fn seek(&mut self, seconds: f64) {
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

    pub(super) fn submit_pack_action(&mut self, action: crate::pack_panel::PackAction) {
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
        self.roleplay_panel.update(
            self.project.as_ref().map(|project| &project.project_id),
            Path::new(self.consent_path.trim()),
            self.desktop_player.selected_media(),
        );
        self.portable.poll();
        let watch_media_reviews = Path::new(self.consent_path.trim()).is_absolute();
        self.desktop_player.poll(watch_media_reviews);
        if watch_media_reviews || self.desktop_player.needs_repaint() {
            context.request_repaint_after(Duration::from_millis(100));
        }
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
            let seconds = match &self.preview_source_end {
                Some((source, end))
                    if self.selected_placement.is_none()
                        && self.source.as_ref() == Some(source) =>
                {
                    // Requests use integer nanoseconds. Round the cap up so
                    // fractional final PTS values still select the final frame.
                    seconds.min((end * 1e9).ceil() / 1e9)
                }
                _ => seconds,
            };
            if self.preview_placement().is_some_and(|placement| {
                placement
                    .source_at_project(ProjectTime::from_nanos((seconds * 1e9).round() as i64))
                    .is_err()
            }) {
                self.preview_playing = false;
                self.status = "Preview reached the source placement boundary.".into();
            } else {
                // A playback tick must not invalidate a frame still decoding.
                if !self.busy && now.duration_since(self.last_preview) >= Duration::from_millis(150)
                {
                    self.seek(seconds);
                } else {
                    self.playhead_seconds = seconds;
                }
                context.request_repaint_after(Duration::from_millis(30));
            }
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
                if self.tab != Tab::Player {
                    self.desktop_player.session_controls(ui);
                }
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
                            (Tab::Player, "Player"),
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
                            (Tab::Roleplay, "Roleplay"),
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
            Tab::Player => {
                self.desktop_player.ui(
                    ui,
                    Path::new(self.consent_path.trim()),
                    self.project.as_ref().map(|project| &project.project_id),
                );
            }
            Tab::Roleplay => self.roleplay_panel.ui(ui),
            Tab::Settings => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.consent_settings(ui);
                    ui.separator();
                    self.assistant_settings.ui(ui);
                });
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

pub(super) fn image_origin_scope_matches(
    report: &CandidateImageOriginReport,
    candidate: &CandidateSnapshot,
) -> bool {
    report.project_id == candidate.project_id
        && report.candidate_id == candidate.candidate_id
        && report.base_revision == candidate.base_revision
        && report.motion == candidate.motion
}
