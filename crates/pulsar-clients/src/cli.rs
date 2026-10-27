//! Public command grammar. Legacy spelling is preserved even where an engine
//! capability is not yet available; unsupported requests fail, never disappear.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub(crate) fn parse_fallback_selection_path(value: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let path = PathBuf::from(value);
    let normalized: PathBuf = path.components().collect();
    if !path.is_absolute()
        || value.len() > 4096
        || value.contains('\0')
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || path.as_os_str() != normalized.as_os_str()
    {
        return Err(
            "fallback selection requires a bounded normalized absolute path without traversal"
                .into(),
        );
    }
    Ok(path)
}

fn parse_detector_score_threshold(value: &str) -> Result<f32, String> {
    let score = value
        .parse::<f32>()
        .map_err(|_| "detector score must be a number in 0..=1".to_string())?;
    if !score.is_finite() || !(0.0..=1.0).contains(&score) {
        return Err("detector score must be finite and in 0..=1".into());
    }
    Ok(score)
}

#[derive(Debug, Parser)]
#[command(
    name = "pulsar",
    version = "0.8.0",
    about = "Pulsar: local motion workstation"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Explicit options shared by folder generation; model bytes stay Engine-owned.
#[derive(Clone, Debug, serde::Serialize, clap::Args)]
#[command(group(clap::ArgGroup::new("library_model_source").args(["model","model_pack"]).multiple(false)))]
pub struct LibraryGenerationOptions {
    /// Explicit six-axis synthesized audio recipe; requires --multi-axis.
    #[arg(long, requires="multi_axis", conflicts_with_all=["model","model_pack","model_class_count","inference_profile","conf","width","height","video_view"])]
    pub audio_bundle_recipe: Option<PathBuf>,
    #[arg(long, conflicts_with = "model_pack")]
    pub model: Option<PathBuf>,
    /// Exact currently owner-selected installed pack; no automatic selection.
    #[arg(long, conflicts_with_all=["model","model_class_count","inference_profile"])]
    pub model_pack: Option<String>,
    #[arg(long, requires = "model", conflicts_with = "inference_profile")]
    pub model_class_count: Option<u32>,
    #[arg(long, requires = "model", conflicts_with = "model_class_count")]
    pub inference_profile: Option<PathBuf>,
    #[arg(long, requires="library_model_source", value_parser=parse_detector_score_threshold)]
    pub conf: Option<f32>,
    #[arg(long)]
    pub width: Option<u32>,
    #[arg(long)]
    pub height: Option<u32>,
    #[arg(long)]
    pub video_view: Option<PathBuf>,
    #[arg(long, default_value="default", value_parser=["default","fast","maximum","quality"])]
    pub preset: String,
    /// Analyze every Nth original frame without retiming source PTS.
    #[arg(long, default_value_t=1,value_parser=clap::value_parser!(u32).range(1..))]
    pub source_sample_stride: u32,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Inspect local attempt declarations and observed completion without retrying.
    JobAttempts {
        #[arg(long)]
        project: String,
        #[arg(long)]
        job: String,
        #[arg(long)]
        after_attempt: Option<String>,
        #[arg(long,default_value_t=8,value_parser=clap::value_parser!(u16).range(1..=16))]
        limit: u16,
    },
    /// Create a training declaration set from one checked source/script/view pair.
    ReferenceSetCreate {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        video: PathBuf,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        view: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        group: String,
        #[arg(long)]
        authorization: String,
        /// Script milliseconds at elapsed source zero; never inferred.
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        reference_offset_ms: i64,
        #[arg(long)]
        destination: PathBuf,
    },
    /// Recheck an exported reference set and its original files without training.
    ReferenceSetInspect {
        directory: PathBuf,
    },
    /// Discover verified packs in this installed runtime; no Engine or model selection.
    BundledModels,
    /// Discover verified generation packs in an explicitly selected retained runtime.
    RuntimeModels {
        cache_root: PathBuf,
        manifest_sha256: String,
    },
    /// Verify and launch an explicitly selected retained runtime without its supplier bundle.
    RuntimeCacheLaunch {
        cache_root: PathBuf,
        manifest_sha256: String,
        #[arg(last = true, allow_hyphen_values = true)]
        arguments: Vec<std::ffi::OsString>,
    },
    /// Verify a retained runtime without launching it or changing the cache.
    RuntimeCacheInspect {
        cache_root: PathBuf,
        manifest_sha256: String,
    },
    /// Verify persistent offline assets, install on first use and launch from cache.
    RuntimeLaunch {
        source: PathBuf,
        cache_root: PathBuf,
        #[arg(last = true, allow_hyphen_values = true)]
        arguments: Vec<std::ffi::OsString>,
    },
    /// Verify and install a new offline runtime candidate without starting Engine.
    RuntimeInstall {
        source: PathBuf,
        destination: PathBuf,
    },
    /// Publish a complete new candidate from changed assets plus a pinned retained base.
    RuntimeUpdate {
        source: PathBuf,
        base: PathBuf,
        destination: PathBuf,
        #[arg(long)]
        expected_base_manifest_sha256: String,
    },
    /// Create a changed-payload archive bound to an exact retained base.
    RuntimeUpdatePack {
        source: PathBuf,
        base: PathBuf,
        destination: PathBuf,
        #[arg(long)]
        expected_base_manifest_sha256: String,
    },
    /// Create a new offline single-file runtime archive; no Engine or inference.
    RuntimePack {
        source: PathBuf,
        destination: PathBuf,
    },
    /// Inspect a recognized archive construction stage without deleting anything.
    RuntimeArchiveStageInspect {
        stage: PathBuf,
    },
    /// Discard an inactive archive construction stage using its exact inspection hash.
    RuntimeArchiveStageDiscard {
        stage: PathBuf,
        #[arg(long)]
        expected_stage_sha256: String,
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        approve_discard: bool,
    },
    /// Inspect an installation stage; no Engine or deletion.
    RuntimeStageInspect {
        stage: PathBuf,
    },
    /// Explicitly discard a recognized inactive stage using its inspection identity.
    RuntimeStageDiscard {
        stage: PathBuf,
        #[arg(long)]
        expected_manifest_sha256: String,
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        approve_discard: bool,
    },
    /// Configure measured local assistant assets; no Engine, inference or project effect.
    AssistantSetup {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        runtime: PathBuf,
        #[arg(long)]
        model: PathBuf,
        /// Measured local vision projector; enables a vision-only profile.
        #[arg(long)]
        projector: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        cpu_threads: u16,
        #[arg(long, default_value_t = 120000)]
        wall_time_ms: u64,
        #[arg(long, default_value_t = 2147483648u64)]
        worker_memory_bytes: u64,
        /// Use legacy absolute asset paths instead of a portable policy-folder root.
        #[arg(long)]
        absolute_assets: bool,
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        approve_host_inference: bool,
    },
    /// Save an explicitly approved local assistant choice for subsequent startup.
    AssistantSelect {
        #[arg(long)]
        policy: PathBuf,
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        approve_host_inference: bool,
    },
    /// Disable the saved assistant choice for subsequent startup.
    AssistantDisable {
        #[arg(long)]
        expected_policy_sha256: String,
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        approve_disable: bool,
    },
    /// Offline learned proposals; inference approval never approves project effects.
    Assistant {
        #[command(subcommand)]
        command: AssistantCommands,
    },
    /// Prepare and retain verified CUDA image bytes with an independent bounded lease.
    NativePreparation {
        #[command(subcommand)]
        command: NativePreparationCommands,
    },
    /// Run an explicitly approved WASM values editor as an uncommitted proposal.
    WasmEdit {
        #[command(subcommand)]
        command: WasmEditCommands,
    },
    /// Inspect or manage an ephemeral capture lease; no acquisition or playback.
    LiveCapture {
        #[command(subcommand)]
        command: LiveCaptureCommands,
    },
    /// Local browser preview, with an optional explicitly enabled Handy control mode.
    BrowserLive {
        project: String,
        revision: u64,
        /// Checked LiveCaptureOptions JSON with an explicit inference profile.
        options: PathBuf,
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long, default_value_t = 300)]
        duration_seconds: u16,
        /// Cap each newly prepared media lease; does not change the server lifetime.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=600_000))]
        media_lease_ms: Option<u32>,
        /// Exact existing-play selection and liveness policy JSON; contains no keys.
        #[arg(long, requires = "allow_handy_player_monitoring")]
        handy_player_guard: Option<PathBuf>,
        /// Allow this browser player to retire the selected existing Handy playback.
        #[arg(long, requires = "handy_player_guard")]
        allow_handy_player_monitoring: bool,
        /// Prepared Active controller and browser/source dossier selection; contains no keys.
        #[arg(
            long,
            requires = "allow_handy_browser_control",
            conflicts_with = "handy_player_guard"
        )]
        handy_browser_play: Option<PathBuf>,
        /// Enable explicit browser Start controls; video Play alone never actuates.
        #[arg(long, requires = "handy_browser_play")]
        allow_handy_browser_control: bool,
        /// Native HSP device/session and accepted dossier selection JSON; contains no keys.
        #[arg(
            long,
            value_name = "CONFIG",
            requires = "allow_handy_hsp_control",
            conflicts_with_all = ["handy_player_guard", "handy_browser_play"]
        )]
        handy_hsp: Option<PathBuf>,
        /// Enable explicit HSP controls using the selected native configuration.
        #[arg(long, requires = "handy_hsp")]
        allow_handy_hsp_control: bool,
    },
    /// Save inert Handy credential references; no keys are read and no device is contacted.
    HandyRegister {
        device: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        api_key_env: String,
        #[arg(long)]
        connection_key_env: String,
        #[arg(long, default_value_t = 0)]
        expected_registration_revision: u64,
        #[arg(long)]
        disabled: bool,
        #[arg(long)]
        request_id: String,
    },
    /// Resolve or withdraw one exact original registration intent; never undo an applied change.
    HandyResolveRegistration {
        #[arg(long)]
        original_request_id: String,
        /// Captured HandyRegistrationIntent JSON; names only, never credential values.
        #[arg(long)]
        intent_json: String,
        #[arg(long)]
        request_id: String,
    },
    /// List current owner registrations; these do not qualify or arm devices.
    HandyRegistrations,
    /// Prepare exact neutral Stroke CSV samples locally; no hosting, device contact or qualification.
    HandyPrepareStroke {
        project: String,
        path: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: String,
    },
    /// Upload exactly one prepared script to Handy temporary PUBLIC hosting; no device setup.
    HandyHostStroke {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        preparation_request_id: String,
        #[arg(long)]
        script_sha256: String,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_temporary_public_hosting: bool,
    },
    /// Read disclosure history and current native link availability without networking.
    HandyHostingStatus {
        project: String,
        upload_request_id: String,
    },
    /// Forget a native hosting link; this neither deletes remote data nor cancels an in-flight upload.
    HandyForgetHosting {
        project: String,
        upload_request_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Explicitly set up one qualified hosted script; never arm, play or retry an uncertain effect.
    HandySetup {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        hosting_request_id: String,
        #[arg(long)]
        qualification_id: String,
        #[arg(long)]
        qualification_revision: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_setup: bool,
    },
    /// Cancel only an unclaimed setup preparation; never stop a device or retract an attempt.
    HandyCancelSetup {
        project: String,
        #[arg(long)]
        setup_request_id: String,
        #[arg(long)]
        record_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Resolve original setup history for this client; never retry or contact hardware.
    HandySetupStatus {
        project: String,
        #[arg(long)]
        setup_request_id: String,
    },
    /// Start one exact prepared Handy playback with explicit effect consent.
    HandyPlay {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        clock_request_id: String,
        #[arg(long)]
        start_time_ms: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_play: bool,
    },
    /// Observe qualified reported playtime for the exact original play; never rearm or resend it.
    HandyRefreshTiming {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        play_request_id: String,
        #[arg(long)]
        play_record_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Read the original timing observation; history cannot reconstruct native authority.
    HandyRefreshTimingStatus {
        project: String,
        #[arg(long)]
        refresh_request_id: String,
    },
    /// Inspect an original browser stop-guard binding without restoring native authority.
    HandyPlayerGuardStatus {
        project: String,
        #[arg(long)]
        binding_request_id: String,
    },
    /// Capture a bounded reconciliation plan for the exact registered device selection.
    HandyReconcilePlan {
        project: String,
        #[arg(long)]
        device_id: String,
        #[arg(long)]
        registration_revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        qualification_id: String,
        #[arg(long)]
        qualification_revision: u64,
    },
    /// Inspect and resolve the exact captured plan with both explicit consents.
    HandyReconcile {
        project: String,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
        #[arg(long, required = true)]
        allow_resolution: bool,
    },
    /// Read original reconciliation history; no playback authority is restored.
    HandyReconcileStatus {
        project: String,
        #[arg(long)]
        reconciliation_request_id: String,
    },
    /// Request cancellation; a durable resolution that already won remains completed.
    HandyReconcileCancel {
        project: String,
        #[arg(long)]
        reconciliation_request_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Bind one accepted CPU capture to its original qualified HSP preparation.
    HandyHspLiveBind {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        preparation_request_id: String,
        #[arg(long)]
        capture_epoch: String,
        #[arg(long)]
        inference_attempt_id: String,
        #[arg(long)]
        browser_timing_binding_id: String,
        #[arg(long)]
        source_qualification_id: String,
        #[arg(long)]
        source_qualification_revision: u64,
        /// Explicitly select the exact reviewed finite-drain policy.
        #[arg(long)]
        finite_drain_policy_sha256: Option<String>,
        #[arg(long)]
        request_id: String,
        /// Confirm the current browser/player/display setup matches the selected source dossier's environment evidence.
        #[arg(long, required = true)]
        acknowledge_current_environment: bool,
        #[arg(long, required = true)]
        allow_online_control: bool,
    },
    /// Explicitly Play an original live HSP stream; never replace its reference.
    HandyHspLivePlay {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        stream_request_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Retire the original live HSP stream without renewing its source or clocks.
    HandyHspLiveStop {
        project: String,
        #[arg(long)]
        stream_request_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Read original live HSP observations without starting or renewing work.
    HandyHspLiveStatus {
        project: String,
        #[arg(long)]
        stream_request_id: String,
    },
    /// Admit bounded ongoing timing observations for an existing live HSP stream.
    HandyHspContinuityEnable {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        stream_request_id: String,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        duration_ms: u32,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_observations: bool,
    },
    /// Read the original continuity admission without starting or renewing work.
    HandyHspContinuityStatus {
        project: String,
        #[arg(long)]
        original_request_id: String,
    },
    /// Prepare one HSP synchronization with an explicit original intent and timeout.
    HandyHspPrepare {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        qualification_id: String,
        #[arg(long)]
        qualification_revision: u64,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..=60_000))]
        timeout_ms: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_synchronization: bool,
    },
    /// Read original HSP preparation; status neither retries nor restores readiness.
    HandyHspPreparationStatus {
        project: String,
        #[arg(long)]
        preparation_request_id: String,
    },
    /// Cancel the original HSP preparation without replacing its timeout or stream.
    HandyHspPreparationCancel {
        project: String,
        #[arg(long)]
        preparation_request_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Read an original HSP stream observation without renewing or controlling it.
    HandyHspStatus {
        project: String,
        #[arg(long)]
        stream_request_id: String,
    },
    /// Read bounded stop observations for an original controller; never physical stopping proof.
    HandyStopStatus {
        project: String,
        #[arg(long)]
        acquisition_request_id: String,
    },
    /// Read original play history; this does not establish current device or controller state.
    HandyPlayStatus {
        project: String,
        #[arg(long)]
        play_request_id: String,
    },
    /// Cancel an exact unclaimed play preparation; this does not stop claimed playback.
    HandyPlayCancel {
        project: String,
        #[arg(long)]
        play_request_id: String,
        #[arg(long)]
        record_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Acquire combined native device/server timing for a fresh controller; no playback starts.
    HandyPlaybackClock {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        sync_request_id: String,
        #[arg(long)]
        sync_record_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_timing: bool,
    },
    /// Synchronize the device clock once, retiring the controller; exact retry returns history.
    HandyClockSync {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_synchronization: bool,
    },
    /// Read original clock synchronization history without contacting hardware.
    HandyClockSyncStatus {
        project: String,
        #[arg(long)]
        sync_request_id: String,
    },
    /// Cancel only an exact unclaimed synchronization preparation.
    HandyClockSyncCancel {
        project: String,
        #[arg(long)]
        sync_request_id: String,
        #[arg(long)]
        record_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Read device clock and user offsets for an exact controller; never initiate synchronization.
    HandyInspectDeviceTiming {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Sample the public server clock for an exact controller; original retry never resamples.
    HandySampleServerClock {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_sampling: bool,
    },
    /// Acquire a renewable local controller lease and arm without starting playback.
    HandyControllerAcquire {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        preparation_request_id: String,
        #[arg(long)]
        preparation_id: String,
        #[arg(long)]
        lease_ms: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_arm: bool,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Renew the current controller generation before expiry; replay does not extend it.
    HandyControllerRenew {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        request_id: String,
    },
    /// Release exact controller ownership and revoke local arming.
    HandyControllerRelease {
        project: String,
        #[arg(long)]
        acquisition_request_id: String,
        #[arg(long)]
        controller_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Read original controller status without restoring native authority.
    HandyControllerStatus {
        project: String,
        #[arg(long)]
        acquisition_request_id: String,
    },
    /// Retain a nonplaying native preparation; no controller, play or stop is created.
    HandyPreparePlayback {
        project: String,
        #[arg(long)]
        setup_request_id: String,
        #[arg(long)]
        record_id: String,
        #[arg(long)]
        qualification_id: String,
        #[arg(long)]
        qualification_revision: u64,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        hosting_request_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Resolve a preparation by its original request; missing native state is retired.
    HandyPlaybackStatus {
        project: String,
        #[arg(long)]
        preparation_request_id: String,
    },
    /// Release a nonplaying native preparation by exact identity.
    HandyRetirePlayback {
        project: String,
        #[arg(long)]
        preparation_request_id: String,
        #[arg(long)]
        preparation_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Correlate the current stopped stream with one acknowledged setup using GET only.
    HandyInspectProgram {
        project: String,
        #[arg(long)]
        setup_request_id: String,
        #[arg(long)]
        record_id: String,
        #[arg(long)]
        qualification_id: String,
        #[arg(long)]
        qualification_revision: u64,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        hosting_request_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Recheck hosted bytes and current device observations using GET only; never setup or play.
    HandyInspectSetup {
        project: String,
        #[arg(long, requires = "qualification_revision")]
        qualification_id: Option<String>,
        #[arg(long, requires = "qualification_id")]
        qualification_revision: Option<u64>,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        hosting_request_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Read the actual loaded control-image and platform binding for evidence authoring.
    HandyQualificationContext {
        #[arg(long, default_value = "hssp", value_parser = ["hssp", "hsp"])]
        transport: String,
    },
    /// Import a separate HSP source/presentation evidence dossier without accepting it.
    HandyHspSourceQualificationImport {
        project: String,
        manifest: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        manifest_sha256: String,
        #[arg(long)]
        request_id: String,
    },
    /// Inspect original HSP source/presentation dossier evidence and review state.
    HandyHspSourceQualificationStatus {
        project: String,
        dossier: String,
    },
    /// Explicitly accept measured physical HSP source/presentation evidence; never start playback.
    HandyHspSourceQualificationAccept {
        project: String,
        dossier: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        record_revision: u64,
        #[arg(long)]
        operator: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        acknowledge_measured_evidence: bool,
    },
    /// Withdraw HSP source/presentation acceptance while preserving its original evidence.
    HandyHspSourceQualificationWithdraw {
        project: String,
        dossier: String,
        #[arg(long)]
        record_revision: u64,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        request_id: String,
    },
    /// Import a separate browser/source evidence dossier without accepting it.
    HandyBrowserStartQualificationImport {
        project: String,
        manifest: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        manifest_sha256: String,
        #[arg(long)]
        request_id: String,
    },
    /// Inspect original browser/source dossier evidence and review state.
    HandyBrowserStartQualificationStatus {
        project: String,
        dossier: String,
    },
    /// Explicitly accept measured physical browser/source evidence; never start playback.
    HandyBrowserStartQualificationAccept {
        project: String,
        dossier: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        record_revision: u64,
        #[arg(long)]
        operator: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        acknowledge_measured_evidence: bool,
    },
    /// Withdraw browser/source acceptance while preserving its original evidence.
    HandyBrowserStartQualificationWithdraw {
        project: String,
        dossier: String,
        #[arg(long)]
        record_revision: u64,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        request_id: String,
    },
    /// Import bounded immutable evidence; never accept it or contact hardware.
    HandyQualificationImport {
        manifest: PathBuf,
        #[arg(long)]
        manifest_sha256: String,
        #[arg(long)]
        request_id: String,
    },
    /// Verify retained evidence and read current review state.
    HandyQualificationStatus {
        dossier: String,
    },
    /// Explicit operator acceptance of reviewed physical evidence; no arm, setup or play.
    HandyQualificationAccept {
        dossier: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        note: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        acknowledge_physical_evidence: bool,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Withdraw local qualification acceptance; this is not a device stop command.
    HandyQualificationWithdraw {
        dossier: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        note: String,
        #[arg(long)]
        request_id: String,
    },
    /// List boot-local connection observations; no networking or playback.
    HandySessions,
    /// Open a credential-route session by read-only online inspection; never arm.
    HandyOpenSession {
        device: String,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        expected_registration_revision: u64,
        #[arg(long)]
        binding_sha256: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Retire a local observation session. This does not send a physical stop.
    HandyCloseSession {
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        device_session: String,
        #[arg(long)]
        request_id: String,
    },
    /// Disable registration lookup, preserving history; this is not a device stop command.
    HandyDisable {
        device: String,
        #[arg(long)]
        expected_registration_revision: u64,
        #[arg(long)]
        request_id: String,
    },
    /// Explicit read-only inspection of an exact current registered binding.
    HandyInspectRegistered {
        device: String,
        #[arg(long, requires = "binding_sha256")]
        expected_registration_revision: Option<u64>,
        #[arg(long, requires = "expected_registration_revision")]
        binding_sha256: Option<String>,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Query Handy REST v3 connectivity/identity; no scripts or motion are sent.
    HandyInspect {
        #[arg(long)]
        api_key_env: String,
        #[arg(long)]
        connection_key_env: String,
        #[arg(long, required = true)]
        allow_online_inspection: bool,
    },
    /// Export a timing-preserving device-space proposal; never arm or transmit.
    ExportDeviceMotion {
        project: String,
        path: PathBuf,
        #[arg(long)]
        profile: PathBuf,
    },
    /// Open Pulsar Desktop. GUI and CLI use the same engine interface.
    Gui,
    /// Validate a portable package and clone it into a NEW project; never overwrite.
    PackageImport {
        source: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long)]
        begin_request_id: Option<String>,
        #[arg(long)]
        seal_request_id: Option<String>,
    },
    /// Inspect clone-import status; a project is usable only after Completed.
    PackageImportStatus {
        operation: String,
    },
    /// Explicit engine cancellation; completed publication is never undone.
    PackageImportCancel {
        operation: String,
    },
    /// Continue a same-epoch import from the exact same package bytes.
    /// Interrupted imports require a deliberate new Start instead.
    PackageImportResume {
        operation: String,
        source: PathBuf,
        #[arg(long)]
        begin_request_id: Option<String>,
        #[arg(long)]
        seal_request_id: Option<String>,
    },

    /// Explicitly permit this local owner session to package private project data.
    AllowPackaging {
        project: String,
        #[arg(long, required = true)]
        acknowledge_unencrypted_private_data: bool,
    },
    /// Export an UNENCRYPTED portable package containing media, prompts and lineage.
    PackageExport {
        project: String,
        destination: PathBuf,
        #[arg(long, required = true)]
        acknowledge_unencrypted_private_data: bool,
        /// Explicit current revision for callers with PackageProject but no Read scope.
        #[arg(long)]
        revision: Option<u64>,
        /// Stable Start identity for recovery after a lost acknowledgement.
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Inspect a retained package operation without cancelling or downloading it.
    PackageStatus {
        project: String,
        operation: String,
    },
    /// Explicitly cancel active packaging; disconnect alone never cancels it.
    PackageCancel {
        project: String,
        operation: String,
    },
    /// Retry download of an existing Ready operation into a NEW destination.
    PackageDownload {
        project: String,
        operation: String,
        destination: PathBuf,
        /// Reuse a Begin identity only after a lost lease acknowledgement, before chunks.
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long, required = true)]
        acknowledge_unencrypted_private_data: bool,
    },
    /// Delete the retained engine package copy, not any client destination file.
    PackageRelease {
        project: String,
        operation: String,
        #[arg(long, required = true)]
        acknowledge_discarding_engine_copy: bool,
    },

    /// Publish neutral playback plus a full private project package to one new directory.
    MotionEvidenceExport {
        project: String,
        destination: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long, default_value = "stroke")]
        axis: String,
        #[arg(long, required = true)]
        acknowledge_unencrypted_private_data: bool,
    },
    /// Resume the exact retained export intent; never blindly retries a begun transfer.
    MotionEvidenceResume {
        staging: PathBuf,
        #[arg(long, required = true)]
        acknowledge_unencrypted_private_data: bool,
    },
    /// Independently check a neutral script against a full native project package.
    /// Reads local files; grants no authority and performs no import or playback.
    MotionEvidenceCheck {
        script: PathBuf,
        package: PathBuf,
        #[arg(long)]
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long, default_value = "stroke")]
        axis: String,
    },
    /// Export exactly one selected neutral axis from a committed project.
    ExportAxis {
        project: String,
        path: PathBuf,
        #[arg(long, default_value = "stroke")]
        axis: String,
    },
    /// Atomically publish selected neutral axes (default: all six) to one NEW directory (Linux/Android).
    ExportBundle {
        project: String,
        destination: PathBuf,
        #[arg(long)]
        revision: u64,
        /// Comma-separated explicit axis selection; omitted retains the six-axis contract.
        #[arg(long, value_delimiter = ',')]
        axes: Vec<String>,
    },
    /// Observe per-axis companion presence; no Engine, generation, or script validation.
    LibraryAxes {
        folder: PathBuf,
        #[arg(long)]
        recursive: bool,
    },
    /// Capture selected missing audio-folder inputs and recipe in an Engine-owned ledger.
    LibraryBatchPrepare {
        folder: PathBuf,
        #[arg(long)]
        recursive: bool,
        #[arg(
            long,
            required_unless_present = "visual_recipe",
            conflicts_with = "visual_recipe"
        )]
        audio_bundle_recipe: Option<PathBuf>,
        /// Checked visual recipe: preset, settings and optional explicit model path.
        #[arg(long, required_unless_present = "audio_bundle_recipe")]
        visual_recipe: Option<PathBuf>,
    },
    /// Schedule retained generation and per-entry publication without replacement work.
    LibraryBatchRun {
        batch: String,
        #[arg(long)]
        publication_state: PathBuf,
        #[arg(long, default_value = "1000")]
        steps: u16,
        #[arg(long, default_value = "25")]
        poll_ms: u16,
    },
    /// Read actual generation/job status and retained publication history without advancement.
    LibraryBatchWorkflowStatus {
        batch: String,
        #[arg(long)]
        publication_state: PathBuf,
    },
    /// Reattach an inspected original stage after an interrupted preparation acknowledgement.
    LibraryBatchRecoverPreparation {
        batch: String,
        #[arg(long)]
        publication_state: PathBuf,
        /// Zero-based retained entry index.
        #[arg(long)]
        index: u16,
        #[arg(long)]
        staging: PathBuf,
        #[arg(long)]
        preparation_receipt: PathBuf,
    },
    /// Retain exact bundle stage/request identities before any Engine export dispatch.
    LibraryBatchPublicationPrepare {
        batch: String,
        #[arg(long)]
        publication_state: PathBuf,
        #[arg(long, default_value = "1")]
        steps: u16,
    },
    /// Retain publication handoffs and explicitly publish up to 100 ready batch entries.
    LibraryBatchPublish {
        batch: String,
        /// Existing private (0700) directory for this batch's client journal.
        #[arg(long)]
        publication_state: PathBuf,
        #[arg(long, default_value = "1")]
        steps: u16,
    },
    /// Retain an inspected pending journal under a retired name after current acknowledgements cover it.
    LibraryBatchRetirePending {
        #[arg(long)]
        publication_state: PathBuf,
        #[arg(long)]
        pending: PathBuf,
        #[arg(long)]
        sha256: String,
    },
    /// Read authenticated publication acknowledgement history; no filesystem adoption.
    LibraryBatchPublicationStatus {
        #[arg(long)]
        publication_state: PathBuf,
    },
    /// Stop retained batch generation; retry this exact cancellation identity after interruption.
    LibraryBatchCancel {
        batch: String,
        #[arg(long)]
        request_id: String,
    },
    /// Release eligible batch metadata; preserves all files and projects.
    LibraryBatchRelease {
        batch: String,
        /// Retain this identity for exact acknowledgement recovery after a lost reply.
        #[arg(long)]
        request_id: String,
    },
    /// Continue stopped, unaccepted entries under a new recipe; no generation is dispatched.
    LibraryBatchMigrate {
        batch: String,
        /// Exact previously inspected library-batch-status JSON (plain or response-wrapped).
        #[arg(long)]
        observed_report: PathBuf,
        #[arg(long)]
        recipe: PathBuf,
        /// Retain this identity to recover the original successor creation.
        #[arg(long)]
        request_id: String,
    },
    /// List this session's retained local batches, including a lost preparation reply.
    LibraryBatchList,
    /// Read retained local batch phases; never dispatches generation.
    LibraryBatchStatus {
        batch: String,
    },
    /// Explicitly advance at most 1..=1000 retained generation phases; no publication.
    LibraryBatchAdvance {
        batch: String,
        #[arg(long, default_value = "1")]
        steps: u16,
    },
    /// Inspect retained local bundle files against the exact master; no export or publication.
    ExportBundleInspect {
        staging: PathBuf,
    },
    /// Resume exact staged export identities; unknown Engine outcomes remain refused.
    ExportBundleResume {
        staging: PathBuf,
    },
    /// Stage one through six explicit axis patterns; review and commit separately.
    PatternCandidate {
        project: String,
        file: PathBuf,
        #[arg(long)]
        revision: u64,
    },
    /// Synthesize strict key=value pattern syntax; not free-form AI.
    Synthesize {
        prompt: String,
        #[arg(long)]
        image: Option<PathBuf>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Map full-band energy or amplitude-onset pulses, not musical beat tracking.
    AudioMap {
        video: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "envelope")]
        mode: String,
        #[arg(long, default_value = "stroke")]
        axis: String,
        #[arg(long, default_value_t = 0.5)]
        amplitude: f64,
        #[arg(long, default_value_t = 0.25)]
        offset: f64,
    },
    /// Propose explicit linear gap bridges and nearest-value edge holds; no commit.
    GapCandidate {
        project: String,
        gaps: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long, default_value = "stroke")]
        axis: String,
        #[arg(long,required=true,action=clap::ArgAction::SetTrue)]
        linear_edge_hold: bool,
    },
    /// Stage values-only manual edits; no commit or evidence promotion.
    EditValues {
        project: String,
        file: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long, default_value = "Manual values edit")]
        label: String,
    },
    /// Explicitly commit a reviewed candidate at the named revision.
    CommitCandidate {
        project: String,
        candidate: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Read or explicitly replace the protection list; writes require manage authority.
    Protection {
        #[command(subcommand)]
        command: ProtectionCommands,
    },
    /// Generate into an existing project and apply only unlocked candidate axes.
    Regenerate {
        project: String,
        source_version: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        settings: Option<PathBuf>,
        #[arg(long)]
        model: Option<PathBuf>,
        #[arg(long, default_value = "fast")]
        preset: String,
    },
    /// Import a funscript as an uncommitted candidate.
    ImportScript {
        script: PathBuf,
        #[arg(long)]
        project: Option<String>,
    },
    /// Read localized evidence/protection diagnostics.
    Review {
        project: String,
        #[arg(long)]
        candidate: Option<String>,
        /// Optional average segment slope limit in percent/second.
        #[arg(long)]
        max_speed: Option<f64>,
    },
    /// Read retained warning identities and revision-bound decisions.
    ReviewStatus {
        project: String,
    },
    /// Declare a warning resolved or reopen it without removing evidence.
    ReviewDisposition {
        project: String,
        review_id: String,
        #[arg(long, requires = "end_ns")]
        start_ns: Option<i64>,
        #[arg(long, requires = "start_ns")]
        end_ns: Option<i64>,
        #[arg(long,value_parser=["resolved","reopened"])]
        disposition: String,
        #[arg(long)]
        note: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Explicitly merge selected candidate axes and project-time range.
    MergeCandidate {
        project: String,
        candidate: String,
        #[arg(long, default_value = "stroke")]
        axes: String,
        #[arg(long)]
        start: Option<f64>,
        #[arg(long)]
        end: Option<f64>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Repair neutral script values through shared Engine history; not device qualification.
    Fix {
        script: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 450.0)]
        max_speed: f64,
        #[arg(long, default_value_t = 20)]
        jitter_window: i64,
        #[arg(long, default_value_t = 2)]
        jitter_threshold: i32,
    },
    /// Generate, review engine completion, commit, and export a stroke program.
    #[command(group(clap::ArgGroup::new("annotated_model").args(["inference_profile","model_pack"]).multiple(false)))]
    Generate {
        video: PathBuf,
        /// Explicit faithful/creative policy and RMS mapping JSON for container audio.
        #[arg(long)]
        mixed_audio: Option<PathBuf>,
        /// Separately captured audio. Requires --mixed-audio and --audio-alignment.
        #[arg(long, requires_all = ["mixed_audio", "audio_alignment"])]
        audio_source: Option<PathBuf>,
        /// JSON unit-rate correspondence with explicit video_anchor/audio_anchor.
        #[arg(long, requires = "audio_source")]
        audio_alignment: Option<PathBuf>,
        /// Exact source-frame ordinal and analysis-view normalized target point JSON.
        #[arg(long, conflicts_with = "raw_frame_pair")]
        target_seed: Option<PathBuf>,
        /// Source SHA-256 and annotated target-view JSON for an explicit temporal profile.
        #[arg(long, requires = "annotated_model", conflicts_with_all=["video_view","target_seed","raw_frame_pair"])]
        annotated_view: Option<PathBuf>,
        /// Declaration for two immutable packed 64x64 RGB frames (previous, current).
        #[arg(long)]
        raw_frame_pair: Option<PathBuf>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        model: Option<PathBuf>,
        /// Use the exact currently owner-selected installed pack.
        #[arg(long, conflicts_with_all=["model","inference_profile","model_class_count"])]
        model_pack: Option<String>,
        /// Explicit class count for the RGB/NCHW YOLO channel-major model ABI.
        #[arg(long)]
        model_class_count: Option<u32>,
        /// Exact model/runtime/preprocessing/provider JSON. Engine validates the pins.
        #[arg(long, requires = "model", conflicts_with = "model_class_count")]
        inference_profile: Option<PathBuf>,
        /// Explicit ordered equivalence selection JSON; requires a pinned model/profile or selected pack.
        #[arg(long, requires = "annotated_model", conflicts_with = "raw_frame_pair", value_parser = parse_fallback_selection_path)]
        fallback_selection: Option<PathBuf>,
        /// Minimum generation detector score in 0..=1 (default: 0.25; uncalibrated).
        #[arg(long, value_parser = parse_detector_score_threshold)]
        conf: Option<f32>,
        #[arg(long, default_value_t = 30.0)]
        fps: f64,
        /// Analyze every Nth original source frame; timestamps remain source PTS.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        source_sample_stride: u32,
        /// Maximum decoded width (default: declared model width, otherwise 320).
        #[arg(long)]
        width: Option<u32>,
        /// Maximum decoded height (default: declared model height, otherwise 180).
        #[arg(long)]
        height: Option<u32>,
        #[arg(long, default_value_t = 2.0)]
        detrend_window: f64,
        #[arg(long, default_value_t = 3.0)]
        norm_window: f64,
        #[arg(long, default_value_t = 2000)]
        batch_size: usize,
        /// JSON declaring stereo layout, selected eye and spherical viewport.
        #[arg(long)]
        video_view: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        vr: bool,
        #[arg(long, default_value_t = false)]
        pov: bool,
        #[arg(long, default_value_t = false)]
        no_balance_global: bool,
        #[arg(long, default_value_t = false)]
        no_keyframe_reduction: bool,
        #[arg(long, default_value_t = 30.0)]
        cut_diff: f32,
        #[arg(long, default_value_t = 8.0)]
        cut_flow: f32,
        #[arg(long, default_value_t = false)]
        multi_axis: bool,
        /// default, fast, maximum or quality; Maximum/quality require a declared detector.
        #[arg(long, default_value = "default")]
        profile: String,
        #[arg(long)]
        cuts: Option<PathBuf>,
    },
    /// Run the engine's script diagnostics.
    Doctor {
        script: PathBuf,
        #[arg(long, default_value_t = 450.0)]
        max_speed: f64,
    },
    /// Inspect container metadata from a captured media source.
    Info {
        media: PathBuf,
    },
    #[command(alias = "audit")]
    Scan {
        folder: PathBuf,
        #[arg(short, long, default_value_t = true)]
        recursive: bool,
        #[arg(long, default_value_t = false)]
        multi_axis: bool,
        #[arg(long, default_value_t = false)]
        missing_only: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
        #[arg(long, default_value_t = false)]
        csv: bool,
        #[arg(long, default_value_t = false)]
        generate: bool,
        #[command(flatten)]
        generation: LibraryGenerationOptions,
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },
    Batch {
        folder: PathBuf,
        #[arg(short, long, default_value_t = true)]
        recursive: bool,
        #[command(flatten)]
        generation: LibraryGenerationOptions,
        #[arg(long, default_value_t = false)]
        multi_axis: bool,
        #[arg(long, default_value_t = false)]
        overwrite: bool,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    AudioSynth {
        video: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "beats")]
        mode: String,
        #[arg(long, default_value = "bass")]
        band: String,
        #[arg(long, default_value = "stroke")]
        axis: String,
        #[arg(long, default_value_t = 0.35)]
        threshold: f64,
        /// Export six explicitly synthesized neutral axes into a NEW directory.
        #[arg(long, default_value_t = false)]
        bundle: bool,
        /// Checked JSON with exactly one mapping/mode/band/threshold per neutral axis.
        #[arg(long, requires = "bundle")]
        bundle_recipe: Option<PathBuf>,
        /// Stage and publish the six files in one no-replace directory rename (Linux/Android).
        #[arg(long, requires = "bundle")]
        atomic_bundle: bool,
    },
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },
    Bench,
    Completions {
        shell: clap_complete::Shell,
    },
    Play {
        video: PathBuf,
        #[arg(short, long)]
        script: Option<PathBuf>,
        #[arg(long)]
        serial: Option<String>,
        #[arg(long, default_value_t = 115200)]
        baud: u32,
        #[arg(long)]
        handy: Option<String>,
        #[arg(long)]
        buttplug: Option<String>,
    },
    Stash {
        #[command(subcommand)]
        command: StashCommands,
    },
    /// Print the engine's actual capabilities and qualification status.
    Capabilities,
    /// Create or open a durable engine-owned project.
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ModelCommands {
    /// Inspect correlated evidence and record explicit owner decisions.
    /// Import is Pending; synthetic evidence cannot be accepted. Reports alone
    /// grant no execution authority. Opt-in fallback requires fresh Engine admission.
    Equivalence {
        #[command(subcommand)]
        command: InferenceEquivalenceCommands,
    },
    /// Authenticate captured offline metadata relative to an explicitly approved bootstrap.
    CatalogRefresh {
        source: PathBuf,
        bootstrap: PathBuf,
        bootstrap_sha256: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        approve_bootstrap: bool,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Inspect the retained historical catalog receipt; does not renew publisher trust.
    CatalogStatus {
        bootstrap_sha256: String,
    },
    /// Verify installed bytes against an exact unexpired accepted offline catalog.
    PackCatalogCheck {
        bootstrap_sha256: String,
        manifest_sha256: String,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Record explicit approval of an installed pack for future offline generation.
    /// Approve a pack under an exact accepted offline catalog fence.
    PackCatalogSelect {
        bootstrap_sha256: String,
        manifest_sha256: String,
        #[arg(long)]
        expected_catalog_revision: u64,
        #[arg(long)]
        expected_selection_revision: u64,
        #[arg(long, required = true)]
        approve_unqualified_native_execution: bool,
        #[arg(long)]
        request_id: Option<String>,
    },
    PackSelect {
        manifest_sha256: String,
        #[arg(long)]
        expected_selection_revision: u64,
        #[arg(long, required = true)]
        approve_unqualified_native_execution: bool,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Remove the future-generation selection; accepted jobs retain their pinned inputs.
    PackDeactivate {
        #[arg(long)]
        expected_selection_revision: u64,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Withdraw an exact approval and cancel its unfinished generation jobs.
    PackWithdraw {
        manifest_sha256: String,
        #[arg(long)]
        selection_revision: u64,
        #[arg(long)]
        expected_selection_revision: u64,
        /// Explicitly also cancel older generation with no retained admission classification.
        #[arg(long)]
        include_unclassified_generation: bool,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Read the recorded selection fence and profile; generation rechecks actual bytes.
    PackSelectionStatus,
    /// Install an offline directory pack as inert, hash-verified data.
    PackImport {
        source: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Recheck installed bytes without activating native code.
    PackStatus {
        manifest_sha256: String,
    },
    /// Observe owned staging locks and identities; no cleanup or activation.
    PackStagingStatus,
    /// Discard one recognized unlocked stage; retain this exact request ID for retry.
    PackDiscardStage {
        basename: String,
        manifest_sha256: String,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Print a strict custom profile using this engine's pinned runtime.
    /// This describes bytes and preprocessing; it does not qualify the model.
    Profile {
        model: PathBuf,
        /// Declare fixed frame-pair script-position regression, not detection.
        #[arg(long)]
        temporal_script_position: bool,
        /// Declare source-bound annotated frame-pair preprocessing.
        #[arg(long, conflicts_with = "temporal_script_position")]
        annotated_temporal_position: bool,
        /// Preserve detector aspect ratio with fixed centered linear letterbox.
        #[arg(long, conflicts_with_all = ["temporal_script_position", "annotated_temporal_position"])]
        letterbox: bool,
        /// Explicit byte value for the letterbox border (default: 114).
        #[arg(long, requires = "letterbox")]
        padding_byte: Option<u8>,
        #[arg(long)]
        width: u32,
        #[arg(long)]
        height: u32,
        #[arg(long)]
        class_count: u32,
    },
    List,
    Download {
        #[arg(default_value = "default")]
        model: String,
    },
    Status,
}

#[derive(Debug, Subcommand)]
pub enum InferenceEquivalenceCommands {
    /// Observe current host identities for a declared profile without loading a model or authorizing fallback.
    Environment {
        /// Absolute path to a bounded declared inference profile JSON file.
        #[arg(long)]
        profile: PathBuf,
    },
    /// Import exact manifest bytes as Pending (reimport preserves existing decisions).
    Import {
        path: PathBuf,
        #[arg(long)]
        manifest_sha256: String,
        /// Retain this identity for explicit recovery; never invent a replacement on retry.
        #[arg(long)]
        request_id: String,
    },
    /// Observe historical decisions and pending withdrawal; this is not execution authority.
    Status { dossier_id: String },
    /// Observe a bounded, lexical-ID page without renewing evidence validity.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t=8, value_parser=clap::value_parser!(u16).range(1..=16))]
        limit: u16,
    },
    /// Record supplied owner acceptance of reviewed measured evidence at an exact revision.
    /// Synthetic evidence is refused. Acceptance alone grants no execution authority;
    /// opt-in fallback also requires current scope, runtime and lifetime admission.
    Accept {
        dossier_id: String,
        #[arg(long)]
        expected_record_revision: u64,
        #[arg(long)]
        operator: String,
        #[arg(long, required = true)]
        acknowledge_measured_equivalence: bool,
        #[arg(long)]
        request_id: String,
    },
    /// Withdraw an exact historical revision; retain the original request for recovery.
    Withdraw {
        dossier_id: String,
        #[arg(long)]
        expected_record_revision: u64,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        request_id: String,
    },
    /// List local retained intents without dispatching them. These are not Engine status.
    RecoveryList,
    /// Explicitly retry the exact original intent and request ID with its bound owner capability.
    RecoveryRetry { request_id: String },
    /// Remove only a receipt-acknowledged local intent; does not change Engine decisions.
    RecoveryDismiss { request_id: String },
}

#[derive(Debug, Subcommand)]
pub enum StashCommands {
    /// Read one bounded page of a durable engine batch.
    BatchStatus {
        batch_request_id: String,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Reconcile and continue the same batch; never create replacement phase IDs.
    ResumeBatch {
        batch_request_id: String,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Explicitly replace future generation intent; retain old effects and remain paused.
    MigrateBatch {
        batch_request_id: String,
        #[arg(long)]
        expected_ledger_sha256: String,
        /// Bounded JSON file containing preset, settings, absolute model_path (or null) and optional model_pack choice.
        #[arg(long)]
        generation: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Stop future batch effects; retained publications are not rolled back.
    CancelBatch {
        batch_request_id: String,
        #[arg(long)]
        request_id: Option<String>,
    },

    /// Add one exact tag after verified scan attachment; preserve existing tags.
    AddTag {
        project: String,
        #[arg(long)]
        scan_request_id: String,
        #[arg(long)]
        tag: String,
        #[arg(long)]
        create_tag: bool,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        revision: Option<u64>,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Read-only remote reconciliation of recorded creation/ADD outcomes.
    TagStatus {
        project: String,
        #[arg(long)]
        tag_request_id: String,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
    },
    /// Attach a resolved tag whose ADD has never been dispatched.
    ResumeTag {
        project: String,
        #[arg(long)]
        tag_request_id: String,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        revision: Option<u64>,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Submit a targeted scan for a successful publication. Retain request ID.
    SubmitScan {
        project: String,
        #[arg(long)]
        publication_request_id: String,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        revision: Option<u64>,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Reconcile a recorded scan job and independently verify scene attachment.
    ScanStatus {
        project: String,
        #[arg(long)]
        scan_request_id: String,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
    },
    /// Observe attachment without recovering or resubmitting a lost scan job.
    ReconcileAttachment {
        project: String,
        #[arg(long)]
        scan_request_id: String,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
    },
    /// Generate and publish missing scripts, with optional Stash scanning and tagging.
    #[command(group=clap::ArgGroup::new("stash_annotated_model").args(["inference_profile","model_pack"]).multiple(false))]
    GenerateMissing {
        /// Source-bound placement/interval plans keyed by exact scene/file IDs.
        #[arg(long)]
        source_plans: Option<PathBuf>,
        /// Source-bound target views keyed by exact Stash scene/file IDs.
        #[arg(
            long,
            requires = "stash_annotated_model",
            conflicts_with = "video_view"
        )]
        annotated_views: Option<PathBuf>,
        #[arg(long)]
        scan: bool,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        create_tag: bool,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long)]
        detach: bool,
        #[arg(long)]
        mappings: PathBuf,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        model: Option<PathBuf>,
        /// Exact currently approved offline pack; each scene rechecks selection.
        #[arg(long,conflicts_with_all=["model","model_class_count","inference_profile"])]
        model_pack: Option<String>,
        #[arg(long)]
        model_class_count: Option<u32>,
        #[arg(long, requires = "model")]
        inference_profile: Option<PathBuf>,
        #[arg(long)]
        video_view: Option<PathBuf>,
        /// Minimum generation detector score (default: 0.25; uncalibrated).
        #[arg(long, value_parser = parse_detector_score_threshold)]
        detector_score_threshold: Option<f32>,
        /// Maximum decoded width (default: declared model width, otherwise 320).
        #[arg(long)]
        width: Option<u32>,
        /// Maximum decoded height (default: declared model height, otherwise 180).
        #[arg(long)]
        height: Option<u32>,
        #[arg(long, default_value = "fast")]
        preset: String,
    },
    /// Publish a committed generation candidate for the captured Stash source.
    /// Observe local publication/proof identity without any dispatch.
    PublicationStatus {
        project: String,
        #[arg(long)]
        publication_request_id: String,
    },
    PublishScript {
        /// Bounded JSON namespace reference from archived generation inspection.
        #[arg(long)]
        archived_origin: Option<PathBuf>,
        /// Map placed motion explicitly to original elapsed-source time.
        #[arg(long)]
        source_time: bool,
        project: String,
        scene: String,
        file: String,
        #[arg(long)]
        source_version: String,
        #[arg(long)]
        candidate: String,
        #[arg(long)]
        job: String,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        revision: Option<u64>,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Re-query the selected scene and capture its mapped source in a project.
    CaptureSource {
        project: String,
        scene: String,
        file: String,
        /// Rebind an existing project source only if freshly captured bytes match.
        #[arg(long)]
        expected_source_version: Option<String>,
        #[arg(long)]
        mappings: PathBuf,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        revision: Option<u64>,
    },
    /// Observe mapped local files without generation or Stash mutations.
    PlanMissing {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key_env: Option<String>,
        /// JSON array of server_root and local_root pairs.
        #[arg(long)]
        mappings: PathBuf,
    },
    #[command(alias = "query")]
    ListMissing {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key: Option<String>,
        /// Environment variable name, available to the engine via secrets run.
        #[arg(long)]
        api_key_env: Option<String>,
    },
    AutoGenerate {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value = "http://localhost:9999/graphql")]
        url: String,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        model: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        multi_axis: bool,
        #[arg(long)]
        tag: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum WasmEditCommands {
    Info {
        project: String,
    },
    /// Recover an accepted start, or permanently withdraw its unpublished original ID.
    ResolveStart {
        project: String,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        request_id: String,
    },
    List {
        project: String,
    },
    /// Observe one page of retained completed invocations owned by this session.
    ListArchived {
        project: String,
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t=32, value_parser=clap::value_parser!(u16).range(1..=32))]
        limit: u16,
    },
    /// Remove a completed invocation from the operational catalog while retaining all evidence.
    ArchiveCompleted {
        project: String,
        attempt: String,
        #[arg(long)]
        original_request_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Explicitly remove one retired unsuccessful attempt. Candidate evidence is preserved.
    RemoveUnsuccessful {
        project: String,
        attempt: String,
        #[arg(long)]
        original_request_id: String,
        #[arg(long, value_parser=["failed", "cancelled", "interrupted"])]
        expected_state: String,
        #[arg(long)]
        request_id: Option<String>,
    },
    Start {
        project: String,
        module: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        worker_sha256: String,
        #[arg(long, required = true)]
        approve_execution: bool,
        #[arg(long, default_value_t = 1048576)]
        guest_memory_bytes: u64,
        #[arg(long, default_value = "WASM values proposal")]
        label: String,
        #[arg(long)]
        request_id: Option<String>,
    },
    Status {
        project: String,
        attempt: String,
    },
    Cancel {
        project: String,
        attempt: String,
        #[arg(long)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProtectionCommands {
    Status {
        project: String,
    },
    /// Supply the complete intended list. Removing entries explicitly unlocks them.
    Set {
        project: String,
        regions: PathBuf,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommands {
    Place {
        project_id: String,
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        replace: Option<String>,
    },
    RemovePlacement {
        project_id: String,
        placement_id: String,
    },
    /// Explicitly retime existing selected axes; never clip or discard motion.
    RetimeStatus {
        project_id: String,
        #[arg(long)]
        revision: u64,
    },
    /// Inspect preserved historical facts and clone mappings; never local edit authority.
    ArchivedRetimeStatus {
        project_id: String,
        #[arg(long)]
        revision: u64,
    },
    /// Verify retained original generation/clock facts for an imported anchor.
    ArchivedGenerationStatus {
        project_id: String,
        candidate_id: String,
        #[arg(long)]
        master_revision: Option<u64>,
    },
    /// Verify retained image approval, pixels and generation evidence; grants no execution authority.
    CandidateImageOriginStatus {
        project_id: String,
        candidate_id: String,
    },
    /// Inspect the exact retained RGB frame for an explicitly selected approval record.
    CandidateImageOriginPixels {
        project_id: String,
        candidate_id: String,
        record_sha256: String,
    },
    Retime {
        project_id: String,
        from_placement: String,
        to_placement: String,
        #[arg(long, required = true, value_delimiter = ',')]
        axes: Vec<String>,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: Option<String>,
    },
    PreviewPlacement {
        project_id: String,
        placement_id: String,
        #[arg(long, allow_hyphen_values = true)]
        time_ns: i64,
        #[arg(long)]
        video_view: Option<PathBuf>,
    },
    /// Preview explicit annotated target pixels and a supported temporal pair.
    #[command(group(clap::ArgGroup::new("preview_annotated_model").args(["model","model_pack"]).required(true).multiple(false)))]
    PreviewAnnotated {
        project_id: String,
        time_ns: i64,
        #[arg(long)]
        annotated_view: PathBuf,
        #[arg(long, default_value_t = 1)]
        source_sample_stride: u32,
        #[arg(
            long,
            required_unless_present = "placement_id",
            conflicts_with = "placement_id"
        )]
        source_version: Option<String>,
        #[arg(long)]
        placement_id: Option<String>,
        #[arg(long, requires = "inference_profile")]
        model: Option<PathBuf>,
        #[arg(long, requires = "model")]
        inference_profile: Option<PathBuf>,
        #[arg(long, conflicts_with = "inference_profile")]
        model_pack: Option<String>,
    },
    /// Preview detector observations from the exact owner-selected offline pack.
    PreviewPack {
        project_id: String,
        time_ns: i64,
        #[arg(long)]
        model_pack: String,
        #[arg(
            long,
            required_unless_present = "placement_id",
            conflicts_with = "placement_id"
        )]
        source_version: Option<String>,
        #[arg(long)]
        placement_id: Option<String>,
        #[arg(long)]
        video_view: Option<PathBuf>,
    },
    GeneratePlacement {
        project_id: String,
        placement_id: String,
        #[arg(long, default_value = "fast")]
        preset: String,
        #[arg(long)]
        video_view: Option<PathBuf>,
    },
    Create {
        name: String,
    },
    Open {
        project_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_player_guard_status_keeps_the_original_binding_and_project() {
        assert!(matches!(Cli::try_parse_from([
            "pulsar", "handy-player-guard-status", "project", "--binding-request-id", "original-bind"
        ]).unwrap().command, Some(Commands::HandyPlayerGuardStatus {project,binding_request_id})
            if project == "project" && binding_request_id == "original-bind"));
        for args in [
            vec!["pulsar", "handy-player-guard-status", "project"],
            vec![
                "pulsar",
                "handy-player-guard-status",
                "--binding-request-id",
                "original-bind",
            ],
            vec![
                "pulsar",
                "handy-player-guard-status",
                "project",
                "--binding-request-id",
                "original-bind",
                "--revision",
                "1",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
    #[test]
    fn browser_media_lease_cap_is_optional_bounded_and_independent_of_server_lifetime() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::BrowserLive {
                media_lease_ms: None,
                duration_seconds: 300,
                ..
            })
        ));
        for cap in ["1", "30000", "600000"] {
            let mut args = base.to_vec();
            args.extend(["--duration-seconds", "180", "--media-lease-ms", cap]);
            assert!(matches!(Cli::try_parse_from(args).unwrap().command,
                Some(Commands::BrowserLive { media_lease_ms: Some(value), duration_seconds: 180, .. }) if value == cap.parse::<u32>().unwrap()));
        }
        for cap in ["0", "600001", "4294967296", "-1", "not-a-duration"] {
            let mut args = base.to_vec();
            args.extend(["--media-lease-ms", cap]);
            assert!(Cli::try_parse_from(args).is_err(), "{cap}");
        }
    }
    #[test]
    fn browser_player_monitoring_requires_both_explicit_consent_and_selection() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        let plain = Cli::try_parse_from(base).unwrap();
        assert!(matches!(
            plain.command,
            Some(Commands::BrowserLive {
                handy_player_guard: None,
                allow_handy_player_monitoring: false,
                ..
            })
        ));
        let mut consent_only = base.to_vec();
        consent_only.push("--allow-handy-player-monitoring");
        assert!(Cli::try_parse_from(consent_only).is_err());
        let mut selection_only = base.to_vec();
        selection_only.extend(["--handy-player-guard", "guard.json"]);
        assert!(Cli::try_parse_from(selection_only.clone()).is_err());
        selection_only.push("--allow-handy-player-monitoring");
        assert!(
            matches!(Cli::try_parse_from(selection_only).unwrap().command,
            Some(Commands::BrowserLive { handy_player_guard: Some(path),
                allow_handy_player_monitoring: true, .. }) if path.as_path() == std::path::Path::new("guard.json"))
        );
    }
    #[test]
    fn handy_refresh_timing_requires_consent_and_each_exact_play_selection() {
        let args = [
            "pulsar",
            "handy-refresh-timing",
            "project",
            "--revision",
            "7",
            "--acquisition-request-id",
            "acquire",
            "--controller-id",
            "controller",
            "--generation",
            "3",
            "--play-request-id",
            "play",
            "--play-record-id",
            "record",
            "--request-id",
            "refresh",
            "--allow-online-inspection",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::HandyRefreshTiming {project,revision:7,acquisition_request_id,controller_id,generation:3,play_request_id,play_record_id,request_id,allow_online_inspection:true})
            if project=="project" && acquisition_request_id=="acquire" && controller_id=="controller"
                && play_request_id=="play" && play_record_id=="record" && request_id=="refresh")
        );
        for option in [
            "--revision",
            "--acquisition-request-id",
            "--controller-id",
            "--generation",
            "--play-request-id",
            "--play-record-id",
            "--request-id",
            "--allow-online-inspection",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.remove(index);
            if option != "--allow-online-inspection" {
                missing.remove(index);
            }
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
        let mut missing_project = args.to_vec();
        missing_project.remove(2);
        assert!(Cli::try_parse_from(missing_project).is_err());
    }

    #[test]
    fn handy_refresh_timing_status_requires_original_project_without_revision() {
        let args = [
            "pulsar",
            "handy-refresh-timing-status",
            "project",
            "--refresh-request-id",
            "refresh",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::HandyRefreshTimingStatus {project,refresh_request_id})
            if project=="project" && refresh_request_id=="refresh")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-refresh-timing-status", "project"]).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-refresh-timing-status",
            "--refresh-request-id",
            "refresh"
        ])
        .is_err());
        let mut with_revision = args.to_vec();
        with_revision.extend(["--revision", "7"]);
        assert!(Cli::try_parse_from(with_revision).is_err());
    }

    #[test]
    fn handy_reconcile_plan_requires_every_captured_selection() {
        let args = [
            "pulsar",
            "handy-reconcile-plan",
            "project",
            "--device-id",
            "device",
            "--registration-revision",
            "1",
            "--engine-epoch",
            "boot",
            "--device-session",
            "session",
            "--qualification-id",
            "qualification",
            "--qualification-revision",
            "2",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcilePlan { project,device_id,registration_revision:1,engine_epoch,device_session,qualification_id,qualification_revision:2 })
            if project=="project" && device_id=="device" && engine_epoch=="boot" && device_session=="session" && qualification_id=="qualification")
        );
        for option in [
            "--device-id",
            "--registration-revision",
            "--engine-epoch",
            "--device-session",
            "--qualification-id",
            "--qualification-revision",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.drain(index..index + 2);
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn handy_reconcile_requires_exact_plan_request_and_both_consents() {
        let args = [
            "pulsar",
            "handy-reconcile",
            "project",
            "--plan-id",
            "plan",
            "--request-id",
            "reconcile",
            "--allow-online-inspection",
            "--allow-resolution",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcile { project,plan_id,request_id,allow_online_inspection:true,allow_resolution:true })
            if project=="project" && plan_id=="plan" && request_id=="reconcile")
        );
        for option in [
            "--plan-id",
            "--request-id",
            "--allow-online-inspection",
            "--allow-resolution",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.remove(index);
            if !option.starts_with("--allow-") {
                missing.remove(index);
            }
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn handy_reconcile_status_and_cancel_require_original_request_identity() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-reconcile-status",
            "project",
            "--reconciliation-request-id",
            "original",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcileStatus { project,reconciliation_request_id })
            if project=="project" && reconciliation_request_id=="original")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-reconcile-status", "project"]).is_err());
        let args = [
            "pulsar",
            "handy-reconcile-cancel",
            "project",
            "--reconciliation-request-id",
            "original",
            "--request-id",
            "cancel",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcileCancel { project,reconciliation_request_id,request_id })
            if project=="project" && reconciliation_request_id=="original" && request_id=="cancel")
        );
        for option in ["--reconciliation-request-id", "--request-id"] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.drain(index..index + 2);
            assert!(Cli::try_parse_from(missing).is_err());
        }
    }

    #[test]
    fn handy_hsp_preparation_cli_requires_explicit_consent_and_original_selection() {
        let args = [
            "pulsar",
            "handy-hsp-prepare",
            "project",
            "--revision",
            "2",
            "--engine-epoch",
            "boot",
            "--device-session",
            "session",
            "--qualification-id",
            "dossier",
            "--qualification-revision",
            "2",
            "--timeout-ms",
            "1000",
            "--request-id",
            "prepare",
            "--allow-online-synchronization",
        ];
        assert!(matches!(Cli::try_parse_from(args).unwrap().command,
            Some(Commands::HandyHspPrepare {project,timeout_ms:1000,allow_online_synchronization:true,..}) if project=="project"));
        for option in [
            "--revision",
            "--engine-epoch",
            "--device-session",
            "--qualification-id",
            "--qualification-revision",
            "--timeout-ms",
            "--request-id",
            "--allow-online-synchronization",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|v| *v == option).unwrap();
            let length = if option == "--allow-online-synchronization" {
                1
            } else {
                2
            };
            missing.drain(index..index + length);
            assert!(Cli::try_parse_from(missing).is_err(), "{option}");
        }
        for timeout in ["0", "60001"] {
            let mut invalid = args.to_vec();
            let index = invalid.iter().position(|v| *v == "--timeout-ms").unwrap();
            invalid[index + 1] = timeout;
            assert!(Cli::try_parse_from(invalid).is_err());
        }
        assert!(
            matches!(Cli::try_parse_from(["pulsar","handy-hsp-preparation-status","project","--preparation-request-id","prepare"]).unwrap().command,
            Some(Commands::HandyHspPreparationStatus {project,preparation_request_id}) if project=="project" && preparation_request_id=="prepare")
        );
        assert!(
            matches!(Cli::try_parse_from(["pulsar","handy-hsp-preparation-cancel","project","--preparation-request-id","prepare","--request-id","cancel"]).unwrap().command,
            Some(Commands::HandyHspPreparationCancel {project,preparation_request_id,request_id}) if project=="project" && preparation_request_id=="prepare" && request_id=="cancel")
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-preparation-cancel",
            "project",
            "--preparation-request-id",
            "prepare"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-preparation-status",
            "project",
            "--preparation-request-id",
            "prepare",
            "--timeout-ms",
            "1000"
        ])
        .is_err());
    }
    #[test]
    fn handy_hsp_status_requires_project_and_original_stream_without_revision() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-hsp-status",
            "project",
            "--stream-request-id",
            "acquire",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyHspStatus { project, stream_request_id })
            if project == "project" && stream_request_id == "acquire")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-hsp-status", "project"]).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-status",
            "--stream-request-id",
            "acquire"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-status",
            "project",
            "--stream-request-id",
            "acquire",
            "--revision",
            "1"
        ])
        .is_err());
    }

    #[test]
    fn handy_stop_status_requires_project_and_original_acquisition_without_revision() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-stop-status",
            "project",
            "--acquisition-request-id",
            "acquire",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyStopStatus { project, acquisition_request_id })
            if project == "project" && acquisition_request_id == "acquire")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-stop-status", "project"]).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-stop-status",
            "--acquisition-request-id",
            "acquire"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-stop-status",
            "project",
            "--acquisition-request-id",
            "acquire",
            "--revision",
            "1"
        ])
        .is_err());
    }

    #[test]
    fn handy_play_requires_consent_and_every_captured_selection() {
        let args = [
            "pulsar",
            "handy-play",
            "project",
            "--revision",
            "7",
            "--acquisition-request-id",
            "acquire",
            "--controller-id",
            "controller",
            "--generation",
            "3",
            "--clock-request-id",
            "clock",
            "--start-time-ms",
            "125",
            "--request-id",
            "play",
            "--allow-play",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(matches!(parsed.command, Some(Commands::HandyPlay {
            project, revision: 7, acquisition_request_id, controller_id, generation: 3,
            clock_request_id, start_time_ms: 125, request_id, allow_play: true,
        }) if project == "project" && acquisition_request_id == "acquire"
            && controller_id == "controller" && clock_request_id == "clock" && request_id == "play"));
        for option in [
            "--revision",
            "--acquisition-request-id",
            "--controller-id",
            "--generation",
            "--clock-request-id",
            "--start-time-ms",
            "--request-id",
            "--allow-play",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.remove(index);
            if option != "--allow-play" {
                missing.remove(index);
            }
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn handy_play_status_requires_the_original_play_request() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-play-status",
            "project",
            "--play-request-id",
            "play",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyPlayStatus { project, play_request_id }) if project == "project" && play_request_id == "play")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-play-status", "project"]).is_err());
        assert!(
            Cli::try_parse_from(["pulsar", "handy-play-status", "--play-request-id", "play"])
                .is_err()
        );
    }

    #[test]
    fn handy_play_cancel_requires_exact_record_and_separate_request_selection() {
        let args = [
            "pulsar",
            "handy-play-cancel",
            "project",
            "--play-request-id",
            "play",
            "--record-id",
            "record",
            "--request-id",
            "cancel",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyPlayCancel { project, play_request_id, record_id, request_id }) if project == "project" && play_request_id == "play" && record_id == "record" && request_id == "cancel")
        );
        for option in ["--play-request-id", "--record-id", "--request-id"] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.drain(index..index + 2);
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn retained_scheduler_requires_state_and_keeps_explicit_step_and_poll_controls() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-run", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-run",
            "batch",
            "--publication-state",
            "/state",
            "--steps",
            "2",
            "--poll-ms",
            "1",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::LibraryBatchRun {
                steps: 2,
                poll_ms: 1,
                ..
            })
        ));
        assert!(Cli::try_parse_from([
            "pulsar",
            "library-batch-workflow-status",
            "batch",
            "--publication-state",
            "/state"
        ])
        .is_ok());
    }

    #[test]
    fn library_publication_commands_require_named_private_state_and_preserve_step_count() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-publish", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-publication-prepare",
            "batch",
            "--publication-state",
            "/state",
            "--steps",
            "2",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::LibraryBatchPublicationPrepare { steps: 2, .. })
        ));
        assert!(Cli::try_parse_from([
            "pulsar",
            "library-batch-publication-status",
            "--publication-state",
            "/state"
        ])
        .is_ok());
    }

    #[test]
    fn library_cancel_requires_explicit_replay_identity() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-cancel", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-cancel",
            "batch",
            "--request-id",
            "cancel-id",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::LibraryBatchCancel {batch,request_id}) if batch=="batch" && request_id=="cancel-id")
        );
    }

    #[test]
    fn library_release_requires_explicit_replay_identity() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-release", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-release",
            "batch",
            "--request-id",
            "release-id",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::LibraryBatchRelease {batch,request_id}) if batch=="batch" && request_id=="release-id")
        );
    }

    #[test]
    fn vision_setup_and_image_prepare_require_their_explicit_scope_controls() {
        let setup = [
            "pulsar",
            "assistant-setup",
            "--output",
            "/policy.json",
            "--runtime",
            "/runtime",
            "--model",
            "/model",
            "--projector",
            "/projector",
        ];
        assert!(Cli::try_parse_from(setup).is_err());
        let mut approved = setup.to_vec();
        approved.push("--approve-host-inference");
        let parsed = Cli::try_parse_from(approved).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::AssistantSetup { projector: Some(path), .. }) if path == std::path::Path::new("/projector"))
        );
        let prepare = [
            "pulsar",
            "assistant",
            "prepare",
            "project",
            "--profile",
            "profile",
            "--revision",
            "0",
            "--prompt",
            "Suggest a theme",
            "--request-id",
            "intent",
            "--output",
            "/intent.json",
            "--image-source",
            "still",
        ];
        assert!(Cli::try_parse_from(prepare).is_err());
        let mut creative = prepare.to_vec();
        creative.extend(["--creative-scope", "/scope.json"]);
        let parsed = Cli::try_parse_from(creative).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::Assistant { command: AssistantCommands::Prepare { image_source: Some(source), .. } }) if source == "still")
        );
    }

    #[test]
    fn creative_cli_requires_separate_approval_and_exclusive_scope() {
        let base = [
            "pulsar",
            "assistant",
            "creative-candidate",
            "project",
            "attempt",
            "--revision",
            "0",
            "--request-id",
            "generation",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut args = base.to_vec();
        args.push("--approve-generation");
        assert!(Cli::try_parse_from(args).is_ok());
        let base = [
            "pulsar",
            "assistant",
            "prepare",
            "project",
            "--profile",
            "profile",
            "--revision",
            "0",
            "--prompt",
            "hold",
            "--request-id",
            "start",
            "--output",
            "/intent.json",
            "--creative-scope",
            "/scope.json",
        ];
        assert!(Cli::try_parse_from(base).is_ok());
        let mut args = base.to_vec();
        args.extend(["--review-id", "flag"]);
        assert!(Cli::try_parse_from(args).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "assistant",
            "prepare-creative-generation",
            "project",
            "attempt",
            "--revision",
            "0",
            "--request-id",
            "generate",
            "--output",
            "/generation.json"
        ])
        .is_ok());
    }
    #[test]
    fn explicit_pattern_candidate_requires_project_file_and_revision() {
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "pattern-candidate",
                "project",
                "/patterns.txt",
                "--revision",
                "7"
            ])
            .unwrap()
            .command,
            Some(Commands::PatternCandidate { revision: 7, .. })
        ));
        assert!(
            Cli::try_parse_from(["pulsar", "pattern-candidate", "project", "/patterns.txt"])
                .is_err()
        );
    }
    #[test]
    fn saved_assistant_generation_cli_requires_separate_approval_and_complete_prepare_options() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "assistant",
            "generate-saved",
            "--intent",
            "/private/request.json"
        ])
        .is_err());
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "assistant",
                "generate-saved",
                "--intent",
                "/private/request.json",
                "--approve-generation"
            ])
            .unwrap()
            .command,
            Some(Commands::Assistant {
                command: AssistantCommands::GenerateSaved {
                    approve_generation: true,
                    ..
                }
            })
        ));
        let args = [
            "pulsar",
            "assistant",
            "prepare-generation",
            "project",
            "attempt",
            "--source",
            "source",
            "--revision",
            "7",
            "--request-id",
            "original",
            "--output",
            "/private/request.json",
        ];
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Some(Commands::Assistant {
                command: AssistantCommands::PrepareGeneration { revision: 7, .. }
            })
        ));
        assert!(Cli::try_parse_from(&args[..args.len() - 2]).is_err());
    }
    #[test]
    fn assistant_review_cli_requires_revision_and_accepts_repeated_focus() {
        assert!(
            Cli::try_parse_from(["pulsar", "assistant", "explain", "project", "attempt"]).is_err()
        );
        let parsed = Cli::try_parse_from([
            "pulsar",
            "assistant",
            "prepare",
            "project",
            "--profile",
            "profile",
            "--revision",
            "7",
            "--prompt",
            "review",
            "--request-id",
            "request",
            "--output",
            "/private/intent",
            "--review-id",
            "first",
            "--review-id",
            "second",
            "--edit-id",
            "edit-first",
            "--target-id",
            "observed-first",
            "--target-id",
            "observed-second",
        ])
        .unwrap();
        let Some(Commands::Assistant {
            command:
                AssistantCommands::Prepare {
                    image_source: None,
                    review_focus,
                    target_focus,
                    edit_focus,
                    ..
                },
        }) = parsed.command
        else {
            panic!("prepare")
        };
        assert_eq!(review_focus, vec!["first", "second"]);
        assert_eq!(target_focus, vec!["observed-first", "observed-second"]);
        assert_eq!(edit_focus, vec!["edit-first"]);

        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "assistant",
                "explain",
                "project",
                "attempt",
                "--revision",
                "7"
            ])
            .unwrap()
            .command,
            Some(Commands::Assistant {
                command: AssistantCommands::Explain { revision: 7, .. }
            })
        ));
    }
    #[test]
    fn assistant_generation_cli_requires_explicit_approval_source_revision_and_identity() {
        let args = [
            "pulsar",
            "assistant",
            "generate",
            "project",
            "attempt",
            "--source",
            "source",
            "--revision",
            "0",
            "--request-id",
            "original",
        ];
        assert!(Cli::try_parse_from(args).is_err());
        let mut approved = args.to_vec();
        approved.push("--approve-generation");
        let parsed = Cli::try_parse_from(approved).unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::Assistant {
                command: AssistantCommands::Generate {
                    approve_generation: true,
                    ..
                }
            })
        ));
    }
    #[test]
    fn pack_withdrawal_cli_keeps_both_revision_fences_and_explicit_legacy_scope() {
        let digest = "a".repeat(64);
        let parsed = Cli::try_parse_from([
            "pulsar",
            "model",
            "pack-withdraw",
            &digest,
            "--selection-revision",
            "1",
            "--expected-selection-revision",
            "3",
            "--include-unclassified-generation",
            "--request-id",
            "original-withdrawal",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::Model {command:ModelCommands::PackWithdraw {selection_revision:1,expected_selection_revision:3,include_unclassified_generation:true,request_id:Some(ref id),..}}) if id=="original-withdrawal")
        );
        assert!(Cli::try_parse_from(["pulsar", "model", "pack-withdraw", &digest]).is_err());
    }

    #[test]
    fn original_live_cuda_download_cli_requires_an_explicit_known_part_and_destination() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "download-evidence",
            "project",
            "0",
            "capture",
            "trace",
            "out.json"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "download-evidence",
            "project",
            "0",
            "capture",
            "unknown",
            "out.json"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "download-evidence",
            "project",
            "0",
            "capture",
            "trace"
        ])
        .is_err());
    }
    #[test]
    fn retention_removal_cli_requires_confirmation_and_effect_identity() {
        let hash = "0".repeat(64);
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "remove-evidence",
            "project",
            "0",
            "capture",
            &hash,
            "--request-id",
            "removal",
            "--confirm-removal"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "remove-evidence",
            "project",
            "0",
            "capture",
            &hash,
            "--request-id",
            "removal"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "remove-evidence",
            "project",
            "0",
            "capture",
            &hash,
            "--confirm-removal"
        ])
        .is_err());
    }
    #[test]
    fn retained_cuda_listing_requires_revision_and_bounded_page() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "list-evidence",
            "project",
            "7",
            "--limit",
            "2",
            "--after",
            "capture"
        ])
        .is_ok());
        for limit in ["0", "17", "65535"] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "live-capture",
                "list-evidence",
                "project",
                "7",
                "--limit",
                limit
            ])
            .is_err());
        }
        assert!(
            Cli::try_parse_from(["pulsar", "live-capture", "list-evidence", "project"]).is_err()
        );
    }
    #[test]
    fn retained_live_cuda_evidence_cli_is_explicitly_project_revision_epoch_scoped() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "evidence",
            "project",
            "7",
            "capture",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::LiveCapture {
                command: LiveCaptureCommands::Evidence { revision: 7, .. }
            })
        ));
        assert!(
            Cli::try_parse_from(["pulsar", "live-capture", "evidence", "project", "capture"])
                .is_err()
        );
    }
    #[test]
    fn legacy_aliases_remain_parseable() {
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "audit", "/media"])
                .unwrap()
                .command,
            Some(Commands::Scan { .. })
        ));
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "stash", "query"])
                .unwrap()
                .command,
            Some(Commands::Stash {
                command: StashCommands::ListMissing { .. }
            })
        ));
    }
    #[test]
    fn stash_environment_name_and_device_profile_grammar_are_explicit() {
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "stash", "query", "--api-key-env", "STASH_API_KEY"])
                .unwrap()
                .command,
            Some(Commands::Stash {
                command: StashCommands::ListMissing {
                    api_key_env: Some(_),
                    ..
                }
            })
        ));
        assert!(Cli::try_parse_from([
            "pulsar",
            "export-device-motion",
            "project-id",
            "proposal.json",
            "--profile",
            "dynamics.json"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "export-device-motion",
            "project-id",
            "proposal.json"
        ])
        .is_err());
    }

    #[test]
    fn capture_old_boot_replay_requires_explicit_request_identity() {
        for mut args in [
            vec![
                "pulsar",
                "live-capture",
                "start",
                "project",
                "0",
                "options.json",
            ],
            vec![
                "pulsar",
                "live-capture",
                "continue",
                "project",
                "0",
                "prior",
                "options.json",
            ],
            vec![
                "pulsar",
                "live-capture",
                "continue-at",
                "project",
                "0",
                "prior",
                "anchor.json",
            ],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok());
            args.extend(["--engine-epoch", "original-boot"]);
            assert!(Cli::try_parse_from(args.clone()).is_err());
            args.extend(["--request-id", "original-request"]);
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }
    #[test]
    fn live_finish_grammar_is_explicit_and_keeps_request_identity() {
        let cli = Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "finish",
            "project",
            "3",
            "epoch",
            "--request-id",
            "finish-id",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::LiveCapture {
                command: LiveCaptureCommands::Finish {
                    revision: 3,
                    request_id: Some(_),
                    ..
                }
            })
        ));
        assert!(
            Cli::try_parse_from(["pulsar", "live-capture", "finish", "project", "epoch"]).is_err()
        );
    }
    #[test]
    fn recording_stage_keeps_explicit_options_revision_and_request() {
        let cli = Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "recording-stage",
            "project",
            "3",
            "/absolute/options.json",
            "--request-id",
            "stage-id",
        ])
        .unwrap();
        assert!(
            matches!(cli.command,Some(Commands::LiveCapture {command:LiveCaptureCommands::RecordingStage {revision:3,request_id:Some(id),..}}) if id=="stage-id")
        );
    }
    #[test]
    fn gap_recipe_requires_explicit_authored_policy_and_revision() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "gap-candidate",
            "project",
            "/absolute/gaps.json",
            "--revision",
            "1"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "gap-candidate",
            "project",
            "/absolute/gaps.json",
            "--revision",
            "1",
            "--linear-edge-hold"
        ])
        .is_ok());
    }
    #[test]
    fn generation_dimensions_distinguish_omitted_and_explicit_cli_requests() {
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "generate", "video.mp4"])
                .unwrap()
                .command,
            Some(Commands::Generate {
                width: None,
                height: None,
                ..
            })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json"
            ])
            .unwrap()
            .command,
            Some(Commands::Stash {
                command: StashCommands::GenerateMissing {
                    width: None,
                    height: None,
                    ..
                }
            })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json",
                "--width",
                "320"
            ])
            .unwrap()
            .command,
            Some(Commands::Stash {
                command: StashCommands::GenerateMissing {
                    width: Some(320),
                    height: None,
                    ..
                }
            })
        ));
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "generate", "video.mp4", "--height", "180"])
                .unwrap()
                .command,
            Some(Commands::Generate {
                width: None,
                height: Some(180),
                ..
            })
        ));
    }

    #[test]
    fn detector_score_options_are_checked_and_omission_keeps_legacy_default() {
        let base = [
            "pulsar",
            "generate",
            "video.mp4",
            "--model",
            "model.onnx",
            "--model-class-count",
            "1",
        ];
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::Generate { conf: None, .. })
        ));
        for value in ["0", "0.35", "1"] {
            let mut args = base.to_vec();
            args.extend(["--conf", value]);
            assert!(matches!(
                Cli::try_parse_from(args).unwrap().command,
                Some(Commands::Generate { conf: Some(_), .. })
            ));
        }
        for value in ["NaN", "inf", "1.01", "-0.1"] {
            let mut args = base.to_vec();
            args.extend(["--conf", value]);
            assert!(Cli::try_parse_from(args).is_err());
            assert!(Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json",
                "--detector-score-threshold",
                value
            ])
            .is_err());
        }
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json",
                "--model-pack",
                "pack",
                "--detector-score-threshold",
                "0.75"
            ])
            .unwrap()
            .command,
            Some(Commands::Stash {
                command: StashCommands::GenerateMissing {
                    detector_score_threshold: Some(0.75),
                    ..
                }
            })
        ));
    }
    #[test]
    fn visual_tier_cli_preserves_explicit_source_stride_and_refuses_invalid_counts() {
        for tier in ["default", "fast", "maximum", "quality"] {
            let parsed = Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mkv",
                "--profile",
                tier,
                "--source-sample-stride",
                "3",
            ])
            .unwrap();
            assert!(
                matches!(parsed.command,Some(Commands::Generate {profile,source_sample_stride:3,..}) if profile==tier)
            );
        }
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "generate", "video.mkv"])
                .unwrap()
                .command,
            Some(Commands::Generate {
                source_sample_stride: 1,
                ..
            })
        ));
        for count in ["0", "-1", "1.5", "NaN", "4294967296"] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mkv",
                "--source-sample-stride",
                count
            ])
            .is_err());
        }
    }
    #[test]
    fn complete_legacy_generate_grammar_is_preserved() {
        Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mp4",
            "-o",
            "out.funscript",
            "--model",
            "model.onnx",
            "--conf",
            "0.5",
            "--fps",
            "24",
            "--width",
            "256",
            "--height",
            "256",
            "--detrend-window",
            "2",
            "--norm-window",
            "3",
            "--batch-size",
            "20",
            "--vr",
            "--pov",
            "--no-balance-global",
            "--no-keyframe-reduction",
            "--cut-diff",
            "30",
            "--cut-flow",
            "8",
            "--multi-axis",
            "--profile",
            "dense",
            "--cuts",
            "cuts.json",
        ])
        .unwrap();
    }

    #[test]
    fn annotated_preview_requires_one_explicit_model_and_source_clock() {
        let base = [
            "pulsar",
            "project",
            "preview-annotated",
            "project",
            "125000000",
            "--annotated-view",
            "view.json",
            "--source-version",
            "source",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut pack = base.to_vec();
        pack.extend(["--model-pack", "digest"]);
        assert!(Cli::try_parse_from(&pack).is_ok());
        pack.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(pack).is_err());
        let mut file = base.to_vec();
        file.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(file).is_ok());
    }
    #[test]
    fn detector_letterbox_profile_is_explicit_and_conflicts_with_temporal_recipes() {
        let base = [
            "pulsar",
            "model",
            "profile",
            "/private/model.onnx",
            "--width",
            "640",
            "--height",
            "640",
            "--class-count",
            "80",
        ];
        let mut args = base.to_vec();
        args.extend(["--letterbox", "--padding-byte", "7"]);
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Some(Commands::Model {
                command: ModelCommands::Profile {
                    letterbox: true,
                    padding_byte: Some(7),
                    ..
                }
            })
        ));
        for tail in [
            vec!["--padding-byte", "114"],
            vec!["--letterbox", "--padding-byte", "256"],
            vec!["--letterbox", "--temporal-script-position"],
            vec!["--letterbox", "--annotated-temporal-position"],
        ] {
            let mut args = base.to_vec();
            args.extend(tail);
            assert!(Cli::try_parse_from(args).is_err());
        }
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::Model {
                command: ModelCommands::Profile {
                    letterbox: false,
                    padding_byte: None,
                    ..
                }
            })
        ));
    }
    #[test]
    fn annotated_view_requires_profile_and_rejects_competing_inputs() {
        let base = [
            "pulsar",
            "generate",
            "video.mkv",
            "--annotated-view",
            "view.json",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut pack = base.to_vec();
        pack.extend(["--model-pack", "digest"]);
        assert!(Cli::try_parse_from(&pack).is_ok());
        pack.extend(["--video-view", "view.json"]);
        assert!(Cli::try_parse_from(pack).is_err());
        let mut incomplete = base.to_vec();
        incomplete.extend(["--model", "model.onnx"]);
        assert!(Cli::try_parse_from(incomplete).is_err());
        let mut args = base.to_vec();
        args.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(&args).is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mkv",
            "--model-pack",
            "digest",
            "--annotated-view",
            "view.json"
        ])
        .is_ok());
        for flag in [
            "--video-view",
            "--target-seed",
            "--raw-frame-pair",
            "--model-pack",
        ] {
            let mut bad = args.clone();
            bad.extend([flag, "other"]);
            assert!(Cli::try_parse_from(bad).is_err());
        }
    }
    #[test]
    fn annotated_stash_catalog_requires_file_profile_or_pack_and_refuses_global_view() {
        let base = [
            "pulsar",
            "stash",
            "generate-missing",
            "--mappings",
            "mappings.json",
            "--annotated-views",
            "views.json",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut pack = base.to_vec();
        pack.extend(["--model-pack", "digest"]);
        assert!(Cli::try_parse_from(&pack).is_ok());
        pack.extend(["--video-view", "view.json"]);
        assert!(Cli::try_parse_from(pack).is_err());
        let mut incomplete = base.to_vec();
        incomplete.extend(["--model", "model.onnx"]);
        assert!(Cli::try_parse_from(incomplete).is_err());
        let mut args = base.to_vec();
        args.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(&args).is_ok());
        for flag in ["--video-view", "--model-pack"] {
            let mut bad = args.clone();
            bad.extend([flag, "other"]);
            assert!(Cli::try_parse_from(bad).is_err());
        }
    }

    #[test]
    fn explicit_inference_profile_requires_model_and_replaces_class_guessing() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mp4",
            "--inference-profile",
            "profile.json"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mp4",
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
            "--model-class-count",
            "1"
        ])
        .is_err());
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mp4",
                "--model",
                "model.onnx",
                "--inference-profile",
                "profile.json"
            ])
            .unwrap()
            .command,
            Some(Commands::Generate {
                inference_profile: Some(_),
                ..
            })
        ));
    }
}

