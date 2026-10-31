#![forbid(unsafe_code)]
//! Versioned client and worker wire contracts. This crate never authorizes an
//! operation, commits project state, loads a model, or actuates a device.

#[cfg(test)]
fn test_host_path(coordinate: &str) -> String {
    std::env::temp_dir()
        .join("pulsar-protocol-authored-fixtures")
        .join(coordinate.trim_start_matches('/'))
        .to_str()
        .expect("test temporary directory is UTF-8")
        .to_owned()
}

mod library_batch;
pub use library_batch::*;
mod media_probe;
pub use media_probe::*;
mod browser_analysis;
mod browser_analysis_input;
pub use browser_analysis_input::*;
mod browser_media;
mod browser_media_bulk;
pub use browser_analysis::*;
mod browser_timing;
pub use browser_media::*;
pub use browser_media_bulk::*;
pub use browser_timing::*;
mod assistant;
pub use assistant::*;
mod creative_patterns;
pub use creative_patterns::*;
mod native_image_preparation;
mod package_wasm;
pub use native_image_preparation::*;
pub use package_wasm::*;
mod handy;
mod handy_browser_start;
pub use handy_browser_start::*;
mod handy_clock_sync;
mod handy_continuous_timing;
mod handy_controller;
mod handy_hsp;
mod handy_hsp_source_qualification;
pub use handy_hsp_source_qualification::*;
mod handy_hsp_live;
pub use handy_hsp_live::*;
mod handy_hsp_continuity;
mod handy_hsp_preparation;
pub use handy_hsp_continuity::*;
mod handy_playback;
mod handy_playback_clock;
mod handy_player_guard;
mod handy_reconciliation;
mod handy_stop;
pub use handy_clock_sync::*;
pub use handy_continuous_timing::*;
pub use handy_hsp::*;
pub use handy_hsp_preparation::*;
pub use handy_playback::*;
pub use handy_playback_clock::*;
pub use handy_player_guard::*;
pub use handy_reconciliation::*;
pub use handy_stop::*;
mod handy_device_timing;
pub use handy_device_timing::*;
mod handy_server_clock;
pub use handy_server_clock::*;
mod handy_hosting;
mod handy_playback_preparation;
pub use handy_controller::*;
mod handy_program;
pub use handy_playback_preparation::*;
mod handy_qualification;
mod handy_settings;
mod handy_setup;
mod handy_setup_status;
pub use handy_program::*;
pub use handy_qualification::*;
pub use handy_settings::*;
pub use handy_setup::*;
pub use handy_setup_status::*;
mod handy_registration;
pub use handy_hosting::*;
mod handy_script;
pub use handy_script::*;
mod handy_session;
pub use handy_registration::*;
pub use handy_session::*;
mod wasm_processing;
pub use wasm_processing::*;
mod wasm_start_resolution;
pub use wasm_start_resolution::*;
mod wasm_archive_history;
pub use wasm_archive_history::*;
mod wasm_edits;
pub use wasm_edits::*;
mod live_capture;
pub use live_capture::*;
mod live_recording_assembly;
pub use live_recording_assembly::*;
mod live_inference;
pub use live_inference::*;
mod live_cuda;
pub use live_cuda::*;
mod live_cpu;
pub use live_cpu::*;
mod live_decode;
mod live_decode_delivery;
mod live_encoded;
mod live_encoded_client;
pub use live_decode::*;
pub use live_decode_delivery::*;
pub use live_encoded::*;
pub use live_encoded_client::*;
mod live_bulk;
pub use live_bulk::*;
mod live_clock_observation;
pub use handy::*;
pub use live_clock_observation::*;
mod annotated_preview;
mod bulk;
mod framing;
mod generation;
mod image_generation_origin;
pub use image_generation_origin::*;
mod cpu_runtime;
pub use cpu_runtime::*;
mod inference_environment;
mod inference_equivalence;
mod inference_fallback_selection;
pub use inference_fallback_selection::*;
mod inference_fallback_history;
pub use inference_fallback_history::*;
mod inference_profile;
mod job_attempts;
pub use annotated_preview::*;
pub use job_attempts::*;
mod temporal_recipe;
pub use temporal_recipe::*;
mod model_pack;
mod motion_retime_receipt;
pub use model_pack::*;
pub use motion_retime_receipt::*;
mod motion;
mod motion_evidence;
pub use motion_evidence::*;
mod project_package;
mod project_package_download;
pub mod project_package_format;
mod project_package_import_operations;
mod project_package_operations;
mod project_package_origins;
mod project_package_placements;
mod project_package_upload;
mod transport;
mod worker;
mod worker_cuda;
pub use worker_cuda::*;
mod native_capture;
pub use native_capture::*;
mod stash;
mod stash_source_plans;
pub use stash_source_plans::*;
mod review_dispositions;
mod source_placements;
pub use review_dispositions::*;
pub use source_placements::*;
pub use stash::*;

pub use bulk::*;
pub use framing::*;
pub use generation::*;
pub use inference_environment::*;
pub use inference_equivalence::*;
pub use inference_profile::*;
pub use motion::*;
pub use project_package::*;
pub use project_package_download::*;
pub use project_package_import_operations::*;
pub use project_package_operations::*;
pub use project_package_origins::*;
pub use project_package_placements::*;
pub use project_package_upload::*;
pub use pulsar_core::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use transport::*;
pub use worker::*;

pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_ARTIFACT_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u16,
    pub request_id: RequestId,
    pub session: Option<SessionId>,
    #[serde(default)]
    pub auth_token: Option<String>,
    pub project: Option<ProjectId>,
    pub expected_revision: Option<RevisionId>,
    pub command: Command,
}

