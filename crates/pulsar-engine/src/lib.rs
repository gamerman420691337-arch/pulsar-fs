//! Sole authority for durable project state and brokered effects.
//! Native media/inference execution is confined to the worker process role.
mod annotated_generation_db;
pub mod artifacts;
#[cfg(target_os = "linux")]
mod assistant_launch;
#[cfg(target_os = "linux")]
mod assistant_owner_policy;
mod assistant_palette;
#[cfg(target_os = "linux")]
pub use assistant_launch::{AssistantExecution, AssistantLaunchAssets, AssistantVisionAssets};
#[cfg(target_os = "linux")]
pub use assistant_owner_policy::{
    create_assistant_policy, disable_assistant_policy, inspect_assistant_policy_selection,
    select_assistant_policy, AssistantPolicySetup, AssistantSelectionInspection,
};
#[cfg(target_os = "linux")]
mod assistant_schema;
#[cfg(target_os = "linux")]
pub mod assistant_worker;
#[cfg(target_os = "linux")]
mod runtime_install;
#[cfg(target_os = "linux")]
pub use runtime_install::{
    discard_runtime_archive_stage, discard_runtime_stage, inspect_cached_generation_packs,
    inspect_runtime_archive_stage, inspect_runtime_stage, install_runtime_candidate,
    pack_runtime_candidate, pack_runtime_update, prepare_cached_runtime, prepare_runtime_launch,
    update_runtime_candidate, BundledGenerationReport, PreparedRuntime,
    RuntimeArchiveStageInspection, RuntimeStageInspection, RuntimeStageState, RuntimeUpdateReport,
};
mod authority;
pub mod benchmark;
mod bulk;
mod cgroup;
mod confinement;
#[cfg(target_os = "linux")]
mod cpu_mounts;
#[cfg(target_os = "linux")]
mod cpu_owner_policy;
mod cpu_runtime_db;
mod cpu_runtime_reader;
pub mod cuda_acceptance;
pub mod cuda_attempts;
#[cfg(target_os = "linux")]
mod cuda_bootstrap;
#[cfg(target_os = "linux")]
pub mod cuda_devices;
#[cfg(target_os = "linux")]
pub mod cuda_memory;
#[cfg(target_os = "linux")]
pub mod cuda_mounts;
#[cfg(target_os = "linux")]
pub mod cuda_owner_policy;
pub mod cuda_result;
#[cfg(unix)]
mod daemon;
pub mod device_effect_journal;
mod device_motion;
mod generation_progress;
mod handy_clock;
mod handy_clock_sync_wire;
mod handy_credentials;
mod inference_fallback_reader;
#[cfg(target_os = "linux")]
mod launch_cpu;
#[cfg(target_os = "linux")]
mod live_cpu_mounts;
#[cfg(target_os = "linux")]
#[path = "cuda_mount_bytes.rs"]
mod native_mount_bytes;
mod portable_cpu_archives;
mod portable_fallback_reader;
mod task_group;
pub use handy_credentials::HandyRouteIdentity;
mod handy_continuous_timing;
mod handy_hosting_wire;
pub mod handy_hsp_dispatch;
mod handy_hsp_execution;
pub mod handy_hsp_preparation_records;
pub mod handy_hsp_records;
pub mod handy_hsp_runtime;
pub mod handy_hsp_wire;
mod handy_online;
mod handy_playback_clock;
pub mod handy_rpc;
mod handy_script;
mod handy_server_clock;
mod handy_setup_wire;
pub mod handy_wire;
mod inference_budget;
mod inference_equivalence_reader;
mod inference_host_observation;
pub mod inference_profiles;
pub mod inference_trace;
mod instance;
mod job_attempt_history;
mod launch;
mod launch_pipes;
mod library_batch_db;
mod live_cpu_history;
mod live_cpu_reader;
mod live_cuda_capture_db;
mod live_cuda_evidence;
mod live_cuda_evidence_db;
mod live_cuda_result;
#[cfg(target_os = "linux")]
mod live_cuda_stream;
pub mod live_frames;
pub mod live_inference;
pub mod live_source;
#[cfg(target_os = "linux")]
mod live_staging;
mod live_worker_lifetime;
mod model_pack_selection;
mod model_packs;
mod motion_artifacts;
#[cfg(target_os = "linux")]
pub mod native_capture;
pub mod native_dependencies;
#[cfg(target_os = "linux")]
pub mod native_mounts;
mod native_preparation_db;
mod pack_onnx;
mod package_wasm_values;
pub mod playback_runtime;
mod portable_annotated_db;
mod portable_cuda_db;
mod portable_wasm_db;
pub mod project_package_format;
mod project_package_import_plan;
mod project_package_import_storage;
mod project_package_storage;
pub mod resources;
mod source_clock_receipts;
#[cfg(target_os = "linux")]
mod staging_cleanup;
#[cfg(target_os = "linux")]
mod staging_obligation;
mod stash;
mod stash_files;
mod wasm_archive_db;
#[cfg(all(feature = "wasm", target_os = "linux"))]
mod wasm_launch;
#[cfg(feature = "wasm")]
pub mod wasm_worker;
pub mod worker;
pub mod worker_entry;
mod worker_fence;
mod worker_fence_db;
mod worker_retirement;
use anyhow::{bail, Context, Result};
#[cfg(target_os = "linux")]
pub use authority::live_cuda_preparation::{
    LiveCudaImagePreparationOptions, PreparedLiveCudaImage,
};
pub use authority::Engine;
pub use instance::instance_lock_is_held;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub state_dir: PathBuf,
    pub worker_executable: PathBuf,
    pub max_ram_bytes: u64,
    pub max_snapshot_bytes: u64,
    pub max_jobs: usize,
    /// Aggregate admitted worker CPU quota, separate from job slots.
    pub max_cpu_threads: u16,
    pub worker_timeout: Duration,
    /// Owner limit for explicit image preparation; does not extend capture leases.
    pub native_image_preparation_timeout: Duration,
    /// Explicit owner startup policy. Absence preserves CPU-only accounting.
    pub cuda_owner_policy: Option<PathBuf>,
    /// Explicit captured CPU runtime image policy; absent preserves custom CPU execution.
    pub cpu_owner_policy: Option<PathBuf>,
    /// Private explicitly approved assistant startup policy; no automatic selection.
    pub assistant_owner_policy: Option<PathBuf>,
}
impl EngineConfig {
    pub fn new(state_dir: PathBuf, worker_executable: PathBuf) -> Self {
        Self {
            state_dir,
            worker_executable,
            max_ram_bytes: 4 * 1024 * 1024 * 1024,
            max_snapshot_bytes: 32 * 1024 * 1024 * 1024,
            max_jobs: 2,
            max_cpu_threads: 4,
            worker_timeout: Duration::from_secs(60 * 60),
            native_image_preparation_timeout: Duration::from_secs(300),
            cuda_owner_policy: None,
            cpu_owner_policy: None,
            assistant_owner_policy: None,
        }
    }
    pub fn for_user() -> Result<Self> {
        let state_dir = if let Some(path) = std::env::var_os("PULSAR_STATE_DIR") {
            PathBuf::from(path)
        } else if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
            PathBuf::from(path).join("pulsar")
        } else if let Some(path) = std::env::var_os("HOME") {
            PathBuf::from(path).join(".local/state/pulsar")
        } else if let Some(path) = std::env::var_os("LOCALAPPDATA") {
            PathBuf::from(path).join("Pulsar")
        } else {
            bail!("cannot determine per-user engine state directory");
        };
        let mut config = Self::new(state_dir, worker_entry::configured_executable()?);
        if let Some(value) = std::env::var_os("PULSAR_MAX_CPU_THREADS") {
            config.max_cpu_threads = value
                .to_str()
                .context("CPU ceiling must be UTF-8")?
                .parse()
                .context("CPU ceiling must be an integer")?;
            anyhow::ensure!(
                (1..=1024).contains(&config.max_cpu_threads),
                "CPU ceiling must be between 1 and 1024"
            );
        }
        config.cuda_owner_policy = std::env::var_os("PULSAR_CUDA_OWNER_POLICY").map(PathBuf::from);
        config.cpu_owner_policy = std::env::var_os("PULSAR_CPU_OWNER_POLICY").map(PathBuf::from);
        let explicit = std::env::var_os("PULSAR_ASSISTANT_OWNER_POLICY").map(PathBuf::from);
        #[cfg(target_os = "linux")]
        {
            config.assistant_owner_policy =
                assistant_owner_policy::startup_policy_path(&config.state_dir, explicit)?;
        }
        #[cfg(not(target_os = "linux"))]
        {
            config.assistant_owner_policy = explicit;
        }
        Ok(config)
    }
    pub fn endpoint(&self) -> PathBuf {
        self.state_dir.join("engine.sock")
    }
    pub fn bulk_endpoint(&self) -> PathBuf {
        self.state_dir.join("bulk.sock")
    }
    pub fn bootstrap_path(&self) -> PathBuf {
        self.state_dir.join("pairing.token")
    }
}

/// Local IPC does not defend against arbitrary software with the user's full
/// OS privileges. Third-party clients still need explicit pairing and grants.
pub fn run_server(config: EngineConfig) -> Result<()> {
    #[cfg(unix)]
    {
        daemon::run(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        bail!("secured current-user named-pipe server is unavailable on this build; no insecure listener was started");
    }
}

/// Offline catalog verifier; engine authorization and durable publication are separate.
#[doc(hidden)]
pub mod offline_catalog;

#[cfg(target_os = "linux")]
mod launch_cuda;