#[derive(Debug, Subcommand)]
pub enum LiveCaptureCommands {
    /// Stage Engine-owned accepted recordings as an uncommitted candidate.
    RecordingStage {
        project: String,
        revision: u64,
        options: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
    },
    RecordingStart {
        project: String,
        revision: u64,
        epoch: String,
        #[arg(long, default_value_t = 1024)]
        maximum_samples: u16,
        #[arg(long)]
        request_id: Option<String>,
    },
    RecordingPage {
        project: String,
        revision: u64,
        epoch: String,
        #[arg(long, default_value_t = 0)]
        after: u32,
        #[arg(long, default_value_t = 64)]
        limit: u16,
    },
    RecordingDiscard {
        project: String,
        revision: u64,
        epoch: String,
        recording_request_id: String,
    },
    Stream {
        project: String,
        revision: u64,
        epoch: String,
    },
    Encoded {
        project: String,
        revision: u64,
        epoch: String,
        source: PathBuf,
    },
    Clock {
        project: String,
        revision: u64,
        epoch: String,
    },
    Push {
        project: String,
        revision: u64,
        epoch: String,
        header: PathBuf,
        pixels: PathBuf,
    },
    Limits {
        project: String,
        revision: u64,
    },
    Start {
        project: String,
        revision: u64,
        options: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(
            long,
            requires = "request_id",
            help = "Use the original boot ID for exact saved-request replay"
        )]
        engine_epoch: Option<String>,
    },
    ContinueAt {
        project: String,
        revision: u64,
        predecessor: String,
        source_anchor: PathBuf,
        #[arg(long, default_value_t = 30000)]
        lease_ms: u32,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(
            long,
            requires = "request_id",
            help = "Use the original boot ID for exact saved-request replay"
        )]
        engine_epoch: Option<String>,
    },
    Continue {
        project: String,
        revision: u64,
        predecessor: String,
        options: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(
            long,
            requires = "request_id",
            help = "Use the original boot ID for exact saved-request replay"
        )]
        engine_epoch: Option<String>,
    },
    Status {
        project: String,
        revision: u64,
        epoch: String,
    },
    /// Inspect retained historical CUDA descriptors, including after Engine restart.
    Evidence {
        project: String,
        revision: u64,
        epoch: String,
    },
    /// Browse retained capture identifiers in binary epoch order.
    ListEvidence {
        project: String,
        revision: u64,
        #[arg(long)]
        after: Option<String>,
        #[arg(long,default_value_t=16,value_parser=clap::value_parser!(u16).range(1..=16))]
        limit: u16,
    },
    /// Remove checked retained originals; not secure erasure of journals/backups.
    RemoveEvidence {
        project: String,
        revision: u64,
        epoch: String,
        manifest_sha256: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true)]
        confirm_removal: bool,
    },
    /// Download one original retained evidence artifact; never overwrite a file.
    DownloadEvidence {
        project: String,
        revision: u64,
        epoch: String,
        #[arg(value_parser=["origin","manifest","summary","receipt","trace"])]
        part: String,
        output: PathBuf,
    },
    Stop {
        project: String,
        revision: u64,
        epoch: String,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Close exact browser CPU input; preserve playback while its original work settles.
    CloseInput {
        project: String,
        revision: u64,
        epoch: String,
        #[arg(long)]
        request_id: String,
        /// JSON containing the exact last native ingress header.
        #[arg(
            long,
            required_unless_present = "no_frames",
            conflicts_with = "no_frames"
        )]
        final_frame: Option<PathBuf>,
        /// Assert that this capture admitted no frames.
        #[arg(
            long,
            required_unless_present = "final_frame",
            conflicts_with = "final_frame"
        )]
        no_frames: bool,
    },
    /// Request CUDA evidence finalization; poll status for its terminal result.
    Finish {
        project: String,
        revision: u64,
        epoch: String,
        #[arg(long)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum NativePreparationCommands {
    /// List private saved Start identities; does not start or resolve engine work.
    Saved { project: String },
    /// Forget one local recovery identity; does not cancel or release engine work.
    Dismiss {
        project: String,
        #[arg(long)]
        original_request_id: String,
    },
    /// Inspect the engine epoch and independent preparation limits.
    Limits {
        project: String,
        #[arg(long)]
        revision: u64,
    },
    /// Prepare verified native bytes asynchronously; no device motion or inference.
    Start {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        engine_epoch: String,
        #[arg(long)]
        preparation_ms: u64,
        #[arg(long)]
        retention_ms: u64,
        #[arg(long)]
        request_id: String,
    },
    /// Resolve a possibly lost Start reply using its original request identity.
    Resolve {
        project: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        original_request_id: String,
    },
    Status {
        project: String,
        #[arg(long)]
        revision: u64,
        operation: String,
    },
    Cancel {
        project: String,
        #[arg(long)]
        revision: u64,
        operation: String,
        #[arg(long)]
        request_id: String,
    },
    Release {
        project: String,
        #[arg(long)]
        revision: u64,
        operation: String,
        #[arg(long)]
        request_id: String,
    },
}