impl Request {
    pub fn new(request_id: RequestId, command: Command) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            session: None,
            auth_token: None,
            project: None,
            expected_revision: None,
            command,
        }
    }

    pub fn in_project(mut self, project: ProjectId, revision: Option<RevisionId>) -> Self {
        self.project = Some(project);
        self.expected_revision = revision;
        self
    }

    pub fn with_session(mut self, session: SessionId) -> Self {
        self.session = Some(session);
        self
    }

    pub fn with_auth_token(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    /// Structural validation is not authorization. The engine must authenticate
    /// the session and check grants again at the effect/commit boundary.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        browser_analysis_input::validate_request(self)?;
        project_package_operations::validate_request(self)?;
        project_package_import_operations::validate_import_request(self)?;
        if let Some(token) = &self.auth_token {
            bounded_text(token, 4096, "authentication token")?;
        }
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::new(
                ErrorCode::Unsupported,
                "unsupported protocol version",
            ));
        }
        if self.command.requires_project() && self.project.is_none() {
            return Err(ProtocolError::invalid("operation requires project scope"));
        }
        if self.command.requires_revision() && self.expected_revision.is_none() {
            return Err(ProtocolError::invalid(
                "operation requires expected_revision",
            ));
        }
        handy_player_guard::validate_request(self)?;
        browser_media::validate_browser_media_request(self)?;
        browser_timing::validate_request(self)?;
        handy_browser_start::validate_request(self)?;
        handy_hsp_source_qualification::validate_request(self)?;
        handy_hsp_live::validate_handy_hsp_live_request(self)?;
        handy_hsp_continuity::validate_request(self)?;
        live_capture::validate_browser_analysis_successor_request(self)?;
        validate_inference_equivalence_request(self)?;
        match &self.command {
            Command::CandidateImageOriginPixels { record_sha256, .. } => {
                validate_sha256(record_sha256)?
            }
            Command::StartNativeImagePreparation { options, .. } => options.validate()?,
            Command::ResolveNativeImagePreparation {
                original_request_id,
            } if *original_request_id == self.request_id => {
                return Err(ProtocolError::invalid(
                    "resolution and original request IDs must differ",
                ))
            }
            Command::ApplyAssistantEdit { approval } => approval.validate()?,
            Command::StartAssistant { spec } => spec.validate()?,
            Command::ResolveAssistantStart { reference } => reference.validate()?,
            Command::StartWasmEdit { spec } => spec.validate()?,
            Command::ResolveWasmStart { reference } => {
                reference.validate()?;
                if reference.original_request_id == self.request_id {
                    return Err(ProtocolError::invalid(
                        "resolution and original request IDs must differ",
                    ));
                }
            }
            Command::ListArchivedWasmEdits { limit, .. } => validate_wasm_archive_limit(*limit)?,
            Command::RemoveUnsuccessfulWasmEdit { expected_state, .. }
                if !expected_state.is_unsuccessful_terminal() =>
            {
                return Err(ProtocolError::invalid(
                    "only unsuccessful terminal WASM attempts can be removed",
                ))
            }
            Command::Diagnostics {
                maximum_speed_percent_per_second: Some(limit),
                ..
            } => validate_segment_speed_limit(*limit).map_err(|_| {
                ProtocolError::invalid(
                    "segment speed limit must be finite and in (0, 100000000000] percent/second",
                )
            })?,
            Command::ContinueLiveCaptureAt { lease_ms, .. }
                if *lease_ms == 0 || *lease_ms > 30_000 =>
            {
                return Err(ProtocolError::invalid("capture lease must be 1..=30000ms"))
            }
            Command::StartLiveCapture { options, .. } => {
                options.validate()?;
                if options.continuation_mode != LiveContinuationMode::Immediate {
                    return Err(ProtocolError::invalid(
                        "prepared continuation cannot start without a predecessor",
                    ));
                }
            }
            Command::ContinueLiveCapture { options, .. } => options.validate()?,
            Command::SelectCatalogModelPack {
                manifest_sha256,
                binding,
                expected_selection_revision,
                approve_unqualified_native_execution,
                owner_proof,
            } => {
                validate_sha256(manifest_sha256)?;
                binding.validate()?;
                if !approve_unqualified_native_execution
                    || *expected_selection_revision >= i64::MAX as u64
                {
                    return Err(ProtocolError::invalid(
                        "catalog-bound approval must be explicit and revision fenced",
                    ));
                }
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::SelectModelPack {
                manifest_sha256,
                expected_selection_revision,
                approve_unqualified_native_execution,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "model pack owner capability")?;
                if *expected_selection_revision >= i64::MAX as u64
                    || manifest_sha256.is_some() != *approve_unqualified_native_execution
                {
                    return Err(ProtocolError::invalid("pack selection requires an explicit matching approval and bounded revision"));
                }
                if let Some(hash) = manifest_sha256 {
                    validate_sha256(hash)?;
                }
            }
            Command::WithdrawModelPackApproval {
                choice,
                expected_selection_revision,
                owner_proof,
                ..
            } => {
                choice.validate()?;
                if *expected_selection_revision >= i64::MAX as u64
                    || choice.selection_revision > *expected_selection_revision
                {
                    return Err(ProtocolError::invalid("withdrawal requires an issued approval and bounded current selection fence"));
                }
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::ModelPackSelectionStatus { owner_proof } => {
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::GenerateFromModelPack {
                choice,
                preset,
                settings,
                ..
            } => {
                choice.validate()?;
                bounded_text(preset, 64, "preset")?;
                if let Some(settings) = settings {
                    settings.validate()?;
                }
            }
            Command::RefreshModelCatalog {
                source,
                bootstrap,
                bootstrap_sha256,
                expected_catalog_revision,
                approve_bootstrap,
                owner_proof,
            } => {
                if !source.is_absolute()
                    || !bootstrap.is_absolute()
                    || source.as_os_str().len() > 4096
                    || bootstrap.as_os_str().len() > 4096
                    || !approve_bootstrap
                    || *expected_catalog_revision >= i64::MAX as u64
                {
                    return Err(ProtocolError::invalid("catalog refresh requires bounded absolute paths, revision and explicit bootstrap approval"));
                }
                validate_sha256(bootstrap_sha256)?;
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::CheckModelPackCatalog {
                bootstrap_sha256,
                manifest_sha256,
                expected_catalog_revision,
                owner_proof,
            } => {
                validate_sha256(bootstrap_sha256)?;
                validate_sha256(manifest_sha256)?;
                if *expected_catalog_revision == 0 || *expected_catalog_revision > i64::MAX as u64 {
                    return Err(ProtocolError::invalid(
                        "invalid catalog association revision",
                    ));
                }
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::ModelCatalogStatus {
                bootstrap_sha256,
                owner_proof,
            } => {
                validate_sha256(bootstrap_sha256)?;
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::ImportModelPack {
                source,
                owner_proof,
            } => {
                if !source.is_absolute() {
                    return Err(ProtocolError::invalid("pack source must be absolute"));
                }
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::DiscardModelPackStage {
                basename,
                manifest_sha256,
                owner_proof,
            } => {
                validate_stage_basename(basename)?;
                validate_sha256(manifest_sha256)?;
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::ModelPackStagingStatus { owner_proof } => {
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::ModelPackStatus {
                manifest_sha256,
                owner_proof,
            } => {
                validate_sha256(manifest_sha256)?;
                bounded_text(owner_proof, 64, "model pack owner capability")?;
            }
            Command::HandyQualificationContext { owner_proof, .. } => {
                validate_registration_scope(self, 0, owner_proof)?;
            }
            Command::ImportHandyQualification {
                manifest_path,
                manifest_sha256,
                owner_proof,
            } => {
                validate_registration_scope(self, 0, owner_proof)?;
                validate_sha256(manifest_sha256)?;
                let bytes = manifest_path.as_os_str().as_encoded_bytes();
                if !manifest_path.is_absolute()
                    || bytes.len() > 32768
                    || bytes.contains(&0)
                    || manifest_path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(ProtocolError::invalid(
                        "qualification manifest requires an absolute bounded path",
                    ));
                }
            }
            Command::HandyQualificationStatus {
                dossier_id,
                owner_proof,
            } => {
                validate_registration_scope(self, 0, owner_proof)?;
                validate_sha256(dossier_id.as_str())?;
            }
            Command::AcceptHandyQualification {
                dossier_id,
                expected_qualification_revision,
                note,
                acknowledge_physical_evidence,
                allow_online_inspection,
                owner_proof,
                ..
            } => {
                validate_registration_scope(self, *expected_qualification_revision, owner_proof)?;
                validate_sha256(dossier_id.as_str())?;
                validate_handy_qualification_note(note)?;
                if *expected_qualification_revision == 0
                    || *expected_qualification_revision >= i64::MAX as u64
                {
                    return Err(ProtocolError::invalid(
                        "invalid qualification review revision",
                    ));
                }
                if !acknowledge_physical_evidence || !allow_online_inspection {
                    return Err(ProtocolError::new(ErrorCode::Forbidden,"qualification acceptance requires explicit physical-evidence review and online-inspection consent"));
                }
            }
            Command::WithdrawHandyQualification {
                dossier_id,
                expected_qualification_revision,
                note,
                owner_proof,
            } => {
                validate_registration_scope(self, *expected_qualification_revision, owner_proof)?;
                validate_sha256(dossier_id.as_str())?;
                validate_handy_qualification_note(note)?;
                if *expected_qualification_revision == 0
                    || *expected_qualification_revision >= i64::MAX as u64
                {
                    return Err(ProtocolError::invalid(
                        "invalid qualification review revision",
                    ));
                }
            }
            Command::SetupHandyDevice {
                qualification,
                hosting_request_id,
                owner_proof,
                allow_online_setup,
                ..
            } => {
                qualification.validate()?;
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if hosting_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "setup requires its own request identity",
                    ));
                }
                if !allow_online_setup {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Handy setup requires explicit online setup consent",
                    ));
                }
            }
            Command::CancelHandySetup {
                setup_request_id,
                record_id,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                validate_sha256(record_id.as_str())?;
                if setup_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "setup cancellation requires its own request identity",
                    ));
                }
            }
            Command::HandySetupStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::AcquireHandyPlaybackClock {
                acquisition_request_id,
                expected_generation,
                synchronization,
                allow_online_timing,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                synchronization.validate()?;
                if *expected_generation > i64::MAX as u64
                    || acquisition_request_id == &self.request_id
                    || synchronization.sync_request_id == self.request_id
                {
                    return Err(ProtocolError::invalid(
                        "invalid playback clock request identity or generation",
                    ));
                }
                if !allow_online_timing {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "playback clock acquisition requires explicit online timing consent",
                    ));
                }
            }
            Command::SynchronizeHandyClock {
                acquisition_request_id,
                expected_generation,
                allow_online_synchronization,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if acquisition_request_id == &self.request_id
                    || *expected_generation > i64::MAX as u64
                {
                    return Err(ProtocolError::invalid(
                        "invalid synchronization request identity or generation",
                    ));
                }
                if !allow_online_synchronization {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "clock synchronization requires explicit effect consent",
                    ));
                }
            }
            Command::StartHandyPlayback {
                acquisition_request_id,
                expected_generation,
                clock_request_id,
                start_time_ms,
                allow_play,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if acquisition_request_id == &self.request_id
                    || clock_request_id == &self.request_id
                    || *expected_generation > i64::MAX as u64
                    || *start_time_ms >= (1u64 << 53)
                {
                    return Err(ProtocolError::invalid("invalid playback request selection"));
                }
                if !allow_play {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Handy playback requires explicit effect consent",
                    ));
                }
            }
            Command::HandyPlaybackStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::PrepareHandyHsp {
                engine_epoch,
                device_session,
                qualification,
                timeout_ms,
                allow_online_synchronization,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                validate_handy_hsp_preparation_selection(
                    engine_epoch,
                    device_session,
                    qualification,
                    *timeout_ms,
                )?;
                if !allow_online_synchronization {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "HSP preparation requires explicit online synchronization consent",
                    ));
                }
            }
            Command::CancelHandyHspPreparation {
                preparation_request_id,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if preparation_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "HSP preparation cancellation requires its own request identity",
                    ));
                }
            }
            Command::HandyHspPreparationStatus { owner_proof, .. }
            | Command::HandyHspStatus { owner_proof, .. }
            | Command::HandyStopStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::RefreshHandyPlaybackTiming {
                selection,
                allow_online_inspection,
                owner_proof,
            } => {
                selection.validate()?;
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if self.request_id == selection.play_request_id
                    || self.request_id == selection.acquisition_request_id
                {
                    return Err(ProtocolError::invalid(
                        "continuous timing refresh requires its own request identity",
                    ));
                }
                if !allow_online_inspection {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "continuous timing refresh requires explicit online inspection consent",
                    ));
                }
            }
            Command::HandyContinuousTimingStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::PlanHandyReconciliation {
                selection,
                owner_proof,
            } => {
                selection.validate()?;
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::ReconcileHandyEffects {
                plan_id,
                allow_online_inspection,
                allow_resolution,
                owner_proof,
            } => {
                validate_sha256(plan_id.as_str())?;
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if !allow_online_inspection || !allow_resolution {
                    return Err(ProtocolError::new(ErrorCode::Forbidden, "Handy reconciliation requires explicit online inspection and resolution consent"));
                }
            }
            Command::HandyReconciliationStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::CancelHandyReconciliation {
                reconciliation_request_id,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if reconciliation_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "reconciliation cancellation requires its own request identity",
                    ));
                }
            }
            Command::CancelHandyPlayback {
                play_request_id,
                record_id,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                validate_sha256(record_id.as_str())?;
                if play_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "play cancellation needs its own request ID",
                    ));
                }
            }
            Command::HandyClockSyncStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::CancelHandyClockSync {
                sync_request_id,
                record_id,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                validate_sha256(record_id.as_str())?;
                if sync_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "synchronization cancellation needs its own request ID",
                    ));
                }
            }
            Command::InspectHandyDeviceTiming {
                expected_generation,
                allow_online_inspection,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if *expected_generation > i64::MAX as u64 {
                    return Err(ProtocolError::invalid("invalid controller generation"));
                }
                if !allow_online_inspection {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "device timing requires explicit online inspection consent",
                    ));
                }
            }
            Command::SampleHandyServerClock {
                acquisition_request_id,
                expected_generation,
                allow_online_sampling,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if acquisition_request_id == &self.request_id
                    || *expected_generation > i64::MAX as u64
                {
                    return Err(ProtocolError::invalid(
                        "invalid server-clock request identity or generation",
                    ));
                }
                if !allow_online_sampling {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "server-clock acquisition requires explicit online sampling consent",
                    ));
                }
            }
            Command::AcquireHandyController {
                preparation_request_id,
                preparation_id,
                duration_ms,
                allow_arm,
                allow_online_inspection,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                validate_sha256(preparation_id.as_str())?;
                validate_handy_controller_duration(*duration_ms)?;
                if preparation_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "controller acquisition requires its own request ID",
                    ));
                }
                if !allow_arm || !allow_online_inspection {
                    return Err(ProtocolError::new(ErrorCode::Forbidden,"controller acquisition requires explicit local arming and online inspection consent"));
                }
            }
            Command::RenewHandyController {
                acquisition_request_id,
                expected_generation,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if acquisition_request_id == &self.request_id
                    || *expected_generation >= i64::MAX as u64
                {
                    return Err(ProtocolError::invalid(
                        "invalid controller renewal identity or generation",
                    ));
                }
            }
            Command::ReleaseHandyController {
                acquisition_request_id,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if acquisition_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "controller release requires its own request ID",
                    ));
                }
            }
            Command::HandyControllerStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::PrepareHandyPlayback {
                program,
                allow_online_inspection,
                owner_proof,
            } => {
                program.validate()?;
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if program.setup_request_id == self.request_id {
                    return Err(ProtocolError::invalid(
                        "preparation requires a separate request ID",
                    ));
                }
                if !allow_online_inspection {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Handy playback preparation requires explicit online inspection consent",
                    ));
                }
            }
            Command::HandyPlaybackPreparationStatus { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::RetireHandyPlaybackPreparation {
                preparation_request_id,
                preparation_id,
                owner_proof,
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                validate_sha256(preparation_id.as_str())?;
                if preparation_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "retirement requires its own request ID",
                    ));
                }
            }
            Command::InspectHandyProgram {
                qualification,
                record_id,
                setup_request_id,
                allow_online_inspection,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                qualification.validate()?;
                validate_sha256(record_id.as_str())?;
                if setup_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "program inspection needs a separate request ID",
                    ));
                }
                if !allow_online_inspection {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Handy program inspection requires explicit online consent",
                    ));
                }
            }
            Command::InspectHandySetup {
                qualification,
                allow_online_inspection,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if let Some(binding) = qualification {
                    binding.validate()?;
                }
                if !allow_online_inspection {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Handy setup inspection requires explicit online consent",
                    ));
                }
            }
            Command::HostHandyStrokeScript {
                preparation_request_id,
                script_sha256,
                allow_temporary_public_hosting,
                owner_proof,
                ..
            } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                validate_sha256(script_sha256)?;
                if preparation_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "Hosting requires its own original request ID",
                    ));
                }
                if !allow_temporary_public_hosting {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Handy temporary public hosting requires explicit consent",
                    ));
                }
            }
            Command::HandyHostingStatus { owner_proof, .. }
            | Command::ForgetHandyHosting { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Handy owner capability")?;
                if self.expected_revision.is_some() {
                    return Err(ProtocolError::invalid(
                        "Hosting history operations have no revision precondition",
                    ));
                }
            }
            Command::OpenHandySession {
                expected_registration_revision,
                binding_sha256,
                allow_online_inspection,
                owner_proof,
                ..
            } => {
                validate_registration_scope(self, *expected_registration_revision, owner_proof)?;
                validate_sha256(binding_sha256)?;
                if !allow_online_inspection {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Opening Handy requires explicit online inspection consent",
                    ));
                }
            }
            Command::CloseHandySession { owner_proof, .. }
            | Command::HandySessions { owner_proof } => {
                validate_registration_scope(self, 0, owner_proof)?;
            }
            Command::ResolveHandyRegistration {
                original_request_id,
                intent,
                owner_proof,
            } => {
                intent.validate()?;
                validate_registration_scope(self, intent.expected_revision(), owner_proof)?;
                if original_request_id == &self.request_id {
                    return Err(ProtocolError::invalid(
                        "resolution needs a distinct request ID",
                    ));
                }
            }
            Command::SetHandyRegistration {
                settings,
                expected_registration_revision,
                owner_proof,
                ..
            } => {
                settings.validate()?;
                validate_registration_scope(self, *expected_registration_revision, owner_proof)?;
            }
            Command::DisableHandyRegistration {
                expected_registration_revision,
                owner_proof,
                ..
            } => {
                validate_registration_scope(self, *expected_registration_revision, owner_proof)?;
            }
            Command::ListHandyRegistrations { owner_proof } => {
                validate_registration_scope(self, 0, owner_proof)?;
            }
            Command::InspectHandyRegistration {
                expected_registration_revision,
                binding_sha256,
                allow_online_inspection,
                owner_proof,
                ..
            } => {
                validate_registration_scope(self, *expected_registration_revision, owner_proof)?;
                validate_sha256(binding_sha256)?;
                if !allow_online_inspection {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "Handy online inspection requires explicit consent",
                    ));
                }
            }
            Command::HandyInspect {
                inspection,
                owner_proof,
            } => {
                inspection.validate()?;
                bounded_text(owner_proof, 64, "Handy owner capability")?;
            }
            Command::SetReviewDisposition {
                review_id, note, ..
            }
            | Command::SetReviewIntervalDisposition {
                review_id, note, ..
            } => validate_review_intent(review_id, note)?,
            Command::StashListMissing {
                connection,
                limit,
                owner_proof,
            }
            | Command::StashPlanMissing {
                connection,
                limit,
                owner_proof,
                ..
            } => {
                connection.validate()?;
                if let Command::StashPlanMissing { mappings, .. } = &self.command {
                    if mappings.is_empty() || mappings.len() > 16 {
                        return Err(ProtocolError::invalid(
                            "Stash requires 1..=16 root mappings",
                        ));
                    }
                    for mapping in mappings {
                        mapping.validate()?;
                    }
                }
                if *limit == 0 || *limit > 100 {
                    return Err(ProtocolError::invalid("Stash limit must be 1..=100"));
                }
                bounded_text(owner_proof, 64, "Stash owner capability")?;
            }
            Command::StashMigrateBatch {
                expected_ledger_sha256,
                generation,
                owner_proof,
                ..
            } => {
                validate_sha256(expected_ledger_sha256)?;
                generation.validate()?;
                bounded_text(owner_proof, 64, "Stash owner capability")?;
            }
            Command::StashStartBatch {
                options,
                owner_proof,
            } => {
                options.validate()?;
                bounded_text(owner_proof, 64, "Stash owner capability")?;
            }
            Command::StashPublicationStatus { owner_proof, .. }
            | Command::StashBatchStatus { owner_proof, .. }
            | Command::StashResumeBatch { owner_proof, .. }
            | Command::StashCancelBatch { owner_proof, .. } => {
                bounded_text(owner_proof, 64, "Stash owner capability")?;
            }
            Command::StashAddTag {
                connection,
                owner_proof,
                tag_name,
                ..
            } => {
                connection.validate()?;
                bounded_text(owner_proof, 64, "Stash owner capability")?;
                bounded_text(tag_name, 256, "Stash tag name")?;
                if tag_name.trim() != tag_name || tag_name.chars().any(char::is_control) {
                    return Err(ProtocolError::invalid(
                        "Stash tag name must be nonempty, trimmed and free of control characters",
                    ));
                }
            }
            Command::StashTagStatus {
                connection,
                owner_proof,
                ..
            }
            | Command::StashResumeTag {
                connection,
                owner_proof,
                ..
            } => {
                connection.validate()?;
                bounded_text(owner_proof, 64, "Stash owner capability")?;
            }
            Command::StashSubmitScan {
                connection,
                owner_proof,
                ..
            }
            | Command::StashScanStatus {
                connection,
                owner_proof,
                ..
            }
            | Command::StashReconcileAttachment {
                connection,
                owner_proof,
                ..
            } => {
                connection.validate()?;
                bounded_text(owner_proof, 64, "Stash owner capability")?;
            }
            Command::StashExportScript {
                connection,
                scene_id,
                file_id,
                owner_proof,
                archived_origin,
                ..
            } => {
                if let Some(origin) = archived_origin {
                    origin.validate()?;
                    if origin.byte_len > MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64 {
                        return Err(ProtocolError::invalid(
                            "archived publication namespace exceeds manifest bound",
                        ));
                    }
                }
                connection.validate()?;
                bounded_text(scene_id, 128, "Stash scene ID")?;
                bounded_text(file_id, 128, "Stash file ID")?;
                bounded_text(owner_proof, 64, "Stash owner capability")?;
            }
            Command::StashImportSource {
                connection,
                scene_id,
                file_id,
                expected_server_path,
                allow_multifile_selection,
                mappings,
                owner_proof,
                ..
            } => {
                if *allow_multifile_selection == Some(false)
                    || (allow_multifile_selection.is_some() && expected_server_path.is_none())
                {
                    return Err(ProtocolError::invalid(
                        "explicit multifile selection requires true and an expected server path",
                    ));
                }
                if let Some(path) = expected_server_path {
                    bounded_text(path, 8192, "expected Stash server path")?;
                }
                connection.validate()?;
                bounded_text(scene_id, 128, "Stash scene ID")?;
                bounded_text(file_id, 128, "Stash file ID")?;
                bounded_text(owner_proof, 64, "Stash owner capability")?;
                if mappings.is_empty() || mappings.len() > 16 {
                    return Err(ProtocolError::invalid(
                        "Stash requires 1..=16 root mappings",
                    ));
                }
                for mapping in mappings {
                    mapping.validate()?;
                }
            }
            Command::CreateProject { name } => bounded_text(name, 256, "project name")?,
            Command::BeginEditUpload {
                byte_len,
                sha256,
                label,
            } => {
                validate_motion_length(*byte_len)?;
                validate_sha256(sha256)?;
                bounded_text(label, 256, "edit label")?;
            }
            Command::BeginMotionDownload {
                offset, byte_len, ..
            } => {
                validate_motion_range(*offset, *byte_len, MAX_MOTION_BYTES)?;
            }
            Command::GenerateWithInferenceFallback {
                preset,
                settings,
                model,
                selection,
                ..
            } => {
                model.validate()?;
                selection.validate()?;
                settings.validate_execution_preset(preset, true)?;
                if settings.raw_frame_pair.is_some()
                    || (matches!(model, InferenceFallbackModel::Path { .. })
                        && settings.inference_profile.is_none())
                {
                    return Err(ProtocolError::invalid(
                        "fallback generation requires explicit model/profile and decoded media",
                    ));
                }
            }
            Command::Generate {
                preset, settings, ..
            } => {
                bounded_text(preset, 64, "preset")?;
                if let Some(settings) = settings {
                    settings.validate()?;
                }
            }
            Command::GenerateWithAssistant {
                approval,
                preset,
                settings,
                ..
            } => {
                approval.validate()?;
                bounded_text(preset, 64, "preset")?;
                settings.validate()?;
            }
            Command::GenerateInput { input } => input.validate()?,
            Command::StageLiveRecording { options } => options.validate()?,
            Command::GenerateCreativeWithAssistant { approval } => approval.validate()?,
            Command::MergeCandidate { axes, range, .. } => {
                validate_merge_axes(axes)?;
                if range.as_ref().is_some_and(|range| {
                    range.start() < ProjectTime::ZERO || range.end() <= range.start()
                }) {
                    return Err(ProtocolError::invalid(
                        "merge range must be nonnegative and nonempty",
                    ));
                }
            }
            Command::Pair {
                client_name,
                pairing_token,
            } => {
                bounded_text(client_name, 256, "client name")?;
                bounded_text(pairing_token, 4096, "pairing token")?;
            }
            Command::LibraryBatchMigrate {
                expected_report_sha256,
                recipe,
                ..
            } => {
                if expected_report_sha256.len() != 64
                    || !expected_report_sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err(ProtocolError::invalid(
                        "Migration requires exact lowercase report digest",
                    ));
                }
                if let LibraryBatchRecipe::Visual(recipe) = recipe {
                    recipe.validate()?;
                }
            }
            Command::LibraryBatchPrepare { options } => {
                options.validate()?;
            }
            Command::ImportSourcePinned {
                path,
                sha256,
                byte_len,
            } => {
                if !path.is_absolute()
                    || path.as_os_str().as_encoded_bytes().len() > 32768
                    || path.as_os_str().as_encoded_bytes().contains(&0)
                    || *byte_len == 0
                    || sha256.len() != 64
                    || !sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err(ProtocolError::invalid("Pinned source requires absolute bounded path, positive length and canonical digest"));
                }
            }
            Command::ImportSource { path }
            | Command::ImportFunscript { path }
            | Command::ImportFunscriptForRepair { path }
            | Command::Export { path } => {
                if path.as_os_str().is_empty() {
                    return Err(ProtocolError::invalid("empty path"));
                }
            }
            Command::ExportAxisPinned {
                path,
                directory_identity,
                ..
            } if !path.is_absolute()
                || directory_identity.inode == 0
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)) =>
            {
                return Err(ProtocolError::invalid("Pinned export requires absolute path, positive directory inode and no traversal"));
            }
            Command::ExportAxis { path, .. }
            | Command::ExportAxisPinned { path, .. }
            | Command::ExportDeviceMotion { path, .. }
            | Command::PrepareHandyStrokeScript { path } => {
                let bytes = path.as_os_str().as_encoded_bytes();
                if bytes.is_empty() || bytes.len() > 32 * 1024 || bytes.contains(&0) {
                    return Err(ProtocolError::invalid(
                        "axis export path must be nonempty, NUL-free, and at most 32768 bytes",
                    ));
                }
                if matches!(self.command, Command::PrepareHandyStrokeScript { .. })
                    && (!path.is_absolute()
                        || path
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir)))
                {
                    return Err(ProtocolError::invalid(
                        "Handy preparation requires an absolute path without traversal",
                    ));
                }
                if let Command::ExportDeviceMotion { profile, .. } = &self.command {
                    profile
                        .validate()
                        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                }
            }
            Command::RetimeMotion {
                from_placement,
                to_placement,
                axes,
            } => {
                if from_placement == to_placement
                    || axes.is_empty()
                    || axes.len() > 6
                    || axes.iter().collect::<std::collections::BTreeSet<_>>().len() != axes.len()
                {
                    return Err(ProtocolError::invalid(
                        "retiming needs distinct placements and unique explicit axes",
                    ));
                }
            }
            Command::SetSourcePlacement { spec, .. } => {
                spec.validate()
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?;
            }
            Command::PreviewAnnotatedFrame {
                context,
                time,
                spec,
                model,
            } => {
                spec.validate()?;
                time.validate(context)?;
                if context.transform != annotated_preview_transform(&spec.view)? {
                    return Err(ProtocolError::invalid(
                        "annotated preview context differs from target view",
                    ));
                }
                match model {
                    AnnotatedPreviewModel::File { profile, .. } => {
                        spec.validate_profile(profile)?
                    }
                    AnnotatedPreviewModel::Pack { choice } => choice.validate()?,
                }
            }
            Command::PreviewModelPackFrame {
                context,
                time,
                video_view,
                choice,
            } => {
                choice.validate()?;
                time.validate(context)?;
                if context.transform != video_view_transform(video_view.as_ref())? {
                    return Err(ProtocolError::invalid(
                        "preview context transform differs from declared video view",
                    ));
                }
            }
            Command::PreviewPlacedFrame {
                context,
                video_view,
                model_input,
                inference_profile,
                ..
            } => {
                if context.transform != video_view_transform(video_view.as_ref())? {
                    return Err(ProtocolError::invalid(
                        "preview context transform differs from declared video view",
                    ));
                }
                if let Some(input) = model_input {
                    input.validate()?;
                }
                if let Some(profile) = inference_profile {
                    profile.validate()?;
                }
            }
            Command::PreviewFrame {
                context,
                video_view,
                model_input,
                inference_profile,
                source_time,
                time_basis,
                ..
            } => {
                if *time_basis == PreviewTimeBasis::ElapsedFromFirstFrame
                    && source_time.numerator() < 0
                {
                    return Err(ProtocolError::invalid(
                        "elapsed preview time must be nonnegative",
                    ));
                }
                if context.transform != video_view_transform(video_view.as_ref())? {
                    return Err(ProtocolError::invalid(
                        "preview context transform differs from declared video view",
                    ));
                }
                if let Some(input) = model_input {
                    input.validate()?;
                }
                if let Some(profile) = inference_profile {
                    profile.validate()?;
                }
            }
            Command::SetProtectedRegions { regions } if regions.len() > 4096 => {
                return Err(ProtocolError::invalid("too many protected regions"));
            }
            Command::Events { limit, .. } if *limit == 0 || *limit > 1024 => {
                return Err(ProtocolError::invalid(
                    "event page limit must be within 1..=1024",
                ));
            }
            Command::OpenProject { project_id }
            | Command::Grant { project_id, .. }
            | Command::Revoke { project_id, .. }
                if self
                    .project
                    .as_ref()
                    .is_some_and(|scope| scope != project_id) =>
            {
                return Err(ProtocolError::invalid("conflicting project scopes"));
            }
            _ => {}
        }
        Ok(())
    }
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("version", &self.version)
            .field("request_id", &self.request_id)
            .field("session", &self.session)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("project", &self.project)
            .field("expected_revision", &self.expected_revision)
            .field("command", &self.command)
            .finish()
    }
}