#[cfg(test)]
mod native_preparation_cli_tests {
    use super::*;
    #[test]
    fn native_preparation_local_recovery_commands_require_project_and_identity() {
        assert!(Cli::try_parse_from(["pulsar", "native-preparation", "saved", "project"]).is_ok());
        assert!(
            Cli::try_parse_from(["pulsar", "native-preparation", "dismiss", "project"]).is_err()
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "native-preparation",
            "dismiss",
            "project",
            "--original-request-id",
            "original"
        ])
        .is_ok());
    }

    #[test]
    fn native_preparation_start_requires_recoverable_identity_and_epoch() {
        let args = [
            "pulsar",
            "native-preparation",
            "start",
            "project",
            "--revision",
            "0",
            "--engine-epoch",
            "boot",
            "--preparation-ms",
            "1000",
            "--retention-ms",
            "2000",
        ];
        assert!(Cli::try_parse_from(args).is_err());
        let mut complete = args.to_vec();
        complete.extend(["--request-id", "start-identity"]);
        let parsed = Cli::try_parse_from(complete).unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::NativePreparation {command:NativePreparationCommands::Start {request_id,..}}) if request_id=="start-identity")
        );
    }
}

#[derive(Debug, Subcommand)]
pub enum AssistantCommands {
    /// Save the exact synthetic proposal and approval without starting generation.
    PrepareCreativeGeneration {
        project: String,
        attempt: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Approve an exact retained synthetic proposal as an uncommitted candidate.
    CreativeCandidate {
        project: String,
        attempt: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required = true, action=clap::ArgAction::SetTrue)]
        approve_generation: bool,
    },
    Recipes {
        project: String,
    },
    /// Save the complete reviewed recipe declaration without applying it.
    PrepareEdit {
        project: String,
        attempt: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Apply or retry exactly the private saved edit declaration.
    ApplySavedEdit {
        #[arg(long)]
        intent: PathBuf,
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        approve_edit: bool,
    },
    /// Create a reviewed edit candidate; committing it remains a separate action.
    ApplyEdit {
        project: String,
        attempt: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        approve_edit: bool,
    },
    /// Show checked, current detector targets retained by this engine session.
    Targets {
        project: String,
    },
    Profiles {
        project: String,
    },
    /// Save a private exact start intent before dispatch; this does not run inference.
    Prepare {
        project: String,
        /// Imported single-frame media for a creative image theme.
        #[arg(long, requires = "creative_scope")]
        image_source: Option<String>,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        prompt: String,
        #[arg(long, conflicts_with_all=["edit_focus","target_focus","review_focus"])]
        creative_scope: Option<PathBuf>,
        #[arg(long = "edit-id")]
        edit_focus: Vec<String>,
        #[arg(long = "target-id")]
        target_focus: Vec<String>,
        #[arg(long = "review-id")]
        review_focus: Vec<String>,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Show the recorded evidence selected by a completed explanation proposal.
    Explain {
        project: String,
        attempt: String,
        #[arg(long)]
        revision: u64,
    },
    /// Submit or retry exactly the saved intent; no new identity or profile substitution.
    Start {
        #[arg(long)]
        intent: PathBuf,
        #[arg(long, required = true)]
        approve_inference: bool,
    },
    /// Approve a completed stride proposal for an explicit source generation job.
    Generate {
        project: String,
        attempt: String,
        #[arg(long)]
        source: String,
        #[arg(long, default_value = "fast")]
        preset: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        settings: Option<PathBuf>,
        #[arg(long)]
        model: Option<PathBuf>,
        #[arg(long, required = true)]
        approve_generation: bool,
    },
    /// Save an exact generation declaration; no job is dispatched.
    PrepareGeneration {
        project: String,
        attempt: String,
        #[arg(long)]
        source: String,
        #[arg(long, default_value = "fast")]
        preset: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        settings: Option<PathBuf>,
        #[arg(long)]
        model: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Approve and retry the complete saved generation declaration.
    GenerateSaved {
        #[arg(long)]
        intent: PathBuf,
        #[arg(long, required = true)]
        approve_generation: bool,
    },
    /// Discover an accepted start or permanently withdraw its original ID if absent.
    Resolve {
        #[arg(long)]
        intent: PathBuf,
        #[arg(long, required = true)]
        acknowledge_withdrawing_unpublished: bool,
    },
    Status {
        project: String,
        attempt: String,
    },
    List {
        project: String,
    },
    Cancel {
        project: String,
        attempt: String,
        #[arg(long)]
        request_id: String,
    },
}

#[cfg(test)]
mod image_origin_cli_tests {
    use super::*;
    #[test]
    fn retained_image_origin_query_requires_explicit_project_and_candidate() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "project",
            "candidate-image-origin-status",
            "project",
            "candidate",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::Project {command:ProjectCommands::CandidateImageOriginStatus {project_id,candidate_id}}) if project_id=="project" && candidate_id=="candidate")
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "project",
            "candidate-image-origin-status",
            "project"
        ])
        .is_err());
    }
}

#[cfg(test)]
mod browser_start_parsing_tests {
    use super::*;
    #[test]
    fn prepared_browser_controls_require_selection_enablement_and_exclude_existing_guard() {
        let base = ["pulsar", "browser-live", "project", "1", "options.json"];
        let mut selected = base.to_vec();
        selected.extend(["--handy-browser-play", "/input/prepared.json"]);
        assert!(Cli::try_parse_from(&selected).is_err());
        selected.push("--allow-handy-browser-control");
        assert!(matches!(
            Cli::try_parse_from(&selected).unwrap().command,
            Some(Commands::BrowserLive {
                handy_browser_play: Some(_),
                allow_handy_browser_control: true,
                ..
            })
        ));
        selected.extend([
            "--handy-player-guard",
            "/input/guard.json",
            "--allow-handy-player-monitoring",
        ]);
        assert!(Cli::try_parse_from(selected).is_err());
        let mut missing = base.to_vec();
        missing.push("--allow-handy-browser-control");
        assert!(Cli::try_parse_from(missing).is_err());
    }
    #[test]
    fn browser_dossier_review_preserves_project_and_record_revisions_with_explicit_operator() {
        let args = [
            "pulsar",
            "handy-browser-start-qualification-accept",
            "project",
            "dossier",
            "--revision",
            "4",
            "--record-revision",
            "2",
            "--operator",
            "operator",
            "--request-id",
            "review",
        ];
        assert!(Cli::try_parse_from(args).is_err());
        let mut approved = args.to_vec();
        approved.push("--acknowledge-measured-evidence");
        assert!(matches!(
            Cli::try_parse_from(approved).unwrap().command,
            Some(Commands::HandyBrowserStartQualificationAccept {
                revision: 4,
                record_revision: 2,
                acknowledge_measured_evidence: true,
                ..
            })
        ));
        let withdraw = [
            "pulsar",
            "handy-browser-start-qualification-withdraw",
            "project",
            "dossier",
            "--record-revision",
            "3",
            "--reason",
            "evidence replaced",
            "--request-id",
            "withdraw",
        ];
        assert!(Cli::try_parse_from(withdraw).is_ok());
    }
}