pub(crate) fn bounded_text(value: &str, limit: usize, name: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(ProtocolError::invalid(format!("invalid {name}")));
    }
    Ok(())
}

/// Wire shape only; the Engine must authenticate the session and match the
/// host-local model-owner capability again at the decision boundary.
fn validate_inference_equivalence_request(request: &Request) -> Result<(), ProtocolError> {
    let owner_proof = match &request.command {
        Command::InspectInferenceEnvironment { owner_proof, .. }
        | Command::ImportInferenceEquivalence { owner_proof, .. }
        | Command::InferenceEquivalenceStatus { owner_proof, .. }
        | Command::ListInferenceEquivalences { owner_proof, .. }
        | Command::AcceptInferenceEquivalence { owner_proof, .. }
        | Command::WithdrawInferenceEquivalence { owner_proof, .. } => owner_proof,
        _ => return Ok(()),
    };
    if request.project.is_some() || request.expected_revision.is_some() {
        return Err(ProtocolError::invalid(
            "inference equivalence operations use a global record revision, not project scope",
        ));
    }
    bounded_text(owner_proof, 64, "model owner capability")?;
    match &request.command {
        Command::InspectInferenceEnvironment { profile, .. } => profile.validate()?,
        Command::ImportInferenceEquivalence {
            path,
            manifest_sha256,
            ..
        } => {
            bounded_text(path, 4096, "inference equivalence manifest path")?;
            if !std::path::Path::new(path).is_absolute() {
                return Err(ProtocolError::invalid(
                    "inference equivalence manifest path must be absolute",
                ));
            }
            validate_sha256(manifest_sha256)?;
        }
        Command::InferenceEquivalenceStatus { dossier_id, .. } => {
            validate_sha256(dossier_id.as_str())?;
        }
        Command::ListInferenceEquivalences { after, limit, .. } => {
            validate_inference_equivalence_page_limit(*limit)?;
            if let Some(after) = after {
                validate_sha256(after.as_str())?;
            }
        }
        Command::AcceptInferenceEquivalence {
            dossier_id,
            expected_record_revision,
            operator,
            acknowledge_measured_equivalence,
            ..
        } => {
            validate_sha256(dossier_id.as_str())?;
            validate_inference_equivalence_revision(*expected_record_revision)?;
            bounded_text(operator, 256, "inference equivalence reviewer")?;
            if !acknowledge_measured_equivalence {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "acceptance requires explicit measured-equivalence acknowledgement",
                ));
            }
        }
        Command::WithdrawInferenceEquivalence {
            dossier_id,
            expected_record_revision,
            reason,
            ..
        } => {
            validate_sha256(dossier_id.as_str())?;
            validate_inference_equivalence_revision(*expected_record_revision)?;
            validate_inference_equivalence_note(reason)?;
        }
        _ => unreachable!("matched inference equivalence command"),
    }
    Ok(())
}
fn validate_inference_equivalence_revision(revision: u64) -> Result<(), ProtocolError> {
    if revision == 0 || revision > i64::MAX as u64 {
        return Err(ProtocolError::invalid(
            "inference equivalence record revision must be 1..=i64::MAX",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod inference_equivalence_wire_tests {
    use super::*;

    fn dossier() -> ArtifactId {
        ArtifactId::new("a".repeat(64)).unwrap()
    }
    fn commands() -> Vec<(Command, bool, &'static str)> {
        let proof = "private-owner-proof".to_owned();
        vec![
            (
                Command::ImportInferenceEquivalence {
                    path: test_host_path("private-equivalence-input.json"),
                    manifest_sha256: "a".repeat(64),
                    owner_proof: proof.clone(),
                },
                true,
                "ImportInferenceEquivalence",
            ),
            (
                Command::InferenceEquivalenceStatus {
                    dossier_id: dossier(),
                    owner_proof: proof.clone(),
                },
                false,
                "InferenceEquivalenceStatus",
            ),
            (
                Command::ListInferenceEquivalences {
                    after: None,
                    limit: 8,
                    owner_proof: proof.clone(),
                },
                false,
                "ListInferenceEquivalences",
            ),
            (
                Command::AcceptInferenceEquivalence {
                    dossier_id: dossier(),
                    expected_record_revision: 1,
                    operator: "private-reviewer-name".into(),
                    acknowledge_measured_equivalence: true,
                    owner_proof: proof.clone(),
                },
                true,
                "AcceptInferenceEquivalence",
            ),
            (
                Command::WithdrawInferenceEquivalence {
                    dossier_id: dossier(),
                    expected_record_revision: 2,
                    reason: "private-withdrawal-reason".into(),
                    owner_proof: proof,
                },
                true,
                "WithdrawInferenceEquivalence",
            ),
        ]
    }
    fn request(command: Command) -> Request {
        Request::new(RequestId::new("equivalence-request").unwrap(), command)
            .with_session(SessionId::new("owner-session").unwrap())
            .with_auth_token("private-session-token".into())
    }
    fn proof(command: &mut Command) -> &mut String {
        match command {
            Command::ImportInferenceEquivalence { owner_proof, .. }
            | Command::InferenceEquivalenceStatus { owner_proof, .. }
            | Command::ListInferenceEquivalences { owner_proof, .. }
            | Command::AcceptInferenceEquivalence { owner_proof, .. }
            | Command::WithdrawInferenceEquivalence { owner_proof, .. } => owner_proof,
            _ => unreachable!("authored command set"),
        }
    }
    fn decision_revision(command: &mut Command) -> &mut u64 {
        match command {
            Command::AcceptInferenceEquivalence {
                expected_record_revision,
                ..
            }
            | Command::WithdrawInferenceEquivalence {
                expected_record_revision,
                ..
            } => expected_record_revision,
            _ => unreachable!("authored decision command"),
        }
    }

    #[test]
    fn global_commands_have_explicit_mutation_shapes_and_redacted_debug() {
        for (command, mutating, name) in commands() {
            assert!(!command.requires_project());
            assert!(!command.requires_revision());
            assert_eq!(command.is_mutating(), mutating);
            assert_eq!(format!("{command:?}"), name);
            let request = request(command);
            request.validate().unwrap();
            let debug = format!("{request:?}");
            for secret in [
                "private-owner-proof",
                "private-session-token",
                "private-equivalence-input",
                "private-reviewer-name",
                "private-withdrawal-reason",
            ] {
                assert!(!debug.contains(secret));
            }
            let bytes = serde_json::to_vec(&request).unwrap();
            let decoded: Request = serde_json::from_slice(&bytes).unwrap();
            decoded.validate().unwrap();
            assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
        }
    }

    #[test]
    fn global_commands_refuse_any_project_or_project_revision_scope() {
        for (command, _, _) in commands() {
            let mut scoped = request(command.clone());
            scoped.project = Some(ProjectId::new("project").unwrap());
            assert!(scoped.validate().is_err());
            scoped.project = None;
            scoped.expected_revision = Some(RevisionId::new(0));
            assert!(scoped.validate().is_err());
            scoped.project = Some(ProjectId::new("project").unwrap());
            assert!(scoped.validate().is_err());
        }
    }

    #[test]
    fn owner_proof_is_required_bounded_and_never_in_validation_errors() {
        for (command, _, _) in commands() {
            for bad in [
                String::new(),
                " ".into(),
                "p".repeat(65),
                "proof\nprivate-content".into(),
            ] {
                let mut changed = command.clone();
                *proof(&mut changed) = bad;
                let error = request(changed).validate().unwrap_err();
                assert!(!error.to_string().contains("private-content"));
            }
            let mut value = serde_json::to_value(request(command)).unwrap();
            value["command"]["arguments"]
                .as_object_mut()
                .unwrap()
                .remove("owner_proof");
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
    }

    #[test]
    fn input_path_and_digest_are_bounded_without_opening_files() {
        let original = commands().remove(0).0;
        for bad in [
            String::new(),
            "relative.json".into(),
            test_host_path("bad\nname"),
            test_host_path(&"x".repeat(4096)),
        ] {
            let mut changed = original.clone();
            let Command::ImportInferenceEquivalence { path, .. } = &mut changed else {
                unreachable!()
            };
            *path = bad;
            assert!(request(changed).validate().is_err());
        }
        for bad in [
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
        ] {
            let mut changed = original.clone();
            let Command::ImportInferenceEquivalence {
                manifest_sha256, ..
            } = &mut changed
            else {
                unreachable!()
            };
            *manifest_sha256 = bad;
            assert!(request(changed).validate().is_err());
        }
        request(original).validate().unwrap();
    }

    #[test]
    fn dossier_and_cursor_ids_are_exact_lowercase_sha256() {
        for bad in ["short".into(), "A".repeat(64), "g".repeat(64)] {
            for (mut command, _, _) in commands().into_iter().skip(1) {
                let invalid = ArtifactId::new(bad.clone()).unwrap();
                match &mut command {
                    Command::InferenceEquivalenceStatus { dossier_id, .. }
                    | Command::AcceptInferenceEquivalence { dossier_id, .. }
                    | Command::WithdrawInferenceEquivalence { dossier_id, .. } => {
                        *dossier_id = invalid
                    }
                    Command::ListInferenceEquivalences { after, .. } => *after = Some(invalid),
                    _ => unreachable!(),
                }
                assert!(request(command).validate().is_err());
            }
        }
    }

    #[test]
    fn decisions_require_positive_sqlite_revision_and_explicit_acknowledgement() {
        for (command, _, _) in commands().into_iter().skip(3) {
            for invalid in [0, i64::MAX as u64 + 1, u64::MAX] {
                let mut changed = command.clone();
                *decision_revision(&mut changed) = invalid;
                assert!(request(changed).validate().is_err());
            }
            for allowed in [1, i64::MAX as u64] {
                let mut changed = command.clone();
                *decision_revision(&mut changed) = allowed;
                request(changed).validate().unwrap();
            }
        }
        let mut command = commands().remove(3).0;
        let Command::AcceptInferenceEquivalence {
            acknowledge_measured_equivalence,
            ..
        } = &mut command
        else {
            unreachable!()
        };
        *acknowledge_measured_equivalence = false;
        assert_eq!(
            request(command).validate().unwrap_err().code,
            ErrorCode::Forbidden
        );
        let mut value = serde_json::to_value(request(commands().remove(3).0)).unwrap();
        value["command"]["arguments"]
            .as_object_mut()
            .unwrap()
            .remove("acknowledge_measured_equivalence");
        assert!(serde_json::from_value::<Request>(value).is_err());
    }

    #[test]
    fn reviewer_and_withdrawal_note_have_distinct_bounded_contracts() {
        for bad in [
            String::new(),
            " ".into(),
            "r".repeat(257),
            "name\nprivate".into(),
        ] {
            let mut command = commands().remove(3).0;
            let Command::AcceptInferenceEquivalence { operator, .. } = &mut command else {
                unreachable!()
            };
            *operator = bad;
            assert!(request(command).validate().is_err());
        }
        for bad in [
            String::new(),
            " ".into(),
            "r".repeat(4097),
            "reason\0private".into(),
        ] {
            let mut command = commands().remove(4).0;
            let Command::WithdrawInferenceEquivalence { reason, .. } = &mut command else {
                unreachable!()
            };
            *reason = bad;
            assert!(request(command).validate().is_err());
        }
        let mut command = commands().remove(4).0;
        let Command::WithdrawInferenceEquivalence { reason, .. } = &mut command else {
            unreachable!()
        };
        *reason = "first line\nsecond line\tcontext".into();
        request(command).validate().unwrap();
    }

    #[test]
    fn list_limits_are_bounded_and_wire_arguments_are_strict() {
        for limit in [0, 17, u16::MAX] {
            let command = Command::ListInferenceEquivalences {
                after: None,
                limit,
                owner_proof: "proof".into(),
            };
            assert!(request(command).validate().is_err());
        }
        for limit in [1, 8, 16] {
            request(Command::ListInferenceEquivalences {
                after: Some(dossier()),
                limit,
                owner_proof: "proof".into(),
            })
            .validate()
            .unwrap();
        }
        for (command, _, _) in commands() {
            let mut value = serde_json::to_value(request(command)).unwrap();
            value["command"]["arguments"]["unexpected_authority"] = true.into();
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
    }

    #[test]
    fn equivalence_list_response_debug_is_redacted_and_wire_round_trips() {
        let body = ResponseBody::InferenceEquivalenceList(Box::new(InferenceEquivalencePage {
            entries: vec![],
            next_after: None,
            report_only: true,
        }));
        assert_eq!(format!("{body:?}"), "InferenceEquivalenceList");
        let bytes = serde_json::to_vec(&body).unwrap();
        let decoded: ResponseBody = serde_json::from_slice(&bytes).unwrap();
        let ResponseBody::InferenceEquivalenceList(page) = decoded else {
            panic!("wrong response")
        };
        page.validate().unwrap();
        assert!(page.report_only && page.entries.is_empty());
    }
}

/// Exact typed request bytes, excluding the request ID. Store and compare these
/// within the authenticated actor's request-ID namespace. This is deliberately
/// not a cryptographic identity for files, models, or executable artifacts.
pub fn request_fingerprint(request: &Request) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&(
        request.version,
        &request.session,
        &request.auth_token,
        &request.project,
        &request.expected_revision,
        &request.command,
    ))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Command {
    AssistantEditRecipes,
    ApplyAssistantEdit {
        approval: AssistantEditApproval,
    },
    AssistantTargets,
    AssistantProfiles,
    PrepareAssistantImage {
        source_version: SourceVersionId,
    },
    StartAssistant {
        spec: AssistantStart,
    },
    ResolveAssistantStart {
        reference: AssistantStartReference,
    },
    AssistantStatus {
        attempt_id: AttemptId,
    },
    CancelAssistant {
        attempt_id: AttemptId,
    },
    ListAssistantAttempts,
    WasmEditorInfo,
    StartWasmEdit {
        spec: WasmEditSpec,
    },
    ResolveWasmStart {
        reference: WasmStartReference,
    },
    WasmEditStatus {
        attempt_id: AttemptId,
    },
    ListWasmEdits,
    ListArchivedWasmEdits {
        after: Option<AttemptId>,
        limit: u16,
    },
    ArchiveCompletedWasmEdit {
        attempt_id: AttemptId,
        original_request_id: RequestId,
    },
    RemoveUnsuccessfulWasmEdit {
        attempt_id: AttemptId,
        original_request_id: RequestId,
        expected_state: WasmEditState,
    },
    CancelWasmEdit {
        attempt_id: AttemptId,
    },
    LiveCaptureLimits,
    NativeImagePreparationLimits,
    StartNativeImagePreparation {
        engine_epoch: AttemptId,
        options: NativeImagePreparationOptions,
    },
    ResolveNativeImagePreparation {
        original_request_id: RequestId,
    },
    NativeImagePreparationStatus {
        operation_id: AttemptId,
    },
    CancelNativeImagePreparation {
        operation_id: AttemptId,
    },
    ReleaseNativeImagePreparation {
        operation_id: AttemptId,
    },
    StartLiveCapture {
        engine_epoch: AttemptId,
        options: LiveCaptureOptions,
    },
    ContinueLiveCaptureAt {
        engine_epoch: AttemptId,
        predecessor: AttemptId,
        source_anchor: SourceTimestamp,
        lease_ms: u32,
    },
    ContinueLiveCapture {
        engine_epoch: AttemptId,
        predecessor: AttemptId,
        options: LiveCaptureOptions,
    },
    PrepareBrowserAnalysisSuccessor {
        engine_epoch: AttemptId,
        predecessor: AttemptId,
        options: BrowserAnalysisSuccessorOptions,
    },
    TransferBrowserAnalysisInput {
        predecessor: AttemptId,
        successor: AttemptId,
        boundary: LiveFrameHeader,
    },
    BrowserAnalysisTransferStatus {
        transfer_request_id: RequestId,
    },
    CancelBrowserAnalysisTransfer {
        transfer_request_id: RequestId,
    },
    LiveCaptureStatus {
        epoch: AttemptId,
    },
    StartLiveRecording {
        epoch: AttemptId,
        maximum_samples: u16,
    },
    LiveRecordingPage {
        epoch: AttemptId,
        after: u32,
        limit: u16,
    },
    DiscardLiveRecording {
        epoch: AttemptId,
        recording_request_id: RequestId,
    },
    /// Engine-owned accepted records only; creates an uncommitted candidate.
    StageLiveRecording {
        options: LiveRecordingStageOptions,
    },
    ProbeMedia {
        source_version: SourceVersionId,
    },
    BrowserMediaCatalog,
    PrepareBrowserMedia {
        engine_epoch: AttemptId,
        selection: BrowserMediaSelection,
        clock: DeclaredSourceClock,
        lease_ms: u32,
        allow_declared_clock: bool,
    },
    BrowserMediaPreparationStatus {
        original_request_id: RequestId,
    },
    PrepareBrowserMediaSuccessor {
        predecessor: BrowserMediaBinding,
        lease_ms: u32,
    },
    ReleaseBrowserMedia {
        original_request_id: RequestId,
    },
    OpenBrowserTiming {
        player: HandyPlayerIdentity,
        browser_media: BrowserMediaBinding,
        policy: BrowserTimingPolicy,
    },
    BrowserTimingOpenStatus {
        original_request_id: RequestId,
    },
    PrepareBrowserTimingSuccessor {
        predecessor_binding_id: ArtifactId,
        player: HandyPlayerIdentity,
        browser_media: BrowserMediaBinding,
        policy: BrowserTimingPolicy,
    },
    IssueBrowserTimingProbe {
        binding_id: ArtifactId,
        player: HandyPlayerIdentity,
        sequence: u64,
    },
    ObserveBrowserTiming {
        binding_id: ArtifactId,
        player: HandyPlayerIdentity,
        sequence: u64,
        probe_id: ArtifactId,
        frame: BrowserTimingFrame,
    },
    BrowserTimingStatus {
        binding_id: ArtifactId,
        player: HandyPlayerIdentity,
    },
    ReleaseBrowserTiming {
        binding_id: ArtifactId,
        player: HandyPlayerIdentity,
    },
    ListLiveCudaEvidence {
        after: Option<AttemptId>,
        limit: u16,
    },
    RemoveRetainedLiveCudaEvidence {
        epoch: AttemptId,
        expected_manifest_sha256: String,
    },
    InspectLiveCudaEvidence {
        epoch: AttemptId,
    },
    BeginLiveCudaEvidenceDownload {
        epoch: AttemptId,
        part: LiveCudaEvidencePart,
    },
    StopLiveCapture {
        epoch: AttemptId,
    },
    FinishLiveCapture {
        epoch: AttemptId,
    },
    CloseBrowserAnalysisInput {
        epoch: AttemptId,
        final_frame: Option<LiveFrameHeader>,
    },
    SelectCatalogModelPack {
        manifest_sha256: String,
        binding: ModelCatalogBinding,
        expected_selection_revision: u64,
        approve_unqualified_native_execution: bool,
        owner_proof: String,
    },
    SelectModelPack {
        manifest_sha256: Option<String>,
        expected_selection_revision: u64,
        approve_unqualified_native_execution: bool,
        owner_proof: String,
    },
    WithdrawModelPackApproval {
        choice: ModelPackChoice,
        expected_selection_revision: u64,
        include_unclassified_generation: bool,
        owner_proof: String,
    },
    ModelPackSelectionStatus {
        owner_proof: String,
    },
    /// Observe the current host for one explicit profile. This read-only
    /// report neither loads the model nor grants equivalence/fallback authority.
    InspectInferenceEnvironment {
        profile: Box<DeclaredInferenceProfile>,
        owner_proof: String,
    },
    ImportInferenceEquivalence {
        path: String,
        manifest_sha256: String,
        owner_proof: String,
    },
    InferenceEquivalenceStatus {
        dossier_id: ArtifactId,
        owner_proof: String,
    },
    ListInferenceEquivalences {
        after: Option<ArtifactId>,
        limit: u16,
        owner_proof: String,
    },
    AcceptInferenceEquivalence {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        operator: String,
        acknowledge_measured_equivalence: bool,
        owner_proof: String,
    },
    WithdrawInferenceEquivalence {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        reason: String,
        owner_proof: String,
    },
    GenerateFromModelPack {
        source_version: SourceVersionId,
        preset: String,
        #[serde(default)]
        settings: Option<GenerationSettings>,
        choice: ModelPackChoice,
    },
    RefreshModelCatalog {
        source: PathBuf,
        bootstrap: PathBuf,
        bootstrap_sha256: String,
        expected_catalog_revision: u64,
        approve_bootstrap: bool,
        owner_proof: String,
    },
    ModelCatalogStatus {
        bootstrap_sha256: String,
        owner_proof: String,
    },
    CheckModelPackCatalog {
        bootstrap_sha256: String,
        manifest_sha256: String,
        expected_catalog_revision: u64,
        owner_proof: String,
    },
    ImportModelPack {
        source: PathBuf,
        owner_proof: String,
    },
    ModelPackStatus {
        manifest_sha256: String,
        owner_proof: String,
    },
    ModelPackStagingStatus {
        owner_proof: String,
    },
    DiscardModelPackStage {
        basename: String,
        manifest_sha256: String,
        owner_proof: String,
    },
    HandyInspect {
        inspection: HandyInspectionRequest,
        owner_proof: String,
    },
    OpenHandySession {
        engine_epoch: DeviceSessionId,
        device_id: DeviceId,
        expected_registration_revision: u64,
        binding_sha256: String,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    CloseHandySession {
        engine_epoch: DeviceSessionId,
        device_session: DeviceSessionId,
        owner_proof: String,
    },
    HandySessions {
        owner_proof: String,
    },
    SetupHandyDevice {
        qualification: HandyQualificationBinding,
        engine_epoch: DeviceSessionId,
        device_session: DeviceSessionId,
        hosting_request_id: RequestId,
        allow_online_setup: bool,
        owner_proof: String,
    },
    CancelHandySetup {
        setup_request_id: RequestId,
        record_id: ArtifactId,
        owner_proof: String,
    },
    HandySetupStatus {
        setup_request_id: RequestId,
        owner_proof: String,
    },
    AcquireHandyPlaybackClock {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        expected_generation: u64,
        synchronization: HandyClockSyncBinding,
        allow_online_timing: bool,
        owner_proof: String,
    },
    SynchronizeHandyClock {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        expected_generation: u64,
        allow_online_synchronization: bool,
        owner_proof: String,
    },
    StartHandyPlayback {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        expected_generation: u64,
        clock_request_id: RequestId,
        start_time_ms: u64,
        allow_play: bool,
        owner_proof: String,
    },
    ImportHandyHspSourceQualification {
        path: std::path::PathBuf,
        manifest_sha256: String,
        owner_proof: String,
    },
    HandyHspSourceQualificationStatus {
        dossier_id: ArtifactId,
        owner_proof: String,
    },
    AcceptHandyHspSourceQualification {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        operator: String,
        acknowledge_measured_evidence: bool,
        owner_proof: String,
    },
    WithdrawHandyHspSourceQualification {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        reason: String,
        owner_proof: String,
    },
    ImportHandyBrowserStartQualification {
        path: std::path::PathBuf,
        manifest_sha256: String,
        owner_proof: String,
    },
    HandyBrowserStartQualificationStatus {
        dossier_id: ArtifactId,
        owner_proof: String,
    },
    AcceptHandyBrowserStartQualification {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        operator: String,
        acknowledge_measured_evidence: bool,
        owner_proof: String,
    },
    WithdrawHandyBrowserStartQualification {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        reason: String,
        owner_proof: String,
    },
    StartHandyPlaybackFromBrowser {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        expected_generation: u64,
        clock_request_id: RequestId,
        browser_timing_binding_id: ArtifactId,
        qualification: HandyBrowserStartQualificationBinding,
        acknowledge_current_environment: bool,
        allow_play: bool,
        owner_proof: String,
    },
    RenewHandyBrowserPlayback {
        browser_timing_binding_id: ArtifactId,
        play_request_id: RequestId,
        play_record_id: Option<ArtifactId>,
        owner_proof: String,
    },
    ReleaseHandyBrowserPlayback {
        browser_timing_binding_id: ArtifactId,
        play_request_id: RequestId,
        play_record_id: Option<ArtifactId>,
        owner_proof: String,
    },
    HandyBrowserPlaybackStatus {
        browser_timing_binding_id: ArtifactId,
        play_request_id: RequestId,
        play_record_id: Option<ArtifactId>,
        owner_proof: String,
    },
    RefreshHandyBrowserPlaybackTiming {
        browser_timing_binding_id: ArtifactId,
        play_request_id: RequestId,
        play_record_id: Option<ArtifactId>,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    HandyPlaybackStatus {
        play_request_id: RequestId,
        owner_proof: String,
    },
    BindHandyHspLive {
        selection: HandyHspLiveSelection,
        /// Operator applicability assertion for the exact selected source
        /// dossier's browser environment; never inferred or machine-attested.
        acknowledge_current_environment: bool,
        allow_online_control: bool,
        owner_proof: String,
    },
    PlayHandyHspLive {
        stream_request_id: RequestId,
        owner_proof: String,
    },
    StopHandyHspLive {
        stream_request_id: RequestId,
        owner_proof: String,
    },
    HandyHspLiveStatus {
        stream_request_id: RequestId,
        owner_proof: String,
    },
    EnableHandyHspContinuity {
        stream_request_id: RequestId,
        duration_ms: u32,
        allow_online_observations: bool,
        owner_proof: String,
    },
    HandyHspContinuityStatus {
        original_request_id: RequestId,
        owner_proof: String,
    },
    PrepareHandyHsp {
        engine_epoch: DeviceSessionId,
        device_session: DeviceSessionId,
        qualification: HandyQualificationBinding,
        timeout_ms: u64,
        allow_online_synchronization: bool,
        owner_proof: String,
    },
    HandyHspPreparationStatus {
        preparation_request_id: RequestId,
        owner_proof: String,
    },
    CancelHandyHspPreparation {
        preparation_request_id: RequestId,
        owner_proof: String,
    },
    HandyHspStatus {
        stream_request_id: RequestId,
        owner_proof: String,
    },
    HandyStopStatus {
        acquisition_request_id: RequestId,
        owner_proof: String,
    },
    RefreshHandyPlaybackTiming {
        selection: HandyContinuousTimingSelection,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    HandyContinuousTimingStatus {
        refresh_request_id: RequestId,
        owner_proof: String,
    },
    BindHandyPlayerGuard {
        selection: HandyContinuousTimingSelection,
        player: HandyPlayerIdentity,
        policy: HandyPlayerGuardPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        browser_media: Option<BrowserMediaBinding>,
        allow_monitoring: bool,
        owner_proof: String,
    },
    HandyPlayerGuardStatus {
        binding_request_id: RequestId,
        owner_proof: String,
    },
    IssueHandyPlayerChallenge {
        binding_id: ArtifactId,
        player: HandyPlayerIdentity,
        sequence: u64,
        owner_proof: String,
    },
    ObserveHandyPlayer {
        binding_id: ArtifactId,
        player: HandyPlayerIdentity,
        sequence: u64,
        challenge_id: ArtifactId,
        sample: HandyPlayerSample,
        owner_proof: String,
    },
    ReleaseHandyPlayerGuard {
        binding_id: ArtifactId,
        player: HandyPlayerIdentity,
        owner_proof: String,
    },
    PlanHandyReconciliation {
        selection: HandyRecoverySelection,
        owner_proof: String,
    },
    ReconcileHandyEffects {
        plan_id: ArtifactId,
        allow_online_inspection: bool,
        allow_resolution: bool,
        owner_proof: String,
    },
    HandyReconciliationStatus {
        reconciliation_request_id: RequestId,
        owner_proof: String,
    },
    CancelHandyReconciliation {
        reconciliation_request_id: RequestId,
        owner_proof: String,
    },
    CancelHandyPlayback {
        play_request_id: RequestId,
        record_id: ArtifactId,
        owner_proof: String,
    },
    HandyClockSyncStatus {
        sync_request_id: RequestId,
        owner_proof: String,
    },
    CancelHandyClockSync {
        sync_request_id: RequestId,
        record_id: ArtifactId,
        owner_proof: String,
    },
    InspectHandyDeviceTiming {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        expected_generation: u64,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    SampleHandyServerClock {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        expected_generation: u64,
        allow_online_sampling: bool,
        owner_proof: String,
    },
    AcquireHandyController {
        preparation_request_id: RequestId,
        preparation_id: ArtifactId,
        duration_ms: u64,
        allow_arm: bool,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    RenewHandyController {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        expected_generation: u64,
        owner_proof: String,
    },
    ReleaseHandyController {
        acquisition_request_id: RequestId,
        controller_id: ClientId,
        owner_proof: String,
    },
    HandyControllerStatus {
        acquisition_request_id: RequestId,
        owner_proof: String,
    },
    PrepareHandyPlayback {
        program: HandyProgramSelection,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    HandyPlaybackPreparationStatus {
        preparation_request_id: RequestId,
        owner_proof: String,
    },
    RetireHandyPlaybackPreparation {
        preparation_request_id: RequestId,
        preparation_id: ArtifactId,
        owner_proof: String,
    },
    InspectHandyProgram {
        setup_request_id: RequestId,
        record_id: ArtifactId,
        qualification: HandyQualificationBinding,
        engine_epoch: DeviceSessionId,
        device_session: DeviceSessionId,
        hosting_request_id: RequestId,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    InspectHandySetup {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        qualification: Option<HandyQualificationBinding>,
        engine_epoch: DeviceSessionId,
        device_session: DeviceSessionId,
        hosting_request_id: RequestId,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    HandyQualificationContext {
        #[serde(default, skip_serializing_if = "HandyQualificationTransport::is_hssp")]
        transport: HandyQualificationTransport,
        owner_proof: String,
    },
    ImportHandyQualification {
        manifest_path: PathBuf,
        manifest_sha256: String,
        owner_proof: String,
    },
    HandyQualificationStatus {
        dossier_id: ArtifactId,
        owner_proof: String,
    },
    AcceptHandyQualification {
        dossier_id: ArtifactId,
        expected_qualification_revision: u64,
        engine_epoch: DeviceSessionId,
        device_session: DeviceSessionId,
        note: String,
        acknowledge_physical_evidence: bool,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    WithdrawHandyQualification {
        dossier_id: ArtifactId,
        expected_qualification_revision: u64,
        note: String,
        owner_proof: String,
    },
    HostHandyStrokeScript {
        preparation_request_id: RequestId,
        script_sha256: String,
        engine_epoch: DeviceSessionId,
        allow_temporary_public_hosting: bool,
        owner_proof: String,
    },
    HandyHostingStatus {
        upload_request_id: RequestId,
        owner_proof: String,
    },
    ForgetHandyHosting {
        upload_request_id: RequestId,
        owner_proof: String,
    },
    ResolveHandyRegistration {
        original_request_id: RequestId,
        intent: HandyRegistrationIntent,
        owner_proof: String,
    },
    SetHandyRegistration {
        device_id: DeviceId,
        expected_registration_revision: u64,
        settings: HandyRegistrationSettings,
        disabled: bool,
        owner_proof: String,
    },
    DisableHandyRegistration {
        device_id: DeviceId,
        expected_registration_revision: u64,
        owner_proof: String,
    },
    ListHandyRegistrations {
        owner_proof: String,
    },
    InspectHandyRegistration {
        device_id: DeviceId,
        expected_registration_revision: u64,
        binding_sha256: String,
        allow_online_inspection: bool,
        owner_proof: String,
    },
    StashListMissing {
        connection: StashConnection,
        limit: u32,
        owner_proof: String,
    },
    StashPlanMissing {
        connection: StashConnection,
        limit: u32,
        mappings: Vec<StashRootMapping>,
        owner_proof: String,
    },
    InferenceCatalog,
    CreateProject {
        name: String,
    },
    OpenProject {
        project_id: ProjectId,
    },
    SetSourcePlacement {
        spec: SourcePlacementSpec,
        replace: Option<SourcePlacementId>,
    },
    RemoveSourcePlacement {
        placement_id: SourcePlacementId,
    },
    MotionRetimeStatus {
        revision: RevisionId,
    },
    ArchivedMotionRetimeStatus {
        revision: RevisionId,
    },
    ArchivedGenerationStatus {
        candidate_id: CandidateId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        master_revision: Option<RevisionId>,
    },
    CandidateImageOriginStatus {
        candidate_id: CandidateId,
    },
    CandidateImageOriginPixels {
        candidate_id: CandidateId,
        record_sha256: String,
    },
    RetimeMotion {
        from_placement: SourcePlacementId,
        to_placement: SourcePlacementId,
        axes: Vec<Axis>,
    },
    PreviewAnnotatedFrame {
        context: FrameContext,
        time: ModelPackPreviewTime,
        spec: AnnotatedPreviewSpec,
        model: AnnotatedPreviewModel,
    },
    PreviewModelPackFrame {
        context: FrameContext,
        time: ModelPackPreviewTime,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        video_view: Option<VideoView>,
        choice: ModelPackChoice,
    },
    PreviewPlacedFrame {
        placement_id: SourcePlacementId,
        project_time: ProjectTime,
        context: FrameContext,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        video_view: Option<VideoView>,
        #[serde(default)]
        model_path: Option<PathBuf>,
        #[serde(default)]
        model_input: Option<ModelInputContract>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inference_profile: Option<DeclaredInferenceProfile>,
    },
    ImportSource {
        path: PathBuf,
    },
    ImportSourcePinned {
        path: PathBuf,
        sha256: String,
        byte_len: u64,
    },
    LibraryBatchMigrate {
        batch_request_id: RequestId,
        expected_report_sha256: String,
        recipe: LibraryBatchRecipe,
    },
    LibraryBatchPrepare {
        options: LibraryBatchOptions,
    },
    LibraryBatchCancel {
        batch_request_id: RequestId,
    },
    LibraryBatchRelease {
        batch_request_id: RequestId,
    },
    LibraryBatchList,
    LibraryBatchStatus {
        batch_request_id: RequestId,
    },
    LibraryBatchAdvance {
        batch_request_id: RequestId,
    },
    StashStartBatch {
        options: StashBatchOptions,
        owner_proof: String,
    },
    StashBatchStatus {
        batch_request_id: RequestId,
        #[serde(default)]
        offset: u32,
        owner_proof: String,
    },
    StashResumeBatch {
        batch_request_id: RequestId,
        owner_proof: String,
    },
    StashMigrateBatch {
        batch_request_id: RequestId,
        expected_ledger_sha256: String,
        generation: StashBatchGeneration,
        owner_proof: String,
    },
    StashCancelBatch {
        batch_request_id: RequestId,
        owner_proof: String,
    },
    StashAddTag {
        connection: StashConnection,
        scan_request_id: RequestId,
        tag_name: String,
        create: bool,
        owner_proof: String,
    },
    StashTagStatus {
        connection: StashConnection,
        tag_request_id: RequestId,
        owner_proof: String,
    },
    StashResumeTag {
        connection: StashConnection,
        tag_request_id: RequestId,
        owner_proof: String,
    },
    StashSubmitScan {
        connection: StashConnection,
        publication_request_id: RequestId,
        owner_proof: String,
    },
    StashScanStatus {
        connection: StashConnection,
        scan_request_id: RequestId,
        owner_proof: String,
    },
    StashReconcileAttachment {
        connection: StashConnection,
        scan_request_id: RequestId,
        owner_proof: String,
    },
    StashExportScript {
        connection: StashConnection,
        source_version: SourceVersionId,
        scene_id: String,
        file_id: String,
        candidate_id: CandidateId,
        job_id: JobId,
        owner_proof: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        source_time: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        archived_origin: Option<PackageObjectRef>,
    },
    StashPublicationStatus {
        publication_request_id: RequestId,
        owner_proof: String,
    },
    StashImportSource {
        connection: StashConnection,
        scene_id: String,
        file_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_server_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_source_version: Option<SourceVersionId>,
        /// Explicitly select a file from a multifile scene; never automatic.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_multifile_selection: Option<bool>,
        mappings: Vec<StashRootMapping>,
        owner_proof: String,
    },
    BeginEditUpload {
        byte_len: u64,
        sha256: String,
        label: String,
    },
    FinishEditUpload {
        lease_id: TransferId,
    },
    TransferStatus {
        lease_id: TransferId,
    },
    AbandonTransfer {
        lease_id: TransferId,
    },
    BeginMotionDownload {
        locator: MotionLocator,
        offset: u64,
        byte_len: u64,
    },
    SetProtectedRegions {
        regions: Vec<ProtectedRegion>,
    },
    Undo,
    Redo,
    /// Apply a completed stride or target proposal to this explicit generation request.
    GenerateWithAssistant {
        approval: AssistantGenerationApproval,
        source_version: SourceVersionId,
        preset: String,
        settings: Box<GenerationSettings>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_path: Option<PathBuf>,
    },
    /// Generate a reviewed candidate from one exact retained creative proposal.
    GenerateCreativeWithAssistant {
        approval: AssistantCreativeApproval,
    },
    /// Explicit opt-in; ordinary Generate and model-pack generation remain pinned.
    GenerateWithInferenceFallback {
        source_version: SourceVersionId,
        preset: String,
        settings: Box<GenerationSettings>,
        model: InferenceFallbackModel,
        selection: Box<InferenceFallbackSelection>,
    },
    Generate {
        source_version: SourceVersionId,
        preset: String,
        #[serde(default)]
        settings: Option<GenerationSettings>,
        #[serde(default)]
        model_path: Option<PathBuf>,
    },
    GenerateInput {
        input: GenerationInput,
    },
    ImportFunscript {
        path: PathBuf,
    },
    ImportFunscriptForRepair {
        path: PathBuf,
    },
    MergeCandidate {
        candidate_id: CandidateId,
        axes: Vec<Axis>,
        range: Option<TimeRange>,
    },
    Diagnostics {
        candidate_id: Option<CandidateId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum_speed_percent_per_second: Option<f64>,
    },
    PinSource {
        source_version: SourceVersionId,
        pinned: bool,
    },
    EvictSource {
        source_version: SourceVersionId,
    },
    JobStatus {
        job_id: JobId,
    },
    JobAttempts {
        job_id: JobId,
        after_attempt: Option<AttemptId>,
        limit: u16,
    },
    CancelJob {
        job_id: JobId,
    },
    ResumeJob {
        job_id: JobId,
    },
    GetCandidate {
        candidate_id: CandidateId,
    },
    CommitCandidate {
        candidate_id: CandidateId,
    },
    RebaseCandidate {
        candidate_id: CandidateId,
    },
    GetSnapshot,
    ReviewStatus,
    SetReviewDisposition {
        review_id: String,
        disposition: ReviewDisposition,
        note: String,
    },
    SetReviewIntervalDisposition {
        review_id: String,
        range: TimeRange,
        disposition: ReviewDisposition,
        note: String,
    },
    AllowProjectPackaging {
        owner_proof: String,
    },
    StartProjectImport {
        package: PackageUploadDeclaration,
    },
    ProjectImportStatus {
        operation_id: PackageImportOperationId,
    },
    BeginProjectPackageUpload {
        operation_id: PackageImportOperationId,
    },
    ProjectPackageUploadStatus {
        lease_id: PackageUploadId,
    },
    AbandonProjectPackageUpload {
        lease_id: PackageUploadId,
    },
    SealProjectImport {
        operation_id: PackageImportOperationId,
        upload_generation: u64,
    },
    CancelProjectImport {
        operation_id: PackageImportOperationId,
    },
    StartProjectExport,
    ProjectPackageStatus {
        operation_id: PackageOperationId,
    },
    CancelProjectExport {
        operation_id: PackageOperationId,
    },
    ReleaseProjectExport {
        operation_id: PackageOperationId,
    },
    BeginProjectPackageDownload {
        operation_id: PackageOperationId,
        offset: u64,
        byte_len: u64,
    },
    ProjectPackageDownloadStatus {
        lease_id: PackageDownloadId,
    },
    AbandonProjectPackageDownload {
        lease_id: PackageDownloadId,
    },
    Capabilities,
    PreviewFrame {
        #[serde(default, skip_serializing_if = "PreviewTimeBasis::is_source_pts")]
        time_basis: PreviewTimeBasis,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        video_view: Option<VideoView>,
        context: FrameContext,
        source_time: SourceTimestamp,
        #[serde(default)]
        model_path: Option<PathBuf>,
        #[serde(default)]
        model_input: Option<ModelInputContract>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inference_profile: Option<DeclaredInferenceProfile>,
    },
    Export {
        path: PathBuf,
    },
    ExportAxis {
        path: PathBuf,
        axis: Axis,
    },
    ExportAxisPinned {
        path: PathBuf,
        axis: Axis,
        directory_identity: PublicationDirectoryIdentity,
    },
    /// Publish exact neutral Stroke CSV samples locally, without hosting or admission.
    PrepareHandyStrokeScript {
        path: PathBuf,
    },
    /// Export an analytic device-space proposal, without arming or transmission.
    ExportDeviceMotion {
        path: PathBuf,
        profile: DynamicsProfile,
    },
    Pair {
        client_name: String,
        pairing_token: String,
    },
    Grant {
        session: SessionId,
        project_id: ProjectId,
        scopes: Vec<Scope>,
    },
    Revoke {
        session: SessionId,
        project_id: ProjectId,
    },
    Events {
        after_cursor: Option<u64>,
        limit: u16,
    },
    Arm {
        device_id: DeviceId,
        revision: RevisionId,
    },
    Stop {
        device_session: DeviceSessionId,
    },
    SwitchPlaybackRevision {
        device_session: DeviceSessionId,
        revision: RevisionId,
    },
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Arguments can contain pairing credentials, private paths or media
        // prompts. Debug logging intentionally exposes only operation names.
        f.write_str(match self {
            Self::AssistantEditRecipes => "AssistantEditRecipes",
            Self::ApplyAssistantEdit { .. } => "ApplyAssistantEdit",
            Self::AssistantTargets => "AssistantTargets",
            Self::AssistantProfiles => "AssistantProfiles",
            Self::PrepareAssistantImage { .. } => "PrepareAssistantImage",
            Self::StartAssistant { .. } => "StartAssistant",
            Self::ResolveAssistantStart { .. } => "ResolveAssistantStart",
            Self::AssistantStatus { .. } => "AssistantStatus",
            Self::CancelAssistant { .. } => "CancelAssistant",
            Self::ListAssistantAttempts => "ListAssistantAttempts",
            Self::WasmEditorInfo => "WasmEditorInfo",
            Self::StartWasmEdit { .. } => "StartWasmEdit",
            Self::ResolveWasmStart { .. } => "ResolveWasmStart",
            Self::WasmEditStatus { .. } => "WasmEditStatus",
            Self::ListWasmEdits => "ListWasmEdits",
            Self::ListArchivedWasmEdits { .. } => "ListArchivedWasmEdits",
            Self::ArchiveCompletedWasmEdit { .. } => "ArchiveCompletedWasmEdit",
            Self::RemoveUnsuccessfulWasmEdit { .. } => "RemoveUnsuccessfulWasmEdit",
            Self::CancelWasmEdit { .. } => "CancelWasmEdit",
            Self::LiveCaptureLimits => "LiveCaptureLimits",
            Self::StartLiveCapture { .. } => "StartLiveCapture",
            Self::NativeImagePreparationLimits => "NativeImagePreparationLimits",
            Self::StartNativeImagePreparation { .. } => "StartNativeImagePreparation",
            Self::ResolveNativeImagePreparation { .. } => "ResolveNativeImagePreparation",
            Self::NativeImagePreparationStatus { .. } => "NativeImagePreparationStatus",
            Self::CancelNativeImagePreparation { .. } => "CancelNativeImagePreparation",
            Self::ReleaseNativeImagePreparation { .. } => "ReleaseNativeImagePreparation",
            Self::ContinueLiveCapture { .. } => "ContinueLiveCapture",
            Self::PrepareBrowserAnalysisSuccessor { .. } => "PrepareBrowserAnalysisSuccessor",
            Self::TransferBrowserAnalysisInput { .. } => "TransferBrowserAnalysisInput",
            Self::BrowserAnalysisTransferStatus { .. } => "BrowserAnalysisTransferStatus",
            Self::CancelBrowserAnalysisTransfer { .. } => "CancelBrowserAnalysisTransfer",
            Self::ContinueLiveCaptureAt { .. } => "ContinueLiveCaptureAt",
            Self::LiveCaptureStatus { .. } => "LiveCaptureStatus",
            Self::StartLiveRecording { .. } => "StartLiveRecording",
            Self::LiveRecordingPage { .. } => "LiveRecordingPage",
            Self::DiscardLiveRecording { .. } => "DiscardLiveRecording",
            Self::StageLiveRecording { .. } => "StageLiveRecording",
            Self::RemoveRetainedLiveCudaEvidence { .. } => "RemoveRetainedLiveCudaEvidence",
            Self::ProbeMedia { .. } => "ProbeMedia",
            Self::BrowserMediaCatalog => "BrowserMediaCatalog",
            Self::PrepareBrowserMedia { .. } => "PrepareBrowserMedia",
            Self::PrepareBrowserMediaSuccessor { .. } => "PrepareBrowserMediaSuccessor",
            Self::BrowserMediaPreparationStatus { .. } => "BrowserMediaPreparationStatus",
            Self::ReleaseBrowserMedia { .. } => "ReleaseBrowserMedia",
            Self::OpenBrowserTiming { .. } => "OpenBrowserTiming",
            Self::PrepareBrowserTimingSuccessor { .. } => "PrepareBrowserTimingSuccessor",
            Self::BrowserTimingOpenStatus { .. } => "BrowserTimingOpenStatus",
            Self::IssueBrowserTimingProbe { .. } => "IssueBrowserTimingProbe",
            Self::ObserveBrowserTiming { .. } => "ObserveBrowserTiming",
            Self::BrowserTimingStatus { .. } => "BrowserTimingStatus",
            Self::ReleaseBrowserTiming { .. } => "ReleaseBrowserTiming",
            Self::ListLiveCudaEvidence { .. } => "ListLiveCudaEvidence",
            Self::InspectLiveCudaEvidence { .. } => "InspectLiveCudaEvidence",
            Self::BeginLiveCudaEvidenceDownload { .. } => "BeginLiveCudaEvidenceDownload",
            Self::StopLiveCapture { .. } => "StopLiveCapture",
            Self::FinishLiveCapture { .. } => "FinishLiveCapture",
            Self::CloseBrowserAnalysisInput { .. } => "CloseBrowserAnalysisInput",
            Self::StashListMissing { .. } => "StashListMissing",
            Self::StashPlanMissing { .. } => "StashPlanMissing",
            Self::CreateProject { .. } => "CreateProject",
            Self::SelectCatalogModelPack { .. } => "SelectCatalogModelPack",
            Self::SelectModelPack { .. } => "SelectModelPack",
            Self::WithdrawModelPackApproval { .. } => "WithdrawModelPackApproval",
            Self::ModelPackSelectionStatus { .. } => "ModelPackSelectionStatus",
            Self::InspectInferenceEnvironment { .. } => "InspectInferenceEnvironment",
            Self::ImportInferenceEquivalence { .. } => "ImportInferenceEquivalence",
            Self::InferenceEquivalenceStatus { .. } => "InferenceEquivalenceStatus",
            Self::ListInferenceEquivalences { .. } => "ListInferenceEquivalences",
            Self::AcceptInferenceEquivalence { .. } => "AcceptInferenceEquivalence",
            Self::WithdrawInferenceEquivalence { .. } => "WithdrawInferenceEquivalence",
            Self::GenerateFromModelPack { .. } => "GenerateFromModelPack",
            Self::RefreshModelCatalog { .. } => "RefreshModelCatalog",
            Self::ModelCatalogStatus { .. } => "ModelCatalogStatus",
            Self::CheckModelPackCatalog { .. } => "CheckModelPackCatalog",
            Self::ImportModelPack { .. } => "ImportModelPack",
            Self::ModelPackStatus { .. } => "ModelPackStatus",
            Self::ModelPackStagingStatus { .. } => "ModelPackStagingStatus",
            Self::DiscardModelPackStage { .. } => "DiscardModelPackStage",
            Self::HandyInspect { .. } => "HandyInspect",
            Self::OpenHandySession { .. } => "OpenHandySession",
            Self::CloseHandySession { .. } => "CloseHandySession",
            Self::HandySessions { .. } => "HandySessions",
            Self::HandyQualificationContext { .. } => "HandyQualificationContext",
            Self::ImportHandyQualification { .. } => "ImportHandyQualification",
            Self::HandyQualificationStatus { .. } => "HandyQualificationStatus",
            Self::AcceptHandyQualification { .. } => "AcceptHandyQualification",
            Self::WithdrawHandyQualification { .. } => "WithdrawHandyQualification",
            Self::HostHandyStrokeScript { .. } => "HostHandyStrokeScript",
            Self::InspectHandySetup { .. } => "InspectHandySetup",
            Self::InspectHandyProgram { .. } => "InspectHandyProgram",
            Self::PrepareHandyPlayback { .. } => "PrepareHandyPlayback",
            Self::AcquireHandyPlaybackClock { .. } => "AcquireHandyPlaybackClock",
            Self::SynchronizeHandyClock { .. } => "SynchronizeHandyClock",
            Self::StartHandyPlayback { .. } => "StartHandyPlayback",
            Self::ImportHandyHspSourceQualification { .. } => "ImportHandyHspSourceQualification",
            Self::ImportHandyBrowserStartQualification { .. } => {
                "ImportHandyBrowserStartQualification"
            }
            Self::HandyHspSourceQualificationStatus { .. } => "HandyHspSourceQualificationStatus",
            Self::HandyBrowserStartQualificationStatus { .. } => {
                "HandyBrowserStartQualificationStatus"
            }
            Self::AcceptHandyHspSourceQualification { .. } => "AcceptHandyHspSourceQualification",
            Self::AcceptHandyBrowserStartQualification { .. } => {
                "AcceptHandyBrowserStartQualification"
            }
            Self::WithdrawHandyHspSourceQualification { .. } => {
                "WithdrawHandyHspSourceQualification"
            }
            Self::WithdrawHandyBrowserStartQualification { .. } => {
                "WithdrawHandyBrowserStartQualification"
            }
            Self::StartHandyPlaybackFromBrowser { .. } => "StartHandyPlaybackFromBrowser",
            Self::RenewHandyBrowserPlayback { .. } => "RenewHandyBrowserPlayback",
            Self::ReleaseHandyBrowserPlayback { .. } => "ReleaseHandyBrowserPlayback",
            Self::HandyBrowserPlaybackStatus { .. } => "HandyBrowserPlaybackStatus",
            Self::RefreshHandyBrowserPlaybackTiming { .. } => "RefreshHandyBrowserPlaybackTiming",
            Self::HandyPlaybackStatus { .. } => "HandyPlaybackStatus",
            Self::BindHandyHspLive { .. } => "BindHandyHspLive",
            Self::EnableHandyHspContinuity { .. } => "EnableHandyHspContinuity",
            Self::HandyHspContinuityStatus { .. } => "HandyHspContinuityStatus",
            Self::PlayHandyHspLive { .. } => "PlayHandyHspLive",
            Self::StopHandyHspLive { .. } => "StopHandyHspLive",
            Self::HandyHspLiveStatus { .. } => "HandyHspLiveStatus",
            Self::PrepareHandyHsp { .. } => "PrepareHandyHsp",
            Self::HandyHspPreparationStatus { .. } => "HandyHspPreparationStatus",
            Self::CancelHandyHspPreparation { .. } => "CancelHandyHspPreparation",
            Self::HandyHspStatus { .. } => "HandyHspStatus",
            Self::HandyStopStatus { .. } => "HandyStopStatus",
            Self::RefreshHandyPlaybackTiming { .. } => "RefreshHandyPlaybackTiming",
            Self::HandyContinuousTimingStatus { .. } => "HandyContinuousTimingStatus",
            Self::BindHandyPlayerGuard { .. } => "BindHandyPlayerGuard",
            Self::HandyPlayerGuardStatus { .. } => "HandyPlayerGuardStatus",
            Self::IssueHandyPlayerChallenge { .. } => "IssueHandyPlayerChallenge",
            Self::ObserveHandyPlayer { .. } => "ObserveHandyPlayer",
            Self::ReleaseHandyPlayerGuard { .. } => "ReleaseHandyPlayerGuard",
            Self::PlanHandyReconciliation { .. } => "PlanHandyReconciliation",
            Self::ReconcileHandyEffects { .. } => "ReconcileHandyEffects",
            Self::HandyReconciliationStatus { .. } => "HandyReconciliationStatus",
            Self::CancelHandyReconciliation { .. } => "CancelHandyReconciliation",
            Self::CancelHandyPlayback { .. } => "CancelHandyPlayback",
            Self::HandyClockSyncStatus { .. } => "HandyClockSyncStatus",
            Self::CancelHandyClockSync { .. } => "CancelHandyClockSync",
            Self::InspectHandyDeviceTiming { .. } => "InspectHandyDeviceTiming",
            Self::SampleHandyServerClock { .. } => "SampleHandyServerClock",
            Self::AcquireHandyController { .. } => "AcquireHandyController",
            Self::RenewHandyController { .. } => "RenewHandyController",
            Self::ReleaseHandyController { .. } => "ReleaseHandyController",
            Self::HandyControllerStatus { .. } => "HandyControllerStatus",
            Self::HandyPlaybackPreparationStatus { .. } => "HandyPlaybackPreparationStatus",
            Self::RetireHandyPlaybackPreparation { .. } => "RetireHandyPlaybackPreparation",
            Self::HandySetupStatus { .. } => "HandySetupStatus",
            Self::CancelHandySetup { .. } => "CancelHandySetup",
            Self::SetupHandyDevice { .. } => "SetupHandyDevice",
            Self::HandyHostingStatus { .. } => "HandyHostingStatus",
            Self::ForgetHandyHosting { .. } => "ForgetHandyHosting",
            Self::ResolveHandyRegistration { .. } => "ResolveHandyRegistration",
            Self::SetHandyRegistration { .. } => "SetHandyRegistration",
            Self::DisableHandyRegistration { .. } => "DisableHandyRegistration",
            Self::ListHandyRegistrations { .. } => "ListHandyRegistrations",
            Self::InspectHandyRegistration { .. } => "InspectHandyRegistration",
            Self::OpenProject { .. } => "OpenProject",
            Self::ImportSource { .. } => "ImportSource",
            Self::ImportSourcePinned { .. } => "ImportSourcePinned",
            Self::LibraryBatchMigrate { .. } => "LibraryBatchMigrate",
            Self::LibraryBatchPrepare { .. } => "LibraryBatchPrepare",
            Self::LibraryBatchStatus { .. } => "LibraryBatchStatus",
            Self::LibraryBatchList => "LibraryBatchList",
            Self::LibraryBatchAdvance { .. } => "LibraryBatchAdvance",
            Self::LibraryBatchCancel { .. } => "LibraryBatchCancel",
            Self::LibraryBatchRelease { .. } => "LibraryBatchRelease",
            Self::StashImportSource { .. } => "StashImportSource",
            Self::StashExportScript { .. } => "StashExportScript",
            Self::StashPublicationStatus { .. } => "StashPublicationStatus",
            Self::StashStartBatch { .. } => "StashStartBatch",
            Self::StashBatchStatus { .. } => "StashBatchStatus",
            Self::StashResumeBatch { .. } => "StashResumeBatch",
            Self::StashMigrateBatch { .. } => "StashMigrateBatch",
            Self::StashCancelBatch { .. } => "StashCancelBatch",
            Self::StashAddTag { .. } => "StashAddTag",
            Self::StashTagStatus { .. } => "StashTagStatus",
            Self::StashResumeTag { .. } => "StashResumeTag",
            Self::StashSubmitScan { .. } => "StashSubmitScan",
            Self::StashScanStatus { .. } => "StashScanStatus",
            Self::StashReconcileAttachment { .. } => "StashReconcileAttachment",
            Self::BeginEditUpload { .. } => "BeginEditUpload",
            Self::FinishEditUpload { .. } => "FinishEditUpload",
            Self::TransferStatus { .. } => "TransferStatus",
            Self::AbandonTransfer { .. } => "AbandonTransfer",
            Self::BeginMotionDownload { .. } => "BeginMotionDownload",
            Self::SetProtectedRegions { .. } => "SetProtectedRegions",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::GenerateWithAssistant { .. } => "GenerateWithAssistant",
            Self::GenerateCreativeWithAssistant { .. } => "GenerateCreativeWithAssistant",
            Self::Generate { .. } => "Generate",
            Self::GenerateWithInferenceFallback { .. } => "GenerateWithInferenceFallback",
            Self::GenerateInput { .. } => "GenerateInput",
            Self::ImportFunscript { .. } => "ImportFunscript",
            Self::ImportFunscriptForRepair { .. } => "ImportFunscriptForRepair",
            Self::MergeCandidate { .. } => "MergeCandidate",
            Self::Diagnostics { .. } => "Diagnostics",
            Self::PinSource { .. } => "PinSource",
            Self::EvictSource { .. } => "EvictSource",
            Self::JobStatus { .. } => "JobStatus",
            Self::JobAttempts { .. } => "JobAttempts",
            Self::CancelJob { .. } => "CancelJob",
            Self::ResumeJob { .. } => "ResumeJob",
            Self::GetCandidate { .. } => "GetCandidate",
            Self::CommitCandidate { .. } => "CommitCandidate",
            Self::RebaseCandidate { .. } => "RebaseCandidate",
            Self::GetSnapshot => "GetSnapshot",
            Self::ReviewStatus => "ReviewStatus",
            Self::SetReviewDisposition { .. } => "SetReviewDisposition",
            Self::SetReviewIntervalDisposition { .. } => "SetReviewIntervalDisposition",
            Self::AllowProjectPackaging { .. } => "AllowProjectPackaging",
            Self::StartProjectImport { .. } => "StartProjectImport",
            Self::ProjectImportStatus { .. } => "ProjectImportStatus",
            Self::BeginProjectPackageUpload { .. } => "BeginProjectPackageUpload",
            Self::ProjectPackageUploadStatus { .. } => "ProjectPackageUploadStatus",
            Self::AbandonProjectPackageUpload { .. } => "AbandonProjectPackageUpload",
            Self::SealProjectImport { .. } => "SealProjectImport",
            Self::CancelProjectImport { .. } => "CancelProjectImport",
            Self::StartProjectExport => "StartProjectExport",
            Self::ProjectPackageStatus { .. } => "ProjectPackageStatus",
            Self::CancelProjectExport { .. } => "CancelProjectExport",
            Self::ReleaseProjectExport { .. } => "ReleaseProjectExport",
            Self::BeginProjectPackageDownload { .. } => "BeginProjectPackageDownload",
            Self::ProjectPackageDownloadStatus { .. } => "ProjectPackageDownloadStatus",
            Self::AbandonProjectPackageDownload { .. } => "AbandonProjectPackageDownload",
            Self::Capabilities => "Capabilities",
            Self::InferenceCatalog => "InferenceCatalog",
            Self::PreviewAnnotatedFrame { .. } => "PreviewAnnotatedFrame",
            Self::PreviewModelPackFrame { .. } => "PreviewModelPackFrame",
            Self::PreviewFrame { .. } => "PreviewFrame",
            Self::PreviewPlacedFrame { .. } => "PreviewPlacedFrame",
            Self::SetSourcePlacement { .. } => "SetSourcePlacement",
            Self::RemoveSourcePlacement { .. } => "RemoveSourcePlacement",
            Self::MotionRetimeStatus { .. } => "MotionRetimeStatus",
            Self::ArchivedMotionRetimeStatus { .. } => "ArchivedMotionRetimeStatus",
            Self::ArchivedGenerationStatus { .. } => "ArchivedGenerationStatus",
            Self::CandidateImageOriginStatus { .. } => "CandidateImageOriginStatus",
            Self::CandidateImageOriginPixels { .. } => "CandidateImageOriginPixels",
            Self::RetimeMotion { .. } => "RetimeMotion",
            Self::Export { .. } => "Export",
            Self::ExportAxis { .. } => "ExportAxis",
            Self::ExportAxisPinned { .. } => "ExportAxisPinned",
            Self::ExportDeviceMotion { .. } => "ExportDeviceMotion",
            Self::PrepareHandyStrokeScript { .. } => "PrepareHandyStrokeScript",
            Self::Pair { .. } => "Pair",
            Self::Grant { .. } => "Grant",
            Self::Revoke { .. } => "Revoke",
            Self::Events { .. } => "Events",
            Self::Arm { .. } => "Arm",
            Self::Stop { .. } => "Stop",
            Self::SwitchPlaybackRevision { .. } => "SwitchPlaybackRevision",
        })
    }
}

impl Command {
    pub fn is_native_preparation(&self) -> bool {
        matches!(
            self,
            Self::NativeImagePreparationLimits
                | Self::StartNativeImagePreparation { .. }
                | Self::ResolveNativeImagePreparation { .. }
                | Self::NativeImagePreparationStatus { .. }
                | Self::CancelNativeImagePreparation { .. }
                | Self::ReleaseNativeImagePreparation { .. }
        )
    }
    pub fn requires_project(&self) -> bool {
        if matches!(
            self,
            Self::InspectInferenceEnvironment { .. }
                | Self::ImportInferenceEquivalence { .. }
                | Self::InferenceEquivalenceStatus { .. }
                | Self::ListInferenceEquivalences { .. }
                | Self::AcceptInferenceEquivalence { .. }
                | Self::WithdrawInferenceEquivalence { .. }
        ) {
            return false;
        }
        if matches!(
            self,
            Self::StartProjectImport { .. }
                | Self::ProjectImportStatus { .. }
                | Self::BeginProjectPackageUpload { .. }
                | Self::ProjectPackageUploadStatus { .. }
                | Self::AbandonProjectPackageUpload { .. }
                | Self::SealProjectImport { .. }
                | Self::CancelProjectImport { .. }
        ) {
            return false;
        }
        if matches!(
            self,
            Self::AllowProjectPackaging { .. }
                | Self::StartProjectExport
                | Self::ProjectPackageStatus { .. }
                | Self::CancelProjectExport { .. }
                | Self::ReleaseProjectExport { .. }
                | Self::BeginProjectPackageDownload { .. }
                | Self::ProjectPackageDownloadStatus { .. }
                | Self::AbandonProjectPackageDownload { .. }
        ) {
            return true;
        }
        !matches!(
            self,
            Self::CreateProject { .. }
                | Self::LibraryBatchMigrate { .. }
                | Self::LibraryBatchPrepare { .. }
                | Self::LibraryBatchStatus { .. }
                | Self::LibraryBatchList
                | Self::LibraryBatchAdvance { .. }
                | Self::LibraryBatchRelease { .. }
                | Self::LibraryBatchCancel { .. }
                | Self::StashStartBatch { .. }
                | Self::StashBatchStatus { .. }
                | Self::StashResumeBatch { .. }
                | Self::StashMigrateBatch { .. }
                | Self::StashCancelBatch { .. }
                | Self::DiscardModelPackStage { .. }
                | Self::SelectModelPack { .. }
                | Self::WithdrawModelPackApproval { .. }
                | Self::SelectCatalogModelPack { .. }
                | Self::RefreshModelCatalog { .. }
                | Self::ImportModelPack { .. }
                | Self::ModelPackSelectionStatus { .. }
                | Self::ModelPackStagingStatus { .. }
                | Self::ModelCatalogStatus { .. }
                | Self::CheckModelPackCatalog { .. }
                | Self::ModelPackStatus { .. }
                | Self::HandyInspect { .. }
                | Self::OpenHandySession { .. }
                | Self::CloseHandySession { .. }
                | Self::HandySessions { .. }
                | Self::HandyQualificationContext { .. }
                | Self::ImportHandyQualification { .. }
                | Self::HandyQualificationStatus { .. }
                | Self::AcceptHandyQualification { .. }
                | Self::WithdrawHandyQualification { .. }
                | Self::ResolveHandyRegistration { .. }
                | Self::SetHandyRegistration { .. }
                | Self::DisableHandyRegistration { .. }
                | Self::ListHandyRegistrations { .. }
                | Self::InspectHandyRegistration { .. }
                | Self::StashListMissing { .. }
                | Self::StashPlanMissing { .. }
                | Self::OpenProject { .. }
                | Self::Capabilities
                | Self::InferenceCatalog
                | Self::Pair { .. }
                | Self::Grant { .. }
                | Self::Revoke { .. }
        )
    }

    pub fn requires_revision(&self) -> bool {
        if matches!(
            self,
            Self::InspectInferenceEnvironment { .. }
                | Self::ImportInferenceEquivalence { .. }
                | Self::InferenceEquivalenceStatus { .. }
                | Self::ListInferenceEquivalences { .. }
                | Self::AcceptInferenceEquivalence { .. }
                | Self::WithdrawInferenceEquivalence { .. }
        ) {
            return false;
        }
        if self.is_native_preparation() {
            return true;
        }
        if matches!(
            self,
            Self::ProbeMedia { .. }
                | Self::BrowserMediaCatalog
                | Self::PrepareBrowserMedia { .. }
                | Self::PrepareBrowserMediaSuccessor { .. }
                | Self::OpenBrowserTiming { .. }
                | Self::PrepareBrowserTimingSuccessor { .. }
                | Self::PrepareBrowserAnalysisSuccessor { .. }
                | Self::TransferBrowserAnalysisInput { .. }
                | Self::RemoveRetainedLiveCudaEvidence { .. }
                | Self::ListLiveCudaEvidence { .. }
                | Self::BeginLiveCudaEvidenceDownload { .. }
                | Self::InspectLiveCudaEvidence { .. }
                | Self::LiveCaptureLimits
                | Self::StartLiveCapture { .. }
                | Self::ContinueLiveCapture { .. }
                | Self::ContinueLiveCaptureAt { .. }
                | Self::LiveCaptureStatus { .. }
                | Self::StartLiveRecording { .. }
                | Self::LiveRecordingPage { .. }
                | Self::DiscardLiveRecording { .. }
                | Self::StopLiveCapture { .. }
                | Self::FinishLiveCapture { .. }
                | Self::CloseBrowserAnalysisInput { .. }
        ) {
            return true;
        }
        if matches!(
            self,
            Self::StartProjectImport { .. }
                | Self::ProjectImportStatus { .. }
                | Self::BeginProjectPackageUpload { .. }
                | Self::ProjectPackageUploadStatus { .. }
                | Self::AbandonProjectPackageUpload { .. }
                | Self::SealProjectImport { .. }
                | Self::CancelProjectImport { .. }
        ) {
            return false;
        }
        matches!(
            self,
            Self::BeginEditUpload { .. }
                | Self::ApplyAssistantEdit { .. }
                | Self::PrepareAssistantImage { .. }
                | Self::StartAssistant { .. }
                | Self::StartWasmEdit { .. }
                | Self::SetReviewDisposition { .. }
                | Self::SetReviewIntervalDisposition { .. }
                | Self::SetSourcePlacement { .. }
                | Self::RetimeMotion { .. }
                | Self::RemoveSourcePlacement { .. }
                | Self::StashImportSource { .. }
                | Self::StashExportScript { .. }
                | Self::StashSubmitScan { .. }
                | Self::StashAddTag { .. }
                | Self::StashResumeTag { .. }
                | Self::Undo
                | Self::Redo
                | Self::SetProtectedRegions { .. }
                | Self::CommitCandidate { .. }
                | Self::RebaseCandidate { .. }
                | Self::GenerateFromModelPack { .. }
                | Self::GenerateWithInferenceFallback { .. }
                | Self::GenerateWithAssistant { .. }
                | Self::GenerateCreativeWithAssistant { .. }
                | Self::GenerateInput { .. }
                | Self::ImportFunscript { .. }
                | Self::ImportFunscriptForRepair { .. }
                | Self::StageLiveRecording { .. }
                | Self::MergeCandidate { .. }
                | Self::ExportAxis { .. }
                | Self::ExportAxisPinned { .. }
                | Self::ExportDeviceMotion { .. }
                | Self::PrepareHandyStrokeScript { .. }
                | Self::SetupHandyDevice { .. }
                | Self::InspectHandySetup { .. }
                | Self::InspectHandyProgram { .. }
                | Self::BindHandyHspLive { .. }
                | Self::EnableHandyHspContinuity { .. }
                | Self::PlayHandyHspLive { .. }
                | Self::PrepareHandyHsp { .. }
                | Self::PrepareHandyPlayback { .. }
                | Self::AcquireHandyPlaybackClock { .. }
                | Self::SynchronizeHandyClock { .. }
                | Self::StartHandyPlayback { .. }
                | Self::StartHandyPlaybackFromBrowser { .. }
                | Self::ImportHandyHspSourceQualification { .. }
                | Self::ImportHandyBrowserStartQualification { .. }
                | Self::AcceptHandyHspSourceQualification { .. }
                | Self::AcceptHandyBrowserStartQualification { .. }
                | Self::RefreshHandyPlaybackTiming { .. }
                | Self::BindHandyPlayerGuard { .. }
                | Self::InspectHandyDeviceTiming { .. }
                | Self::SampleHandyServerClock { .. }
                | Self::AcquireHandyController { .. }
                | Self::RenewHandyController { .. }
                | Self::HostHandyStrokeScript { .. }
                | Self::StartProjectExport
        )
    }

    pub fn is_mutating(&self) -> bool {
        if matches!(
            self,
            Self::InspectInferenceEnvironment { .. }
                | Self::InferenceEquivalenceStatus { .. }
                | Self::ListInferenceEquivalences { .. }
        ) {
            return false;
        }
        if matches!(
            self,
            Self::ProbeMedia { .. }
                | Self::BrowserMediaCatalog
                | Self::BrowserMediaPreparationStatus { .. }
                | Self::BrowserTimingOpenStatus { .. }
                | Self::BrowserTimingStatus { .. }
                | Self::BrowserAnalysisTransferStatus { .. }
        ) {
            return false;
        }
        if matches!(
            self,
            Self::NativeImagePreparationLimits
                | Self::ResolveNativeImagePreparation { .. }
                | Self::NativeImagePreparationStatus { .. }
        ) {
            return false;
        }
        if matches!(
            self,
            Self::AssistantEditRecipes
                | Self::AssistantTargets
                | Self::AssistantProfiles
                | Self::PrepareAssistantImage { .. }
                | Self::AssistantStatus { .. }
                | Self::ListAssistantAttempts
                | Self::WasmEditorInfo
                | Self::WasmEditStatus { .. }
                | Self::ListWasmEdits
                | Self::ListArchivedWasmEdits { .. }
        ) {
            return false;
        }
        if matches!(
            self,
            Self::ListLiveCudaEvidence { .. }
                | Self::InspectLiveCudaEvidence { .. }
                | Self::LiveCaptureLimits
                | Self::LiveCaptureStatus { .. }
                | Self::LiveRecordingPage { .. }
        ) {
            return false;
        }
        match self {
            Self::ProjectImportStatus { .. } | Self::ProjectPackageUploadStatus { .. } => {
                return false
            }
            Self::StartProjectImport { .. }
            | Self::BeginProjectPackageUpload { .. }
            | Self::AbandonProjectPackageUpload { .. }
            | Self::SealProjectImport { .. }
            | Self::CancelProjectImport { .. } => return true,
            _ => {}
        }
        match self {
            Self::ProjectPackageStatus { .. } | Self::ProjectPackageDownloadStatus { .. } => {
                return false
            }
            Self::AllowProjectPackaging { .. }
            | Self::StartProjectExport
            | Self::CancelProjectExport { .. }
            | Self::ReleaseProjectExport { .. }
            | Self::BeginProjectPackageDownload { .. }
            | Self::AbandonProjectPackageDownload { .. } => return true,
            _ => {}
        }
        !matches!(
            self,
            Self::OpenProject { .. }
                | Self::PreviewAnnotatedFrame { .. }
                | Self::PreviewModelPackFrame { .. }
                | Self::PreviewFrame { .. }
                | Self::PreviewPlacedFrame { .. }
                | Self::ModelPackSelectionStatus { .. }
                | Self::ModelPackStagingStatus { .. }
                | Self::ModelCatalogStatus { .. }
                | Self::CheckModelPackCatalog { .. }
                | Self::ModelPackStatus { .. }
                | Self::HandySessions { .. }
                | Self::HandyQualificationContext { .. }
                | Self::HandyQualificationStatus { .. }
                | Self::HandyHostingStatus { .. }
                | Self::HandySetupStatus { .. }
                | Self::HandyPlaybackPreparationStatus { .. }
                | Self::HandyControllerStatus { .. }
                | Self::HandyClockSyncStatus { .. }
                | Self::HandyPlaybackStatus { .. }
                | Self::HandyBrowserPlaybackStatus { .. }
                | Self::HandyHspSourceQualificationStatus { .. }
                | Self::HandyBrowserStartQualificationStatus { .. }
                | Self::HandyHspPreparationStatus { .. }
                | Self::HandyHspLiveStatus { .. }
                | Self::HandyHspContinuityStatus { .. }
                | Self::HandyHspStatus { .. }
                | Self::HandyStopStatus { .. }
                | Self::HandyContinuousTimingStatus { .. }
                | Self::HandyPlayerGuardStatus { .. }
                | Self::PlanHandyReconciliation { .. }
                | Self::HandyReconciliationStatus { .. }
                | Self::InspectHandyDeviceTiming { .. }
                | Self::InspectHandySetup { .. }
                | Self::InspectHandyProgram { .. }
                | Self::HandyInspect { .. }
                | Self::ListHandyRegistrations { .. }
                | Self::InspectHandyRegistration { .. }
                | Self::StashListMissing { .. }
                | Self::StashPlanMissing { .. }
                | Self::MotionRetimeStatus { .. }
                | Self::ArchivedMotionRetimeStatus { .. }
                | Self::ArchivedGenerationStatus { .. }
                | Self::CandidateImageOriginStatus { .. }
                | Self::CandidateImageOriginPixels { .. }
                | Self::GetSnapshot
                | Self::ReviewStatus
                | Self::StashScanStatus { .. }
                | Self::StashPublicationStatus { .. }
                | Self::StashReconcileAttachment { .. }
                | Self::StashTagStatus { .. }
                | Self::StashBatchStatus { .. }
                | Self::LibraryBatchStatus { .. }
                | Self::LibraryBatchList
                | Self::Capabilities
                | Self::InferenceCatalog
                | Self::GetCandidate { .. }
                | Self::JobStatus { .. }
                | Self::JobAttempts { .. }
                | Self::Events { .. }
                | Self::Diagnostics { .. }
                | Self::TransferStatus { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Read,
    Edit,
    Generate,
    Export,
    ManageGrants,
    ManageProtection,
    ImportSource,
    /// Full editable-project capture, distinct from neutral motion export.
    PackageProject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u16,
    pub request_id: RequestId,
    pub result: Result<ResponseBody, ProtocolError>,
}

impl Response {
    pub fn success(request_id: RequestId, body: ResponseBody) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            result: Ok(body),
        }
    }
    pub fn failure(request_id: RequestId, error: ProtocolError) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            result: Err(error),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ResponseBody {
    InferenceEnvironment(Box<InferenceEnvironmentReport>),
    InferenceEquivalenceStatus(Box<InferenceEquivalenceStatus>),
    InferenceEquivalenceList(Box<InferenceEquivalencePage>),
    InferenceEquivalenceSaved(Box<InferenceEquivalenceReceipt>),
    HandyHspSourceQualification(Box<HandyHspSourceQualificationReport>),
    HandyHspSourceQualificationSaved(Box<HandyHspSourceQualificationReceipt>),
    HandyBrowserStartQualification(Box<HandyBrowserStartQualificationReport>),
    HandyBrowserStartQualificationSaved(Box<HandyBrowserStartQualificationReceipt>),
    HandyBrowserPlayback(Box<HandyBrowserPlaybackReport>),
    MediaProbe(Box<MediaProbeReport>),
    BrowserMediaCatalog(Box<BrowserMediaCatalog>),
    BrowserMediaPreparation(Box<BrowserMediaPreparationReport>),
    BrowserTiming(Box<BrowserTimingReport>),
    BrowserTimingProbe(Box<BrowserTimingProbe>),
    AssistantStartResolution(Box<AssistantStartResolution>),
    AssistantEditRecipes(Vec<AssistantEditRecipe>),
    AssistantTargets(Box<AssistantTargetCatalog>),
    AssistantProfiles(Vec<AssistantProfile>),
    AssistantImage(Box<AssistantImageContext>),
    Assistant(Box<AssistantReport>),
    AssistantAttempts(Vec<AssistantReport>),
    WasmEditorInfo(WasmEditorInfo),
    WasmEdit(Box<WasmEditReport>),
    WasmStartResolution(Box<WasmStartResolution>),
    WasmEditCatalog(WasmEditCatalog),
    WasmArchivePage(WasmArchivePage),
    WasmEditArchived(Box<WasmEditReport>),
    WasmEditRemoved(Box<WasmEditReport>),
    LiveCapture(Box<LiveCaptureReport>),
    LiveRecordingPage(Box<LiveRecordingPage>),
    LiveCudaEvidenceRemoval(LiveCudaEvidenceRemoval),
    LiveCudaEvidencePage(LiveCudaEvidencePage),
    RetainedLiveCudaEvidence(Box<RetainedLiveCudaEvidence>),
    LiveCaptureLimits(LiveCaptureLimits),
    MotionRetimeReceipt(Box<MotionRetimeReceipt>),
    ArchivedMotionRetimeReceipt(Box<ArchivedMotionRetimeReceipt>),
    ArchivedGeneration(Box<ArchivedGenerationReport>),
    CandidateImageOrigin(Box<CandidateImageOriginReport>),
    CandidateImageOriginPixels(Box<CandidateImageOriginPixels>),
    ModelPackSelection(Box<ModelPackSelectionReport>),
    ModelPackWithdrawal(Box<ModelPackWithdrawalReport>),
    ModelPack(ModelPackReport),
    ModelCatalog(ModelCatalogReport),
    ModelPackCatalog(Box<ModelPackCatalogReport>),
    ModelPackStaging(ModelPackStagingReport),
    ModelPackDiscard(ModelPackDiscardReport),
    HandyInspection(HandyInspection),
    HandyStrokeScript(Box<HandyStrokeScript>),
    HandyHosting(Box<HandyHostingReport>),
    HandyQualificationContext(Box<HandyQualificationContext>),
    HandyQualification(Box<HandyQualificationReport>),
    HandyQualificationSaved(Box<HandyQualificationReceipt>),
    HandySetupInspection(Box<HandySetupInspection>),
    HandyProgramCorrelation(Box<HandyProgramCorrelation>),
    HandyPlaybackPreparation(Box<HandyPlaybackPreparation>),
    HandyController(Box<HandyControllerReceipt>),
    HandyServerClock(Box<HandyServerClockObservation>),
    HandyDeviceTiming(Box<HandyDeviceTimingInspection>),
    HandyClockSync(Box<HandyClockSyncStatus>),
    HandyPlayback(Box<HandyPlaybackStatus>),
    HandyHspLive(Box<HandyHspLiveStatus>),
    HandyHspContinuity(Box<HandyHspContinuityStatus>),
    BrowserAnalysisSuccessor(Box<BrowserAnalysisSuccessorReport>),
    BrowserAnalysisTransfer(Box<BrowserAnalysisTransferReport>),
    HandyHspPreparation(Box<HandyHspPreparationStatus>),
    HandyHsp(Box<HandyHspStatus>),
    HandyStop(Box<HandyStopStatus>),
    HandyContinuousTiming(Box<HandyContinuousTimingObservation>),
    HandyPlayerGuard(Box<HandyPlayerGuardReport>),
    HandyPlayerChallenge(Box<HandyPlayerChallenge>),
    HandyRecoveryPlan(Box<HandyRecoveryPlan>),
    HandyRecovery(Box<HandyRecoveryStatus>),
    HandyPlaybackClock(Box<HandyPlaybackClockObservation>),
    HandyPlaybackRetirement(Box<HandyPlaybackRetirement>),
    HandySetupStatus(Box<HandySetupStatus>),
    HandySetupCancellation(Box<HandySetupCancellation>),
    HandySession(Box<HandyConnectionSession>),
    HandySessions(Box<HandyConnectionCatalog>),
    HandyRegistrationSaved(Box<HandyRegistrationReceipt>),
    HandyRegistrationResolution(Box<HandyRegistrationResolution>),
    HandyRegistrations(Box<HandyRegistrationCatalog>),
    HandyRegisteredInspection(Box<HandyRegisteredInspection>),
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
    Project(Box<ProjectSnapshot>),
    Job(JobSnapshot),
    JobAttempts(Box<JobAttemptPage>),
    Candidate(Box<CandidateSnapshot>),
    Diagnostics(DiagnosticsReport),
    ReviewStatus(ReviewStatus),
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
    DeviceMotionExported {
        path: PathBuf,
        revision: RevisionId,
        gain: f64,
        profile_sha256: String,
        master_sha256: String,
    },
    ModelPackPreview(Box<ModelPackPreviewResult>),
    Preview(Box<PreviewResult>),
    Capabilities(Capabilities),
    Events(EventPage),
    Paired {
        session: SessionId,
        auth_token: String,
    },
    PackageImport(Box<PackageImportStatus>),
    NativeImagePreparation(Box<NativeImagePreparationStatus>),
    NativeImagePreparationLimits(Box<NativeImagePreparationLimits>),
    PackageUpload(PackageUploadLease),
    PackageUploadAbandoned {
        lease_id: PackageUploadId,
    },
    PackageExport(Box<PackageExportStatus>),
    PackageDownload(Box<PackageDownloadLease>),
    PackageDownloadPending(PackageDownloadPending),
    PackageDownloadAbandoned {
        lease_id: PackageDownloadId,
    },
    Ack,
}

impl std::fmt::Debug for ResponseBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InferenceEnvironment(_) => f.write_str("InferenceEnvironment"),
            Self::InferenceEquivalenceStatus(_) => f.write_str("InferenceEquivalenceStatus"),
            Self::InferenceEquivalenceList(_) => f.write_str("InferenceEquivalenceList"),
            Self::InferenceEquivalenceSaved(_) => f.write_str("InferenceEquivalenceSaved"),
            Self::HandyHspSourceQualification(value) => f
                .debug_tuple("HandyHspSourceQualification")
                .field(value)
                .finish(),
            Self::HandyBrowserStartQualification(value) => f
                .debug_tuple("HandyBrowserStartQualification")
                .field(value)
                .finish(),
            Self::HandyHspSourceQualificationSaved(value) => f
                .debug_tuple("HandyHspSourceQualificationSaved")
                .field(value)
                .finish(),
            Self::HandyBrowserStartQualificationSaved(value) => f
                .debug_tuple("HandyBrowserStartQualificationSaved")
                .field(value)
                .finish(),
            Self::HandyBrowserPlayback(value) => {
                f.debug_tuple("HandyBrowserPlayback").field(value).finish()
            }
            Self::AssistantStartResolution(_) => f.write_str("AssistantStartResolution"),
            Self::AssistantEditRecipes(value) => {
                f.debug_tuple("AssistantEditRecipes").field(value).finish()
            }
            Self::AssistantTargets(value) => {
                f.debug_tuple("AssistantTargets").field(value).finish()
            }
            Self::AssistantImage(value) => f
                .debug_tuple("AssistantImage")
                .field(&value.image_id)
                .finish(),
            Self::AssistantProfiles(value) => {
                f.debug_tuple("AssistantProfiles").field(value).finish()
            }
            Self::Assistant(value) => f.debug_tuple("Assistant").field(value).finish(),
            Self::AssistantAttempts(value) => {
                f.debug_tuple("AssistantAttempts").field(value).finish()
            }
            Self::MediaProbe(value) => f.debug_tuple("MediaProbe").field(value).finish(),
            Self::BrowserMediaCatalog(value) => {
                f.debug_tuple("BrowserMediaCatalog").field(value).finish()
            }
            Self::BrowserMediaPreparation(value) => f
                .debug_tuple("BrowserMediaPreparation")
                .field(value)
                .finish(),
            Self::BrowserTiming(value) => f.debug_tuple("BrowserTiming").field(value).finish(),
            Self::BrowserTimingProbe(value) => {
                f.debug_tuple("BrowserTimingProbe").field(value).finish()
            }
            Self::WasmEditorInfo(value) => f.debug_tuple("WasmEditorInfo").field(value).finish(),
            Self::WasmEdit(value) => f.debug_tuple("WasmEdit").field(value).finish(),
            Self::WasmStartResolution(value) => {
                f.debug_tuple("WasmStartResolution").field(value).finish()
            }
            Self::WasmEditCatalog(value) => f.debug_tuple("WasmEditCatalog").field(value).finish(),
            Self::WasmArchivePage(value) => f.debug_tuple("WasmArchivePage").field(value).finish(),
            Self::WasmEditArchived(value) => {
                f.debug_tuple("WasmEditArchived").field(value).finish()
            }
            Self::WasmEditRemoved(value) => f.debug_tuple("WasmEditRemoved").field(value).finish(),
            Self::LiveCapture(_) => f.write_str("LiveCapture"),
            Self::LiveRecordingPage(_) => f.write_str("LiveRecordingPage"),
            Self::LiveCudaEvidenceRemoval(_) => f.write_str("LiveCudaEvidenceRemoval"),
            Self::LiveCudaEvidencePage(_) => f.write_str("LiveCudaEvidencePage"),
            Self::RetainedLiveCudaEvidence(_) => f.write_str("RetainedLiveCudaEvidence"),
            Self::LiveCaptureLimits(_) => f.write_str("LiveCaptureLimits"),
            Self::StashScriptExported(value) => {
                f.debug_tuple("StashScriptExported").field(value).finish()
            }
            Self::StashPublication(value) => {
                f.debug_tuple("StashPublication").field(value).finish()
            }
            Self::StashSourceCaptured(value) => {
                f.debug_tuple("StashSourceCaptured").field(value).finish()
            }
            Self::StashBatch(value) => f.debug_tuple("StashBatch").field(value).finish(),
            Self::LibraryBatchReleased(value) => {
                f.debug_tuple("LibraryBatchReleased").field(value).finish()
            }
            Self::LibraryBatch(value) => f.debug_tuple("LibraryBatch").field(value).finish(),
            Self::LibraryBatchCatalog(value) => {
                f.debug_tuple("LibraryBatchCatalog").field(value).finish()
            }
            Self::StashTag(value) => f.debug_tuple("StashTag").field(value).finish(),
            Self::StashScan(value) => f.debug_tuple("StashScan").field(value).finish(),
            Self::StashPlan(value) => f.debug_tuple("StashPlan").field(value).finish(),
            Self::MotionRetimeReceipt(value) => {
                f.debug_tuple("MotionRetimeReceipt").field(value).finish()
            }
            Self::ArchivedGeneration(value) => {
                f.debug_tuple("ArchivedGeneration").field(value).finish()
            }
            Self::CandidateImageOriginPixels(value) => f
                .debug_tuple("CandidateImageOriginPixels")
                .field(value)
                .finish(),
            Self::CandidateImageOrigin(value) => {
                f.debug_tuple("CandidateImageOrigin").field(value).finish()
            }
            Self::ArchivedMotionRetimeReceipt(value) => f
                .debug_tuple("ArchivedMotionRetimeReceipt")
                .field(value)
                .finish(),
            Self::ModelPackDiscard(value) => {
                f.debug_tuple("ModelPackDiscard").field(value).finish()
            }
            Self::ModelPackCatalog(value) => {
                f.debug_tuple("ModelPackCatalog").field(value).finish()
            }
            Self::ModelCatalog(value) => f.debug_tuple("ModelCatalog").field(value).finish(),
            Self::ModelPackStaging(value) => {
                f.debug_tuple("ModelPackStaging").field(value).finish()
            }
            Self::ModelPackWithdrawal(value) => {
                f.debug_tuple("ModelPackWithdrawal").field(value).finish()
            }
            Self::ModelPackSelection(value) => {
                f.debug_tuple("ModelPackSelection").field(value).finish()
            }
            Self::ModelPack(value) => f.debug_tuple("ModelPack").field(value).finish(),
            Self::HandySession(value) => f.debug_tuple("HandySession").field(value).finish(),
            Self::HandySessions(value) => f.debug_tuple("HandySessions").field(value).finish(),
            Self::HandySetupCancellation(value) => f
                .debug_tuple("HandySetupCancellation")
                .field(value)
                .finish(),
            Self::HandySetupStatus(value) => {
                f.debug_tuple("HandySetupStatus").field(value).finish()
            }
            Self::HandyPlaybackClock(value) => {
                f.debug_tuple("HandyPlaybackClock").field(value).finish()
            }
            Self::HandyPlayback(value) => f.debug_tuple("HandyPlayback").field(value).finish(),
            Self::HandyHspLive(value) => f.debug_tuple("HandyHspLive").field(value).finish(),
            Self::HandyHspContinuity(value) => {
                f.debug_tuple("HandyHspContinuity").field(value).finish()
            }
            Self::BrowserAnalysisSuccessor(value) => f
                .debug_tuple("BrowserAnalysisSuccessor")
                .field(value)
                .finish(),
            Self::BrowserAnalysisTransfer(value) => f
                .debug_tuple("BrowserAnalysisTransfer")
                .field(value)
                .finish(),
            Self::HandyHspPreparation(value) => {
                f.debug_tuple("HandyHspPreparation").field(value).finish()
            }
            Self::HandyHsp(value) => f.debug_tuple("HandyHsp").field(value).finish(),
            Self::HandyStop(value) => f.debug_tuple("HandyStop").field(value).finish(),
            Self::HandyContinuousTiming(value) => {
                f.debug_tuple("HandyContinuousTiming").field(value).finish()
            }
            Self::HandyPlayerGuard(value) => {
                f.debug_tuple("HandyPlayerGuard").field(value).finish()
            }
            Self::HandyPlayerChallenge(value) => {
                f.debug_tuple("HandyPlayerChallenge").field(value).finish()
            }
            Self::HandyRecoveryPlan(value) => {
                f.debug_tuple("HandyRecoveryPlan").field(value).finish()
            }
            Self::HandyRecovery(value) => f.debug_tuple("HandyRecovery").field(value).finish(),
            Self::HandyClockSync(value) => f.debug_tuple("HandyClockSync").field(value).finish(),
            Self::HandyDeviceTiming(value) => {
                f.debug_tuple("HandyDeviceTiming").field(value).finish()
            }
            Self::HandyServerClock(value) => {
                f.debug_tuple("HandyServerClock").field(value).finish()
            }
            Self::HandyController(value) => f.debug_tuple("HandyController").field(value).finish(),
            Self::HandyPlaybackPreparation(value) => f
                .debug_tuple("HandyPlaybackPreparation")
                .field(value)
                .finish(),
            Self::HandyPlaybackRetirement(value) => f
                .debug_tuple("HandyPlaybackRetirement")
                .field(value)
                .finish(),
            Self::HandyProgramCorrelation(value) => f
                .debug_tuple("HandyProgramCorrelation")
                .field(value)
                .finish(),
            Self::HandySetupInspection(value) => {
                f.debug_tuple("HandySetupInspection").field(value).finish()
            }
            Self::HandyQualificationContext(value) => f
                .debug_tuple("HandyQualificationContext")
                .field(value)
                .finish(),
            Self::HandyQualification(value) => {
                f.debug_tuple("HandyQualification").field(value).finish()
            }
            Self::HandyQualificationSaved(value) => f
                .debug_tuple("HandyQualificationSaved")
                .field(value)
                .finish(),
            Self::HandyHosting(value) => f.debug_tuple("HandyHosting").field(value).finish(),
            Self::HandyStrokeScript(value) => {
                f.debug_tuple("HandyStrokeScript").field(value).finish()
            }
            Self::HandyInspection(value) => f.debug_tuple("HandyInspection").field(value).finish(),
            Self::HandyRegistrationResolution(value) => f
                .debug_tuple("HandyRegistrationResolution")
                .field(value)
                .finish(),
            Self::HandyRegistrationSaved(value) => f
                .debug_tuple("HandyRegistrationSaved")
                .field(value)
                .finish(),
            Self::HandyRegistrations(value) => {
                f.debug_tuple("HandyRegistrations").field(value).finish()
            }
            Self::HandyRegisteredInspection(value) => f
                .debug_tuple("HandyRegisteredInspection")
                .field(value)
                .finish(),
            Self::StashDiscovery(value) => f.debug_tuple("StashDiscovery").field(value).finish(),
            Self::Project(value) => f.debug_tuple("Project").field(value).finish(),
            Self::Job(value) => f.debug_tuple("Job").field(value).finish(),
            Self::JobAttempts(value) => f.debug_tuple("JobAttempts").field(value).finish(),
            Self::Candidate(value) => f.debug_tuple("Candidate").field(value).finish(),
            Self::ReviewStatus(_) => f.write_str("ReviewStatus"),
            Self::Diagnostics(value) => f.debug_tuple("Diagnostics").field(value).finish(),
            Self::Transfer(value) => f.debug_tuple("Transfer").field(value).finish(),
            Self::TransferAbandoned { lease_id } => f
                .debug_struct("TransferAbandoned")
                .field("lease_id", lease_id)
                .finish(),
            Self::Source { source_version } => f
                .debug_struct("Source")
                .field("source_version", source_version)
                .finish(),
            Self::Exported { path, revision } => f
                .debug_struct("Exported")
                .field("path", path)
                .field("revision", revision)
                .finish(),
            Self::AxisExported {
                path,
                revision,
                axis,
            } => f
                .debug_struct("AxisExported")
                .field("path", path)
                .field("revision", revision)
                .field("axis", axis)
                .finish(),
            Self::ModelPackPreview(value) => {
                f.debug_tuple("ModelPackPreview").field(value).finish()
            }
            Self::Preview(value) => f.debug_tuple("Preview").field(value).finish(),
            Self::DeviceMotionExported {
                path,
                revision,
                gain,
                ..
            } => f
                .debug_struct("DeviceMotionExported")
                .field("path", path)
                .field("revision", revision)
                .field("gain", gain)
                .finish(),
            Self::Capabilities(value) => f.debug_tuple("Capabilities").field(value).finish(),
            Self::InferenceCatalog(value) => {
                f.debug_tuple("InferenceCatalog").field(value).finish()
            }
            Self::Events(value) => f.debug_tuple("Events").field(value).finish(),
            Self::Paired { session, .. } => f
                .debug_struct("Paired")
                .field("session", session)
                .field("auth_token", &"[REDACTED]")
                .finish(),
            Self::PackageExport(value) => f.debug_tuple("PackageExport").field(value).finish(),
            Self::PackageDownload(value) => f.debug_tuple("PackageDownload").field(value).finish(),
            Self::PackageDownloadPending(value) => f
                .debug_tuple("PackageDownloadPending")
                .field(value)
                .finish(),
            Self::PackageDownloadAbandoned { lease_id } => f
                .debug_struct("PackageDownloadAbandoned")
                .field("lease_id", lease_id)
                .finish(),
            Self::PackageImport(_) => f.write_str("PackageImport"),
            Self::PackageUpload(_) => f.write_str("PackageUpload"),
            Self::PackageUploadAbandoned { .. } => f.write_str("PackageUploadAbandoned"),
            Self::NativeImagePreparation(value) => f
                .debug_tuple("NativeImagePreparation")
                .field(value)
                .finish(),
            Self::NativeImagePreparationLimits(value) => f
                .debug_tuple("NativeImagePreparationLimits")
                .field(value)
                .finish(),
            Self::Ack => f.write_str("Ack"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSnapshot {
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub name: String,
    pub motion: MotionDescriptor,
    pub sources: Vec<SourceSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<SourcePlacement>,
}

/// Engine-assigned source purpose, not MIME detection or a capability grant.
/// Unknown media imported through ImportSource remains Media until a worker
/// validates its format; this must not be inferred from filenames or labels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    Media,
    GenerationInput,
    Funscript,
    LiveRecording,
}
impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Media => "media",
            Self::GenerationInput => "generation_input",
            Self::Funscript => "funscript",
            Self::LiveRecording => "live_recording",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSummary {
    pub source_version: SourceVersionId,
    pub label: String,
    /// Legacy wire summaries describe the original generic media-import path.
    /// Persisted newer synthetic sources require an engine-owned migration.
    #[serde(default)]
    pub kind: SourceKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSnapshot {
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub state: String,
    pub candidate_id: Option<CandidateId>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSnapshot {
    /// Native finite-generation coverage, derived from the retained job manifest.
    /// Imported proposals retain archival provenance without native job authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_interval: Option<TimeRange>,
    pub candidate_id: CandidateId,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub motion: MotionDescriptor,
    pub job_id: Option<JobId>,
    #[serde(default)]
    pub review: Vec<ReviewFlag>,
}

/// Ordinary desktop preview admission. Target declarations retain these maxima so
/// the native view adapter reproduces the displayed aspect-ratio fit exactly.
pub const PREVIEW_FRAME_MAX_DIMENSIONS: [u32; 2] = [640, 480];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotated: Option<AnnotatedPreviewEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placed: Option<PlacedPreviewReceipt>,
    #[serde(default, skip_serializing_if = "PreviewTimeBasis::is_source_pts")]
    pub time_basis: PreviewTimeBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_source_time: Option<SourceTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_origin: Option<SourceTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate_map: Option<pulsar_core::ViewCoordinateMap>,
    pub context: FrameContext,
    pub requested_source_time: SourceTimestamp,
    pub source_time: SourceTimestamp,
    pub source_frame_index: u64,
    pub observations: Vec<DetectionObservation>,
    pub frame: Option<FrameArtifact>,
    pub analysis: AnalysisStatus,
}

impl PreviewResult {
    /// Exact displayed-frame association. A neighboring frame is not a match.
    pub fn matches_request(&self, context: &FrameContext) -> bool {
        &self.context == context && self.actual_frame_is_consistent()
    }

    /// A time-based seek may resolve to a different source-frame ordinal.
    /// Matching this request does not mean its guessed frame was displayed:
    /// clients must adopt the returned actual context and timestamp, then use
    /// exact frame matching when attaching observations to displayed pixels.
    pub fn matches_seek_request(
        &self,
        context: &FrameContext,
        requested_source_time: &SourceTimestamp,
    ) -> bool {
        self.matches_timed_seek_request(context, requested_source_time, PreviewTimeBasis::SourcePts)
    }

    pub fn matches_timed_seek_request(
        &self,
        context: &FrameContext,
        requested_source_time: &SourceTimestamp,
        time_basis: PreviewTimeBasis,
    ) -> bool {
        self.placed.is_none()
            && self.time_basis == time_basis
            && match time_basis {
                PreviewTimeBasis::SourcePts => {
                    timestamp_at_or_before(self.source_time, *requested_source_time)
                }
                PreviewTimeBasis::ElapsedFromFirstFrame => self
                    .elapsed_source_time
                    .is_some_and(|elapsed| timestamp_at_or_before(elapsed, *requested_source_time)),
            }
            && (time_basis == PreviewTimeBasis::SourcePts
                || (self.elapsed_source_time.is_some() && self.source_origin.is_some()))
            && &self.requested_source_time == requested_source_time
            && self.context.source_version == context.source_version
            && self.context.source_placement == context.source_placement
            && self.context.transform == context.transform
            && self.context.seek_generation == context.seek_generation
            && self.context.request_generation == context.request_generation
            && self.actual_frame_is_consistent()
    }

    pub fn matches_placed_seek_request(
        &self,
        context: &FrameContext,
        request: &PlacedPreviewRequest,
    ) -> bool {
        self.placed
            .as_ref()
            .is_some_and(|receipt| &receipt.request == request)
            && request.validate(context).is_ok()
            && self.context.source_version == context.source_version
            && self.context.source_placement == context.source_placement
            && self.context.transform == context.transform
            && self.context.seek_generation == context.seek_generation
            && self.context.request_generation == context.request_generation
            && self.actual_frame_is_consistent()
    }

    fn placed_receipt_is_consistent(&self) -> bool {
        let Some(receipt) = &self.placed else {
            return true;
        };
        let request = &receipt.request;
        if request.validate(&self.context).is_err()
            || self.time_basis != PreviewTimeBasis::ElapsedFromFirstFrame
        {
            return false;
        }
        let Some(elapsed) = self.elapsed_source_time else {
            return false;
        };
        let Ok(actual) = request
            .placement
            .spec
            .project_at_source(ExactSeconds::from_source(elapsed))
        else {
            return false;
        };
        let Ok(target) = request
            .placement
            .spec
            .source_at_project(request.project_time)
        else {
            return false;
        };
        matches!(
            ExactSeconds::from_source(elapsed).checked_cmp(target),
            Ok(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) && actual == receipt.actual_project_time
            && actual.quantize_nanoseconds().ok().as_ref() == Some(&receipt.quantization)
    }

    fn actual_frame_is_consistent(&self) -> bool {
        if self
            .annotated
            .as_ref()
            .is_some_and(|e| e.validate(self).is_err())
            || (self
                .context
                .transform
                .as_str()
                .starts_with("annotated-preview-v1.")
                && self.annotated.is_none())
        {
            return false;
        }
        if !self.placed_receipt_is_consistent() {
            return false;
        }
        match (self.source_origin, self.elapsed_source_time) {
            (None, None) if self.time_basis == PreviewTimeBasis::SourcePts => {}
            (Some(origin), Some(elapsed)) if elapsed.numerator() >= 0 => {
                let difference = i128::from(self.source_time.numerator())
                    * i128::from(origin.denominator())
                    - i128::from(origin.numerator()) * i128::from(self.source_time.denominator());
                let left = difference.checked_mul(i128::from(elapsed.denominator()));
                let right = i128::from(elapsed.numerator())
                    .checked_mul(i128::from(origin.denominator()))
                    .and_then(|n| n.checked_mul(i128::from(self.source_time.denominator())));
                if left.is_none() || left != right {
                    return false;
                }
            }
            _ => return false,
        }
        if self
            .context
            .transform
            .as_str()
            .starts_with("video-view-v1.")
            && self.coordinate_map.is_none()
        {
            return false;
        }
        self.coordinate_map.as_ref().is_none_or(|map| {
            map.validate().is_ok()
                && video_view_transform(Some(map.view()))
                    .is_ok_and(|id| id == self.context.transform)
                && self
                    .frame
                    .as_ref()
                    .is_some_and(|frame| map.output_dimensions() == (frame.width, frame.height))
        }) && self.context.frame.value() == self.source_frame_index
            && self
                .observations
                .iter()
                .all(|o| o.matches_frame(&self.context))
    }
}

fn timestamp_at_or_before(actual: SourceTimestamp, requested: SourceTimestamp) -> bool {
    i128::from(actual.numerator()) * i128::from(requested.denominator())
        <= i128::from(requested.numerator()) * i128::from(actual.denominator())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnalysisStatus {
    Available,
    Pending,
    Unavailable { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub protocol_version: u16,
    pub operations: Vec<String>,
    pub backends: Vec<CapabilityStatus>,
    pub physical_playback: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityStatus {
    pub name: String,
    pub available: bool,
    pub qualification: QualificationStatus,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualificationStatus {
    Qualified { record: String },
    Unqualified,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventPage {
    pub events: Vec<Event>,
    pub next_cursor: u64,
    pub resync_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub version: u16,
    pub cursor: u64,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub body: EventBody,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EventBody {
    ProjectChanged,
    JobChanged(JobSnapshot),
    CandidateReady {
        candidate_id: CandidateId,
    },
    GrantRevoked {
        session: SessionId,
    },
    PlaybackDisarmed {
        device_session: DeviceSessionId,
        stopping_confirmed: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// An authenticated, exactly bound package-upload lease already has an
    /// active connection. Only the upload-handshake guard emits this code.
    /// Retry requires retryable=true and the same validated lease binding,
    /// bounded attempts, and the existing absolute deadline. This is not
    /// generic resource exhaustion or proof that the old connection is retiring.
    PackageUploadConnectionBusy,
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    RevisionConflict,
    RequestConflict,
    ResourceExhausted,
    Unsupported,
    Unavailable,
    Cancelled,
    DependencyMismatch,
    Internal,
    UnknownOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }
    pub fn unsupported(operation: impl AsRef<str>) -> Self {
        Self::new(
            ErrorCode::Unsupported,
            format!("{} is not supported by this build", operation.as_ref()),
        )
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for ProtocolError {}

fn validate_registration_scope(
    request: &Request,
    revision: u64,
    proof: &str,
) -> Result<(), ProtocolError> {
    if request.project.is_some()
        || request.expected_revision.is_some()
        || revision > i64::MAX as u64
    {
        return Err(ProtocolError::invalid(
            "Handy registration uses a bounded registration revision and no project scope",
        ));
    }
    bounded_text(proof, 64, "Handy owner capability")
}

#[cfg(test)]
mod inference_environment_wire_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn artifact(bytes: &[u8]) -> ArtifactIdentity {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }

    fn platform() -> InferencePlatformObservation {
        InferencePlatformObservation {
            schema_version: 1,
            os: "linux".into(),
            architecture: "x86_64".into(),
            kernel_release: "6.12.0-fixture".into(),
            kernel_version: "#1 SMP test observation".into(),
        }
    }

    fn model() -> InferenceCpuModelObservation {
        InferenceCpuModelObservation {
            vendor_id: "GenuineIntel".into(),
            family: 6,
            model: 85,
            model_name: "Fixture CPU".into(),
            stepping: 4,
            microcode: 0x2000065,
            flags: vec!["avx".into(), "sse2".into()],
        }
    }

    fn hardware() -> InferenceHardwareObservation {
        InferenceHardwareObservation {
            schema_version: 1,
            architecture: "x86_64".into(),
            cpu_models: vec![model()],
            logical_cpus: vec![
                InferenceLogicalCpuObservation {
                    processor_id: 0,
                    package_id: 0,
                    core_id: 0,
                    model_index: 0,
                },
                InferenceLogicalCpuObservation {
                    processor_id: 1,
                    package_id: 0,
                    core_id: 1,
                    model_index: 0,
                },
            ],
            cpu_packages: vec![InferenceCpuPackageObservation {
                package_id: 0,
                reported_logical_cpus: 4,
                reported_physical_cores: 2,
            }],
            allowed_cpu_ids: vec![0, 1],
            physical_memory_bytes: 16 << 30,
            gpu: None,
        }
    }

    pub(crate) fn profile() -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact(b"declared model, not installed"),
            runtime: artifact(b"observed fixture runtime"),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 320,
                    height: 180,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 4096,
            minimum_output_bytes: 1024,
            cpu_threads: 2,
            cuda_resources: None,
        }
    }

    pub(crate) fn report() -> InferenceEnvironmentReport {
        let declared_profile = profile();
        let platform = platform();
        let hardware = hardware();
        InferenceEnvironmentReport {
            schema_version: 1,
            profile_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&declared_profile).unwrap())
            ),
            environment: InferenceEquivalenceEnvironment {
                engine: artifact(b"fixture engine"),
                worker: artifact(b"fixture worker"),
                os: "linux".into(),
                architecture: "x86_64".into(),
                platform_build: platform.identity().unwrap(),
                hardware: hardware.identity().unwrap(),
                dependencies: vec![declared_profile.runtime.clone()],
            },
            declared_profile,
            platform,
            hardware,
            observed_at_unix_ms: 1,
            runtime_dependency_closure_verified: false,
            qualified: false,
            report_only: true,
        }
    }

    fn request() -> Request {
        Request::new(
            RequestId::new("environment-observation").unwrap(),
            Command::InspectInferenceEnvironment {
                profile: Box::new(profile()),
                owner_proof: "private-owner-proof".into(),
            },
        )
        .with_session(SessionId::new("owner-session").unwrap())
        .with_auth_token("private-session-token".into())
    }

    #[test]
    fn environment_command_is_global_read_only_and_debug_redacts_its_payload() {
        let request = request();
        request.validate().unwrap();
        assert!(!request.command.requires_project());
        assert!(!request.command.requires_revision());
        assert!(!request.command.is_mutating());
        assert_eq!(
            format!("{:?}", request.command),
            "InspectInferenceEnvironment"
        );
        let debug = format!("{request:?}");
        let profile_digest = profile().model.sha256;
        for secret in [
            "private-owner-proof",
            "private-session-token",
            profile_digest.as_str(),
        ] {
            assert!(!debug.contains(secret));
        }
        let bytes = serde_json::to_vec(&request).unwrap();
        let decoded: Request = serde_json::from_slice(&bytes).unwrap();
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    }

    #[test]
    fn environment_command_refuses_scope_invalid_profile_and_bad_proof_bounds() {
        let mut scoped = request();
        scoped.project = Some(ProjectId::new("project").unwrap());
        assert!(scoped.validate().is_err());
        scoped.project = None;
        scoped.expected_revision = Some(RevisionId::new(1));
        assert!(scoped.validate().is_err());
        for proof in [String::new(), "x".repeat(65), "embedded\nnewline".into()] {
            let mut invalid = request();
            if let Command::InspectInferenceEnvironment { owner_proof, .. } = &mut invalid.command {
                *owner_proof = proof;
            }
            assert!(invalid.validate().is_err());
        }
        let mut invalid = request();
        if let Command::InspectInferenceEnvironment { profile, .. } = &mut invalid.command {
            profile.cpu_threads = 0;
        }
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn environment_response_roundtrips_with_names_only_debug() {
        let report = report();
        report.validate().unwrap();
        let response = ResponseBody::InferenceEnvironment(Box::new(report));
        assert_eq!(format!("{response:?}"), "InferenceEnvironment");
        let bytes = serde_json::to_vec(&response).unwrap();
        let decoded: ResponseBody = serde_json::from_slice(&bytes).unwrap();
        let ResponseBody::InferenceEnvironment(decoded) = decoded else {
            panic!("wrong response variant")
        };
        decoded.validate_for_profile(&profile()).unwrap();
    }
}