#[cfg(test)]
mod browser_hsp_parsing_tests {
    use super::*;

    #[test]
    fn browser_hsp_defaults_to_disabled_and_requires_selection_and_enablement_together() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::BrowserLive {
                handy_hsp: None,
                allow_handy_hsp_control: false,
                ..
            })
        ));
        let mut enablement_only = base.to_vec();
        enablement_only.push("--allow-handy-hsp-control");
        assert!(Cli::try_parse_from(enablement_only).is_err());
        let mut selected = base.to_vec();
        selected.extend(["--handy-hsp", "/input/hsp.json"]);
        assert!(Cli::try_parse_from(&selected).is_err());
        selected.push("--allow-handy-hsp-control");
        assert!(matches!(
            Cli::try_parse_from(selected).unwrap().command,
            Some(Commands::BrowserLive {
                project,
                revision: 7,
                handy_hsp: Some(path),
                allow_handy_hsp_control: true,
                handy_player_guard: None,
                handy_browser_play: None,
                ..
            }) if project == "project" && path == std::path::Path::new("/input/hsp.json")
        ));
    }

    #[test]
    fn browser_hsp_excludes_each_other_handy_mode_in_either_argument_order() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        let hsp = [
            "--handy-hsp",
            "/input/hsp.json",
            "--allow-handy-hsp-control",
        ];
        for other in [
            [
                "--handy-player-guard",
                "/input/guard.json",
                "--allow-handy-player-monitoring",
            ],
            [
                "--handy-browser-play",
                "/input/prepared.json",
                "--allow-handy-browser-control",
            ],
        ] {
            for hsp_first in [true, false] {
                let mut args = base.to_vec();
                for mode in if hsp_first {
                    [hsp, other]
                } else {
                    [other, hsp]
                } {
                    args.extend(mode);
                }
                assert_eq!(
                    Cli::try_parse_from(args).unwrap_err().kind(),
                    clap::error::ErrorKind::ArgumentConflict
                );
            }
        }
    }
}

#[cfg(test)]
mod hsp_qualification_cli_tests {
    use super::*;
    #[test]
    fn qualification_context_defaults_to_hssp_and_requires_an_explicit_supported_selector() {
        let legacy = Cli::try_parse_from(["pulsar", "handy-qualification-context"]).unwrap();
        assert!(
            matches!(legacy.command, Some(Commands::HandyQualificationContext {transport}) if transport == "hssp")
        );
        let hsp = Cli::try_parse_from([
            "pulsar",
            "handy-qualification-context",
            "--transport",
            "hsp",
        ])
        .unwrap();
        assert!(
            matches!(hsp.command, Some(Commands::HandyQualificationContext {transport}) if transport == "hsp")
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-qualification-context",
            "--transport",
            "automatic"
        ])
        .is_err());
    }
}

#[cfg(test)]
mod live_hsp_cli_tests {
    use super::*;

    #[test]
    fn live_binding_requires_separate_control_consent_and_complete_selection() {
        let args = [
            "pulsar",
            "handy-hsp-live-bind",
            "project",
            "--revision",
            "3",
            "--preparation-request-id",
            "prepare",
            "--capture-epoch",
            "capture",
            "--inference-attempt-id",
            "inference",
            "--browser-timing-binding-id",
            "timing",
            "--source-qualification-id",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--source-qualification-revision",
            "2",
            "--request-id",
            "stream",
            "--allow-online-control",
            "--acknowledge-current-environment",
        ];
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Some(Commands::HandyHspLiveBind {
                acknowledge_current_environment: true,
                allow_online_control: true,
                revision: 3,
                ..
            })
        ));
        for omitted in [
            "--allow-online-control",
            "--acknowledge-current-environment",
        ] {
            let missing = args
                .iter()
                .copied()
                .filter(|value| *value != omitted)
                .collect::<Vec<_>>();
            assert!(Cli::try_parse_from(missing).is_err(), "missing {omitted}");
        }
        let missing_capture = args
            .iter()
            .copied()
            .filter(|value| *value != "--capture-epoch" && *value != "capture")
            .collect::<Vec<_>>();
        assert!(Cli::try_parse_from(missing_capture).is_err());
    }

    #[test]
    fn live_play_requires_revision_but_original_stop_and_status_do_not() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-live-play",
            "project",
            "--stream-request-id",
            "stream",
            "--request-id",
            "play"
        ])
        .is_err());
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "handy-hsp-live-play",
                "project",
                "--revision",
                "3",
                "--stream-request-id",
                "stream",
                "--request-id",
                "play"
            ])
            .unwrap()
            .command,
            Some(Commands::HandyHspLivePlay { revision: 3, .. })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "handy-hsp-live-stop",
                "project",
                "--stream-request-id",
                "stream",
                "--request-id",
                "stop"
            ])
            .unwrap()
            .command,
            Some(Commands::HandyHspLiveStop { .. })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "handy-hsp-live-status",
                "project",
                "--stream-request-id",
                "stream"
            ])
            .unwrap()
            .command,
            Some(Commands::HandyHspLiveStatus { .. })
        ));
    }
}

#[cfg(test)]
mod finite_input_cli_tests {
    use super::*;
    #[test]
    fn closing_input_requires_an_original_request_and_one_exact_final_selection() {
        let base = [
            "pulsar",
            "live-capture",
            "close-input",
            "project",
            "1",
            "epoch",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut zero = base.to_vec();
        zero.extend(["--request-id", "close-original", "--no-frames"]);
        assert!(matches!(
            Cli::try_parse_from(zero).unwrap().command,
            Some(Commands::LiveCapture {
                command: LiveCaptureCommands::CloseInput {
                    no_frames: true,
                    final_frame: None,
                    ..
                }
            })
        ));
        let mut exact = base.to_vec();
        exact.extend([
            "--request-id",
            "close-original",
            "--final-frame",
            "/absolute/original-header.json",
        ]);
        assert!(Cli::try_parse_from(exact.clone()).is_ok());
        exact.push("--no-frames");
        assert!(Cli::try_parse_from(exact).is_err());
        let mut missing_id = base.to_vec();
        missing_id.push("--no-frames");
        assert!(Cli::try_parse_from(missing_id).is_err());
    }
}

#[cfg(test)]
mod inference_equivalence_cli_tests {
    use super::*;
    #[test]
    fn equivalence_environment_requires_profile_but_no_mutation_request_id() {
        let args = ["pulsar", "model", "equivalence", "environment"];
        assert!(Cli::try_parse_from(args).is_err());
        let path = crate::test_host_path("declared-profile.json");
        let mut complete = args.to_vec();
        complete.extend(["--profile", path.as_str()]);
        let cli = Cli::try_parse_from(complete.clone()).unwrap();
        assert!(
            matches!(cli.command, Some(Commands::Model { command: ModelCommands::Equivalence { command: InferenceEquivalenceCommands::Environment { profile } } }) if profile.as_path() == std::path::Path::new(&path))
        );
        complete.extend(["--request-id", "unused-mutation-id"]);
        assert!(Cli::try_parse_from(complete).is_err());
    }

    #[test]
    fn equivalence_decisions_require_original_identity_and_explicit_acceptance() {
        for incomplete in [
            vec![
                "pulsar",
                "model",
                "equivalence",
                "import",
                "/authored/manifest.json",
                "--manifest-sha256",
                "digest",
            ],
            vec![
                "pulsar",
                "model",
                "equivalence",
                "accept",
                "dossier",
                "--expected-record-revision",
                "1",
                "--operator",
                "supplied-owner",
                "--request-id",
                "accept-original",
            ],
            vec![
                "pulsar",
                "model",
                "equivalence",
                "withdraw",
                "dossier",
                "--expected-record-revision",
                "1",
                "--reason",
                "supplied reason",
            ],
        ] {
            assert!(Cli::try_parse_from(incomplete).is_err());
        }
        let cli = Cli::try_parse_from([
            "pulsar",
            "model",
            "equivalence",
            "accept",
            "dossier",
            "--expected-record-revision",
            "1",
            "--operator",
            "supplied-owner",
            "--acknowledge-measured-equivalence",
            "--request-id",
            "accept-original",
        ])
        .unwrap();
        assert!(
            matches!(cli.command, Some(Commands::Model { command: ModelCommands::Equivalence { command: InferenceEquivalenceCommands::Accept { request_id, acknowledge_measured_equivalence: true, expected_record_revision: 1, .. } } }) if request_id == "accept-original")
        );
        for limit in ["0", "17"] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "model",
                "equivalence",
                "list",
                "--limit",
                limit
            ])
            .is_err());
        }
        assert!(Cli::try_parse_from(["pulsar", "model", "equivalence", "recovery-retry"]).is_err());
        let retry = Cli::try_parse_from([
            "pulsar",
            "model",
            "equivalence",
            "recovery-retry",
            "original-id",
        ])
        .unwrap();
        assert!(
            matches!(retry.command, Some(Commands::Model { command: ModelCommands::Equivalence { command: InferenceEquivalenceCommands::RecoveryRetry { request_id } } }) if request_id == "original-id")
        );
    }
}

#[cfg(test)]
mod inference_fallback_cli_tests {
    use super::*;

    #[test]
    fn fallback_cli_is_opt_in_and_requires_explicit_profile_model_or_pack() {
        let parsed = Cli::try_parse_from(["pulsar", "generate", "video.mp4"]).unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::Generate {
                fallback_selection: None,
                ..
            })
        ));
        let selection = crate::test_host_path("fallback-selection.json");
        for extra in [
            vec![],
            vec!["--model", "model.onnx"],
            vec!["--model", "model.onnx", "--model-class-count", "1"],
            vec!["--inference-profile", "profile.json"],
        ] {
            let mut args = vec![
                "pulsar",
                "generate",
                "video.mp4",
                "--fallback-selection",
                selection.as_str(),
            ];
            args.extend(extra);
            assert!(Cli::try_parse_from(args).is_err());
        }
        for extra in [
            vec![
                "--model",
                "model.onnx",
                "--inference-profile",
                "profile.json",
            ],
            vec!["--model-pack", "declared-pack-digest"],
        ] {
            let mut args = vec![
                "pulsar",
                "generate",
                "video.mp4",
                "--fallback-selection",
                selection.as_str(),
            ];
            args.extend(extra);
            assert!(
                matches!(Cli::try_parse_from(args.clone()).unwrap().command,Some(Commands::Generate {fallback_selection:Some(path),..}) if path==std::path::Path::new(&selection))
            );
            args.extend(["--raw-frame-pair", "pair.json"]);
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn fallback_cli_refuses_relative_or_non_normalized_selection_paths() {
        let root = std::env::temp_dir();
        for path in [
            "policy.json".to_owned(),
            root.join("..")
                .join("policy.json")
                .to_string_lossy()
                .into_owned(),
            format!("{}/./policy.json", root.display()),
        ] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mp4",
                "--model-pack",
                "digest",
                "--fallback-selection",
                path.as_str()
            ])
            .is_err());
        }
    }
}
