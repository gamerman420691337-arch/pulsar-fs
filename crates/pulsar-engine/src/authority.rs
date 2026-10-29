use crate::{
    artifacts, launch,
    resources::{Reservation, ResourcePool},
    worker, EngineConfig,
};
use anyhow::{Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use uuid::Uuid;

#[path = "generation_attempt_origin.rs"]
mod generation_attempt_origin;
#[path = "inference_fallback_execution.rs"]
mod inference_fallback_execution;
#[path = "inference_fallback_lifetime.rs"]
mod inference_fallback_lifetime;
#[path = "inference_fallback_store.rs"]
mod inference_fallback_store;
#[path = "engine_shutdown.rs"]
mod shutdown;
use generation_attempt_origin::AttemptOrigin;

include!("model_pack_admission.rs");
#[path = "model_pack_withdrawal.rs"]
mod model_pack_withdrawal;

#[cfg(target_os = "linux")]
#[path = "cpu_generation.rs"]
mod cpu_generation;
#[cfg(target_os = "linux")]
#[path = "cuda_generation.rs"]
mod cuda_generation;

#[cfg(target_os = "linux")]
#[path = "engine_live_cuda.rs"]
mod engine_live_cuda;

#[cfg(target_os = "linux")]
#[path = "live_cuda_preparation.rs"]
pub(crate) mod live_cuda_preparation;

#[cfg(target_os = "linux")]
#[path = "native_preparations.rs"]
mod native_preparations;

#[cfg(target_os = "linux")]
#[path = "assistant_admission.rs"]
mod assistant_admission;

type PResult<T> = std::result::Result<T, ProtocolError>;
#[path = "browser_media.rs"]
pub(crate) mod browser_media;
#[path = "browser_timing.rs"]
mod browser_timing;
#[path = "handy_browser_start_qualification.rs"]
mod handy_browser_start_qualification;
#[path = "handy_hosting.rs"]
mod handy_hosting;
#[path = "engine_handy_hsp_control.rs"]
mod handy_hsp_control;
#[path = "handy_hsp_live.rs"]
mod handy_hsp_live;
#[path = "handy_hsp_preparation.rs"]
mod handy_hsp_preparation;
#[path = "handy_hsp_source_qualification.rs"]
mod handy_hsp_source_qualification;
#[path = "handy_playback_preparation.rs"]
mod handy_playback_preparation;
#[path = "handy_program.rs"]
mod handy_program;
#[path = "handy_qualification.rs"]
mod handy_qualification;
#[path = "handy_reconciliation.rs"]
mod handy_reconciliation;
#[path = "handy_registrations.rs"]
mod handy_registrations;
#[path = "handy_sessions.rs"]
mod handy_sessions;
#[path = "handy_setup.rs"]
mod handy_setup;
#[path = "handy_setup_cancel.rs"]
mod handy_setup_cancel;
#[path = "handy_setup_send.rs"]
mod handy_setup_send;
#[path = "inference_environment_control.rs"]
mod inference_environment_control;
#[path = "inference_equivalence_control.rs"]
mod inference_equivalence_control;
#[path = "inference_equivalence_store.rs"]
mod inference_equivalence_store;
const PROGRAM_LIMIT: usize = 512 * 1024;
const WORKER_MEMORY: u64 = 1536 * 1024 * 1024;
const OUTPUT_LIMIT: u64 = 64 * 1024 * 1024;
macro_rules! id {
    ($ty:ty) => {
        <$ty>::new(Uuid::new_v4().to_string()).expect("UUID is a checked identifier")
    };
}
#[path = "archived_generation.rs"]
mod archived_generation;
#[path = "archived_motion_retimes.rs"]
mod archived_motion_retimes;
#[path = "archived_project_origins.rs"]
mod archived_project_origins;
#[path = "assistant_creative.rs"]
mod assistant_creative;
#[path = "assistant_edits.rs"]
mod assistant_edits;
#[cfg(target_os = "linux")]
#[path = "assistant_handoff.rs"]
mod assistant_handoff;
#[path = "assistant_image_origin.rs"]
mod assistant_image_origin;
#[path = "assistant_images.rs"]
mod assistant_images;
#[path = "assistant_operations.rs"]
mod assistant_operations;
#[cfg(target_os = "linux")]
#[path = "assistant_recovery.rs"]
mod assistant_recovery;
#[path = "assistant_start_resolution.rs"]
mod assistant_start_resolution;
#[path = "assistant_targets.rs"]
mod assistant_targets;
#[path = "candidate_image_origin.rs"]
mod candidate_image_origin;
#[path = "edit_clock_origins.rs"]
mod edit_clock_origins;
#[path = "editing.rs"]
mod editing;
#[path = "imported_candidate_intervals.rs"]
mod imported_candidate_intervals;
#[path = "library_batches.rs"]
mod library_batches;
#[path = "lineage.rs"]
mod lineage;
#[path = "live_captures.rs"]
mod live_captures;
#[path = "live_recording_candidate.rs"]
mod live_recording_candidate;
#[path = "media_probe.rs"]
mod media_probe;
#[path = "motion_retime_receipts.rs"]
mod motion_retime_receipts;
#[path = "motion_state.rs"]
mod motion_state;
#[cfg(test)]
#[path = "../tests/project_package_closure/mod.rs"]
mod project_package_closure;
#[path = "project_package_exports.rs"]
mod project_package_exports;
#[path = "project_package_imports.rs"]
pub(crate) mod project_package_imports;
#[path = "project_packages.rs"]
mod project_packages;
#[path = "review_interval_receipts.rs"]
mod review_interval_receipts;
#[path = "source_clock_origins.rs"]
pub(crate) mod source_clock_origins;
#[path = "source_kinds.rs"]
mod source_kinds;
#[path = "source_placements.rs"]
mod source_placements;
#[path = "stash_action_ancestry.rs"]
mod stash_action_ancestry;
#[path = "stash_archived_lineage.rs"]
mod stash_archived_lineage;
#[path = "stash_batches.rs"]
mod stash_batches;
#[path = "stash_clock_proofs.rs"]
mod stash_clock_proofs;
#[path = "stash_lineage.rs"]
mod stash_lineage;
#[path = "stash_publications.rs"]
mod stash_publications;
#[path = "stash_scans.rs"]
mod stash_scans;
#[path = "stash_tags.rs"]
mod stash_tags;
#[path = "transfers.rs"]
mod transfers;
#[path = "wasm_edits.rs"]
mod wasm_edits;

type PreparedJobResult = (
    ProgramDescriptor,
    Vec<ArtifactIdentity>,
    Vec<ReviewFlag>,
    Option<WorkerCudaArtifacts>,
);

type JobRow = (String, i64, String, Option<String>, Option<String>);

fn internal(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, error.to_string())
}
fn unavailable(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, error.to_string())
}
fn encode(value: &impl Serialize) -> PResult<String> {
    serde_json::to_string(value).map_err(internal)
}
fn decode<T: for<'de> Deserialize<'de>>(value: &str) -> PResult<T> {
    serde_json::from_str(value).map_err(internal)
}
fn rev_sql(rev: RevisionId) -> PResult<i64> {
    i64::try_from(rev.value())
        .map_err(|_| ProtocolError::invalid("revision exceeds durable storage range"))
}
fn scope_name(scope: Scope) -> String {
    serde_json::to_string(&scope).expect("scope serialization")
}

type PreparedSource = (PathBuf, String, u64, PathBuf, Option<StashSourceCapture>);

enum ExportKind<'a> {
    Neutral,
    Axis(Axis, Option<&'a PublicationDirectoryIdentity>),
    Device(&'a DynamicsProfile),
    HandyStroke,
    #[cfg(unix)]
    Stash {
        capture: &'a StashSourceCapture,
        candidate: &'a CandidateId,
        job: &'a JobId,
        archived_origin: Option<&'a PackageObjectRef>,
        publisher: &'a crate::stash_files::Publication,
    },
}

pub struct Engine {
    shutdown_requested: Arc<AtomicBool>,
    background: crate::task_group::TaskGroup,
    handy_controller_monitor: Mutex<Option<handy_playback_preparation::ControllerMonitor>>,
    handy_execution: Mutex<Option<handy_playback_preparation::ExecutionPump>>,
    handy_hsp_execution: Mutex<Option<crate::handy_hsp_execution::HspExecutionPump>>,
    handy_hsp_preparations: Arc<Mutex<handy_hsp_preparation::Registry>>,
    handy_hsp_live: Mutex<handy_hsp_live::Registry>,
    handy_reconciliation_admission: Arc<RwLock<()>>,
    handy_reconciliations: Mutex<handy_reconciliation::Registry>,
    pub(crate) config: EngineConfig,
    db: Arc<Mutex<Connection>>,
    token: String,
    pool: ResourcePool,
    #[cfg(target_os = "linux")]
    worker_fence: Arc<crate::worker_fence::WorkerFence>,
    #[cfg(target_os = "linux")]
    native_image_reuse: crate::native_capture::LiveNativeImageReuse,
    #[cfg(target_os = "linux")]
    native_retention_slots: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(target_os = "linux")]
    native_preparations: Mutex<native_preparations::Registry>,
    #[cfg(target_os = "linux")]
    cuda_owner_policy: Option<crate::cuda_owner_policy::LoadedCudaOwnerPolicy>,
    #[cfg(target_os = "linux")]
    cpu_owner_policy: Option<crate::cpu_owner_policy::LoadedCpuOwnerPolicy>,
    tools: std::result::Result<WorkerTools, String>,
    binary_identity: ArtifactIdentity,
    artifact_gate: Mutex<()>,
    workers: Mutex<HashMap<String, Arc<AtomicBool>>>,
    fallback_lifetimes: inference_fallback_lifetime::Registry,
    #[cfg(target_os = "linux")]
    assistant_profiles: Mutex<HashMap<AttemptId, assistant_operations::Profile>>,
    #[cfg(target_os = "linux")]
    assistant_completions: Mutex<HashMap<AttemptId, assistant_operations::Completion>>,
    #[cfg(target_os = "linux")]
    assistant_recovery_running: AtomicBool,
    assistant_targets: Mutex<assistant_targets::Registry>,
    assistant_images: Mutex<assistant_images::Registry>,
    live_captures: Arc<Mutex<live_captures::Registry>>,
    stash_batch_runtime: Mutex<stash_batches::Runtime>,
    library_batch_gate: Mutex<()>,
    motion_store: crate::motion_artifacts::ProgramStore,
    transfers: Mutex<transfers::TransferState>,
    engine_epoch: String,
    import_store: crate::project_package_import_storage::ImportStore,
    package_imports: Mutex<project_package_imports::ImportRuntime>,
    package_owner_token: String,
    stash_owner_token: String,
    handy_owner_token: String,
    handy_sessions: Arc<Mutex<handy_sessions::Registry>>,
    handy_preparations: Arc<Mutex<handy_playback_preparation::Registry>>,
    handy_hosting: Mutex<handy_hosting::Registry>,
    model_pack_owner_token: String,
    _device_effect_journal: Arc<Mutex<crate::device_effect_journal::DeviceEffectJournal>>,
    package_store: crate::project_package_storage::PackageStore,
    package_holds: crate::project_package_storage::HoldRegistry,
    package_exports: Mutex<project_package_exports::PackageRuntime>,
    browser_media: Mutex<browser_media::Registry>,
    browser_timing: Arc<Mutex<browser_timing::Registry>>,
    handy_browser_start_qualifications: Mutex<handy_browser_start_qualification::Registry>,
    handy_hsp_source_qualifications: Mutex<handy_hsp_source_qualification::Registry>,
    #[cfg(test)]
    browser_timing_test_tick: std::sync::atomic::AtomicU64,
}

fn authenticate_session(db: &Connection, session: &SessionId, token: Option<&str>) -> PResult<()> {
    let supplied = token.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::Unauthorized,
            "session authentication token required",
        )
    })?;
    let expected: Option<String> = db
        .query_row(
            "SELECT token_hash FROM sessions WHERE id=?1",
            [session.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    let actual = format!("{:x}", Sha256::digest(supplied.as_bytes()));
    if expected.as_ref().is_none_or(|expected| {
        expected.len() != actual.len()
            || expected
                .bytes()
                .zip(actual.bytes())
                .fold(0u8, |sum, (a, b)| sum | (a ^ b))
                != 0
    }) {
        return Err(ProtocolError::new(
            ErrorCode::Unauthorized,
            "invalid session authentication token",
        ));
    }
    actor(db, session)?;
    Ok(())
}

fn initialize_auxiliary_schema(db: &mut Connection) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    crate::model_pack_selection::initialize(&tx)?;
    crate::job_attempt_history::initialize(&tx)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS stash_sources(project TEXT NOT NULL, version TEXT NOT NULL, scene TEXT NOT NULL, file TEXT NOT NULL, receipt TEXT NOT NULL, PRIMARY KEY(project,version,scene,file), FOREIGN KEY(project,version) REFERENCES sources(project,version));")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS candidate_rebases(candidate TEXT PRIMARY KEY REFERENCES candidates(id),parent TEXT NOT NULL REFERENCES candidates(id));")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS stash_scans(session TEXT NOT NULL, request TEXT NOT NULL, publication TEXT NOT NULL, project TEXT NOT NULL, receipt TEXT NOT NULL, PRIMARY KEY(session,request), UNIQUE(session,publication));")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS stash_tags(session TEXT NOT NULL, request TEXT NOT NULL, project TEXT NOT NULL, scan TEXT NOT NULL, tag_name TEXT NOT NULL, receipt TEXT NOT NULL, PRIMARY KEY(session,request), UNIQUE(session,scan,tag_name));")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS stash_batches(session TEXT NOT NULL, request TEXT NOT NULL, state TEXT NOT NULL, record TEXT NOT NULL, PRIMARY KEY(session,request));")?;
    let has_review = {
        let mut statement = tx.prepare("PRAGMA table_info(candidates)")?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        names.iter().any(|name| name == "review")
    };
    if !has_review {
        tx.execute(
            "ALTER TABLE candidates ADD COLUMN review TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    let has_export_axis = {
        let mut statement = tx.prepare("PRAGMA table_info(exports)")?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        names.iter().any(|name| name == "axis")
    };
    if !has_export_axis {
        tx.execute(
            "ALTER TABLE exports ADD COLUMN axis TEXT NOT NULL DEFAULT 'stroke'",
            [],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Explicit inputs for the shared preview path; authorization remains on Request.
struct PreviewIntent<'a> {
    assistant_image_claim: Option<Reservation>,
    context: &'a FrameContext,
    source_time: SourceTimestamp,
    time_basis: PreviewTimeBasis,
    model_path: Option<&'a Path>,
    model_input: Option<ModelInputContract>,
    inference_profile: Option<DeclaredInferenceProfile>,
    video_view: Option<VideoView>,
    placement_seek: Option<PlacedPreviewRequest>,
    model_pack: Option<ModelPackChoice>,
    annotated: Option<AnnotatedPreviewSpec>,
}
/// Retained dependencies for one attempt; this descriptor owns no admission.
struct OperationDependencies<'a> {
    origin: AttemptOrigin<'a>,
    extra: &'a [ArtifactIdentity],
    #[cfg(target_os = "linux")]
    cuda: Option<&'a cuda_generation::PreparedCudaGeneration>,
    #[cfg(target_os = "linux")]
    cpu: Option<&'a cpu_generation::PreparedCpuGeneration>,
    #[cfg(target_os = "linux")]
    fallback: Option<&'a inference_fallback_execution::PreparedFallback>,
}

#[cfg(all(test, target_os = "linux"))]
struct StartupPhaseGuard {
    phase: &'static str,
    completed: bool,
}
#[cfg(all(test, target_os = "linux"))]
impl StartupPhaseGuard {
    fn begin(phase: &'static str) -> Self {
        tests::captured_cpu_live_native_tests::startup_phase(phase, "begin", None);
        Self {
            phase,
            completed: false,
        }
    }
    fn complete(mut self) {
        self.completed = true;
    }
}
#[cfg(all(test, target_os = "linux"))]
impl Drop for StartupPhaseGuard {
    fn drop(&mut self) {
        tests::captured_cpu_live_native_tests::startup_phase(
            self.phase,
            "end",
            Some(self.completed),
        );
    }
}

impl Engine {
    pub fn open(config: EngineConfig) -> Result<Arc<Self>> {
        Self::open_with_shutdown(config, Arc::new(AtomicBool::new(false)))
    }
    pub(crate) fn open_with_shutdown(
        mut config: EngineConfig,
        shutdown_requested: Arc<AtomicBool>,
    ) -> Result<Arc<Self>> {
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.owner_preflight");
        anyhow::ensure!(
            (1..=1024).contains(&config.max_cpu_threads),
            "invalid owner CPU ceiling; no state opened"
        );
        #[cfg(not(target_os = "linux"))]
        anyhow::ensure!(
            config.assistant_owner_policy.is_none(),
            "assistant owner policy requires Linux confinement"
        );
        #[cfg(target_os = "linux")]
        let assistant_policy = config
            .assistant_owner_policy
            .as_ref()
            .map(|path| crate::assistant_owner_policy::load(path, &config))
            .transpose()?;
        // Report a missing companion before creating any durable engine state.
        let worker_metadata = fs::metadata(&config.worker_executable)
            .context("open configured worker executable; install pulsar-worker beside pulsar or set PULSAR_WORKER_EXECUTABLE")?;
        anyhow::ensure!(
            worker_metadata.is_file() && worker_metadata.len() > 0,
            "configured worker executable must be a nonempty regular file"
        );
        #[cfg(not(target_os = "linux"))]
        if config.cuda_owner_policy.is_some() {
            anyhow::bail!("CUDA owner policy requires Linux; no state opened");
        }
        #[cfg(target_os = "linux")]
        let cuda_owner_policy = config
            .cuda_owner_policy
            .as_ref()
            .map(|path| crate::cuda_owner_policy::LoadedCudaOwnerPolicy::load(path))
            .transpose()?;
        #[cfg(target_os = "linux")]
        if let Some(owner) = &cuda_owner_policy {
            let tools = worker::discover_tools()?;
            owner.policy().match_runtime(tools.onnx_runtime.as_ref())?;
        }
        #[cfg(not(target_os = "linux"))]
        anyhow::ensure!(
            config.cpu_owner_policy.is_none(),
            "captured CPU runtime policy requires Linux; no state opened"
        );
        #[cfg(target_os = "linux")]
        let cpu_owner_policy = config
            .cpu_owner_policy
            .as_ref()
            .map(|path| crate::cpu_owner_policy::LoadedCpuOwnerPolicy::load(path))
            .transpose()?;
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.state_probe");
        crate::portable_fallback_reader::preflight_main_header(&config.state_dir)?;
        artifacts::private_directory(&config.state_dir)?;
        config.state_dir = config.state_dir.canonicalize()?;
        let database_path = config.state_dir.join("projects.sqlite3");
        let fence_binding = if database_path.try_exists()? {
            let probe = Connection::open_with_flags(
                &database_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let version: i64 = probe.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            crate::inference_equivalence_reader::version(&probe)?;
            inference_equivalence_store::validate(&probe)?;
            crate::cpu_runtime_db::validate(&probe)?;
            inference_fallback_store::validate(&probe)?;
            crate::live_cpu_history::validate(&probe)?;
            handy_registrations::validate(&probe)?;
            handy_qualification::validate(&probe)?;
            handy_browser_start_qualification::validate(&probe)?;
            handy_hsp_source_qualification::validate(&probe)?;
            handy_setup_send::validate_journal_links(&probe, &config.state_dir)?;
            handy_playback_preparation::validate_sync_journal_links(&probe, &config.state_dir)?;
            handy_playback_preparation::validate_play_journal_links(&probe, &config.state_dir)?;
            handy_hsp_preparation::validate_journal_links(&probe, &config.state_dir)?;
            handy_hsp_live::validate_history(&probe, &config.state_dir)?;
            handy_reconciliation::validate_journal_links(&probe, &config.state_dir)?;
            library_batches::validate_reader(&probe, &config)?;
            crate::native_preparation_db::validate(&probe)?;
            crate::job_attempt_history::validate(&probe)?;
            wasm_edits::validate(&probe)?;
            assistant_operations::validate(&probe)?;
            anyhow::ensure!(version == 0 || (2..=crate::portable_fallback_reader::MAX_STORAGE_VERSION).contains(&version),"project database schema requires an explicit recoverable migration; original was not modified");
            crate::portable_annotated_db::validate_metadata(&probe)?;
            stash_batches::validate_annotated_reader(&probe)?;
            crate::annotated_generation_db::validate(
                &probe,
                &ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                    .with_cpu_limit(config.max_cpu_threads)?,
            )?;
            crate::live_cuda_evidence_db::validate(
                &probe,
                version,
                &ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                    .with_cpu_limit(config.max_cpu_threads)?,
            )?;
            let version = crate::live_cuda_evidence_db::base_version(&probe, version)?;
            crate::portable_cuda_db::validate_metadata(&probe, version)?;
            crate::worker_fence_db::binding(&probe, version)?
        } else {
            None
        };
        #[cfg(not(target_os = "linux"))]
        anyhow::ensure!(
            fence_binding.is_none(),
            "Linux worker ownership state requires explicit platform migration"
        );
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.fence_open");
        #[cfg(target_os = "linux")]
        let worker_fence = Arc::new(crate::worker_fence::WorkerFence::open_bound_required(
            &config.state_dir,
            fence_binding.as_ref().map(|b| b.scope.as_str()),
            fence_binding
                .as_ref()
                .map_or(2, |b| b.minimum_ledger_version),
        )?);
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.worker_snapshot");
        for sub in [
            "snapshots",
            "attempts",
            "motion-objects",
            "transfer-staging",
            "edit-inputs",
            "edit-receipts",
        ] {
            artifacts::private_directory(&config.state_dir.join(sub))?;
        }
        let (binary_path, binary_hash, binary_size) = artifacts::snapshot_worker(
            &config.worker_executable,
            &config.state_dir.join("snapshots"),
            config.max_snapshot_bytes,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o500))?;
        }
        config.worker_executable = binary_path;
        let binary_identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(binary_hash.clone())?,
            sha256: binary_hash,
            byte_len: binary_size,
        };
        #[cfg(target_os = "linux")]
        if let Some(policy) = &assistant_policy {
            policy.match_worker(&binary_identity)?;
        }
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.capability_files");
        let token_path = config.bootstrap_path();
        let token = if token_path.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let meta = fs::symlink_metadata(&token_path)?;
                anyhow::ensure!(
                    !meta.file_type().is_symlink()
                        && meta.uid() == unsafe { libc::geteuid() }
                        && meta.mode() & 0o077 == 0,
                    "pairing token requires current-user ownership and mode 0600"
                );
            }
            let token = fs::read_to_string(&token_path)?;
            anyhow::ensure!(
                token.len() >= 64 && token.len() <= 256,
                "invalid bootstrap token"
            );
            token
        } else {
            let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&token_path)?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
            artifacts::sync_dir(&config.state_dir)?;
            token
        };
        let pool = ResourcePool::new(config.max_ram_bytes, config.max_jobs)
            .with_cpu_limit(config.max_cpu_threads)?;
        #[cfg(target_os = "linux")]
        let pool = if let Some(owner) = &cuda_owner_policy {
            ResourcePool::new_with_gpu_devices(
                config.max_ram_bytes,
                config.max_jobs,
                vec![owner.policy().accounting.clone()],
            )?
            .with_cpu_limit(config.max_cpu_threads)?
        } else {
            pool
        };
        let motion_store =
            crate::motion_artifacts::ProgramStore::new(&config.state_dir.join("motion-objects"))?;
        let package_owner_token = project_package_exports::owner_token(&config.state_dir)?;
        let stash_owner_token =
            project_package_exports::local_owner_token(&config.state_dir, "stash-owner.token")?;
        let handy_owner_token =
            project_package_exports::local_owner_token(&config.state_dir, "handy-owner.token")?;
        let model_pack_owner_token = project_package_exports::local_owner_token(
            &config.state_dir,
            "model-pack-owner.token",
        )?;
        let engine_epoch = Uuid::new_v4().to_string();
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.device_journal");
        let device_effect_journal = crate::device_effect_journal::DeviceEffectJournal::open(
            &config.state_dir,
            DeviceSessionId::new(engine_epoch.clone())?,
        )?;
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.storage_cleanup");
        let package_store = crate::project_package_storage::PackageStore::new(
            &config.state_dir.join("project-packages"),
        )?;
        package_store.cleanup_pending()?;
        let package_holds = crate::project_package_storage::HoldRegistry::default();
        let import_store = crate::project_package_import_storage::ImportStore::new(
            &config.state_dir,
            package_holds.clone(),
        )?;
        import_store.cleanup_staging()?;
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.layout_probe");
        if database_path.try_exists()? {
            // Reject unknown schema/layout through a read-only connection before
            // writable SQLite startup can checkpoint a previously committed WAL.
            let probe = Connection::open_with_flags(
                &database_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let version: i64 = probe.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            crate::inference_equivalence_reader::version(&probe)?;
            inference_equivalence_store::validate(&probe)?;
            crate::cpu_runtime_db::validate(&probe)?;
            inference_fallback_store::validate(&probe)?;
            crate::live_cpu_history::validate(&probe)?;
            handy_registrations::validate(&probe)?;
            handy_qualification::validate(&probe)?;
            handy_browser_start_qualification::validate(&probe)?;
            handy_hsp_source_qualification::validate(&probe)?;
            anyhow::ensure!(version == 0 || (2..=crate::portable_fallback_reader::MAX_STORAGE_VERSION).contains(&version),"project database schema requires an explicit recoverable migration; original was not modified");
            library_batches::validate_reader(&probe, &config)?;
            crate::native_preparation_db::validate(&probe)?;
            crate::job_attempt_history::validate(&probe)?;
            crate::portable_annotated_db::validate_metadata(&probe)?;
            crate::annotated_generation_db::validate(
                &probe,
                &ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                    .with_cpu_limit(config.max_cpu_threads)?,
            )?;
            crate::live_cuda_evidence_db::validate(
                &probe,
                version,
                &ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                    .with_cpu_limit(config.max_cpu_threads)?,
            )?;
            let version = crate::live_cuda_evidence_db::base_version(&probe, version)?;
            crate::portable_cuda_db::validate_metadata(&probe, version)?;
            crate::cuda_attempts::validate(&probe, version)?;
            anyhow::ensure!(
                crate::worker_fence_db::binding(&probe, version)? == fence_binding,
                "worker fence binding changed during startup"
            );
            crate::cuda_acceptance::validate(&probe, version)?;
            crate::portable_cuda_db::validate_archived_bytes(
                &probe,
                version,
                &import_store,
                &ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                    .with_cpu_limit(config.max_cpu_threads)?,
            )?;
            if version >= 6 {
                motion_retime_receipts::validate_layout(&probe)?;
            }
            if version >= 7 {
                review_interval_receipts::validate_layout(&probe)?;
            }
            if version >= 9 {
                imported_candidate_intervals::validate_layout(&probe)?;
            }
            if version >= 12 {
                crate::model_pack_selection::validate_layout(&probe)?;
            }
            if version >= 13 {
                stash_batches::validate_pack_layout(&probe)?;
            }
        }
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.database_open");
        let mut db = Connection::open(database_path)?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA wal_autocheckpoint=100; PRAGMA journal_size_limit=8388608;")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        crate::inference_equivalence_reader::version(&db)?;
        inference_equivalence_store::validate(&db)?;
        crate::cpu_runtime_db::validate(&db)?;
        inference_fallback_store::validate(&db)?;
        crate::live_cpu_history::validate(&db)?;
        anyhow::ensure!(version == 0 || (2..=crate::portable_fallback_reader::MAX_STORAGE_VERSION).contains(&version), "project database schema requires an explicit recoverable migration; original was not modified");
        library_batches::validate_reader(&db, &config)?;
        crate::native_preparation_db::validate(&db)?;
        crate::job_attempt_history::validate(&db)?;
        crate::portable_annotated_db::validate_metadata(&db)?;
        crate::annotated_generation_db::validate(
            &db,
            &ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                .with_cpu_limit(config.max_cpu_threads)?,
        )?;
        crate::live_cuda_evidence_db::validate(
            &db,
            version,
            &ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                .with_cpu_limit(config.max_cpu_threads)?,
        )?;
        let version = crate::live_cuda_evidence_db::base_version(&db, version)?;
        crate::portable_cuda_db::validate_metadata(&db, version)?;
        crate::cuda_attempts::validate(&db, version)?;
        anyhow::ensure!(
            crate::worker_fence_db::binding(&db, version)? == fence_binding,
            "worker fence binding changed during startup"
        );
        crate::cuda_acceptance::validate(&db, version)?;
        if version >= 6 {
            motion_retime_receipts::validate_layout(&db)?;
        }
        if version >= 7 {
            review_interval_receipts::validate_layout(&db)?;
        }
        if version >= 9 {
            imported_candidate_intervals::validate_layout(&db)?;
        }
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.database_schema_recovery");
        if version == 0 {
            let tx = db.transaction()?;
            tx.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY, actor TEXT NOT NULL, label TEXT NOT NULL, token_hash TEXT NOT NULL);
                CREATE TABLE projects(id TEXT PRIMARY KEY, name TEXT NOT NULL, revision INTEGER NOT NULL, program TEXT NOT NULL, history_cursor INTEGER NOT NULL DEFAULT 0, protected TEXT NOT NULL DEFAULT '[]');
                CREATE TABLE grants(session TEXT NOT NULL REFERENCES sessions(id), project TEXT NOT NULL REFERENCES projects(id), scope TEXT NOT NULL, PRIMARY KEY(session,project,scope));
                CREATE TABLE revisions(project TEXT NOT NULL REFERENCES projects(id), revision INTEGER NOT NULL, actor TEXT NOT NULL, kind TEXT NOT NULL, label TEXT NOT NULL, program TEXT NOT NULL, protected TEXT NOT NULL DEFAULT '[]', PRIMARY KEY(project,revision));
                CREATE TABLE edit_states(project TEXT NOT NULL REFERENCES projects(id), position INTEGER NOT NULL, program TEXT NOT NULL, protected TEXT NOT NULL DEFAULT '[]', PRIMARY KEY(project,position));
                CREATE TABLE requests(session TEXT NOT NULL, request TEXT NOT NULL, fingerprint TEXT NOT NULL, response TEXT NOT NULL, PRIMARY KEY(session,request));
                CREATE TABLE sources(project TEXT NOT NULL REFERENCES projects(id), version TEXT NOT NULL, path TEXT NOT NULL, original TEXT NOT NULL, label TEXT NOT NULL, sha256 TEXT NOT NULL, bytes INTEGER NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, evicted INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(project,version));
                CREATE TABLE jobs(id TEXT PRIMARY KEY, attempt TEXT NOT NULL, project TEXT NOT NULL REFERENCES projects(id), base_revision INTEGER NOT NULL, session TEXT NOT NULL REFERENCES sessions(id), source TEXT NOT NULL, state TEXT NOT NULL, manifest TEXT NOT NULL, candidate TEXT, error TEXT, attempts INTEGER NOT NULL DEFAULT 1);
                CREATE TABLE attempts(id TEXT PRIMARY KEY, job TEXT NOT NULL REFERENCES jobs(id), manifest TEXT NOT NULL, state TEXT NOT NULL);
                CREATE TABLE candidates(id TEXT PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id), base_revision INTEGER NOT NULL, program TEXT NOT NULL, job TEXT, lineage TEXT NOT NULL, committed_revision INTEGER);
                CREATE TABLE events(cursor INTEGER PRIMARY KEY AUTOINCREMENT, project TEXT NOT NULL REFERENCES projects(id), revision INTEGER NOT NULL, body TEXT NOT NULL);
                CREATE TABLE exports(session TEXT NOT NULL, request TEXT NOT NULL, project TEXT NOT NULL, revision INTEGER NOT NULL, path TEXT NOT NULL, sha256 TEXT NOT NULL, state TEXT NOT NULL, PRIMARY KEY(session,request));
                ")?;
            crate::inference_equivalence_reader::publish(&tx, 2)?;
            tx.commit()?;
        }
        source_placements::initialize(&mut db, version)?;
        initialize_auxiliary_schema(&mut db)?;
        {
            let _migration = pool.reserve_memory(transfers::VALIDATION_MEMORY)?;
            motion_state::initialize(&mut db)?;
        }
        crate::native_preparation_db::recover(&mut db)?;
        project_package_imports::initialize(&db)?;
        imported_candidate_intervals::recover(&mut db, &pool, &import_store)?;
        lineage::initialize(&mut db)?;
        transfers::initialize(&db)?;
        wasm_edits::initialize(&mut db)?;
        assistant_operations::initialize(&mut db)?;
        source_kinds::initialize(&mut db)?;
        project_package_exports::initialize(&db)?;
        {
            let tx = db.transaction()?;
            inference_fallback_store::retire_pending_after_restart(&tx)?;
            tx.commit()?;
        }
        db.execute("UPDATE jobs SET state='interrupted',error='engine restart; validated dependency restart required' WHERE state IN ('queued','running')", [])?;
        db.execute(
            "UPDATE attempts SET state='interrupted' WHERE state IN ('queued','running')",
            [],
        )?;
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.tool_discovery");
        let tools = worker::discover_tools()
            .and_then(|mut tools| {
                #[cfg(all(test, target_os = "linux"))]
                phase.complete();
                #[cfg(all(test, target_os = "linux"))]
                let phase = StartupPhaseGuard::begin("open.runtime_snapshot");
                if let Some(runtime) = &mut tools.onnx_runtime {
                    let (path, hash, bytes) = artifacts::snapshot(
                        &runtime.path,
                        &config.state_dir.join("snapshots"),
                        config.max_snapshot_bytes,
                    )?;
                    runtime.path = path;
                    runtime.identity = ArtifactIdentity {
                        artifact_id: ArtifactId::new(hash.clone())?,
                        sha256: hash,
                        byte_len: bytes,
                    };
                }
                #[cfg(all(test, target_os = "linux"))]
                phase.complete();
                Ok(tools)
            })
            .map_err(|e| e.to_string());
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.runtime_policy_and_assembly");
        #[cfg(target_os = "linux")]
        if let Some(owner) = &cuda_owner_policy {
            let current = tools
                .as_ref()
                .map_err(|e| anyhow::anyhow!("CUDA owner runtime discovery failed: {e}"))?;
            owner
                .policy()
                .match_runtime(current.onnx_runtime.as_ref())?;
        }
        let engine = Arc::new(Self {
            shutdown_requested,
            background: crate::task_group::TaskGroup::default(),
            config,
            db: Arc::new(Mutex::new(db)),
            token,
            pool,
            #[cfg(target_os = "linux")]
            cuda_owner_policy,
            #[cfg(target_os = "linux")]
            cpu_owner_policy,
            #[cfg(target_os = "linux")]
            worker_fence,
            #[cfg(target_os = "linux")]
            native_image_reuse: crate::native_capture::LiveNativeImageReuse::default(),
            #[cfg(target_os = "linux")]
            native_retention_slots: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            #[cfg(target_os = "linux")]
            native_preparations: Mutex::new(native_preparations::Registry::default()),
            tools,
            binary_identity,
            artifact_gate: Mutex::new(()),
            workers: Mutex::new(HashMap::new()),
            fallback_lifetimes: inference_fallback_lifetime::Registry::default(),
            #[cfg(target_os = "linux")]
            assistant_profiles: Mutex::new(HashMap::new()),
            #[cfg(target_os = "linux")]
            assistant_completions: Mutex::new(HashMap::new()),
            #[cfg(target_os = "linux")]
            assistant_recovery_running: AtomicBool::new(false),
            assistant_targets: Mutex::new(assistant_targets::Registry::default()),
            assistant_images: Mutex::new(assistant_images::Registry::default()),
            live_captures: Arc::new(Mutex::new(live_captures::Registry::default())),
            stash_batch_runtime: Mutex::new(stash_batches::Runtime::default()),
            library_batch_gate: Mutex::new(()),
            motion_store,
            transfers: Mutex::new(transfers::TransferState::default()),
            engine_epoch,
            package_owner_token,
            stash_owner_token,
            handy_owner_token,
            handy_sessions: Arc::new(Mutex::new(handy_sessions::Registry::default())),
            handy_preparations: Arc::new(Mutex::new(
                handy_playback_preparation::Registry::default(),
            )),
            handy_controller_monitor: Mutex::new(None),
            handy_execution: Mutex::new(None),
            handy_hsp_execution: Mutex::new(None),
            handy_hsp_live: Mutex::new(handy_hsp_live::Registry::default()),
            handy_hsp_preparations: Arc::new(
                Mutex::new(handy_hsp_preparation::Registry::default()),
            ),
            handy_reconciliation_admission: Arc::new(RwLock::new(())),
            handy_reconciliations: Mutex::new(handy_reconciliation::Registry::default()),
            handy_hosting: Mutex::new(handy_hosting::Registry::default()),
            model_pack_owner_token,
            _device_effect_journal: Arc::new(Mutex::new(device_effect_journal)),
            package_store,
            package_holds,
            import_store,
            package_imports: Mutex::new(project_package_imports::ImportRuntime::default()),
            package_exports: Mutex::new(project_package_exports::PackageRuntime::default()),
            browser_media: Mutex::new(browser_media::Registry::default()),
            browser_timing: Arc::new(Mutex::new(browser_timing::Registry::default())),
            handy_hsp_source_qualifications: Mutex::new(
                handy_hsp_source_qualification::Registry::default(),
            ),
            handy_browser_start_qualifications: Mutex::new(
                handy_browser_start_qualification::Registry::default(),
            ),
            #[cfg(test)]
            browser_timing_test_tick: std::sync::atomic::AtomicU64::new(0),
        });
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        #[cfg(all(test, target_os = "linux"))]
        let phase = StartupPhaseGuard::begin("open.background_recovery");
        let startup = (|| -> Result<()> {
            #[cfg(target_os = "linux")]
            if let Some(policy) = assistant_policy {
                policy.register(&engine)?;
            }
            browser_media::start_sweeper(&engine)?;
            transfers::start_sweeper(&engine)?;
            project_package_exports::start_sweeper(&engine)?;
            project_package_imports::start_sweeper(&engine)?;
            engine.recover_interrupted();
            engine.recover_stash_batches()?;
            Ok(())
        })();
        if let Err(error) = startup {
            if let Err(cleanup) = engine.finish_shutdown() {
                return Err(
                    error.context(format!("Engine startup retirement also failed: {cleanup}"))
                );
            }
            return Err(error);
        }
        #[cfg(all(test, target_os = "linux"))]
        phase.complete();
        Ok(engine)
    }

    pub fn handle(self: &Arc<Self>, request: Request) -> Response {
        if self.is_shutting_down() {
            return Response::failure(
                request.request_id,
                ProtocolError::new(ErrorCode::Unavailable, "Engine is shutting down"),
            );
        }
        if let Some(result) = self.browser_timing_fast(&request) {
            return match result {
                Ok(body) => Response::success(request.request_id, body),
                Err(error) => Response::failure(request.request_id, error),
            };
        }
        if let Err(error) = self.reserve_handy_browser_start_owner(&request) {
            return Response::failure(request.request_id, error);
        }
        if let Some(result) = self.handy_browser_owner_fast(&request) {
            return match result {
                Ok(body) => Response::success(request.request_id, body),
                Err(error) => Response::failure(request.request_id, error),
            };
        }
        if let Some(result) = self.handy_player_guard_fast(&request) {
            return match result {
                Ok(body) => Response::success(request.request_id, body),
                Err(error) => Response::failure(request.request_id, error),
            };
        }
        let _ = self.sweep_live_captures();
        let result = self.dispatch(&request);
        self.sweep_browser_timing();
        let _ = self.sweep_live_captures();
        match result {
            Ok(body) => Response::success(request.request_id, body),
            Err(error) => Response::failure(request.request_id, error),
        }
    }

    fn dispatch(self: &Arc<Self>, request: &Request) -> PResult<ResponseBody> {
        request.validate()?;
        // Cover the complete synchronous producer, including bounded setup/sync
        // I/O. Release, historical status, cancellation and stop cleanup remain
        // independent; exact registry retirement fences already queued play.
        let _handy_admission = self.handy_reconciliation_admission(&request.command)?;
        self.transfers.lock().map_err(internal)?.sweep();
        if let Command::Pair {
            client_name,
            pairing_token,
        } = &request.command
        {
            return self.pair(request, client_name, pairing_token);
        }
        let session = request
            .session
            .as_ref()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing is required"))?;
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(request).map_err(internal)?)
        );
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            check_request_identity(&db, session.as_str(), request, &fingerprint)?;
            if request.command.is_mutating()
                && !request.command.is_native_preparation()
                && !live_captures::is_command(&request.command)
                && !browser_timing::is_history_command(&request.command)
                && !browser_media::is_command(&request.command)
                && !project_package_exports::is_command(&request.command)
                && !project_package_imports::is_command(&request.command)
                && !handy_registrations::is_command(&request.command)
                && !handy_sessions::is_command(&request.command)
                && !handy_hosting::is_command(&request.command)
                && !handy_qualification::is_command(&request.command)
                && !handy_browser_start_qualification::is_command(&request.command)
                && !handy_hsp_source_qualification::is_command(&request.command)
                && !inference_equivalence_control::is_command(&request.command)
                && !handy_reconciliation::is_command(&request.command)
                && !handy_hsp_preparation::is_command(&request.command)
                && !handy_hsp_live::is_command(&request.command)
                && !matches!(
                    request.command,
                    Command::SetupHandyDevice { .. }
                        | Command::CancelHandySetup { .. }
                        | Command::PrepareHandyPlayback { .. }
                        | Command::RetireHandyPlaybackPreparation { .. }
                        | Command::AcquireHandyPlaybackClock { .. }
                        | Command::RefreshHandyPlaybackTiming { .. }
                        | Command::BindHandyPlayerGuard { .. }
                        | Command::SynchronizeHandyClock { .. }
                        | Command::CancelHandyClockSync { .. }
                        | Command::StartHandyPlayback { .. }
                        | Command::StartHandyPlaybackFromBrowser { .. }
                        | Command::RefreshHandyBrowserPlaybackTiming { .. }
                        | Command::CancelHandyPlayback { .. }
                        | Command::InspectHandyDeviceTiming { .. }
                        | Command::SampleHandyServerClock { .. }
                        | Command::AcquireHandyController { .. }
                        | Command::RenewHandyController { .. }
                        | Command::ReleaseHandyController { .. }
                )
            {
                if let Some(body) = replay(&db, session.as_str(), request, &fingerprint)? {
                    return Ok(body);
                }
            }
            if matches!(&request.command, Command::StashImportSource { .. }) {
                check_revision(&db, request)?;
                check_stash_source_binding(&db, request)?;
            }
        }
        if browser_timing::is_history_command(&request.command) {
            return self.browser_timing_control(request, session, &fingerprint);
        }
        if browser_media::is_command(&request.command) {
            return self.browser_media_control(request, session, &fingerprint);
        }
        if matches!(request.command, Command::InspectInferenceEnvironment { .. }) {
            return self.inspect_inference_environment(request, session);
        }
        if inference_equivalence_control::is_command(&request.command) {
            return self.inference_equivalence_control(request, session, &fingerprint);
        }
        if handy_hsp_source_qualification::is_command(&request.command) {
            return self.handy_hsp_source_qualification_control(request, session, &fingerprint);
        }
        if handy_browser_start_qualification::is_command(&request.command) {
            return self.handy_browser_start_qualification_control(request, session, &fingerprint);
        }
        if handy_reconciliation::is_command(&request.command) {
            return self.handy_reconciliation_control(request, session, &fingerprint);
        }
        if handy_hsp_live::is_command(&request.command) {
            return self.handy_hsp_live_control(request, session, &fingerprint);
        }
        if handy_hsp_preparation::is_command(&request.command) {
            return self.handy_hsp_preparation_control(request, session, &fingerprint);
        }
        if matches!(
            request.command,
            Command::StartHandyPlayback { .. }
                | Command::StartHandyPlaybackFromBrowser { .. }
                | Command::HandyPlaybackStatus { .. }
                | Command::CancelHandyPlayback { .. }
        ) {
            return self.handy_playback_control(request, session, &fingerprint);
        }
        if matches!(request.command, Command::HandyStopStatus { .. }) {
            return self.handy_stop_status(request, session);
        }
        if matches!(request.command, Command::HandyHspStatus { .. }) {
            return self.handy_hsp_status(request, session);
        }
        if matches!(
            request.command,
            Command::BindHandyPlayerGuard { .. } | Command::HandyPlayerGuardStatus { .. }
        ) {
            return self.handy_player_guard_control(request, session, &fingerprint);
        }
        if matches!(
            request.command,
            Command::RefreshHandyPlaybackTiming { .. }
                | Command::HandyContinuousTimingStatus { .. }
        ) {
            return self.handy_continuous_timing_control(request, session, &fingerprint);
        }
        if matches!(
            request.command,
            Command::RefreshHandyBrowserPlaybackTiming { .. }
        ) {
            return self.handy_browser_timing_control(request, session, &fingerprint);
        }
        if matches!(request.command, Command::AcquireHandyPlaybackClock { .. }) {
            return self.handy_playback_clock_control(request, session, &fingerprint);
        }
        if matches!(
            request.command,
            Command::SynchronizeHandyClock { .. }
                | Command::HandyClockSyncStatus { .. }
                | Command::CancelHandyClockSync { .. }
        ) {
            return self.handy_clock_sync_control(request, session, &fingerprint);
        }
        if matches!(request.command, Command::InspectHandyDeviceTiming { .. }) {
            return self.handy_device_timing_control(request, session);
        }
        if matches!(request.command, Command::SampleHandyServerClock { .. }) {
            return self.handy_server_clock_control(request, session, &fingerprint);
        }
        if matches!(
            request.command,
            Command::AcquireHandyController { .. }
                | Command::RenewHandyController { .. }
                | Command::ReleaseHandyController { .. }
                | Command::HandyControllerStatus { .. }
        ) {
            return self.handy_controller_control(request, session, &fingerprint);
        }
        if matches!(
            request.command,
            Command::PrepareHandyPlayback { .. }
                | Command::RetireHandyPlaybackPreparation { .. }
                | Command::HandyPlaybackPreparationStatus { .. }
        ) {
            return self.handy_preparation_control(request, session, &fingerprint);
        }
        if matches!(request.command, Command::SetupHandyDevice { .. }) {
            return self.handy_setup_send(request, session, &fingerprint);
        }
        if matches!(request.command, Command::CancelHandySetup { .. }) {
            return self.handy_setup_cancel(request, session, &fingerprint);
        }
        if matches!(request.command, Command::HandySetupStatus { .. }) {
            return self.handy_setup_status(request, session);
        }
        if matches!(request.command, Command::InspectHandyProgram { .. }) {
            return self.handy_program_inspection(request, session);
        }
        if matches!(request.command, Command::InspectHandySetup { .. }) {
            return self.handy_setup_inspection(request, session);
        }
        if handy_qualification::is_command(&request.command) {
            return self.handy_qualification_control(request, session, &fingerprint);
        }
        if handy_hosting::is_command(&request.command) {
            return self.handy_hosting_control(request, session, &fingerprint);
        }
        if handy_sessions::is_command(&request.command) {
            return self.handy_session_control(request, session, &fingerprint);
        }
        if handy_registrations::is_command(&request.command) {
            return self.handy_registration_control(request, session, &fingerprint);
        }
        if assistant_operations::is_command(&request.command) {
            return self.assistant_control(request, &fingerprint);
        }
        if request.command.is_native_preparation() {
            #[cfg(target_os = "linux")]
            return self.native_preparation_command(request, session, &fingerprint);
            #[cfg(not(target_os = "linux"))]
            return Err(ProtocolError::unsupported(
                "native preparation requires Linux",
            ));
        }
        if live_captures::is_command(&request.command) {
            return self.live_capture_command(request, session, &fingerprint);
        }
        if matches!(
            request.command,
            Command::WasmEditorInfo
                | Command::StartWasmEdit { .. }
                | Command::ResolveWasmStart { .. }
                | Command::WasmEditStatus { .. }
                | Command::CancelWasmEdit { .. }
                | Command::ListWasmEdits
                | Command::ListArchivedWasmEdits { .. }
                | Command::RemoveUnsuccessfulWasmEdit { .. }
                | Command::ArchiveCompletedWasmEdit { .. }
        ) {
            return self.wasm_edit_control(request, &fingerprint);
        }
        if matches!(request.command, Command::WithdrawModelPackApproval { .. }) {
            return self.model_pack_withdrawal_command(request, session, &fingerprint);
        }
        if matches!(
            &request.command,
            Command::SelectCatalogModelPack { .. }
                | Command::SelectModelPack { .. }
                | Command::ModelPackSelectionStatus { .. }
        ) {
            return self.model_pack_selection_command(request, session, &fingerprint);
        }
        if let Command::CheckModelPackCatalog {
            bootstrap_sha256,
            manifest_sha256,
            expected_catalog_revision,
            ..
        } = &request.command
        {
            let _gate = self.artifact_gate.lock().map_err(internal)?;
            let _reservation = self.pool.reserve_memory(256 * 1024 * 1024)?;
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            let report = crate::model_packs::catalog_pack_check(
                &self.config.state_dir,
                bootstrap_sha256,
                manifest_sha256,
                *expected_catalog_revision,
            )
            .map_err(internal)?;
            self.authorize(&db, request, session)?;
            return Ok(ResponseBody::ModelPackCatalog((report).into()));
        }
        if matches!(
            &request.command,
            Command::RefreshModelCatalog { .. } | Command::ModelCatalogStatus { .. }
        ) {
            let _gate = self.artifact_gate.lock().map_err(internal)?;
            let _reservation = self.pool.reserve_memory(256 * 1024 * 1024)?;
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            if request.command.is_mutating() {
                if let Some(body) = replay(&db, session.as_str(), request, &fingerprint)? {
                    return Ok(body);
                }
            }
            if request.command.is_mutating() {
                let tx = db.transaction().map_err(internal)?;
                claim_request_identity(&tx, session.as_str(), request, &fingerprint)?;
                tx.commit().map_err(internal)?;
            }
            let report = crate::model_packs::catalog_command(
                &self.config.state_dir,
                request,
                session,
                &fingerprint,
            )
            .map_err(internal)?;
            self.authorize(&db, request, session)?;
            let body = ResponseBody::ModelCatalog(report);
            if request.command.is_mutating() {
                let tx = db.transaction().map_err(internal)?;
                remember(&tx, session.as_str(), request, &fingerprint, &body)?;
                tx.commit().map_err(internal)?;
            }
            return Ok(body);
        }
        if matches!(
            &request.command,
            Command::ImportModelPack { .. }
                | Command::ModelPackStatus { .. }
                | Command::ModelPackStagingStatus { .. }
                | Command::DiscardModelPackStage { .. }
        ) {
            let _gate = self.artifact_gate.lock().map_err(internal)?;
            let _reservation = self.pool.reserve_memory(4 * 1024 * 1024)?;
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            if request.command.is_mutating() {
                if let Some(body) = replay(&db, session.as_str(), request, &fingerprint)? {
                    return Ok(body);
                }
            }
            if matches!(&request.command, Command::DiscardModelPackStage { .. }) {
                let tx = db.transaction().map_err(internal)?;
                claim_request_identity(&tx, session.as_str(), request, &fingerprint)?;
                tx.commit().map_err(internal)?;
            }
            let body = if let Command::DiscardModelPackStage {
                basename,
                manifest_sha256,
                ..
            } = &request.command
            {
                let key = format!(
                    "{:x}",
                    Sha256::digest(
                        format!("{}\0{}", session.as_str(), request.request_id.as_str()).as_bytes()
                    )
                );
                ResponseBody::ModelPackDiscard(
                    crate::model_packs::discard_stage(
                        &self.config.state_dir,
                        basename,
                        manifest_sha256,
                        &key,
                    )
                    .map_err(internal)?,
                )
            } else if matches!(&request.command, Command::ModelPackStagingStatus { .. }) {
                ResponseBody::ModelPackStaging(
                    crate::model_packs::staging_status(&self.config.state_dir).map_err(internal)?,
                )
            } else {
                ResponseBody::ModelPack(match &request.command {
                    Command::ImportModelPack { source, .. } => {
                        crate::model_packs::install(&self.config.state_dir, source)
                            .map_err(internal)?
                    }
                    Command::ModelPackStatus {
                        manifest_sha256, ..
                    } => crate::model_packs::status(&self.config.state_dir, manifest_sha256)
                        .map_err(internal)?,
                    _ => unreachable!(),
                })
            };
            self.authorize(&db, request, session)?;
            if request.command.is_mutating() {
                let tx = db.transaction().map_err(internal)?;
                remember(&tx, session.as_str(), request, &fingerprint, &body)?;
                tx.commit().map_err(internal)?;
            }
            return Ok(body);
        }
        if let Command::HandyInspect { inspection, .. } = &request.command {
            let _reservation = self.pool.reserve_memory(1024 * 1024)?;
            let report = crate::handy_online::inspect(inspection)?;
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            return Ok(ResponseBody::HandyInspection(report));
        }
        if project_package_imports::is_command(&request.command) {
            return self.import_control(request, &fingerprint);
        }
        if matches!(
            request.command,
            Command::LibraryBatchMigrate { .. }
                | Command::LibraryBatchPrepare { .. }
                | Command::LibraryBatchStatus { .. }
                | Command::LibraryBatchList
                | Command::LibraryBatchAdvance { .. }
                | Command::LibraryBatchRelease { .. }
                | Command::LibraryBatchCancel { .. }
        ) {
            return self.library_batch_control(request, &fingerprint);
        }
        if matches!(
            &request.command,
            Command::StashStartBatch { .. }
                | Command::StashBatchStatus { .. }
                | Command::StashResumeBatch { .. }
                | Command::StashCancelBatch { .. }
                | Command::StashMigrateBatch { .. }
        ) {
            return self.stash_batch_control(request, &fingerprint);
        }
        if matches!(
            &request.command,
            Command::StashAddTag { .. }
                | Command::StashTagStatus { .. }
                | Command::StashResumeTag { .. }
        ) {
            return self.stash_tag_control(request, &fingerprint);
        }
        if matches!(
            &request.command,
            Command::StashSubmitScan { .. }
                | Command::StashScanStatus { .. }
                | Command::StashReconcileAttachment { .. }
        ) {
            return self.stash_scan_control(request, &fingerprint);
        }
        if let Command::StashPlanMissing {
            connection,
            limit,
            mappings,
            ..
        } = &request.command
        {
            let _reservation = self.pool.reserve_memory(16 * 1024 * 1024)?;
            let discovery = crate::stash::discover(connection, *limit)?;
            return crate::stash_files::plan(discovery, mappings).map(ResponseBody::StashPlan);
        }
        if let Command::StashListMissing {
            connection, limit, ..
        } = &request.command
        {
            let _reservation = self.pool.reserve_memory(16 * 1024 * 1024)?;
            return crate::stash::discover(connection, *limit).map(ResponseBody::StashDiscovery);
        }
        if project_package_exports::is_command(&request.command) {
            return self.package_control(request, &fingerprint);
        }
        if matches!(
            &request.command,
            Command::BeginEditUpload { .. }
                | Command::BeginMotionDownload { .. }
                | Command::BeginLiveCudaEvidenceDownload { .. }
                | Command::FinishEditUpload { .. }
                | Command::TransferStatus { .. }
                | Command::AbandonTransfer { .. }
        ) {
            return self.transfer_control(request, &fingerprint);
        }
        if let Command::PreviewAnnotatedFrame {
            context,
            time,
            spec,
            model,
        } = &request.command
        {
            let (source_time, time_basis, placed) = match time {
                ModelPackPreviewTime::Source {
                    source_time,
                    time_basis,
                } => (*source_time, *time_basis, None),
                ModelPackPreviewTime::Placement {
                    placement_id,
                    project_time,
                } => {
                    let placement = {
                        let db = self.db.lock().map_err(internal)?;
                        source_placements::active(&db, project(request)?, placement_id)?
                    };
                    let placed = PlacedPreviewRequest {
                        placement,
                        project_time: *project_time,
                    };
                    placed.validate(context)?;
                    (
                        SourceTimestamp::new(0, 1).expect("constant"),
                        PreviewTimeBasis::ElapsedFromFirstFrame,
                        Some(placed),
                    )
                }
            };
            let (path, input, profile, pack) = match model {
                AnnotatedPreviewModel::File {
                    model_path,
                    profile,
                } => (
                    Some(model_path.as_path()),
                    Some(profile.preprocessing.input.clone()),
                    Some(profile.as_ref().clone()),
                    None,
                ),
                AnnotatedPreviewModel::Pack { choice } => (None, None, None, Some(choice.clone())),
            };
            return self.preview(
                request,
                PreviewIntent {
                    assistant_image_claim: None,
                    context,
                    source_time,
                    time_basis,
                    model_path: path,
                    model_input: input,
                    inference_profile: profile,
                    video_view: None,
                    placement_seek: placed,
                    model_pack: pack,
                    annotated: Some(spec.clone()),
                },
            );
        }
        if let Command::ProbeMedia { source_version } = &request.command {
            return self.probe_media(request, source_version);
        }
        if let Command::PreviewFrame {
            time_basis,
            video_view,
            context,
            source_time,
            model_path,
            model_input,
            inference_profile,
        } = &request.command
        {
            return self.preview(
                request,
                PreviewIntent {
                    assistant_image_claim: None,
                    context,
                    source_time: *source_time,
                    time_basis: *time_basis,
                    model_path: model_path.as_deref(),
                    model_input: model_input.clone(),
                    inference_profile: inference_profile.clone(),
                    video_view: video_view.clone(),
                    placement_seek: None,
                    model_pack: None,
                    annotated: None,
                },
            );
        }
        if let Command::PreviewPlacedFrame {
            placement_id,
            project_time,
            context,
            video_view,
            model_path,
            model_input,
            inference_profile,
        } = &request.command
        {
            let placement = {
                let db = self.db.lock().map_err(internal)?;
                source_placements::active(&db, project(request)?, placement_id)?
            };
            let placed = PlacedPreviewRequest {
                placement,
                project_time: *project_time,
            };
            placed.validate(context)?;
            return self.preview(
                request,
                PreviewIntent {
                    assistant_image_claim: None,
                    context,
                    source_time: SourceTimestamp::new(0, 1).expect("constant"),
                    time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    model_path: model_path.as_deref(),
                    model_input: model_input.clone(),
                    inference_profile: inference_profile.clone(),
                    video_view: video_view.clone(),
                    placement_seek: Some(placed),
                    model_pack: None,
                    annotated: None,
                },
            );
        }
        if let Command::PreviewModelPackFrame {
            context,
            time,
            video_view,
            choice,
        } = &request.command
        {
            let (source_time, time_basis, placed) = match time {
                ModelPackPreviewTime::Source {
                    source_time,
                    time_basis,
                } => (*source_time, *time_basis, None),
                ModelPackPreviewTime::Placement {
                    placement_id,
                    project_time,
                } => {
                    let placement = {
                        let db = self.db.lock().map_err(internal)?;
                        source_placements::active(&db, project(request)?, placement_id)?
                    };
                    let placed = PlacedPreviewRequest {
                        placement,
                        project_time: *project_time,
                    };
                    placed.validate(context)?;
                    (
                        SourceTimestamp::new(0, 1).expect("constant"),
                        PreviewTimeBasis::ElapsedFromFirstFrame,
                        Some(placed),
                    )
                }
            };
            return self.preview(
                request,
                PreviewIntent {
                    assistant_image_claim: None,
                    context,
                    source_time,
                    time_basis,
                    model_path: None,
                    model_input: None,
                    inference_profile: None,
                    video_view: video_view.clone(),
                    placement_seek: placed,
                    model_pack: Some(choice.clone()),
                    annotated: None,
                },
            );
        }
        if let Command::StashPublicationStatus {
            publication_request_id,
            ..
        } = &request.command
        {
            return self.stash_publication_observation(request, publication_request_id);
        }
        if let Command::StashExportScript {
            connection,
            source_version,
            scene_id,
            file_id,
            candidate_id,
            job_id,
            archived_origin,
            ..
        } = &request.command
        {
            #[cfg(not(unix))]
            {
                let _ = (
                    connection,
                    source_version,
                    scene_id,
                    file_id,
                    candidate_id,
                    job_id,
                );
                return Err(ProtocolError::unsupported(
                    "Stash publication requires filesystem descriptor traversal",
                ));
            }
            #[cfg(unix)]
            {
                let _admission = self.pool.reserve_memory(16 * 1024 * 1024)?;
                let capture = {
                    let db = self.db.lock().map_err(internal)?;
                    let dispatched: bool = db
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM exports WHERE session=?1 AND request=?2)",
                            params![
                                request.session.as_ref().expect("authenticated").as_str(),
                                request.request_id.as_str()
                            ],
                            |row| row.get(0),
                        )
                        .map_err(internal)?;
                    if dispatched {
                        return Err(ProtocolError::new(ErrorCode::UnknownOutcome,"previous Stash export outcome is unresolved; inspect and reconcile instead of retrying"));
                    }
                    check_revision(&db, request)?;
                    let raw:Option<String>=db.query_row("SELECT receipt FROM stash_sources WHERE project=?1 AND version=?2 AND scene=?3 AND file=?4",params![project(request)?.as_str(),source_version.as_str(),scene_id,file_id],|row|row.get(0)).optional().map_err(internal)?;
                    decode::<StashSourceCapture>(&raw.ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::NotFound,
                            "Stash captured source association is absent",
                        )
                    })?)?
                };
                if capture.endpoint.as_deref()
                    != Some(crate::stash::endpoint_identity(connection)?.as_str())
                {
                    return Err(ProtocolError::new(ErrorCode::DependencyMismatch,"Stash publication origin differs from capture or legacy origin is unknown; re-capture explicitly"));
                }
                let scene = crate::stash::scene(connection, scene_id)?;
                if scene.interactive
                    || scene.files.len() != 1
                    || scene.files[0].id != *file_id
                    || scene.files[0].path != capture.server_path
                {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "Stash scene/file changed or is already interactive",
                    ));
                }
                let publisher = crate::stash_files::Publication::prepare(&capture)?;
                return self.export(
                    request,
                    &capture.script_destination,
                    &fingerprint,
                    ExportKind::Stash {
                        capture: &capture,
                        candidate: candidate_id,
                        job: job_id,
                        archived_origin: archived_origin.as_ref(),
                        publisher: &publisher,
                    },
                );
            }
        }
        if let Command::Export { path } = &request.command {
            return self.export(request, path, &fingerprint, ExportKind::Neutral);
        }
        if let Command::ExportAxis { path, axis } = &request.command {
            return self.export(request, path, &fingerprint, ExportKind::Axis(*axis, None));
        }
        if let Command::ExportAxisPinned {
            path,
            axis,
            directory_identity,
        } = &request.command
        {
            return self.export(
                request,
                path,
                &fingerprint,
                ExportKind::Axis(*axis, Some(directory_identity)),
            );
        }
        if let Command::PrepareHandyStrokeScript { path } = &request.command {
            return self.export(request, path, &fingerprint, ExportKind::HandyStroke);
        }
        if let Command::ExportDeviceMotion { path, profile } = &request.command {
            return self.export(request, path, &fingerprint, ExportKind::Device(profile));
        }
        // Keep the original approval command as the receipt/fingerprint subject.
        // Only preparation and operation construction use its checked derivation.
        let assistant_generation = if matches!(
            request.command,
            Command::GenerateWithAssistant { .. } | Command::GenerateCreativeWithAssistant { .. }
        ) {
            let db = self.db.lock().map_err(internal)?;
            self.approved_assistant_generation(&db, request)?
        } else {
            None
        };
        #[cfg(not(target_os = "linux"))]
        if matches!(
            request.command,
            Command::GenerateWithInferenceFallback { .. }
        ) {
            return Err(ProtocolError::unsupported(
                "qualified fallback requires supported captured execution",
            ));
        }
        let fallback_generation = if let Command::GenerateWithInferenceFallback {
            source_version,
            preset,
            settings,
            model,
            ..
        } = &request.command
        {
            let mut derived = request.clone();
            derived.command = match model {
                InferenceFallbackModel::Path { path } => Command::Generate {
                    source_version: source_version.clone(),
                    preset: preset.clone(),
                    settings: Some(*settings.clone()),
                    model_path: Some(path.clone()),
                },
                InferenceFallbackModel::ModelPack { choice } => Command::GenerateFromModelPack {
                    source_version: source_version.clone(),
                    preset: preset.clone(),
                    choice: choice.clone(),
                    settings: Some(*settings.clone()),
                },
            };
            Some(derived)
        } else {
            None
        };
        let generation_request = assistant_generation
            .as_ref()
            .or(fallback_generation.as_ref())
            .unwrap_or(request);
        if let Command::Generate {
            source_version,
            settings: Some(settings),
            model_path,
            ..
        } = &generation_request.command
        {
            if let Some(view) = &settings.annotated_view {
                if model_path.is_none()
                    || !settings.inference_profile.as_ref().is_some_and(|p| {
                        matches!(p.provider, InferenceProvider::Cpu | InferenceProvider::Cuda)
                    })
                {
                    return Err(ProtocolError::unsupported(
                        "annotated generation requires an explicit CPU or CUDA model/profile",
                    ));
                }
                let db = self.db.lock().map_err(internal)?;
                check_revision(&db, request)?;
                source_kinds::require_media(&db, project(request)?, source_version)?;
                if self
                    .source(&db, project(request)?, source_version)?
                    .identity
                    .sha256
                    != view.source_sha256
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "annotated view is not bound to the imported source",
                    ));
                }
            }
        }
        if let Command::GenerateFromModelPack {
            source_version,
            settings: Some(settings),
            ..
        } = &generation_request.command
        {
            if let Some(view) = &settings.annotated_view {
                let db = self.db.lock().map_err(internal)?;
                check_revision(&db, request)?;
                source_kinds::require_media(&db, project(request)?, source_version)?;
                if self
                    .source(&db, project(request)?, source_version)?
                    .identity
                    .sha256
                    != view.source_sha256
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "annotated view is not bound to the imported source",
                    ));
                }
            }
        }
        let _motion_admission = if matches!(
            &generation_request.command,
            Command::ImportFunscript { .. }
                | Command::ImportFunscriptForRepair { .. }
                | Command::StageLiveRecording { .. }
                | Command::CommitCandidate { .. }
                | Command::MergeCandidate { .. }
                | Command::Undo
                | Command::Redo
                | Command::Diagnostics { .. }
                | Command::ReviewStatus
                | Command::SetReviewDisposition { .. }
                | Command::SetReviewIntervalDisposition { .. }
                | Command::SetProtectedRegions { .. }
                | Command::SetSourcePlacement { .. }
                | Command::RetimeMotion { .. }
                | Command::RemoveSourcePlacement { .. }
        ) {
            Some(self.pool.reserve_memory(transfers::VALIDATION_MEMORY)?)
        } else {
            None
        };
        let _artifact_guard = if matches!(
            &generation_request.command,
            Command::ImportSource { .. }
                | Command::ImportSourcePinned { .. }
                | Command::StashImportSource { .. }
                | Command::ImportFunscript { .. }
                | Command::ImportFunscriptForRepair { .. }
                | Command::GenerateInput { .. }
                | Command::StageLiveRecording { .. }
                | Command::GenerateFromModelPack { .. }
                | Command::Generate { .. }
                | Command::ResumeJob { .. }
                | Command::EvictSource { .. }
        ) {
            Some(self.artifact_gate.lock().map_err(internal)?)
        } else {
            None
        };
        // Expensive copying happens outside the database lock; authority and revision
        // are checked again in the transaction that publishes the source reference.
        let _stash_capture_admission = if matches!(
            &generation_request.command,
            Command::StashImportSource { .. }
        ) {
            Some(self.pool.reserve_memory(16 * 1024 * 1024)?)
        } else {
            None
        };
        let prepared_source: Option<PreparedSource> = match &generation_request.command {
            Command::ImportSource { path } | Command::ImportSourcePinned { path, .. } => {
                let (snapshot, hash, bytes) = artifacts::snapshot_reserved(
                    path,
                    &self.config.state_dir.join("snapshots"),
                    self.config.max_snapshot_bytes,
                    self.pool.reserved_storage(),
                )
                .map_err(|e| ProtocolError::new(ErrorCode::ResourceExhausted, e.to_string()))?;
                if let Command::ImportSourcePinned {
                    sha256, byte_len, ..
                } = &generation_request.command
                {
                    if sha256 != &hash || *byte_len != bytes {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "Prepared source differs from captured batch identity",
                        ));
                    }
                }
                Some((snapshot, hash, bytes, path.clone(), None))
            }
            Command::StashImportSource {
                connection,
                scene_id,
                file_id,
                expected_server_path,
                expected_source_version,
                mappings,
                ..
            } => {
                #[cfg(not(unix))]
                {
                    let _ = (
                        connection,
                        scene_id,
                        file_id,
                        expected_server_path,
                        mappings,
                    );
                    return Err(ProtocolError::unsupported(
                        "Stash source capture requires qualified filesystem descriptor traversal",
                    ));
                }
                #[cfg(unix)]
                {
                    let scene = crate::stash::scene(connection, scene_id)?;
                    if scene.files.len() != 1
                        || scene.files[0].id != *file_id
                        || expected_server_path
                            .as_ref()
                            .is_some_and(|path| path != &scene.files[0].path)
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::RequestConflict,
                            "Stash selected file is ambiguous or changed",
                        ));
                    }
                    let (snapshot, hash, bytes, plan) = crate::stash_files::capture(
                        &scene,
                        mappings,
                        &self.config.state_dir.join("snapshots"),
                        self.config.max_snapshot_bytes,
                        self.pool.reserved_storage(),
                    )?;
                    if expected_source_version
                        .as_ref()
                        .is_some_and(|expected| expected.as_str() != hash)
                    {
                        return Err(ProtocolError::new(ErrorCode::DependencyMismatch,"fresh Stash file bytes differ from selected project source; no association was rebound"));
                    }
                    let source_version = SourceVersionId::new(hash.clone()).map_err(internal)?;
                    let identity = ArtifactIdentity {
                        artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                        sha256: hash.clone(),
                        byte_len: bytes,
                    };
                    let original = plan.local_source.expect("captured source");
                    let receipt = StashSourceCapture {
                        endpoint: Some(crate::stash::endpoint_identity(connection)?),
                        source_version,
                        identity,
                        scene_id: scene.id,
                        file_id: scene.files[0].id.clone(),
                        server_path: scene.files[0].path.clone(),
                        local_source: original.clone(),
                        script_destination: plan.script_destination.expect("captured destination"),
                    };
                    Some((snapshot, hash, bytes, original, Some(receipt)))
                }
            }
            _ => None,
        };
        let prepared_funscript = match &generation_request.command {
            Command::ImportFunscript { path } => Some(self.prepare_funscript(path)?),
            Command::ImportFunscriptForRepair { path } => {
                Some(self.prepare_funscript_for_repair(path)?)
            }
            _ => None,
        };
        let prepared_recording =
            if let Command::StageLiveRecording { options } = &generation_request.command {
                Some(self.prepare_live_recording_candidate(request, session, options)?)
            } else {
                None
            };
        let prepared_input = if let Command::GenerateInput { input } = &generation_request.command {
            if input.source_version().is_none() {
                Some(self.materialize_input(input)?)
            } else {
                None
            }
        } else {
            None
        };
        let prepared_model = if let Command::Generate {
            model_path: Some(path),
            ..
        } = &generation_request.command
        {
            Some(self.snapshot_model(path)?)
        } else {
            None
        };
        let prepared_pack = if let Command::GenerateFromModelPack {
            choice, settings, ..
        } = &generation_request.command
        {
            Some(self.prepare_selected_pack(choice, settings.clone().unwrap_or_default())?)
        } else {
            None
        };
        self.check_stash_batch_model(
            request,
            prepared_model
                .as_ref()
                .or_else(|| prepared_pack.as_ref().map(|pack| &pack.model)),
        )?;
        #[cfg(target_os = "linux")]
        let prepared_fallback = match &request.command {
            Command::GenerateWithInferenceFallback {
                selection,
                settings,
                ..
            } => {
                let selected = prepared_pack
                    .as_ref()
                    .map(|pack| &pack.settings)
                    .unwrap_or(settings.as_ref())
                    .inference_profile
                    .as_ref()
                    .ok_or_else(|| {
                        ProtocolError::invalid("fallback requires explicit selected profile")
                    })?;
                Some(self.prepare_initial_fallback(selection, selected)?)
            }
            Command::ResumeJob { job_id } => {
                let previous = {
                    let db = self.db.lock().map_err(internal)?;
                    self.authorize(&db, request, session)?;
                    job_snapshot(&db, job_id, project(request)?)?;
                    let raw: String = db
                        .query_row(
                            "SELECT manifest FROM jobs WHERE id=?1 AND project=?2",
                            params![job_id.as_str(), project(request)?.as_str()],
                            |row| row.get(0),
                        )
                        .map_err(internal)?;
                    decode::<WorkerRequest>(&raw)?
                };
                self.prepare_fallback_resume(&previous)?
            }
            _ => None,
        };
        #[cfg(target_os = "linux")]
        let prepared_cuda = self.prepare_cuda_generation(
            generation_request,
            prepared_pack.as_ref().map(|pack| &pack.settings),
        )?;
        #[cfg(target_os = "linux")]
        let prepared_cpu = self.prepare_cpu_generation(
            generation_request,
            prepared_pack.as_ref().map(|pack| &pack.settings),
        )?;
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, session)?;
        if request.command.is_mutating() {
            if let Some(body) = replay(&tx, session.as_str(), request, &fingerprint)? {
                return Ok(body);
            }
        }
        check_revision(&tx, request)?;
        // Revalidate after asset capture and under the publishing transaction.
        self.approved_assistant_generation(&tx, request)?;
        if request.command.is_mutating() {
            claim_request_identity(&tx, session.as_str(), request, &fingerprint)?;
        }
        let mut launch_work: Option<(WorkerRequest, Reservation, Arc<AtomicBool>)> = None;
        let mut cancel_after_commit: Vec<Arc<AtomicBool>> = Vec::new();
        let body = match &generation_request.command {
            Command::GenerateWithAssistant { .. }
            | Command::GenerateCreativeWithAssistant { .. } => {
                unreachable!("assistant generation derived before preparation")
            }
            Command::NativeImagePreparationLimits
            | Command::StartNativeImagePreparation { .. }
            | Command::ResolveNativeImagePreparation { .. }
            | Command::NativeImagePreparationStatus { .. }
            | Command::CancelNativeImagePreparation { .. }
            | Command::ReleaseNativeImagePreparation { .. } => {
                unreachable!("native preparation routed before project transactions")
            }
            Command::LiveCaptureLimits
            | Command::StartLiveCapture { .. }
            | Command::ContinueLiveCapture { .. }
            | Command::ContinueLiveCaptureAt { .. }
            | Command::PrepareBrowserAnalysisSuccessor { .. }
            | Command::TransferBrowserAnalysisInput { .. }
            | Command::BrowserAnalysisTransferStatus { .. }
            | Command::CancelBrowserAnalysisTransfer { .. }
            | Command::LiveCaptureStatus { .. }
            | Command::StartLiveRecording { .. }
            | Command::LiveRecordingPage { .. }
            | Command::DiscardLiveRecording { .. }
            | Command::StopLiveCapture { .. }
            | Command::FinishLiveCapture { .. }
            | Command::CloseBrowserAnalysisInput { .. } => {
                unreachable!("live lifecycle routed before project transactions")
            }
            Command::BeginLiveCudaEvidenceDownload { .. } => {
                unreachable!("download routed to transfer broker")
            }
            Command::RemoveRetainedLiveCudaEvidence {
                epoch,
                expected_manifest_sha256,
            } => {
                validate_sha256(expected_manifest_sha256)?;
                let receipt = crate::live_cuda_capture_db::remove(
                    &tx,
                    &self.pool,
                    project(request)?,
                    epoch,
                    expected_manifest_sha256,
                )
                .map_err(|e| {
                    ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        format!("retained CUDA evidence removal: {e:#}"),
                    )
                })?
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::NotFound,
                        "no retained CUDA evidence for this project and capture",
                    )
                })?;
                ResponseBody::LiveCudaEvidenceRemoval(receipt)
            }
            Command::ListLiveCudaEvidence { after, limit } => {
                if !(1..=16).contains(limit) {
                    return Err(ProtocolError::invalid(
                        "retained CUDA listing limit must be between 1 and 16",
                    ));
                }
                let page = crate::live_cuda_capture_db::list(
                    &tx,
                    project(request)?,
                    after.as_ref(),
                    *limit,
                )
                .map_err(|e| {
                    ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        format!("retained CUDA evidence listing: {e:#}"),
                    )
                })?;
                ResponseBody::LiveCudaEvidencePage(page)
            }
            Command::InspectLiveCudaEvidence { epoch } => {
                let report =
                    crate::live_cuda_capture_db::inspect(&tx, &self.pool, project(request)?, epoch)
                        .map_err(|e| {
                            ProtocolError::new(
                                ErrorCode::DependencyMismatch,
                                format!("retained CUDA evidence inspection: {e:#}"),
                            )
                        })?
                        .ok_or_else(|| {
                            ProtocolError::new(
                                ErrorCode::NotFound,
                                "no retained CUDA evidence for this project and capture",
                            )
                        })?;
                ResponseBody::RetainedLiveCudaEvidence((report).into())
            }
            Command::Capabilities => ResponseBody::Capabilities(self.capabilities()),
            Command::AssistantProfiles
            | Command::StartAssistant { .. }
            | Command::ResolveAssistantStart { .. }
            | Command::AssistantStatus { .. }
            | Command::CancelAssistant { .. }
            | Command::ListAssistantAttempts
            | Command::WasmEditorInfo
            | Command::StartWasmEdit { .. }
            | Command::ResolveWasmStart { .. }
            | Command::WasmEditStatus { .. }
            | Command::CancelWasmEdit { .. }
            | Command::ListWasmEdits
            | Command::ListArchivedWasmEdits { .. }
            | Command::RemoveUnsuccessfulWasmEdit { .. }
            | Command::ArchiveCompletedWasmEdit { .. } => {
                return Err(internal(
                    "WASM editor command escaped authenticated operation routing",
                ));
            }
            Command::CheckModelPackCatalog { .. }
            | Command::RefreshModelCatalog { .. }
            | Command::ModelCatalogStatus { .. }
            | Command::SelectCatalogModelPack { .. }
            | Command::SelectModelPack { .. }
            | Command::WithdrawModelPackApproval { .. }
            | Command::ModelPackSelectionStatus { .. }
            | Command::ImportModelPack { .. }
            | Command::ModelPackStatus { .. } => {
                unreachable!("pack commands are routed before project transactions")
            }
            Command::ImportInferenceEquivalence { .. }
            | Command::InferenceEquivalenceStatus { .. }
            | Command::ListInferenceEquivalences { .. }
            | Command::AcceptInferenceEquivalence { .. }
            | Command::WithdrawInferenceEquivalence { .. } => {
                return Err(internal(
                    "inference equivalence command escaped owner routing",
                ));
            }
            Command::InferenceCatalog => ResponseBody::InferenceCatalog(self.inference_catalog()),
            Command::CreateProject { name } => {
                let project = id!(ProjectId);
                let actor = actor(&tx, session)?;
                let program = motion_state::save(&tx, &MotionProgram::default())?;
                tx.execute(
                    "INSERT INTO projects(id,name,revision,program) VALUES(?1,?2,0,?3)",
                    params![project.as_str(), name, program],
                )
                .map_err(internal)?;
                tx.execute(
                    "INSERT INTO revisions VALUES(?1,0,?2,'create','Create project',?3,'[]')",
                    params![project.as_str(), actor, program],
                )
                .map_err(internal)?;
                tx.execute(
                    "INSERT INTO edit_states VALUES(?1,0,?2,'[]')",
                    params![project.as_str(), program],
                )
                .map_err(internal)?;
                for scope in [
                    Scope::Read,
                    Scope::Edit,
                    Scope::Generate,
                    Scope::Export,
                    Scope::ManageGrants,
                    Scope::ManageProtection,
                    Scope::ImportSource,
                    Scope::PackageProject,
                ] {
                    tx.execute(
                        "INSERT INTO grants VALUES(?1,?2,?3)",
                        params![session.as_str(), project.as_str(), scope_name(scope)],
                    )
                    .map_err(internal)?;
                }
                lineage::created(&tx, &project)?;
                event(&tx, &project, RevisionId::new(0), EventBody::ProjectChanged)?;
                ResponseBody::Project((project_snapshot(&tx, &project)?).into())
            }
            Command::OpenProject { project_id } => {
                ResponseBody::Project((project_snapshot(&tx, project_id)?).into())
            }
            Command::GetSnapshot => {
                ResponseBody::Project((project_snapshot(&tx, project(request)?)?).into())
            }
            Command::ImportSource { .. }
            | Command::ImportSourcePinned { .. }
            | Command::StashImportSource { .. } => {
                let (snapshot, hash, bytes, path, stash_receipt) =
                    prepared_source.as_ref().expect("prepared import");
                if check_stash_source_binding(&tx, request)?
                    .is_some_and(|expected| expected != *bytes)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "captured Stash length differs from selected source identity",
                    ));
                }
                let source_version = SourceVersionId::new(hash.clone()).map_err(internal)?;
                let label = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let count: i64 = tx
                    .query_row(
                        "SELECT COUNT(*) FROM sources WHERE project=?1",
                        [project(request)?.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                let existing: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2)",
                        params![project(request)?.as_str(), source_version.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(internal)?;
                if count >= 128 && !existing {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "project source catalog reached enabled control-path admission limit",
                    ));
                }
                tx.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(project,version) DO UPDATE SET evicted=0,path=excluded.path",
                    params![project(request)?.as_str(),source_version.as_str(),snapshot.to_string_lossy(),path.to_string_lossy(),label,hash,*bytes as i64]).map_err(internal)?;
                let snapshot = project_snapshot(&tx, project(request)?)?;
                event(
                    &tx,
                    &snapshot.project_id,
                    snapshot.revision,
                    EventBody::ProjectChanged,
                )?;
                if let Some(receipt) = stash_receipt {
                    tx.execute("INSERT INTO stash_sources(project,version,scene,file,receipt) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project,version,scene,file) DO UPDATE SET receipt=excluded.receipt",params![project(request)?.as_str(),source_version.as_str(),receipt.scene_id,receipt.file_id,encode(receipt)?]).map_err(internal)?;
                    ResponseBody::StashSourceCaptured((receipt.clone()).into())
                } else {
                    ResponseBody::Source { source_version }
                }
            }
            Command::CandidateImageOriginPixels {
                candidate_id,
                record_sha256,
            } => ResponseBody::CandidateImageOriginPixels(
                self.candidate_image_pixels(&tx, project(request)?, candidate_id, record_sha256)?
                    .into(),
            ),
            Command::CandidateImageOriginStatus { candidate_id } => {
                ResponseBody::CandidateImageOrigin(
                    self.candidate_image_origin(&tx, project(request)?, candidate_id)?
                        .into(),
                )
            }
            Command::ArchivedGenerationStatus {
                candidate_id,
                master_revision,
            } => {
                let mut report = archived_generation::load(
                    &tx,
                    project(request)?,
                    candidate_id,
                    &self.pool,
                    &self.import_store,
                )?;
                if let Some(revision) = master_revision {
                    report.master_derivation = Some(stash_lineage::verified_archived_master(
                        &tx,
                        project(request)?,
                        *revision,
                        &report,
                        &self.pool,
                        &self.import_store,
                    )?);
                    report.validate()?;
                }
                ResponseBody::ArchivedGeneration((report).into())
            }
            Command::ArchivedMotionRetimeStatus { revision } => {
                ResponseBody::ArchivedMotionRetimeReceipt(
                    (archived_motion_retimes::load_verified(
                        &tx,
                        project(request)?,
                        *revision,
                        &self.pool,
                        &self.import_store,
                    )?)
                    .into(),
                )
            }
            Command::MotionRetimeStatus { revision } => ResponseBody::MotionRetimeReceipt(
                (motion_retime_receipts::load_verified(
                    &tx,
                    project(request)?,
                    *revision,
                    &self.pool,
                )?)
                .into(),
            ),
            Command::RetimeMotion {
                from_placement,
                to_placement,
                axes,
            } => {
                let p = project(request)?;
                let state = project_state(&tx, p)?;
                let from = source_placements::retained(&tx, p, from_placement)?;
                let to = source_placements::active(&tx, p, to_placement)?;
                let program =
                    retime_motion_between_placements(&state.program, &from.spec, &to.spec, axes)
                        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                let next = commit_program(
                    &tx,
                    request,
                    session,
                    &program,
                    "source_retime",
                    &format!("Retime motion from {from_placement} to {to_placement}"),
                    true,
                )?;
                let after = project_snapshot(&tx, p)?.motion.program;
                let identity = |d: &ProgramDescriptor| ArtifactIdentity {
                    artifact_id: d.artifact_id.clone(),
                    sha256: d.sha256.clone(),
                    byte_len: d.byte_len,
                };
                let receipt = MotionRetimeReceipt {
                    schema_version: 1,
                    project_id: p.clone(),
                    base_revision: state.revision,
                    revision: next,
                    session_id: session.clone(),
                    request_id: request.request_id.clone(),
                    actor: actor(&tx, session)?,
                    operation_sha256: fingerprint.clone(),
                    from,
                    to,
                    axes: axes.clone(),
                    before: identity(&state.motion.program),
                    after: identity(&after),
                    recorded_at_unix_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(internal)?
                        .as_millis()
                        .try_into()
                        .map_err(internal)?,
                };
                motion_retime_receipts::record(&tx, &receipt)?;
                ResponseBody::Project((project_snapshot(&tx, p)?).into())
            }
            Command::SetSourcePlacement { .. } | Command::RemoveSourcePlacement { .. } => {
                source_placements::write(self, &tx, request, session)?
            }
            Command::StageLiveRecording { .. } => self.stage_captured_candidate(
                &tx,
                request,
                prepared_recording.expect("prepared recording"),
                SourceKind::LiveRecording,
            )?,
            Command::ImportFunscript { .. } | Command::ImportFunscriptForRepair { .. } => self
                .import_funscript(
                    &tx,
                    request,
                    prepared_funscript.expect("prepared funscript"),
                )?,
            Command::ReviewStatus => {
                let snapshot = project_snapshot(&tx, project(request)?)?;
                let state = lineage::state(&tx, &snapshot.project_id, snapshot.revision)?;
                let mut entries = std::collections::BTreeMap::new();
                for flag in &state.flags {
                    let id = review_identity(flag)?;
                    entries.entry(id.clone()).or_insert_with(|| ReviewEntry {
                        review_id: id.clone(),
                        flag: flag.clone(),
                        decision: if state
                            .decisions
                            .iter()
                            .any(|d| d.review_id == id && d.range.is_some())
                        {
                            None
                        } else {
                            state.decisions.iter().find(|d| d.review_id == id).cloned()
                        },
                        decision_history: if state
                            .decisions
                            .iter()
                            .any(|d| d.review_id == id && d.range.is_some())
                        {
                            state
                                .decisions
                                .iter()
                                .filter(|d| d.review_id == id)
                                .cloned()
                                .collect()
                        } else {
                            vec![]
                        },
                    });
                }
                let report = ReviewStatus {
                    project_id: snapshot.project_id,
                    revision: snapshot.revision,
                    entries: entries.into_values().collect(),
                    unknown: state.unknown,
                    conservative: state.conservative,
                };
                report.validate()?;
                ResponseBody::ReviewStatus(report)
            }
            Command::SetReviewDisposition { .. } | Command::SetReviewIntervalDisposition { .. } => {
                let state = project_state(&tx, project(request)?)?;
                commit_program(
                    &tx,
                    request,
                    session,
                    &state.program,
                    if matches!(
                        request.command,
                        Command::SetReviewIntervalDisposition { .. }
                    ) {
                        "review_interval_disposition"
                    } else {
                        "review_disposition"
                    },
                    "Record explicit review disposition",
                    true,
                )?;
                ResponseBody::Project((project_snapshot(&tx, &state.project_id)?).into())
            }
            Command::SetProtectedRegions { regions } => {
                if regions.len() > 1024 {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "too many protected regions",
                    ));
                }
                let state = project_state(&tx, project(request)?)?;
                let revision = state.revision.checked_next().map_err(internal)?;
                tx.execute(
                    "UPDATE projects SET protected=?2,revision=?3 WHERE id=?1",
                    params![
                        state.project_id.as_str(),
                        encode(regions)?,
                        rev_sql(revision)?
                    ],
                )
                .map_err(internal)?;
                tx.execute("INSERT INTO revisions VALUES(?1,?2,?3,'protection','Set protected regions',?4,?5)",params![state.project_id.as_str(),rev_sql(revision)?,actor(&tx,session)?,motion_state::register(&tx, &state.motion.program)?,encode(regions)?]).map_err(internal)?;
                append_history(
                    &tx,
                    &state.project_id,
                    &motion_state::register(&tx, &state.motion.program)?,
                    &encode(regions)?,
                )?;
                source_placements::capture_revision(&tx, &state.project_id, revision)?;
                lineage::record(
                    &tx,
                    request,
                    revision,
                    state.revision,
                    &state.program,
                    &state.program,
                    true,
                )?;
                event(&tx, &state.project_id, revision, EventBody::ProjectChanged)?;
                ResponseBody::Project((project_snapshot(&tx, &state.project_id)?).into())
            }
            Command::PinSource {
                source_version,
                pinned,
            } => {
                let source = self.source(&tx, project(request)?, source_version)?;
                tx.execute(
                    "UPDATE sources SET pinned=?3 WHERE project=?1 AND version=?2",
                    params![
                        project(request)?.as_str(),
                        source.source_version.as_str(),
                        pinned
                    ],
                )
                .map_err(internal)?;
                ResponseBody::Ack
            }
            Command::EvictSource { source_version } => {
                let source = self.source(&tx, project(request)?, source_version)?;
                if self.package_holds.contains(&source.identity.sha256)? {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "snapshot retained by active artifact readers",
                    ));
                }
                let protected:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE version=?1 AND pinned=1) OR EXISTS(SELECT 1 FROM jobs WHERE source=?1 AND state IN ('queued','running','interrupted','fallback_pending'))",[source_version.as_str()],|r|r.get(0)).map_err(internal)?;
                let preview_active = self.workers.lock().map_err(internal)?.keys().any(|key| {
                    (key.starts_with("preview:") || key.starts_with("probe:"))
                        && key.contains(&format!(":{}:", source_version.as_str()))
                });
                let runtime_pinned = self
                    .tools
                    .as_ref()
                    .ok()
                    .and_then(|tools| tools.onnx_runtime.as_ref())
                    .is_some_and(|runtime| runtime.identity.sha256 == source.identity.sha256);
                let mut manifests=tx.prepare("SELECT manifest FROM jobs WHERE state IN ('queued','running','interrupted','fallback_pending')").map_err(internal)?;
                let active = manifests
                    .query_map([], |r| r.get::<_, String>(0))
                    .map_err(internal)?;
                let mut dependency_pinned = false;
                for raw in active {
                    let manifest: WorkerRequest = decode(&raw.map_err(internal)?)?;
                    if manifest
                        .dependencies
                        .iter()
                        .any(|d| d.sha256 == source.identity.sha256)
                    {
                        dependency_pinned = true;
                        break;
                    }
                }
                if source_placements::is_retained_source(&tx, source_version)?
                    || protected
                    || preview_active
                    || runtime_pinned
                    || dependency_pinned
                    || crate::library_batch_db::is_retained_source(&tx, &source.identity.sha256)
                        .map_err(internal)?
                    || source.identity.sha256 == self.binary_identity.sha256
                {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "snapshot is pinned or required by active work",
                    ));
                }
                // Metadata is retained. An absent snapshot is an explicit dependency
                // failure, never permission to consume a changed original.
                fs::remove_file(&source.path).map_err(unavailable)?;
                artifacts::sync_dir(&self.config.state_dir.join("snapshots"))
                    .map_err(unavailable)?;
                tx.execute(
                    "UPDATE sources SET evicted=1 WHERE version=?1",
                    [source_version.as_str()],
                )
                .map_err(internal)?;
                ResponseBody::Ack
            }
            Command::Undo | Command::Redo => {
                let delta: i64 = if matches!(request.command, Command::Undo) {
                    -1
                } else {
                    1
                };
                let current: i64 = tx
                    .query_row(
                        "SELECT history_cursor FROM projects WHERE id=?1",
                        [project(request)?.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                let target = current
                    .checked_add(delta)
                    .ok_or_else(|| ProtocolError::invalid("history position overflow"))?;
                let entry: Option<(String,String)> = tx.query_row("SELECT program,protected FROM edit_states WHERE project=?1 AND position=?2", params![project(request)?.as_str(),target], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
                let (program, protected) = entry.ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::NotFound,
                        "no history entry in requested direction",
                    )
                })?;
                let existing: String = tx
                    .query_row(
                        "SELECT protected FROM projects WHERE id=?1",
                        [project(request)?.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                if existing != protected {
                    require_grant(&tx, session, project(request)?, Scope::ManageProtection)?;
                }
                let program = motion_state::load(&tx, &program)?;
                source_placements::restore_history(&tx, project(request)?, target)?;
                commit_program(
                    &tx,
                    request,
                    session,
                    &program,
                    if delta < 0 { "undo" } else { "redo" },
                    "Shared project history",
                    false,
                )?;
                tx.execute(
                    "UPDATE projects SET history_cursor=?2,protected=?3 WHERE id=?1",
                    params![project(request)?.as_str(), target, protected],
                )
                .map_err(internal)?;
                tx.execute("UPDATE revisions SET protected=?2 WHERE project=?1 AND revision=(SELECT revision FROM projects WHERE id=?1)",params![project(request)?.as_str(),protected]).map_err(internal)?;
                ResponseBody::Project((project_snapshot(&tx, project(request)?)?).into())
            }
            Command::GenerateFromModelPack {
                source_version,
                preset,
                choice,
                ..
            } => {
                self.require_current_pack_choice(&tx, choice)?;
                if !["fast", "default", "quality", "maximum"].contains(&preset.as_str()) {
                    return Err(ProtocolError::unsupported("generation preset"));
                }
                let prepared = prepared_pack.expect("pack capture prepared");
                let (manifest, reservation, cancel) = self.prepare_operation_with_capture(
                    &tx,
                    request,
                    source_version,
                    WorkerOperation::Generate {
                        preset: preset.clone(),
                        settings: (prepared.settings).into(),
                        model: Some(prepared.model),
                    },
                    OperationDependencies {
                        origin: AttemptOrigin::New,
                        extra: &prepared.dependencies,
                        #[cfg(target_os = "linux")]
                        cuda: prepared_cuda.as_ref(),
                        #[cfg(target_os = "linux")]
                        cpu: prepared_cpu.as_ref(),
                        #[cfg(target_os = "linux")]
                        fallback: prepared_fallback.as_ref(),
                    },
                )?;
                let snapshot = job_snapshot(&tx, &manifest.job_id, project(request)?)?;
                launch_work = Some((manifest, reservation, cancel));
                ResponseBody::Job(snapshot)
            }
            Command::Generate {
                source_version,
                preset,
                settings,
                ..
            } => {
                if !["fast", "default", "quality", "maximum"].contains(&preset.as_str()) {
                    return Err(ProtocolError::unsupported("generation preset"));
                }
                let (manifest, reservation, cancel) = self.prepare_operation_with_capture(
                    &tx,
                    request,
                    source_version,
                    WorkerOperation::Generate {
                        preset: preset.clone(),
                        settings: (settings.clone().unwrap_or_default()).into(),
                        model: prepared_model,
                    },
                    OperationDependencies {
                        origin: AttemptOrigin::New,
                        extra: &[],
                        #[cfg(target_os = "linux")]
                        cuda: prepared_cuda.as_ref(),
                        #[cfg(target_os = "linux")]
                        cpu: prepared_cpu.as_ref(),
                        #[cfg(target_os = "linux")]
                        fallback: prepared_fallback.as_ref(),
                    },
                )?;
                let snapshot = job_snapshot(&tx, &manifest.job_id, project(request)?)?;
                launch_work = Some((manifest, reservation, cancel));
                ResponseBody::Job(snapshot)
            }
            Command::GenerateInput { input } => {
                let source_version = if let Some(version) = input.source_version() {
                    version.clone()
                } else {
                    let source = prepared_input.expect("materialized generation input");
                    let count: i64 = tx
                        .query_row(
                            "SELECT COUNT(*) FROM sources WHERE project=?1",
                            [project(request)?.as_str()],
                            |row| row.get(0),
                        )
                        .map_err(internal)?;
                    if count >= 128 {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "project source catalog is full",
                        ));
                    }
                    tx.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes,kind) VALUES(?1,?2,?3,'','Generation input',?4,?5,'generation_input') ON CONFLICT(project,version) DO UPDATE SET evicted=0,path=excluded.path,kind=excluded.kind",
                        params![project(request)?.as_str(),source.source_version.as_str(),source.path.to_string_lossy(),source.identity.sha256,source.identity.byte_len as i64]).map_err(internal)?;
                    source.source_version
                };
                let image_dependencies = self.capture_image_origin(&tx, request, input)?;
                let (manifest, reservation, cancel) = self.prepare_operation_with_dependencies(
                    &tx,
                    request,
                    &source_version,
                    WorkerOperation::GenerateInput {
                        input: input.clone(),
                    },
                    None,
                    &image_dependencies,
                )?;
                let snapshot = job_snapshot(&tx, &manifest.job_id, project(request)?)?;
                launch_work = Some((manifest, reservation, cancel));
                ResponseBody::Job(snapshot)
            }
            Command::JobStatus { job_id } => {
                ResponseBody::Job(job_snapshot(&tx, job_id, project(request)?)?)
            }
            Command::JobAttempts {
                job_id,
                after_attempt,
                limit,
            } => {
                let history =
                    inference_fallback_store::history_snapshot(&tx, project(request)?, job_id)?;
                ResponseBody::JobAttempts(Box::new(crate::job_attempt_history::page_with_fallback(
                    &tx,
                    project(request)?,
                    job_id,
                    after_attempt.as_ref(),
                    *limit,
                    |manifest| history.trace_for(manifest),
                )?))
            }
            Command::CancelJob { job_id } => {
                let old = job_snapshot(&tx, job_id, project(request)?)?;
                if ["queued", "running", "interrupted", "fallback_pending"]
                    .contains(&old.state.as_str())
                {
                    tx.execute("UPDATE jobs SET state='cancelled',error='cancelled by authorized client' WHERE id=?1", [job_id.as_str()]).map_err(internal)?;
                    tx.execute(
                        "UPDATE attempts SET state='cancelled' WHERE id=?1 AND state IN ('queued','running','interrupted','assistant_paused')",
                        [old.attempt_id.as_str()],
                    )
                    .map_err(internal)?;
                    inference_fallback_store::clear_pending(&tx, job_id)?;
                    if let Some(flag) = self.workers.lock().map_err(internal)?.get(job_id.as_str())
                    {
                        cancel_after_commit.push(flag.clone());
                    }
                }
                ResponseBody::Job(job_snapshot(&tx, job_id, project(request)?)?)
            }
            Command::ResumeJob { job_id } => {
                let old = job_snapshot(&tx, job_id, project(request)?)?;
                if old.state != "interrupted" {
                    return Err(ProtocolError::invalid("only interrupted jobs can resume"));
                }
                let raw: String = tx
                    .query_row(
                        "SELECT manifest FROM jobs WHERE id=?1",
                        [job_id.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                let previous: WorkerRequest = decode(&raw)?;
                self.validate_dependencies(&previous)?;
                self.validate_checkpoint(&previous)?;
                if matches!(&previous.operation, WorkerOperation::Preview { .. }) {
                    return Err(ProtocolError::unsupported("preview job resume"));
                }
                let (manifest, reservation, cancel) = self.prepare_operation_with_capture(
                    &tx,
                    request,
                    &previous.source.source_version,
                    previous.operation.clone(),
                    OperationDependencies {
                        origin: AttemptOrigin::Resume(&previous),
                        extra: &[],
                        #[cfg(target_os = "linux")]
                        cuda: prepared_cuda.as_ref(),
                        #[cfg(target_os = "linux")]
                        cpu: prepared_cpu.as_ref(),
                        #[cfg(target_os = "linux")]
                        fallback: prepared_fallback.as_ref(),
                    },
                )?;
                let snapshot = job_snapshot(&tx, &manifest.job_id, project(request)?)?;
                launch_work = Some((manifest, reservation, cancel));
                ResponseBody::Job(snapshot)
            }
            Command::MergeCandidate {
                candidate_id,
                axes,
                range,
            } => self.merge_candidate(&tx, request, session, candidate_id, axes, *range)?,
            Command::Diagnostics {
                candidate_id,
                maximum_speed_percent_per_second,
            } => self.diagnostics(
                &tx,
                request,
                candidate_id.as_ref(),
                *maximum_speed_percent_per_second,
            )?,
            Command::GetCandidate { candidate_id } => ResponseBody::Candidate(
                (candidate_snapshot(&tx, candidate_id, project(request)?)?).into(),
            ),
            Command::RebaseCandidate { candidate_id } => {
                let old = candidate_snapshot(&tx, candidate_id, project(request)?)?;
                let project = project_snapshot(&tx, project(request)?)?;
                let new_id = id!(CandidateId);
                let lineage: String = tx
                    .query_row(
                        "SELECT lineage FROM candidates WHERE id=?1",
                        [candidate_id.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                tx.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![new_id.as_str(),project.project_id.as_str(),rev_sql(project.revision)?,motion_state::register(&tx,&old.motion.program)?,old.job_id.as_ref().map(|j|j.as_str()),lineage,encode(&old.review)?]).map_err(internal)?;
                tx.execute("INSERT INTO edit_candidate_lineage SELECT ?1,receipt,review_state,authored_count,inherited_count FROM edit_candidate_lineage WHERE candidate=?2",params![new_id.as_str(),candidate_id.as_str()]).map_err(internal)?;
                tx.execute(
                    "INSERT INTO candidate_rebases(candidate,parent) VALUES(?1,?2)",
                    params![new_id.as_str(), candidate_id.as_str()],
                )
                .map_err(internal)?;
                project_package_imports::rebase_alias(&tx, candidate_id, &new_id)?;
                ResponseBody::Candidate(
                    (candidate_snapshot(&tx, &new_id, &project.project_id)?).into(),
                )
            }
            Command::CommitCandidate { candidate_id } => {
                let candidate = candidate_state(&tx, candidate_id, project(request)?)?;
                let state = project_snapshot(&tx, project(request)?)?;
                if candidate.base_revision != state.revision {
                    return Err(ProtocolError::new(ErrorCode::RevisionConflict,"candidate is stale; explicitly rebase to a new candidate before committing"));
                }
                if candidate_generation_interval(&tx, candidate_id, project(request)?)?.is_some()
                    && !state.motion.program.axes.is_empty()
                {
                    return Err(ProtocolError::new(ErrorCode::Forbidden,"interval generation must be merged with an explicit contained project range; whole-project replacement is refused"));
                }
                let committed: Option<i64> = tx
                    .query_row(
                        "SELECT committed_revision FROM candidates WHERE id=?1",
                        [candidate_id.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                if committed.is_some() {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "candidate already committed",
                    ));
                }
                let revision = commit_program(
                    &tx,
                    request,
                    session,
                    &candidate.program,
                    "candidate",
                    "Commit generated candidate",
                    true,
                )?;
                tx.execute(
                    "UPDATE candidates SET committed_revision=?2 WHERE id=?1",
                    params![candidate_id.as_str(), rev_sql(revision)?],
                )
                .map_err(internal)?;
                ResponseBody::Project((project_snapshot(&tx, project(request)?)?).into())
            }
            Command::Grant {
                session: target,
                project_id,
                scopes,
            } => {
                actor(&tx, target)?;
                for scope in scopes {
                    require_grant(&tx, session, project_id, *scope)?;
                    tx.execute(
                        "INSERT OR IGNORE INTO grants VALUES(?1,?2,?3)",
                        params![target.as_str(), project_id.as_str(), scope_name(*scope)],
                    )
                    .map_err(internal)?;
                }
                ResponseBody::Ack
            }
            Command::Revoke {
                session: target,
                project_id,
            } => {
                cancel_after_commit.extend(assistant_operations::revoke(
                    &tx,
                    target,
                    project_id,
                    &self.workers,
                )?);
                cancel_after_commit.extend(wasm_edits::revoke(
                    &tx,
                    target,
                    project_id,
                    &self.workers,
                )?);
                project_package_exports::revoke(&tx, target, project_id)?;
                tx.execute(
                    "DELETE FROM grants WHERE session=?1 AND project=?2",
                    params![target.as_str(), project_id.as_str()],
                )
                .map_err(internal)?;
                let mut stmt = tx.prepare("SELECT id FROM jobs WHERE session=?1 AND project=?2 AND state IN ('queued','running','interrupted','fallback_pending')").map_err(internal)?;
                let ids = stmt
                    .query_map(params![target.as_str(), project_id.as_str()], |r| {
                        r.get::<_, String>(0)
                    })
                    .map_err(internal)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(internal)?;
                for job in ids {
                    tx.execute("UPDATE jobs SET state='cancelled',error='authorizing grant revoked' WHERE id=?1",[&job]).map_err(internal)?;
                    tx.execute("UPDATE attempts SET state='cancelled' WHERE id=(SELECT attempt FROM jobs WHERE id=?1) AND state IN ('queued','running','interrupted','assistant_paused')",[&job]).map_err(internal)?;
                    inference_fallback_store::clear_pending(
                        &tx,
                        &JobId::new(job.clone()).map_err(internal)?,
                    )?;
                    if let Some(flag) = self.workers.lock().map_err(internal)?.get(&job) {
                        cancel_after_commit.push(flag.clone());
                    }
                }
                let prefix = format!("preview:{}:{}:", project_id.as_str(), target.as_str());
                let probe_prefix = format!("probe:{}:{}:", project_id.as_str(), target.as_str());
                for (key, flag) in self.workers.lock().map_err(internal)?.iter() {
                    if key.starts_with(&prefix) || key.starts_with(&probe_prefix) {
                        cancel_after_commit.push(flag.clone());
                    }
                }
                let state = project_snapshot(&tx, project_id)?;
                event(
                    &tx,
                    project_id,
                    state.revision,
                    EventBody::GrantRevoked {
                        session: target.clone(),
                    },
                )?;
                ResponseBody::Ack
            }
            Command::Events {
                after_cursor,
                limit,
            } => {
                let cursor = i64::try_from(after_cursor.unwrap_or(0))
                    .map_err(|_| ProtocolError::invalid("event cursor overflow"))?;
                let mut stmt = tx.prepare("SELECT cursor,revision,body FROM events WHERE project=?1 AND cursor>?2 ORDER BY cursor LIMIT ?3").map_err(internal)?;
                let rows = stmt
                    .query_map(
                        params![project(request)?.as_str(), cursor, (*limit).clamp(1, 256)],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, i64>(1)?,
                                r.get::<_, String>(2)?,
                            ))
                        },
                    )
                    .map_err(internal)?;
                let mut events = Vec::new();
                for row in rows {
                    let (cursor, revision, body) = row.map_err(internal)?;
                    events.push(Event {
                        version: PROTOCOL_VERSION,
                        cursor: cursor as u64,
                        project_id: project(request)?.clone(),
                        revision: RevisionId::new(revision as u64),
                        body: decode(&body)?,
                    });
                }
                let next_cursor = events.last().map(|e| e.cursor).unwrap_or(cursor as u64);
                ResponseBody::Events(EventPage {
                    events,
                    next_cursor,
                    resync_required: false,
                })
            }
            Command::Arm { .. } | Command::Stop { .. } | Command::SwitchPlaybackRevision { .. } => {
                return Err(ProtocolError::unsupported(
                    "unqualified physical device effects",
                ))
            }
            _ => return Err(ProtocolError::unsupported("operation")),
        };
        if request.command.is_mutating() {
            remember(&tx, session.as_str(), request, &fingerprint, &body)?;
        }
        tx.commit().map_err(internal)?;
        if request.command.is_mutating() {
            self.retire_changed_browser_media(&db);
            self.retire_changed_handy_preparations(&db);
            self.retire_changed_handy_browser_start_qualifications(&db);
            self.retire_changed_handy_hsp_source_qualifications(&db);
        }
        match &request.command {
            Command::CancelJob { job_id } => self.fallback_lifetimes.invalidate_job(job_id),
            Command::Revoke {
                session,
                project_id,
            } => self
                .fallback_lifetimes
                .invalidate_actor_project(session, project_id),
            _ => {}
        }
        // Capture the exact handle before commit, but signal only durable cancellation.
        // Keep publication fenced by the DB mutex until the infallible signal completes.
        for flag in cancel_after_commit {
            flag.store(true, Ordering::Release);
        }
        drop(db);
        if let Command::Revoke {
            session: target,
            project_id,
        } = &request.command
        {
            self.transfers
                .lock()
                .map_err(internal)?
                .revoke(target, project_id);
            self.package_revoke(target, project_id);
        }
        if let Some((manifest, reservation, cancel)) = launch_work {
            let engine = self.clone();
            let failed_manifest = manifest.clone();
            if let Err(error) = self.spawn_task("pulsar-generation", None, move || {
                engine.run_job(
                    manifest,
                    cancel,
                    reservation,
                    #[cfg(target_os = "linux")]
                    prepared_cuda,
                    #[cfg(target_os = "linux")]
                    prepared_cpu,
                    #[cfg(target_os = "linux")]
                    prepared_fallback,
                );
            }) {
                self.finish_job(&failed_manifest, Err(unavailable(error)))?;
            }
        }
        Ok(body)
    }

    fn pair(&self, request: &Request, name: &str, supplied: &str) -> PResult<ResponseBody> {
        let mismatch = supplied
            .as_bytes()
            .iter()
            .zip(self.token.as_bytes())
            .fold(0u8, |a, (b, c)| a | (b ^ c));
        if supplied.len() != self.token.len() || mismatch != 0 {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "invalid pairing credential",
            ));
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(request).map_err(internal)?)
        );
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        if let Some(body) = replay(&tx, "pair", request, &fingerprint)? {
            return Ok(body);
        }
        let session = id!(SessionId);
        let actor = id!(ActorId);
        let auth_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let token_hash = format!("{:x}", Sha256::digest(auth_token.as_bytes()));
        tx.execute(
            "INSERT INTO sessions VALUES(?1,?2,?3,?4)",
            params![session.as_str(), actor.as_str(), name, token_hash],
        )
        .map_err(internal)?;
        let body = ResponseBody::Paired {
            session,
            auth_token,
        };
        remember(&tx, "pair", request, &fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }

    fn authenticate(
        &self,
        db: &Connection,
        session: &SessionId,
        token: Option<&str>,
    ) -> PResult<()> {
        authenticate_session(db, session, token)
    }

    /// Test fixtures must not interpret dropping their own Arc as completed retirement.
    #[cfg(test)]
    pub(crate) fn retire_for_test(engine: Arc<Self>) -> EngineConfig {
        let config = engine.config.clone();
        let lifetime = Arc::downgrade(&engine);
        #[cfg(target_os = "linux")]
        let lease = artifacts::open_regular(&config.state_dir.join("worker-fence.lock"))
            .expect("fixture lease descriptor");
        drop(engine);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if lifetime.strong_count() == 0 {
                #[cfg(target_os = "linux")]
                match fs2::FileExt::try_lock_exclusive(&lease) {
                    Ok(()) => {
                        fs2::FileExt::unlock(&lease).expect("release only fixture probe lease");
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("fixture retirement lease observation: {error}"),
                }
                #[cfg(not(target_os = "linux"))]
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixture Engine or worker-fence custody did not retire"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        config
    }

    fn authorize(&self, db: &Connection, request: &Request, session: &SessionId) -> PResult<()> {
        if self.is_shutting_down() {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "Engine is shutting down",
            ));
        }
        if !self.stash_batch_permitted(db, request, session)? {
            self.authenticate(db, session, request.auth_token.as_deref())?;
        }
        if let Command::CheckModelPackCatalog { owner_proof, .. }
        | Command::RefreshModelCatalog { owner_proof, .. }
        | Command::ModelCatalogStatus { owner_proof, .. }
        | Command::ImportModelPack { owner_proof, .. }
        | Command::ModelPackStatus { owner_proof, .. }
        | Command::ModelPackStagingStatus { owner_proof }
        | Command::DiscardModelPackStage { owner_proof, .. }
        | Command::SelectCatalogModelPack { owner_proof, .. }
        | Command::SelectModelPack { owner_proof, .. }
        | Command::WithdrawModelPackApproval { owner_proof, .. }
        | Command::InspectInferenceEnvironment { owner_proof, .. }
        | Command::ImportInferenceEquivalence { owner_proof, .. }
        | Command::InferenceEquivalenceStatus { owner_proof, .. }
        | Command::ListInferenceEquivalences { owner_proof, .. }
        | Command::AcceptInferenceEquivalence { owner_proof, .. }
        | Command::WithdrawInferenceEquivalence { owner_proof, .. }
        | Command::ModelPackSelectionStatus { owner_proof } = &request.command
        {
            let difference = self
                .model_pack_owner_token
                .bytes()
                .zip(owner_proof.bytes())
                .fold(0u8, |sum, (a, b)| sum | (a ^ b));
            if owner_proof.len() != self.model_pack_owner_token.len() || difference != 0 {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "model pack intake requires a host-local owner capability",
                ));
            }
            return Ok(());
        }
        if browser_media::is_command(&request.command)
            || browser_timing::is_history_command(&request.command)
        {
            require_grant(db, session, project(request)?, Scope::Read)?;
            require_grant(db, session, project(request)?, Scope::Export)?;
            return Ok(());
        }
        if let Command::HandyInspect { owner_proof, .. }
        | Command::OpenHandySession { owner_proof, .. }
        | Command::CloseHandySession { owner_proof, .. }
        | Command::HandySessions { owner_proof }
        | Command::HandyQualificationContext { owner_proof, .. }
        | Command::ImportHandyQualification { owner_proof, .. }
        | Command::HandyQualificationStatus { owner_proof, .. }
        | Command::AcceptHandyQualification { owner_proof, .. }
        | Command::WithdrawHandyQualification { owner_proof, .. }
        | Command::ImportHandyBrowserStartQualification { owner_proof, .. }
        | Command::ImportHandyHspSourceQualification { owner_proof, .. }
        | Command::HandyBrowserStartQualificationStatus { owner_proof, .. }
        | Command::HandyHspSourceQualificationStatus { owner_proof, .. }
        | Command::AcceptHandyBrowserStartQualification { owner_proof, .. }
        | Command::AcceptHandyHspSourceQualification { owner_proof, .. }
        | Command::WithdrawHandyBrowserStartQualification { owner_proof, .. }
        | Command::WithdrawHandyHspSourceQualification { owner_proof, .. }
        | Command::InspectHandySetup { owner_proof, .. }
        | Command::InspectHandyProgram { owner_proof, .. }
        | Command::PrepareHandyPlayback { owner_proof, .. }
        | Command::AcquireHandyPlaybackClock { owner_proof, .. }
        | Command::RefreshHandyPlaybackTiming { owner_proof, .. }
        | Command::HandyContinuousTimingStatus { owner_proof, .. }
        | Command::BindHandyPlayerGuard { owner_proof, .. }
        | Command::HandyPlayerGuardStatus { owner_proof, .. }
        | Command::IssueHandyPlayerChallenge { owner_proof, .. }
        | Command::ObserveHandyPlayer { owner_proof, .. }
        | Command::ReleaseHandyPlayerGuard { owner_proof, .. }
        | Command::SynchronizeHandyClock { owner_proof, .. }
        | Command::StartHandyPlayback { owner_proof, .. }
        | Command::StartHandyPlaybackFromBrowser { owner_proof, .. }
        | Command::RenewHandyBrowserPlayback { owner_proof, .. }
        | Command::ReleaseHandyBrowserPlayback { owner_proof, .. }
        | Command::HandyBrowserPlaybackStatus { owner_proof, .. }
        | Command::RefreshHandyBrowserPlaybackTiming { owner_proof, .. }
        | Command::HandyPlaybackStatus { owner_proof, .. }
        | Command::HandyStopStatus { owner_proof, .. }
        | Command::HandyHspStatus { owner_proof, .. }
        | Command::BindHandyHspLive { owner_proof, .. }
        | Command::PlayHandyHspLive { owner_proof, .. }
        | Command::StopHandyHspLive { owner_proof, .. }
        | Command::HandyHspLiveStatus { owner_proof, .. }
        | Command::EnableHandyHspContinuity { owner_proof, .. }
        | Command::HandyHspContinuityStatus { owner_proof, .. }
        | Command::PrepareHandyHsp { owner_proof, .. }
        | Command::HandyHspPreparationStatus { owner_proof, .. }
        | Command::CancelHandyHspPreparation { owner_proof, .. }
        | Command::PlanHandyReconciliation { owner_proof, .. }
        | Command::ReconcileHandyEffects { owner_proof, .. }
        | Command::HandyReconciliationStatus { owner_proof, .. }
        | Command::CancelHandyReconciliation { owner_proof, .. }
        | Command::CancelHandyPlayback { owner_proof, .. }
        | Command::HandyClockSyncStatus { owner_proof, .. }
        | Command::CancelHandyClockSync { owner_proof, .. }
        | Command::InspectHandyDeviceTiming { owner_proof, .. }
        | Command::SampleHandyServerClock { owner_proof, .. }
        | Command::AcquireHandyController { owner_proof, .. }
        | Command::RenewHandyController { owner_proof, .. }
        | Command::ReleaseHandyController { owner_proof, .. }
        | Command::HandyControllerStatus { owner_proof, .. }
        | Command::HandyPlaybackPreparationStatus { owner_proof, .. }
        | Command::RetireHandyPlaybackPreparation { owner_proof, .. }
        | Command::SetupHandyDevice { owner_proof, .. }
        | Command::HandySetupStatus { owner_proof, .. }
        | Command::CancelHandySetup { owner_proof, .. }
        | Command::HostHandyStrokeScript { owner_proof, .. }
        | Command::HandyHostingStatus { owner_proof, .. }
        | Command::ForgetHandyHosting { owner_proof, .. }
        | Command::ResolveHandyRegistration { owner_proof, .. }
        | Command::SetHandyRegistration { owner_proof, .. }
        | Command::DisableHandyRegistration { owner_proof, .. }
        | Command::ListHandyRegistrations { owner_proof }
        | Command::InspectHandyRegistration { owner_proof, .. } = &request.command
        {
            let difference = self
                .handy_owner_token
                .bytes()
                .zip(owner_proof.bytes())
                .fold(0u8, |sum, (a, b)| sum | (a ^ b));
            if owner_proof.len() != self.handy_owner_token.len() || difference != 0 {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "Handy operations require a host-local owner capability",
                ));
            }
            if matches!(
                request.command,
                Command::HandySetupStatus { .. }
                    | Command::HandyClockSyncStatus { .. }
                    | Command::HandyPlaybackStatus { .. }
                    | Command::HandyBrowserPlaybackStatus { .. }
                    | Command::ReleaseHandyBrowserPlayback { .. }
                    | Command::HandyContinuousTimingStatus { .. }
                    | Command::HandyPlayerGuardStatus { .. }
                    | Command::IssueHandyPlayerChallenge { .. }
                    | Command::ObserveHandyPlayer { .. }
                    | Command::ReleaseHandyPlayerGuard { .. }
                    | Command::HandyStopStatus { .. }
                    | Command::HandyHspStatus { .. }
                    | Command::HandyHspPreparationStatus { .. }
                    | Command::HandyHspLiveStatus { .. }
                    | Command::HandyHspContinuityStatus { .. }
                    | Command::HandyReconciliationStatus { .. }
                    | Command::CancelHandyReconciliation { .. }
                    | Command::CancelHandyPlayback { .. }
                    | Command::CancelHandyClockSync { .. }
                    | Command::CancelHandySetup { .. }
                    | Command::HandyPlaybackPreparationStatus { .. }
                    | Command::RetireHandyPlaybackPreparation { .. }
                    | Command::ReleaseHandyController { .. }
                    | Command::HandyControllerStatus { .. }
            ) {
                require_grant(db, session, project(request)?, Scope::Read)?;
            }
            if handy_hosting::is_command(&request.command)
                || handy_browser_start_qualification::is_command(&request.command)
                || handy_hsp_source_qualification::is_command(&request.command)
                || handy_hsp_live::is_command(&request.command)
                || matches!(
                    request.command,
                    Command::InspectHandySetup { .. }
                        | Command::PrepareHandyHsp { .. }
                        | Command::CancelHandyHspPreparation { .. }
                        | Command::PlanHandyReconciliation { .. }
                        | Command::ReconcileHandyEffects { .. }
                        | Command::SetupHandyDevice { .. }
                        | Command::InspectHandyProgram { .. }
                        | Command::PrepareHandyPlayback { .. }
                        | Command::AcquireHandyPlaybackClock { .. }
                        | Command::RefreshHandyPlaybackTiming { .. }
                        | Command::BindHandyPlayerGuard { .. }
                        | Command::SynchronizeHandyClock { .. }
                        | Command::StartHandyPlayback { .. }
                        | Command::StartHandyPlaybackFromBrowser { .. }
                        | Command::RenewHandyBrowserPlayback { .. }
                        | Command::RefreshHandyBrowserPlaybackTiming { .. }
                        | Command::InspectHandyDeviceTiming { .. }
                        | Command::SampleHandyServerClock { .. }
                        | Command::AcquireHandyController { .. }
                        | Command::RenewHandyController { .. }
                )
            {
                require_grant(db, session, project(request)?, Scope::Read)?;
                require_grant(db, session, project(request)?, Scope::Export)?;
            }
            return Ok(());
        }
        if let Command::StashListMissing { owner_proof, .. }
        | Command::StashPlanMissing { owner_proof, .. }
        | Command::StashImportSource { owner_proof, .. }
        | Command::StashPublicationStatus { owner_proof, .. }
        | Command::StashExportScript { owner_proof, .. }
        | Command::StashSubmitScan { owner_proof, .. }
        | Command::StashScanStatus { owner_proof, .. }
        | Command::StashReconcileAttachment { owner_proof, .. }
        | Command::StashAddTag { owner_proof, .. }
        | Command::StashTagStatus { owner_proof, .. }
        | Command::StashResumeTag { owner_proof, .. }
        | Command::StashStartBatch { owner_proof, .. }
        | Command::StashBatchStatus { owner_proof, .. }
        | Command::StashResumeBatch { owner_proof, .. }
        | Command::StashCancelBatch { owner_proof, .. }
        | Command::StashMigrateBatch { owner_proof, .. } = &request.command
        {
            let difference = self
                .stash_owner_token
                .bytes()
                .zip(owner_proof.bytes())
                .fold(0u8, |sum, (a, b)| sum | (a ^ b));
            if owner_proof.len() != self.stash_owner_token.len() || difference != 0 {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "Stash requires a host-local owner capability",
                ));
            }
            if !matches!(
                &request.command,
                Command::StashImportSource { .. }
                    | Command::StashPublicationStatus { .. }
                    | Command::StashExportScript { .. }
                    | Command::StashSubmitScan { .. }
                    | Command::StashScanStatus { .. }
                    | Command::StashReconcileAttachment { .. }
                    | Command::StashAddTag { .. }
                    | Command::StashTagStatus { .. }
                    | Command::StashResumeTag { .. }
            ) {
                return Ok(());
            }
        }
        if project_package_imports::is_command(&request.command) {
            return Ok(());
        }
        let (project, scope) = match &request.command {
            Command::RemoveRetainedLiveCudaEvidence { .. }
            | Command::AllowProjectPackaging { .. } => (project(request)?, Scope::ManageGrants),
            Command::StartProjectExport
            | Command::ProjectPackageStatus { .. }
            | Command::CancelProjectExport { .. }
            | Command::ReleaseProjectExport { .. }
            | Command::BeginProjectPackageDownload { .. }
            | Command::ProjectPackageDownloadStatus { .. }
            | Command::AbandonProjectPackageDownload { .. } => {
                (project(request)?, Scope::PackageProject)
            }
            Command::CreateProject { .. }
            | Command::LibraryBatchMigrate { .. }
            | Command::LibraryBatchPrepare { .. }
            | Command::LibraryBatchStatus { .. }
            | Command::LibraryBatchList
            | Command::LibraryBatchAdvance { .. }
            | Command::LibraryBatchRelease { .. }
            | Command::LibraryBatchCancel { .. }
            | Command::Capabilities
            | Command::InferenceCatalog
            | Command::TransferStatus { .. }
            | Command::AbandonTransfer { .. } => return Ok(()),
            Command::OpenProject { project_id } => (project_id, Scope::Read),
            Command::ReviewStatus => (project(request)?, Scope::Read),
            Command::Grant { project_id, .. } | Command::Revoke { project_id, .. } => {
                (project_id, Scope::ManageGrants)
            }
            Command::StartNativeImagePreparation { .. }
            | Command::FinishLiveCapture { .. }
            | Command::CloseBrowserAnalysisInput { .. }
            | Command::StartLiveCapture { .. }
            | Command::StartLiveRecording { .. }
            | Command::DiscardLiveRecording { .. }
            | Command::StageLiveRecording { .. }
            | Command::ContinueLiveCapture { .. }
            | Command::ContinueLiveCaptureAt { .. }
            | Command::PrepareBrowserAnalysisSuccessor { .. }
            | Command::TransferBrowserAnalysisInput { .. }
            | Command::GenerateFromModelPack { .. }
            | Command::Generate { .. }
            | Command::GenerateWithInferenceFallback { .. }
            | Command::GenerateInput { .. }
            | Command::GenerateWithAssistant { .. }
            | Command::GenerateCreativeWithAssistant { .. }
            | Command::CancelJob { .. }
            | Command::ResumeJob { .. } => (project(request)?, Scope::Generate),
            Command::Export { .. }
            | Command::ExportAxis { .. }
            | Command::ExportAxisPinned { .. }
            | Command::ExportDeviceMotion { .. }
            | Command::PrepareHandyStrokeScript { .. }
            | Command::StashPublicationStatus { .. }
            | Command::StashExportScript { .. }
            | Command::StashSubmitScan { .. }
            | Command::StashScanStatus { .. }
            | Command::StashReconcileAttachment { .. }
            | Command::StashAddTag { .. }
            | Command::StashTagStatus { .. }
            | Command::StashResumeTag { .. } => (project(request)?, Scope::Export),
            Command::SetProtectedRegions { .. } => (project(request)?, Scope::ManageProtection),
            Command::ImportSource { .. }
            | Command::ImportSourcePinned { .. }
            | Command::StashImportSource { .. }
            | Command::ImportFunscript { .. }
            | Command::ImportFunscriptForRepair { .. } => (project(request)?, Scope::ImportSource),
            Command::BeginEditUpload { .. }
            | Command::ApplyAssistantEdit { .. }
            | Command::StartAssistant { .. }
            | Command::ResolveAssistantStart { .. }
            | Command::CancelAssistant { .. }
            | Command::StartWasmEdit { .. }
            | Command::ResolveWasmStart { .. }
            | Command::CancelWasmEdit { .. }
            | Command::RemoveUnsuccessfulWasmEdit { .. }
            | Command::ArchiveCompletedWasmEdit { .. }
            | Command::FinishEditUpload { .. }
            | Command::SetReviewDisposition { .. }
            | Command::SetReviewIntervalDisposition { .. }
            | Command::SetSourcePlacement { .. }
            | Command::RetimeMotion { .. }
            | Command::RemoveSourcePlacement { .. }
            | Command::PinSource { .. }
            | Command::EvictSource { .. }
            | Command::Undo
            | Command::Redo
            | Command::CommitCandidate { .. }
            | Command::MergeCandidate { .. }
            | Command::RebaseCandidate { .. } => (project(request)?, Scope::Edit),
            _ => (project(request)?, Scope::Read),
        };
        require_grant(db, session, project, scope)?;
        if matches!(request.command, Command::StartNativeImagePreparation { .. }) {
            require_grant(db, session, project, Scope::Read)?;
        }
        if matches!(
            request.command,
            Command::StartAssistant { .. }
                | Command::ApplyAssistantEdit { .. }
                | Command::ResolveAssistantStart { .. }
                | Command::CancelAssistant { .. }
                | Command::StartWasmEdit { .. }
                | Command::ResolveWasmStart { .. }
                | Command::CancelWasmEdit { .. }
                | Command::RemoveUnsuccessfulWasmEdit { .. }
                | Command::ArchiveCompletedWasmEdit { .. }
        ) {
            require_grant(db, session, project, Scope::Read)?;
        }
        if matches!(
            request.command,
            Command::RemoveRetainedLiveCudaEvidence { .. }
        ) {
            require_grant(db, session, project, Scope::Read)?;
        }
        if matches!(
            request.command,
            Command::StartLiveCapture { .. }
                | Command::ContinueLiveCapture { .. }
                | Command::ContinueLiveCaptureAt { .. }
                | Command::PrepareBrowserAnalysisSuccessor { .. }
                | Command::TransferBrowserAnalysisInput { .. }
                | Command::FinishLiveCapture { .. }
                | Command::CloseBrowserAnalysisInput { .. }
        ) {
            require_grant(db, session, project, Scope::ImportSource)?;
            require_grant(db, session, project, Scope::Read)?;
        }
        if matches!(
            &request.command,
            Command::GenerateFromModelPack { .. }
                | Command::PreviewModelPackFrame { .. }
                | Command::PreviewAnnotatedFrame { .. }
        ) {
            require_grant(db, session, project, Scope::ImportSource)?;
        }
        if matches!(&request.command, Command::StageLiveRecording { .. }) {
            require_grant(db, session, project, Scope::Read)?;
            require_grant(db, session, project, Scope::ImportSource)?;
            require_grant(db, session, project, Scope::Edit)?;
        }
        if let Command::GenerateWithAssistant { model_path, .. } = &request.command {
            require_grant(db, session, project, Scope::Read)?;
            require_grant(db, session, project, Scope::Edit)?;
            if model_path.is_some() {
                require_grant(db, session, project, Scope::ImportSource)?;
            }
        }
        if matches!(
            request.command,
            Command::GenerateCreativeWithAssistant { .. }
        ) {
            require_grant(db, session, project, Scope::Read)?;
            require_grant(db, session, project, Scope::Edit)?;
        }
        if matches!(
            &request.command,
            Command::ImportFunscript { .. } | Command::ImportFunscriptForRepair { .. }
        ) {
            require_grant(db, session, project, Scope::Edit)?;
        }
        if matches!(
            &request.command,
            Command::GenerateWithInferenceFallback { .. }
                | Command::Generate {
                    model_path: Some(_),
                    ..
                }
                | Command::PreviewPlacedFrame {
                    model_path: Some(_),
                    ..
                }
                | Command::PreviewFrame {
                    model_path: Some(_),
                    ..
                }
        ) {
            require_grant(db, session, project, Scope::ImportSource)?;
        }
        if matches!(request.command, Command::PrepareHandyStrokeScript { .. }) {
            require_grant(db, session, project, Scope::Read)?;
        }
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        let enabled = self.tools.is_ok() && launch::available();
        Capabilities {
            protocol_version: PROTOCOL_VERSION,
            operations: [
                "probe_media",
                "assistant_profiles",
                "start_assistant",
                "resolve_assistant_start",
                "generate_with_assistant",
                "assistant_status",
                "cancel_assistant",
                "list_assistant_attempts",
                "native_image_preparation_limits",
                "start_native_image_preparation",
                "resolve_native_image_preparation",
                "native_image_preparation_status",
                "cancel_native_image_preparation",
                "release_native_image_preparation",
                "live_capture_limits",
                "start_live_capture",
                "live_capture_status",
                "stage_live_recording",
                "stop_live_capture",
                "handy_inspect",
                "set_handy_registration",
                "disable_handy_registration",
                "list_handy_registrations",
                "inspect_handy_registration",
                "resolve_handy_registration",
                "open_handy_session",
                "close_handy_session",
                "handy_sessions",
                "set_source_placement",
                "remove_source_placement",
                "preview_placed_frame",
                "preview_annotated_frame",
                "create_project",
                "inference_catalog",
                "stash_list_missing",
                "stash_plan_missing",
                "stash_import_source",
                "stash_export_script",
                "review_status",
                "review_disposition",
                "stash_publication_status",
                "stash_submit_scan",
                "stash_scan_status",
                "stash_reconcile_attachment",
                "stash_add_tag",
                "stash_tag_status",
                "stash_resume_tag",
                "stash_start_batch",
                "stash_batch_status",
                "stash_resume_batch",
                "stash_cancel_batch",
                "export_device_motion",
                "prepare_handy_stroke_script",
                "host_handy_stroke_script",
                "inspect_handy_setup",
                "inspect_handy_program",
                "prepare_handy_playback",
                "acquire_handy_playback_clock",
                "start_handy_playback",
                "handy_stop_status",
                "handy_hsp_status",
                "prepare_handy_hsp",
                "handy_hsp_preparation_status",
                "bind_handy_hsp_live",
                "play_handy_hsp_live",
                "stop_handy_hsp_live",
                "handy_hsp_live_status",
                "inspect_inference_environment",
                "import_inference_equivalence",
                "inference_equivalence_status",
                "list_inference_equivalences",
                "accept_inference_equivalence",
                "withdraw_inference_equivalence",
                "import_handy_hsp_source_qualification",
                "handy_hsp_source_qualification_status",
                "accept_handy_hsp_source_qualification",
                "withdraw_handy_hsp_source_qualification",
                "cancel_handy_hsp_preparation",
                "handy_playback_status",
                "cancel_handy_playback",
                "synchronize_handy_clock",
                "handy_clock_sync_status",
                "cancel_handy_clock_sync",
                "inspect_handy_device_timing",
                "sample_handy_server_clock",
                "acquire_handy_controller",
                "renew_handy_controller",
                "release_handy_controller",
                "handy_controller_status",
                "handy_playback_preparation_status",
                "retire_handy_playback_preparation",
                "setup_handy_device",
                "handy_setup_status",
                "cancel_handy_setup",
                "handy_qualification_context",
                "import_handy_qualification",
                "handy_qualification_status",
                "accept_handy_qualification",
                "withdraw_handy_qualification",
                "handy_hosting_status",
                "forget_handy_hosting",
                "start_project_import",
                "project_import_status",
                "begin_project_package_upload",
                "project_package_upload_status",
                "abandon_project_package_upload",
                "seal_project_import",
                "cancel_project_import",
                "allow_project_packaging",
                "start_project_export",
                "project_package_status",
                "cancel_project_export",
                "release_project_export",
                "begin_project_package_download",
                "project_package_download_status",
                "abandon_project_package_download",
                "open_project",
                "import_source",
                "import_funscript",
                "generate_input",
                "merge_candidate",
                "diagnostics",
                "begin_edit_upload",
                "finish_edit_upload",
                "begin_motion_download",
                "transfer_status",
                "abandon_transfer",
                "undo",
                "redo",
                "generate",
                "job_status",
                "job_attempts",
                "cancel_job",
                "resume_job",
                "get_candidate",
                "commit_candidate",
                "rebase_candidate",
                "get_snapshot",
                "preview_frame",
                "export",
                "export_axis",
                "grant",
                "revoke",
                "events",
            ]
            .into_iter()
            .chain(
                cfg!(all(target_os = "linux", target_arch = "x86_64"))
                    .then_some("generate_with_inference_fallback"),
            )
            .map(str::to_owned)
            .collect(),
            backends: vec![CapabilityStatus {
                name: "isolated_cpu_analysis".into(),
                available: enabled,
                qualification: if enabled {
                    QualificationStatus::Unqualified
                } else {
                    QualificationStatus::Unavailable
                },
                reason: if enabled {
                    Some(
                        "Architecture path only; model quality and performance are not qualified"
                            .into(),
                    )
                } else {
                    Some(
                        self.tools
                            .as_ref()
                            .err()
                            .cloned()
                            .unwrap_or_else(|| "qualified worker confinement unavailable".into()),
                    )
                },
            }],
            physical_playback: false,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn cuda_owner_policy_status(
        &self,
    ) -> PResult<Option<crate::cuda_owner_policy::CudaOwnerPolicyStatus>> {
        let Some(owner) = self.cuda_owner_policy.as_ref() else {
            return Ok(None);
        };
        let mut status = owner.status();
        status.pool_usage = Some(
            self.pool
                .gpu_usage(&owner.policy().accounting.device_uuid)?,
        );
        Ok(Some(status))
    }

    fn inference_catalog(&self) -> InferenceCatalog {
        #[allow(unused_mut)]
        let mut catalog=InferenceCatalog {
            schema_version: 1,
            worker_available: self.tools.is_ok() && launch::available(),
            runtime: self.tools.as_ref().ok().and_then(|tools| tools.onnx_runtime.as_ref())
                .map(|runtime| runtime.identity.clone()),
            providers: vec![InferenceProvider::Cpu],
            maximum_input_dimension: 1024,
            maximum_class_count: 256,
            maximum_model_bytes: self.config.max_snapshot_bytes,
            cpu_threads: 2,
            memory_bytes: self.config.max_ram_bytes,
            output_bytes: OUTPUT_LIMIT,
            qualified_profiles: Vec::new(),
            qualified_fallback_available: false,
            limitation: "Custom RGB/NCHW stretch or centered linear letterbox YOLO channel-major and fixed temporal RGB-pair script-position adapters are implemented on CPU. No model, accelerated provider or equivalent fallback is qualified in this catalog. Fresh RAM claims use the larger of 1536 MiB and the declared profile minimum, within the owner pool; current aggregate reservations can still refuse admission.".into(),
        };
        #[cfg(target_os = "linux")]
        if let Some(owner) = &self.cuda_owner_policy {
            if cfg!(feature = "cuda")
                && catalog.worker_available
                && owner
                    .policy()
                    .native_dependencies
                    .libraries
                    .iter()
                    .any(|lib| lib.names.iter().any(|name| name == "libnvidia-ml.so.1"))
            {
                catalog.providers.push(InferenceProvider::Cuda);
            }
            catalog.limitation.push_str(" Owner CUDA policy is configured; explicit generation requires a CUDA-enabled build and captured NVML. CUDA detector frame Preview requires an explicit profile; opt-in fallback requires accepted evidence and fresh admission for each selected profile.");
        }
        catalog
    }

    fn source(
        &self,
        db: &Connection,
        project: &ProjectId,
        version: &SourceVersionId,
    ) -> PResult<SourceArtifact> {
        let row: Option<(String, String, i64, i64)> = db
            .query_row(
                "SELECT path,sha256,bytes,evicted FROM sources WHERE project=?1 AND version=?2",
                params![project.as_str(), version.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(internal)?;
        let (path, hash, bytes, evicted) = row.ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, "source not imported into this project")
        })?;
        if evicted != 0 {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "source snapshot evicted; reimport matching content before execution",
            ));
        }
        Ok(SourceArtifact {
            source_version: version.clone(),
            path: PathBuf::from(path),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                sha256: hash,
                byte_len: bytes as u64,
            },
        })
    }

    fn snapshot_model(&self, path: &Path) -> PResult<SourceArtifact> {
        let captured_digest =
            (path.parent() == Some(self.config.state_dir.join("snapshots").as_path())).then(|| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_owned()
            });
        let captured_digest = captured_digest.filter(|name| {
            name.len() == 64
                && name
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        });
        let (path, hash, bytes) = artifacts::snapshot_reserved(
            path,
            &self.config.state_dir.join("snapshots"),
            self.config.max_snapshot_bytes,
            self.pool.reserved_storage(),
        )
        .map_err(unavailable)?;
        if captured_digest
            .as_ref()
            .is_some_and(|expected| expected != &hash)
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Captured model differs from original content-addressed identity",
            ));
        }
        Ok(SourceArtifact {
            source_version: SourceVersionId::new(hash.clone()).map_err(internal)?,
            path,
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                sha256: hash,
                byte_len: bytes,
            },
        })
    }

    fn prepare_operation(
        &self,
        tx: &Transaction<'_>,
        request: &Request,
        source: &SourceVersionId,
        operation: WorkerOperation,
        previous: Option<&WorkerRequest>,
    ) -> PResult<(WorkerRequest, Reservation, Arc<AtomicBool>)> {
        self.prepare_operation_with_dependencies(tx, request, source, operation, previous, &[])
    }

    fn prepare_operation_with_dependencies(
        &self,
        tx: &Transaction<'_>,
        request: &Request,
        source: &SourceVersionId,
        operation: WorkerOperation,
        previous: Option<&WorkerRequest>,
        extra: &[ArtifactIdentity],
    ) -> PResult<(WorkerRequest, Reservation, Arc<AtomicBool>)> {
        self.prepare_operation_with_capture(
            tx,
            request,
            source,
            operation,
            OperationDependencies {
                origin: AttemptOrigin::from_resume(previous),
                extra,
                #[cfg(target_os = "linux")]
                cuda: None,
                #[cfg(target_os = "linux")]
                cpu: None,
                #[cfg(target_os = "linux")]
                fallback: None,
            },
        )
    }

    fn prepare_operation_with_capture(
        &self,
        tx: &Transaction<'_>,
        request: &Request,
        source: &SourceVersionId,
        operation: WorkerOperation,
        dependencies: OperationDependencies<'_>,
    ) -> PResult<(WorkerRequest, Reservation, Arc<AtomicBool>)> {
        let OperationDependencies {
            origin,
            extra,
            #[cfg(target_os = "linux")]
            cuda,
            #[cfg(target_os = "linux")]
            cpu,
            #[cfg(target_os = "linux")]
            fallback,
        } = dependencies;
        let previous = origin.previous();
        origin.validate_context(project(request)?, source)?;
        origin.validate_current(tx)?;
        #[cfg(target_os = "linux")]
        if cuda.is_some() && cpu.is_some() {
            return Err(ProtocolError::invalid(
                "one attempt cannot select both CPU and CUDA images",
            ));
        }
        #[cfg(target_os = "linux")]
        if self.cpu_owner_policy.is_some()
            && matches!(&operation, WorkerOperation::Generate { model: Some(_), settings, .. }
                if settings.inference_profile.as_ref().is_none_or(|p| p.provider == InferenceProvider::Cpu))
            && cpu.is_none()
        {
            return Err(ProtocolError::unsupported(
                "configured CPU runtime requires a freshly captured image",
            ));
        }
        if let WorkerOperation::Generate {
            preset,
            settings,
            model,
        } = &operation
        {
            settings.validate_execution_preset(preset, model.is_some())?;
            if settings.annotated_view.is_some()
                && (model.is_none()
                    || !settings.inference_profile.as_ref().is_some_and(|p| {
                        matches!(p.provider, InferenceProvider::Cpu | InferenceProvider::Cuda)
                    }))
            {
                return Err(ProtocolError::unsupported(
                    "annotated generation requires an explicit CPU or CUDA model/profile",
                ));
            }
        }
        let declared = match &operation {
            WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
            WorkerOperation::Preview {
                inference_profile, ..
            } => inference_profile.as_deref(),
            WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
        };
        let budget = crate::inference_budget::admit(
            ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: OUTPUT_LIMIT,
                wall_time_ms: self.config.worker_timeout.as_millis().min(u64::MAX as u128) as u64,
                cpu_threads: 2,
            },
            declared,
            origin.retained_budget(),
            self.config.max_ram_bytes,
        )?;
        if let Some(profile) = declared.filter(|p| p.provider == InferenceProvider::Cuda) {
            #[cfg(not(target_os = "linux"))]
            return Err(ProtocolError::unsupported(
                "unsupported pinned provider; CUDA owner policy requires Linux",
            ));
            #[cfg(target_os = "linux")]
            {
                let owner = self.cuda_owner_policy.as_ref().ok_or_else(|| {
                    ProtocolError::unsupported(
                        "unsupported pinned provider; no owner-configured CUDA policy",
                    )
                })?;
                let priority = if matches!(&operation, WorkerOperation::Preview { .. }) {
                    crate::resources::GpuPriority::Interactive
                } else {
                    crate::resources::GpuPriority::Offline
                };
                owner
                    .policy()
                    .authorize_profile(profile, &budget, priority)
                    .map_err(|error| {
                        ProtocolError::new(ErrorCode::DependencyMismatch, error.to_string())
                    })?;
                if !cfg!(feature = "cuda") || cuda.is_none() {
                    return Err(ProtocolError::unsupported("unsupported pinned provider; CUDA generation requires a CUDA-enabled build and retained supervised capture"));
                }
            }
        }
        if let WorkerOperation::Generate { settings, .. } = &operation {
            if let Some(placement) = &settings.source_placement {
                let active =
                    source_placements::active(tx, project(request)?, &placement.placement_id)?;
                if &active != placement || &placement.spec.source_version != source {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "generation placement is not the active immutable source mapping",
                    ));
                }
            }
        }
        if matches!(&operation, WorkerOperation::Generate { .. })
            || matches!(&operation, WorkerOperation::GenerateInput { input } if input.source_version().is_some())
        {
            source_kinds::require_media(tx, project(request)?, source)?;
        }
        if !launch::available() {
            return Err(ProtocolError::unsupported("qualified worker confinement"));
        }
        let offline: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE state IN ('queued','running','fallback_pending') AND id!=?1",
                [origin.previous().map(|previous|previous.job_id.as_str()).unwrap_or("")],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if offline as usize >= self.config.max_jobs.saturating_sub(1) {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "offline work cannot consume reserved interactive worker slot",
            ));
        }
        let tools = self.tools.as_ref().map_err(unavailable)?.clone();
        let storage_limit = if matches!(&operation,WorkerOperation::Generate {settings,..}
            if settings.inference_profile.as_ref().is_none_or(|p|p.provider==InferenceProvider::Cpu))
        {
            OUTPUT_LIMIT * 4
        } else {
            OUTPUT_LIMIT
        };
        let reservation = self
            .pool
            .reserve_worker(budget.memory_bytes, budget.cpu_threads)?
            .with_storage(
                storage_limit + 16 * 1024,
                fs2::available_space(&self.config.state_dir).map_err(internal)?,
            )?;
        if fs2::available_space(&self.config.state_dir).map_err(internal)?
            < storage_limit + 16 * 1024 * 1024
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "insufficient attempt storage",
            ));
        }
        let project = project(request)?;
        let snapshot = project_snapshot(tx, project)?;
        let independent_audio = if let WorkerOperation::Generate { settings, .. } = &operation {
            if let Some(audio) = settings
                .mixed_audio
                .as_ref()
                .and_then(|audio| audio.independent.as_ref())
            {
                if &audio.source_version == source {
                    return Err(ProtocolError::invalid(
                        "independent audio requires a distinct imported source",
                    ));
                }
                source_kinds::require_media(tx, project, &audio.source_version)?;
                Some(self.source(tx, project, &audio.source_version)?)
            } else {
                None
            }
        } else {
            None
        };
        let job_id = previous
            .map(|p| p.job_id.clone())
            .unwrap_or_else(|| id!(JobId));
        let attempt_id = id!(AttemptId);
        let output_dir = self
            .config
            .state_dir
            .join("attempts")
            .join(attempt_id.as_str());
        origin.create_output_directory(&output_dir)?;
        let mut manifest = WorkerRequest {
            cuda_execution: None,
            independent_audio,
            version: PROTOCOL_VERSION,
            attempt_id: attempt_id.clone(),
            job_id: job_id.clone(),
            project_id: project.clone(),
            base_revision: previous
                .map(|p| p.base_revision)
                .unwrap_or(snapshot.revision),
            source: self.source(tx, project, source)?,
            output_dir,
            operation,
            budget,
            dependencies: vec![
                tools.ffmpeg.identity.clone(),
                tools.ffprobe.identity.clone(),
            ],
            tools,
        };
        #[cfg(target_os = "linux")]
        if let Some(cuda) = cuda {
            cuda.bind(self, &mut manifest)?;
        }
        #[cfg(target_os = "linux")]
        if let Some(cpu) = cpu {
            cpu.bind(self, &mut manifest)?;
        }
        #[cfg(target_os = "linux")]
        if let Some(fallback) = fallback {
            fallback.bind(&mut manifest)?;
        }
        let profile = if manifest.cuda_execution.is_some() {
            crate::inference_profiles::admit_cuda_worker_profile(&manifest).map_err(unavailable)?;
            match &manifest.operation {
                WorkerOperation::Generate { settings, .. } => settings.inference_profile.clone(),
                _ => None,
            }
        } else {
            crate::inference_profiles::resolve_profile(&manifest)
                .map_err(|error| ProtocolError::invalid(error.to_string()))?
        };
        if let Some(profile) = profile {
            manifest
                .dependencies
                .push(crate::inference_profiles::profile_identity(&profile).map_err(internal)?);
        }
        manifest.dependencies.push(self.binary_identity.clone());
        if let Some(audio) = &manifest.independent_audio {
            manifest.dependencies.push(audio.identity.clone());
        }
        if let Some(runtime) = &manifest.tools.onnx_runtime {
            if !manifest.dependencies.contains(&runtime.identity) {
                manifest.dependencies.push(runtime.identity.clone());
            }
        }
        if let WorkerOperation::Generate {
            model: Some(model), ..
        } = &manifest.operation
        {
            manifest.dependencies.push(model.identity.clone());
        }
        for dependency in extra.iter().chain(origin.retained_dependencies().iter()) {
            if !manifest.dependencies.contains(dependency) {
                manifest.dependencies.push(dependency.clone());
            }
        }
        manifest.validate()?;
        origin.validate_successor(&manifest)?;
        if let Some(previous) = origin.resume() {
            crate::generation_progress::capture_resume(previous, &manifest).map_err(unavailable)?;
        }
        assistant_image_origin::retained(&self.config.state_dir, &manifest)?;
        let raw = encode(&manifest)?;
        // Retain the exact immutable request boundary. CPU progress, when
        // captured, has its own checked private body/identity sidecar.
        let checkpoint_path = manifest.output_dir.join("initial-checkpoint.json");
        artifacts::publish_export(&checkpoint_path, raw.as_bytes()).map_err(unavailable)?;
        let mut permissions = fs::metadata(&checkpoint_path)
            .map_err(unavailable)?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&checkpoint_path, permissions).map_err(unavailable)?;
        if previous.is_some() {
            let count = if origin.resume().is_some() {
                inference_fallback_store::ordinary_attempt_count(tx, &job_id)?
            } else {
                0
            };
            let total: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM attempts WHERE job=?1",
                    [job_id.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            let recorded: i64 = tx
                .query_row(
                    "SELECT attempts FROM jobs WHERE id=?1",
                    [job_id.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if recorded != total {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "attempt counter differs from retained history",
                ));
            }
            if total >= 128 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "retained attempt limit reached",
                ));
            }
            if count >= 3 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "bounded attempt retry limit reached",
                ));
            }
            origin.advance_job(tx, &manifest)?;
        } else {
            tx.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,?4,?5,?6,'queued',?7)",params![job_id.as_str(),attempt_id.as_str(),project.as_str(),rev_sql(manifest.base_revision)?,request.session.as_ref().unwrap().as_str(),source.as_str(),raw]).map_err(internal)?;
        }
        tx.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![attempt_id.as_str(), job_id.as_str(), raw],
        )
        .map_err(internal)?;
        #[cfg(target_os = "linux")]
        if let Some(cpu) = cpu {
            crate::cpu_runtime_db::declare(tx, &manifest, &cpu.descriptor).map_err(unavailable)?;
        }
        #[cfg(target_os = "linux")]
        if let Some(fallback) = fallback {
            fallback.declare(tx, &manifest, origin.previous())?;
        }
        if previous.is_none() {
            model_pack_withdrawal::record_admission(tx, &manifest, request, extra)?;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        if manifest.cuda_execution.is_some() {
            crate::cuda_attempts::declare(tx, &manifest, &self.engine_epoch).map_err(internal)?;
        }
        self.arm_worker_fence(tx)?;
        if matches!(&manifest.operation,WorkerOperation::Generate{settings,..} if settings.annotated_view.is_some())
        {
            crate::annotated_generation_db::retain(tx, &manifest, &self.pool).map_err(internal)?;
        }
        event(
            tx,
            project,
            snapshot.revision,
            EventBody::JobChanged(job_snapshot(tx, &job_id, project)?),
        )?;
        Ok((manifest, reservation, cancel))
    }

    fn validate_dependencies(&self, manifest: &WorkerRequest) -> PResult<()> {
        let tools = self.tools.as_ref().map_err(unavailable)?;
        if manifest.tools.ffmpeg.identity.sha256 != tools.ffmpeg.identity.sha256
            || manifest.tools.ffprobe.identity.sha256 != tools.ffprobe.identity.sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "worker tool dependencies changed; new job required",
            ));
        }
        #[cfg(target_os = "linux")]
        let captured_cpu = self.validate_cpu_image_dependencies(manifest)?;
        #[cfg(not(target_os = "linux"))]
        let captured_cpu = {
            if manifest.dependencies.iter().any(|d| {
                d.artifact_id
                    .as_str()
                    .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
            }) {
                return Err(ProtocolError::unsupported(
                    "captured CPU execution requires Linux",
                ));
            }
            false
        };
        if (!captured_cpu
            && manifest.tools.onnx_runtime.as_ref().map(|r| &r.identity)
                != tools.onnx_runtime.as_ref().map(|r| &r.identity))
            || !manifest.dependencies.contains(&self.binary_identity)
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "runtime or executable dependency changed; new job required",
            ));
        }
        let (hash, size) =
            artifacts::digest_file(&manifest.source.path, self.config.max_snapshot_bytes)
                .map_err(unavailable)?;
        if hash != manifest.source.identity.sha256 || size != manifest.source.identity.byte_len {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "immutable source identity mismatch",
            ));
        }
        if let Some(audio) = &manifest.independent_audio {
            let (hash, size) = artifacts::digest_file(&audio.path, self.config.max_snapshot_bytes)
                .map_err(unavailable)?;
            if hash != audio.identity.sha256 || size != audio.identity.byte_len {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "immutable independent audio identity mismatch",
                ));
            }
        }
        let model = match &manifest.operation {
            WorkerOperation::Generate { model, .. } | WorkerOperation::Preview { model, .. } => {
                model.as_ref()
            }
            WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
        };
        if let Some(model) = model {
            let (hash, size) = artifacts::digest_file(&model.path, self.config.max_snapshot_bytes)
                .map_err(unavailable)?;
            if hash != model.identity.sha256 || size != model.identity.byte_len {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "model dependency changed",
                ));
            }
        }
        Ok(())
    }

    fn validate_checkpoint(&self, manifest: &WorkerRequest) -> PResult<()> {
        let path = manifest.output_dir.join("initial-checkpoint.json");
        let (hash, size) = artifacts::digest_file(&path, MAX_CONTROL_BYTES as u64)
            .map_err(|e| ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string()))?;
        let expected = encode(manifest)?;
        if size != expected.len() as u64
            || hash != format!("{:x}", Sha256::digest(expected.as_bytes()))
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "checkpoint does not match exact persisted attempt dependencies/configuration",
            ));
        }
        Ok(())
    }

    fn recover_interrupted(self: &Arc<Self>) {
        let result = (|| -> PResult<Vec<(WorkerRequest, Reservation, Arc<AtomicBool>)>> {
            let _artifact_guard = self.artifact_gate.lock().map_err(internal)?;
            let mut db = self.db.lock().map_err(internal)?;
            let pending = {
                let mut stmt=db.prepare("SELECT manifest,session FROM jobs WHERE state='interrupted' ORDER BY rowid LIMIT 16").map_err(internal)?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                    .map_err(internal)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(internal)?
            };
            let mut launches = Vec::new();
            for (raw, owner) in pending {
                let manifest: WorkerRequest = decode(&raw)?;
                // A retained declaration cannot restore an ephemeral captured image.
                // Resume requires fresh owner checks, resource admission and capture.
                if manifest.cuda_execution.is_some()
                    || manifest.dependencies.iter().any(|d| {
                        d.artifact_id
                            .as_str()
                            .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
                            || d.artifact_id
                                .as_str()
                                .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
                    })
                {
                    continue;
                }
                let owner = SessionId::new(owner).map_err(internal)?;
                if self
                    .validate_dependencies(&manifest)
                    .and_then(|_| self.validate_checkpoint(&manifest))
                    .is_err()
                {
                    continue;
                }
                if require_grant(&db, &owner, &manifest.project_id, Scope::Generate).is_err() {
                    continue;
                }
                if matches!(&manifest.operation, WorkerOperation::Preview { .. }) {
                    continue;
                }
                let request = Request::new(
                    id!(RequestId),
                    Command::ResumeJob {
                        job_id: manifest.job_id.clone(),
                    },
                )
                .with_session(owner)
                .in_project(manifest.project_id.clone(), None);
                let tx = db.transaction().map_err(internal)?;
                match self.prepare_operation(
                    &tx,
                    &request,
                    &manifest.source.source_version,
                    manifest.operation.clone(),
                    Some(&manifest),
                ) {
                    Ok(work) => {
                        tx.commit().map_err(internal)?;
                        launches.push(work);
                    }
                    Err(error) if error.code == ErrorCode::ResourceExhausted => break,
                    Err(_) => continue,
                }
            }
            Ok(launches)
        })();
        if let Ok(launches) = result {
            for (manifest, reservation, cancel) in launches {
                let engine = self.clone();
                let failed_manifest = manifest.clone();
                if let Err(error) = self.spawn_task("pulsar-generation-recovery", None, move || {
                    engine.run_job(
                        manifest,
                        cancel,
                        reservation,
                        #[cfg(target_os = "linux")]
                        None,
                        #[cfg(target_os = "linux")]
                        None,
                        #[cfg(target_os = "linux")]
                        None,
                    );
                }) {
                    let _ = self.finish_job(&failed_manifest, Err(unavailable(error)));
                }
            }
        }
    }

    fn arm_worker_fence(&self, tx: &Transaction<'_>) -> PResult<()> {
        #[cfg(target_os = "linux")]
        {
            // Refuse an already committed requirement below its bound ledger;
            // activation must never silently repair a downgraded owner.
            let version: i64 = tx
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .map_err(internal)?;
            if crate::worker_fence_db::binding(tx, version)
                .map_err(internal)?
                .is_some()
            {
                self.require_worker_fence(tx)?;
            }
            self.worker_fence.enable_creation().map_err(internal)?;
            crate::worker_fence_db::arm_creation(tx, &self.worker_fence.scope().map_err(internal)?)
                .map_err(internal)?;
            self.require_worker_fence(tx)?;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = tx;
        Ok(())
    }
    fn arm_staging_fence(&self, tx: &Transaction<'_>) -> PResult<()> {
        #[cfg(target_os = "linux")]
        {
            self.worker_fence.enable_staging().map_err(internal)?;
            crate::worker_fence_db::arm_staging(tx, &self.worker_fence.scope().map_err(internal)?)
                .map_err(internal)?;
            self.require_worker_fence(tx)?;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = tx;
        Ok(())
    }
    fn require_worker_fence(&self, db: &Connection) -> PResult<()> {
        #[cfg(target_os = "linux")]
        {
            let version: i64 = db
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .map_err(internal)?;
            let binding = crate::worker_fence_db::binding(db, version).map_err(internal)?;
            let scope = self.worker_fence.scope().map_err(internal)?;
            let ledger_version = self.worker_fence.version().map_err(internal)?;
            if !binding
                .is_some_and(|b| b.scope == scope && b.minimum_ledger_version <= ledger_version)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "worker fence scope or ledger requirement is not armed",
                ));
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = db;
        Ok(())
    }

    fn run_job(
        self: &Arc<Self>,
        manifest: WorkerRequest,
        cancel: Arc<AtomicBool>,
        reservation: Reservation,
        #[cfg(target_os = "linux")] cuda: Option<cuda_generation::PreparedCudaGeneration>,
        #[cfg(target_os = "linux")] cpu: Option<cpu_generation::PreparedCpuGeneration>,
        #[cfg(target_os = "linux")] fallback: Option<
            inference_fallback_execution::PreparedFallback,
        >,
    ) {
        let mut completion_claim = Some(Arc::new(reservation));
        let mut fallback_lease: Option<inference_fallback_lifetime::Lease> = None;
        let launched = (|| -> PResult<launch::AttemptReport> {
            if self.is_shutting_down() {
                return Err(ProtocolError::new(
                    ErrorCode::Cancelled,
                    "Engine is shutting down",
                ));
            }
            {
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db.transaction().map_err(internal)?;
                self.require_worker_fence(&tx)?;
                let retained_fallback = inference_fallback_store::retained(&tx, &manifest)?;
                #[cfg(target_os = "linux")]
                match (fallback.as_ref(), retained_fallback.as_ref()) {
                    (Some(prepared), Some(_)) => {
                        let admitted = prepared.require_bound(&tx, &manifest)?;
                        let owner: String = tx
                            .query_row(
                                "SELECT session FROM jobs WHERE id=?1",
                                [manifest.job_id.as_str()],
                                |row| row.get(0),
                            )
                            .map_err(internal)?;
                        require_grant(
                            &tx,
                            &SessionId::new(owner.clone()).map_err(internal)?,
                            &manifest.project_id,
                            Scope::Generate,
                        )?;
                        fallback_lease = Some(
                            self.fallback_lifetimes.register(
                                inference_fallback_lifetime::Registration {
                                    job: manifest.job_id.clone(),
                                    project: manifest.project_id.clone(),
                                    owner: SessionId::new(owner).map_err(internal)?,
                                    attempt: manifest.attempt_id.clone(),
                                    reference: admitted.reference,
                                    admitted_at_unix_ms: admitted.admitted_at_unix_ms,
                                    expires_at_unix_ms: admitted.expires_at_unix_ms,
                                    attempt_budget_ms: manifest.budget.wall_time_ms,
                                    original_deadline: std::time::Instant::now()
                                        .checked_add(std::time::Duration::from_millis(
                                            manifest.budget.wall_time_ms,
                                        ))
                                        .ok_or_else(|| {
                                            ProtocolError::invalid(
                                                "fallback execution deadline overflow",
                                            )
                                        })?,
                                    cancel: cancel.clone(),
                                },
                            )?,
                        );
                    }
                    (None, None) => {}
                    _ => {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "fallback dispatch lacks its exact live preparation",
                        ))
                    }
                }
                #[cfg(not(target_os = "linux"))]
                if retained_fallback.is_some() {
                    return Err(ProtocolError::unsupported(
                        "fallback runtime requires supported captured execution",
                    ));
                }
                let retained_cpu =
                    crate::cpu_runtime_db::retained(&tx, &manifest).map_err(unavailable)?;
                #[cfg(target_os = "linux")]
                if retained_cpu.as_ref().map(encode).transpose()?
                    != cpu
                        .as_ref()
                        .map(|prepared| encode(&prepared.descriptor))
                        .transpose()?
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "CPU dispatch differs from retained attempt image",
                    ));
                }
                #[cfg(not(target_os = "linux"))]
                if retained_cpu.is_some() {
                    return Err(ProtocolError::unsupported(
                        "captured CPU execution requires Linux",
                    ));
                }
                let changed=tx.execute("UPDATE jobs SET state='running' WHERE id=?1 AND attempt=?2 AND state='queued'",params![manifest.job_id.as_str(),manifest.attempt_id.as_str()]).map_err(internal)?;
                if changed != 1 {
                    return Err(ProtocolError::new(
                        ErrorCode::Cancelled,
                        "job cancelled before dispatch",
                    ));
                }
                tx.execute(
                    "UPDATE attempts SET state='running' WHERE id=?1",
                    [manifest.attempt_id.as_str()],
                )
                .map_err(internal)?;
                if manifest.cuda_execution.is_some() {
                    crate::cuda_attempts::require_running(&tx, &manifest, &self.engine_epoch)
                        .map_err(unavailable)?;
                }
                let mut workers = self.workers.lock().map_err(internal)?;
                workers.insert(manifest.job_id.to_string(), cancel.clone());
                if self.is_shutting_down() {
                    cancel.store(true, Ordering::Release);
                }
                if let Err(error) = tx.commit() {
                    workers.remove(manifest.job_id.as_str());
                    return Err(internal(error));
                }
                drop(workers);
            }
            self.validate_dependencies(&manifest)?;
            if let Some(lease) = &fallback_lease {
                lease.check()?;
            }
            #[cfg(target_os = "linux")]
            if manifest.cuda_execution.is_some() {
                let prepared = cuda.ok_or_else(|| {
                    ProtocolError::unsupported("CUDA dispatch lacks its retained capture")
                })?;
                let owner = self
                    .cuda_owner_policy
                    .as_ref()
                    .ok_or_else(|| ProtocolError::unsupported("CUDA owner policy unavailable"))?;
                let mut queued = Arc::try_unwrap(
                    completion_claim.take().expect("queued CPU claim"),
                )
                .map_err(|_| {
                    ProtocolError::new(ErrorCode::Internal, "queued CPU claim has another owner")
                })?;
                let mut output_storage = queued.split_storage()?;
                let custody = self
                    .worker_fence
                    .admit_group(
                        Arc::new(queued),
                        manifest.budget.memory_bytes,
                        manifest.budget.cpu_threads,
                    )
                    .map_err(unavailable)?
                    .into_worker();
                let observed = crate::cuda_memory::observe(
                    &self.config,
                    &manifest,
                    prepared.image,
                    prepared.devices,
                    custody,
                    &cancel,
                )
                .map_err(unavailable)?;
                let mut admitted = observed
                    .admit(
                        &self.pool,
                        owner,
                        &manifest,
                        crate::resources::GpuPriority::Offline,
                    )
                    .map_err(unavailable)?;
                Arc::get_mut(&mut admitted.reservation)
                    .ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::Internal,
                            "fresh CUDA claim has another owner",
                        )
                    })?
                    .absorb_storage(&mut output_storage)?;
                completion_claim = Some(admitted.reservation.clone());
                return Ok(if let Some(lease) = &fallback_lease {
                    lease.check()?;
                    crate::launch_cuda::execute_reserved_attempt_before(
                        &self.config,
                        &manifest,
                        &cancel,
                        admitted,
                        self.worker_fence.clone(),
                        lease.deadline(),
                    )
                } else {
                    crate::launch_cuda::execute_reserved_attempt(
                        &self.config,
                        &manifest,
                        &cancel,
                        admitted,
                        self.worker_fence.clone(),
                    )
                });
            }
            #[cfg(target_os = "linux")]
            if let Some(cpu) = cpu {
                return Ok(if let Some(lease) = &fallback_lease {
                    lease.check()?;
                    crate::launch_cpu::execute_reserved_attempt_before(
                        &self.config,
                        &manifest,
                        &cancel,
                        cpu.image,
                        cpu.descriptor,
                        completion_claim.as_ref().expect("CPU claim").clone(),
                        self.worker_fence.clone(),
                        lease.deadline(),
                    )
                } else {
                    crate::launch_cpu::execute_reserved_attempt(
                        &self.config,
                        &manifest,
                        &cancel,
                        cpu.image,
                        cpu.descriptor,
                        completion_claim.as_ref().expect("CPU claim").clone(),
                        self.worker_fence.clone(),
                    )
                });
            }
            Ok(launch::execute_reserved_attempt(
                &self.config,
                &manifest,
                &cancel,
                completion_claim.as_ref().expect("CPU claim").clone(),
                #[cfg(target_os = "linux")]
                self.worker_fence.clone(),
            ))
        })();
        let (mut outcome, settled) = match launched {
            Ok(report) => report.into_parts(),
            Err(error) => (Err(error), None),
        };
        let worker_retirement = match (&outcome, settled.as_ref()) {
            (Ok(_), _) => Some(WorkerRetirementObservation {
                worker_error: None,
                provider_unavailable: None,
            }),
            (Err(_), Some(settled)) => Some(WorkerRetirementObservation {
                worker_error: Some(JobAttemptDiagnostic::from_error(settled.error())),
                provider_unavailable: settled.provider_unavailable().cloned(),
            }),
            _ => None,
        };
        if let Some(lease) = &fallback_lease {
            if let Err(error) = lease.check() {
                outcome = Err(error);
            }
        }
        let pending = self.finish_job_observed_inner(
            &manifest,
            outcome,
            worker_retirement,
            fallback_lease.as_ref(),
            settled.as_ref(),
        );
        if let Ok(mut workers) = self.workers.lock() {
            if workers
                .get(manifest.job_id.as_str())
                .is_some_and(|current| Arc::ptr_eq(current, &cancel))
            {
                workers.remove(manifest.job_id.as_str());
            }
        }
        let retirement = fallback_lease
            .map(inference_fallback_lifetime::Lease::retire)
            .transpose();
        #[cfg(target_os = "linux")]
        drop(fallback);
        drop(completion_claim);
        match (pending, retirement) {
            (Ok(true), Ok(_)) => {
                #[cfg(target_os = "linux")]
                if let Some(settled) = settled {
                    if let Err(error) = self.continue_inference_fallback(&manifest, settled) {
                        let _ = self.fail_pending_fallback(&manifest, &error);
                    }
                }
            }
            (Ok(true), Err(error)) => { let _ = self.fail_pending_fallback(&manifest, &error); }
            (Err(error), _) => eprintln!("pulsar: attempt completion was not durably recorded ({:?}); automatic fallback suppressed", error.code),
            _ => {},
        }
    }

    fn finish_job(
        &self,
        manifest: &WorkerRequest,
        outcome: PResult<launch::LaunchExecution>,
    ) -> PResult<()> {
        self.finish_job_observed(manifest, outcome, None)
    }

    fn finish_job_observed(
        &self,
        manifest: &WorkerRequest,
        outcome: PResult<launch::LaunchExecution>,
        worker_retirement: Option<WorkerRetirementObservation>,
    ) -> PResult<()> {
        self.finish_job_observed_inner(manifest, outcome, worker_retirement, None, None)
            .map(|_| ())
    }

    fn finish_job_observed_inner(
        &self,
        manifest: &WorkerRequest,
        outcome: PResult<launch::LaunchExecution>,
        worker_retirement: Option<WorkerRetirementObservation>,
        fallback_lease: Option<&inference_fallback_lifetime::Lease>,
        settled: Option<&launch::SettledWorkerFailure>,
    ) -> PResult<bool> {
        // Validate worker bytes before entering the durable publication transaction.
        let result: PResult<PreparedJobResult> = (|| {
            let execution = outcome?;
            manifest.validate()?;
            let cuda_evidence = match (&execution.cuda_artifacts, manifest.cuda_execution.as_ref())
            {
                (Some(evidence), Some(_)) => {
                    crate::cuda_result::verify_files(manifest, evidence).map_err(unavailable)?;
                    Some(evidence.clone())
                }
                (None, None) => None,
                _ => {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "completion CUDA evidence differs from declared provider",
                    ))
                }
            };
            match execution.output {
                WorkerOutput::Generated {
                    program,
                    receipt,
                    lineage,
                    review,
                } => {
                    if let Some((origin, _, _)) =
                        assistant_image_origin::retained(&self.config.state_dir, manifest)?
                    {
                        if !lineage.contains(&origin)
                            || manifest.dependencies.iter().any(|d| !lineage.contains(d))
                        {
                            return Err(ProtocolError::new(
                                ErrorCode::DependencyMismatch,
                                "worker omitted image provenance dependencies",
                            ));
                        }
                    }
                    validate_reviews(&review)?;
                    let original_program = program.identity.clone();
                    let path = program.path.canonicalize().map_err(unavailable)?;
                    let root = manifest.output_dir.canonicalize().map_err(unavailable)?;
                    if !path.starts_with(&root) {
                        return Err(ProtocolError::new(
                            ErrorCode::Forbidden,
                            "worker artifact escaped attempt directory",
                        ));
                    }
                    let (hash, bytes) =
                        artifacts::digest_file(&path, MAX_MOTION_BYTES).map_err(unavailable)?;
                    if hash != program.identity.sha256 || bytes != program.identity.byte_len {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "worker program artifact identity mismatch",
                        ));
                    }
                    let file = artifacts::open_regular(&path).map_err(unavailable)?;
                    let program: MotionProgram = serde_json::from_reader(file)
                        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                    validate_program(&program)?;
                    let receipt_path = receipt.path.canonicalize().map_err(unavailable)?;
                    if !receipt_path.starts_with(&root) {
                        return Err(ProtocolError::new(
                            ErrorCode::Forbidden,
                            "worker receipt escaped attempt",
                        ));
                    }
                    let (receipt_hash, receipt_size) =
                        artifacts::digest_file(&receipt_path, manifest.budget.output_bytes)
                            .map_err(unavailable)?;
                    if receipt_hash != receipt.identity.sha256
                        || receipt_size != receipt.identity.byte_len
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "receipt identity mismatch",
                        ));
                    }
                    assistant_image_origin::verify_receipt(
                        manifest,
                        &receipt_path,
                        &receipt.identity,
                        &original_program,
                        &lineage,
                    )?;
                    if matches!(&manifest.operation, WorkerOperation::Generate { settings, .. } if settings.annotated_view.is_some())
                    {
                        crate::worker::annotated_receipt::verify_file(
                            manifest,
                            &original_program,
                            &receipt,
                            &lineage,
                            &review,
                        )?;
                    }
                    let mut lineage = lineage;
                    lineage.push(receipt.identity);
                    if let Some(evidence) = &cuda_evidence {
                        lineage.extend([
                            evidence.receipt.identity.clone(),
                            evidence.trace.identity.clone(),
                        ]);
                    }
                    Ok((
                        self.motion_store.publish(&program)?,
                        lineage,
                        review,
                        cuda_evidence,
                    ))
                }
                _ => Err(ProtocolError::invalid(
                    "generation worker returned non-generation output",
                )),
            }
        })();
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        let state = job_snapshot(&tx, &manifest.job_id, &manifest.project_id)?;
        let observation = JobAttemptOutcome {
            schema_version: 1,
            manifest_sha256: format!("{:x}", Sha256::digest(encode(manifest)?.as_bytes())),
            engine_epoch: AttemptId::new(self.engine_epoch.clone()).map_err(internal)?,
            validation_error: result.as_ref().err().map(JobAttemptDiagnostic::from_error),
            worker_retirement,
        };
        crate::job_attempt_history::retain(&tx, manifest, &observation)?;
        // Dispatch can fail before the queued-to-running transaction. Such a
        // failed current attempt must become terminal; a queued success still
        // cannot publish, and cancelled/stale attempts only retain history.
        if state.attempt_id != manifest.attempt_id
            || (state.state != "running" && !(state.state == "queued" && result.is_err()))
        {
            tx.commit().map_err(internal)?;
            return Ok(false);
        }
        let owner: String = tx
            .query_row(
                "SELECT session FROM jobs WHERE id=?1",
                [manifest.job_id.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        let owner = SessionId::new(owner).map_err(internal)?;
        if require_grant(&tx, &owner, &manifest.project_id, Scope::Generate).is_err() {
            tx.execute("UPDATE jobs SET state='cancelled',error='authorization revoked before publication' WHERE id=?1",[manifest.job_id.as_str()]).map_err(internal)?;
        } else {
            let result = result.and_then(|ready| {
                self.require_fallback_publication(&tx, manifest, fallback_lease)?;
                if manifest.cuda_execution.is_some() {
                    crate::cuda_attempts::require_running(&tx, manifest, &self.engine_epoch)
                        .map_err(unavailable)?;
                }
                Ok(ready)
            });
            match result {
                Ok((program, lineage, review, cuda_evidence)) => {
                    let candidate = id!(CandidateId);
                    tx.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![candidate.as_str(),manifest.project_id.as_str(),rev_sql(manifest.base_revision)?,motion_state::register(&tx,&program)?,manifest.job_id.as_str(),encode(&lineage)?,encode(&review)?]).map_err(internal)?;
                    tx.execute(
                        "UPDATE jobs SET state='completed',candidate=?2 WHERE id=?1",
                        params![manifest.job_id.as_str(), candidate.as_str()],
                    )
                    .map_err(internal)?;
                    if let Some(evidence) = &cuda_evidence {
                        tx.execute(
                            "UPDATE attempts SET state='completed' WHERE id=?1",
                            [manifest.attempt_id.as_str()],
                        )
                        .map_err(internal)?;
                        crate::cuda_acceptance::retain(&tx, manifest, &candidate, evidence)
                            .map_err(internal)?;
                    }
                    event(
                        &tx,
                        &manifest.project_id,
                        manifest.base_revision,
                        EventBody::CandidateReady {
                            candidate_id: candidate,
                        },
                    )?;
                }
                Err(error) => {
                    tx.execute(
                        "UPDATE jobs SET state='failed',error=?2 WHERE id=?1",
                        params![manifest.job_id.as_str(), error.to_string()],
                    )
                    .map_err(internal)?;
                }
            }
        }
        tx.execute(
            "UPDATE attempts SET state=(SELECT state FROM jobs WHERE id=?2) WHERE id=?1",
            params![manifest.attempt_id.as_str(), manifest.job_id.as_str()],
        )
        .map_err(internal)?;
        let pending = if !self.is_shutting_down()
            && settled.is_some_and(|value| value.provider_unavailable().is_some())
            && fallback_lease.is_some()
            && self
                .require_fallback_publication(&tx, manifest, fallback_lease)
                .is_ok()
            && job_snapshot(&tx, &manifest.job_id, &manifest.project_id)?.state == "failed"
        {
            inference_fallback_store::mark_pending(
                &tx,
                manifest,
                &AttemptId::new(self.engine_epoch.clone()).map_err(internal)?,
            )?
        } else {
            false
        };
        event(
            &tx,
            &manifest.project_id,
            manifest.base_revision,
            EventBody::JobChanged(job_snapshot(&tx, &manifest.job_id, &manifest.project_id)?),
        )?;
        tx.commit().map_err(internal)?;
        Ok(pending)
    }

    fn preview(&self, request: &Request, intent: PreviewIntent<'_>) -> PResult<ResponseBody> {
        let PreviewIntent {
            assistant_image_claim,
            context,
            source_time,
            time_basis,
            model_path,
            model_input,
            mut inference_profile,
            video_view,
            placement_seek,
            model_pack,
            annotated,
        } = intent;
        let artifact_guard = self.artifact_gate.lock().map_err(internal)?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, request.session.as_ref().unwrap())?;
            source_kinds::require_media(&db, project(request)?, &context.source_version)?;
            if let Some(spec) = &annotated {
                if self
                    .source(&db, project(request)?, &context.source_version)?
                    .identity
                    .sha256
                    != spec.view.source_sha256
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "annotated preview view is not bound to the imported source",
                    ));
                }
            }
            if let Some(placed) = &placement_seek {
                if source_placements::active(
                    &db,
                    project(request)?,
                    &placed.placement.placement_id,
                )? != placed.placement
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "source placement changed before preview",
                    ));
                }
            }
        }
        let prepared_pack = if let Some(choice) = &model_pack {
            let captured = self.prepare_selected_pack_with_priority(
                choice,
                GenerationSettings {
                    annotated_view: annotated.as_ref().map(|s| s.view.clone()),
                    source_sample_stride: annotated.as_ref().map_or(1, |s| s.source_sample_stride),
                    analysis_width: 64,
                    analysis_height: 64,
                    ..Default::default()
                },
                crate::resources::GpuPriority::Interactive,
            )?;
            if annotated.is_none()
                && !captured
                    .settings
                    .model_input
                    .as_ref()
                    .is_some_and(|i| i.decoder == DetectorDecoder::YoloChannelMajor)
            {
                return Err(ProtocolError::unsupported("single-frame pack preview requires a detector; temporal script-position profiles need a frame pair"));
            }
            Some(captured)
        } else {
            None
        };
        let mut model_input = model_input;
        let (model, pack_dependencies) = if let Some(captured) = prepared_pack {
            model_input = captured.settings.model_input;
            inference_profile = captured.settings.inference_profile;
            (Some(captured.model), captured.dependencies)
        } else {
            (
                model_path
                    .map(|path| self.snapshot_model(path))
                    .transpose()?,
                vec![],
            )
        };
        let pack_model = model.as_ref().map(|m| m.identity.clone());
        let annotated_binding = if let Some(spec) = &annotated {
            spec.validate_profile(
                inference_profile
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("annotated preview profile absent"))?,
            )?;
            let model = model
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("annotated preview model absent"))?;
            Some(
                crate::pack_onnx::validate_annotated(
                    fs::File::open(&model.path).map_err(unavailable)?,
                    model.identity.byte_len,
                    model_input.as_ref().ok_or_else(|| {
                        ProtocolError::invalid("annotated preview contract absent")
                    })?,
                )
                .map_err(unavailable)?,
            )
        } else {
            None
        };
        let tools = self.tools.as_ref().map_err(unavailable)?.clone();
        let budget = crate::inference_budget::admit(
            ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: 8 * 1024 * 1024,
                wall_time_ms: 30_000,
                cpu_threads: 2,
            },
            inference_profile.as_ref(),
            None,
            self.config.max_ram_bytes,
        )?;
        let source_version = context.source_version.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let key = format!(
            "preview:{}:{}:{}:{}:{}",
            project(request)?.as_str(),
            request.session.as_ref().unwrap().as_str(),
            source_version.as_str(),
            model
                .as_ref()
                .map(|m| m.identity.sha256.as_str())
                .unwrap_or("none"),
            request.request_id.as_str()
        );
        let source = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, request.session.as_ref().unwrap())?;
            if let Some(choice) = &model_pack {
                self.require_current_pack_choice(&db, choice)?;
            }
            let source = self.source(&db, project(request)?, &source_version)?;
            self.workers
                .lock()
                .map_err(internal)?
                .insert(key.clone(), cancel.clone());
            source
        };
        struct PreviewLease<'a> {
            workers: &'a Mutex<HashMap<String, Arc<AtomicBool>>>,
            key: String,
        }
        impl Drop for PreviewLease<'_> {
            fn drop(&mut self) {
                if let Ok(mut workers) = self.workers.lock() {
                    workers.remove(&self.key);
                }
            }
        }
        let _lease = PreviewLease {
            workers: &self.workers,
            key,
        };
        let attempt_id = id!(AttemptId);
        let output_dir = self
            .config
            .state_dir
            .join("attempts")
            .join(attempt_id.as_str());
        let mut image_cleanup = assistant_image_claim
            .map(|claim| assistant_images::PendingImage::new(self, output_dir.clone(), claim));
        let assistant_image = image_cleanup.is_some();
        artifacts::private_directory(&output_dir).map_err(internal)?;
        let revision = {
            let db = self.db.lock().map_err(internal)?;
            project_snapshot(&db, project(request)?)?.revision
        };
        drop(artifact_guard);
        let mut manifest = WorkerRequest {
            cuda_execution: None,
            independent_audio: None,
            version: PROTOCOL_VERSION,
            attempt_id,
            job_id: id!(JobId),
            project_id: project(request)?.clone(),
            base_revision: revision,
            source,
            output_dir,
            operation: WorkerOperation::Preview {
                still_image: assistant_image,
                annotated: (annotated.clone()).map(Into::into),
                placement_seek: (placement_seek.clone()).map(Into::into),
                time_basis,
                video_view,
                context: context.clone(),
                source_time,
                max_width: if assistant_image {
                    512
                } else {
                    PREVIEW_FRAME_MAX_DIMENSIONS[0]
                },
                max_height: if assistant_image {
                    512
                } else {
                    PREVIEW_FRAME_MAX_DIMENSIONS[1]
                },
                model,
                model_input,
                inference_profile: (inference_profile).map(Into::into),
            },
            budget,
            dependencies: vec![
                tools.ffmpeg.identity.clone(),
                tools.ffprobe.identity.clone(),
            ],
            tools,
        };
        #[cfg(target_os = "linux")]
        let cuda = if let WorkerOperation::Preview {
            inference_profile: Some(profile),
            ..
        } = &manifest.operation
        {
            if profile.provider == InferenceProvider::Cuda {
                Some(self.capture_cuda_inference(
                    request,
                    profile,
                    &manifest.budget,
                    crate::resources::GpuPriority::Interactive,
                    &cancel,
                )?)
            } else {
                None
            }
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let cpu = if self.cpu_owner_policy.is_some() {
            match &manifest.operation {
                WorkerOperation::Preview {
                    model: Some(_),
                    inference_profile,
                    ..
                } => {
                    let profile = inference_profile.as_deref().ok_or_else(|| {
                        ProtocolError::invalid(
                            "configured captured CPU runtime requires an explicit preview profile",
                        )
                    })?;
                    if profile.provider == InferenceProvider::Cpu {
                        Some(self.capture_cpu_inference(
                            request,
                            profile,
                            &manifest.budget,
                            crate::resources::GpuPriority::Interactive,
                            &cancel,
                        )?)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        if let Some(cuda) = &cuda {
            cuda.bind(self, &mut manifest)?;
        }
        #[cfg(target_os = "linux")]
        if let Some(cpu) = &cpu {
            cpu.bind(self, &mut manifest)?;
        }
        let admitted_profile = if manifest.cuda_execution.is_some() {
            crate::inference_profiles::admit_cuda_worker_profile(&manifest).map_err(unavailable)?;
            match &manifest.operation {
                WorkerOperation::Preview {
                    inference_profile, ..
                } => inference_profile.as_deref().cloned(),
                _ => unreachable!("preview operation"),
            }
        } else {
            crate::inference_profiles::resolve_profile(&manifest).map_err(unavailable)?
        };
        let reservation = self
            .pool
            .reserve_worker(manifest.budget.memory_bytes, manifest.budget.cpu_threads)?
            .with_storage(
                manifest.budget.output_bytes,
                fs2::available_space(&self.config.state_dir).map_err(internal)?,
            )?;
        #[cfg(target_os = "linux")]
        if cpu.is_some() {
            manifest.dependencies.push(self.binary_identity.clone());
            if let Some(model) = &pack_model {
                if !manifest.dependencies.contains(model) {
                    manifest.dependencies.push(model.clone());
                }
            }
            if let Some(profile) = &admitted_profile {
                let identity =
                    crate::inference_profiles::profile_identity(profile).map_err(internal)?;
                if !manifest.dependencies.contains(&identity) {
                    manifest.dependencies.push(identity);
                }
            }
        }
        let pack_profile_sha256 = if model_pack.is_some() {
            let profile = admitted_profile
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("pack preview profile is absent"))?;
            let identity =
                crate::inference_profiles::profile_identity(profile).map_err(internal)?;
            manifest.dependencies.extend(pack_dependencies);
            manifest.dependencies.push(identity.clone());
            if let Some(model) = &pack_model {
                manifest.dependencies.push(model.clone());
            }
            if let Some(runtime) = &manifest.tools.onnx_runtime {
                if !manifest.dependencies.contains(&runtime.identity) {
                    manifest.dependencies.push(runtime.identity.clone());
                }
            }
            Some(identity.sha256)
        } else {
            None
        };
        manifest.validate()?;
        {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            self.authorize(&tx, request, request.session.as_ref().unwrap())?;
            if let Some(choice) = &model_pack {
                self.require_current_pack_choice(&tx, choice)?;
            }
            self.arm_worker_fence(&tx)?;
            tx.commit().map_err(internal)?;
        }
        let mut completion_claim = Some(Arc::new(reservation));
        #[cfg(target_os = "linux")]
        let execution = if let Some(cuda) = cuda {
            let mut queued = Arc::try_unwrap(completion_claim.take().expect("preview CPU claim"))
                .map_err(|_| {
                ProtocolError::new(ErrorCode::Internal, "preview CPU claim has another owner")
            })?;
            let mut storage = queued.split_storage()?;
            let custody = self
                .worker_fence
                .admit_group(
                    Arc::new(queued),
                    manifest.budget.memory_bytes,
                    manifest.budget.cpu_threads,
                )
                .map_err(unavailable)?
                .into_worker();
            let observed = crate::cuda_memory::observe(
                &self.config,
                &manifest,
                cuda.image,
                cuda.devices,
                custody,
                &cancel,
            )
            .map_err(unavailable)?;
            let owner = self
                .cuda_owner_policy
                .as_ref()
                .expect("capture checked policy");
            let mut admitted = observed
                .admit(
                    &self.pool,
                    owner,
                    &manifest,
                    crate::resources::GpuPriority::Interactive,
                )
                .map_err(unavailable)?;
            Arc::get_mut(&mut admitted.reservation)
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::Internal,
                        "fresh preview CUDA claim has another owner",
                    )
                })?
                .absorb_storage(&mut storage)?;
            completion_claim = Some(admitted.reservation.clone());
            crate::launch_cuda::execute_reserved(
                &self.config,
                &manifest,
                &cancel,
                admitted,
                self.worker_fence.clone(),
            )?
        } else if let Some(cpu) = cpu {
            crate::launch_cpu::execute_reserved(
                &self.config,
                &manifest,
                &cancel,
                cpu.image,
                cpu.descriptor,
                completion_claim
                    .as_ref()
                    .expect("preview CPU claim")
                    .clone(),
                self.worker_fence.clone(),
            )?
        } else {
            launch::execute_reserved(
                &self.config,
                &manifest,
                &cancel,
                completion_claim
                    .as_ref()
                    .expect("preview CPU claim")
                    .clone(),
                self.worker_fence.clone(),
            )?
        };
        #[cfg(not(target_os = "linux"))]
        let execution = launch::execute_reserved(
            &self.config,
            &manifest,
            &cancel,
            completion_claim
                .as_ref()
                .expect("preview CPU claim")
                .clone(),
        )?;
        let _completion_claim = completion_claim;
        let output = execution.output;
        let db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, request.session.as_ref().unwrap())?;
        if let Some(choice) = &model_pack {
            self.require_current_pack_choice(&db, choice)?;
        }
        if let Some(placed) = &placement_seek {
            if source_placements::active(&db, project(request)?, &placed.placement.placement_id)?
                != placed.placement
            {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "source placement changed while preview ran",
                ));
            }
        }
        match output {
            WorkerOutput::Preview(preview)
                if if let Some(placed) = &placement_seek {
                    preview.matches_placed_seek_request(context, placed)
                } else {
                    preview.matches_timed_seek_request(context, &source_time, time_basis)
                } =>
            {
                if let Some(spec) = &annotated {
                    let evidence = preview.annotated.as_ref().ok_or_else(|| {
                        ProtocolError::invalid("annotated preview evidence absent")
                    })?;
                    evidence.validate(&preview)?;
                    let profile = admitted_profile.as_ref().ok_or_else(|| {
                        ProtocolError::invalid("annotated preview profile absent")
                    })?;
                    let binding = annotated_binding
                        .as_ref()
                        .expect("annotated metadata checked");
                    if serde_json::to_value(&evidence.spec).map_err(internal)?
                        != serde_json::to_value(spec).map_err(internal)?
                        || Some(&evidence.model) != pack_model.as_ref()
                        || evidence.profile
                            != crate::inference_profiles::profile_identity(profile)
                                .map_err(internal)?
                        || manifest
                            .tools
                            .onnx_runtime
                            .as_ref()
                            .is_none_or(|r| r.identity != evidence.runtime)
                        || evidence.contract != binding.contract
                        || evidence.training_intake_sha256 != binding.training_intake_sha256
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "annotated preview evidence differs from admitted model/view",
                        ));
                    }
                } else if preview.annotated.is_some() {
                    return Err(ProtocolError::invalid(
                        "unexpected annotated preview evidence",
                    ));
                }
                for frame in preview.frame.iter().chain(
                    preview
                        .annotated
                        .iter()
                        .filter_map(|e| e.previous.as_ref())
                        .map(|p| &p.frame),
                ) {
                    frame.validate(manifest.budget.output_bytes)?;
                    let path = frame.artifact.path.canonicalize().map_err(unavailable)?;
                    if !path.starts_with(&manifest.output_dir) {
                        return Err(ProtocolError::new(
                            ErrorCode::Forbidden,
                            "preview artifact outside admitted attempt",
                        ));
                    }
                    let (hash, bytes) = artifacts::digest_file(&path, manifest.budget.output_bytes)
                        .map_err(unavailable)?;
                    if hash != frame.artifact.identity.sha256
                        || bytes != frame.artifact.identity.byte_len
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "preview artifact identity mismatch",
                        ));
                    }
                    let mut permissions = fs::metadata(&path).map_err(unavailable)?.permissions();
                    permissions.set_readonly(true);
                    fs::set_permissions(path, permissions).map_err(unavailable)?;
                }
                if annotated.is_some() {
                    crate::worker::validate_annotated_preview_pair(&preview).map_err(|e| {
                        ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string())
                    })?;
                }
                if let WorkerOperation::Preview {
                    model, video_view, ..
                } = &manifest.operation
                {
                    let current = project_snapshot(&db, project(request)?)?;
                    let catalog = assistant_targets::capture(
                        project(request)?.clone(),
                        request.session.clone().ok_or_else(|| {
                            ProtocolError::new(
                                ErrorCode::Unauthorized,
                                "target catalog requires session",
                            )
                        })?,
                        manifest.base_revision,
                        &preview,
                        model.as_ref().map(|m| &m.identity),
                        video_view.as_ref(),
                        current.revision == manifest.base_revision,
                    )?;
                    self.assistant_targets
                        .lock()
                        .map_err(internal)?
                        .replace(catalog, &self.pool)?;
                }
                if assistant_image {
                    return self.retain_assistant_image(
                        &db,
                        request,
                        &manifest,
                        &preview,
                        image_cleanup.as_mut().expect("assistant image custody"),
                    );
                }
                if let Some(choice) = model_pack {
                    let report = ModelPackPreviewResult {
                        choice,
                        model: pack_model.expect("captured pack model"),
                        profile_sha256: pack_profile_sha256.expect("admitted pack profile"),
                        preview: *preview,
                        qualified: false,
                    };
                    report.validate()?;
                    Ok(ResponseBody::ModelPackPreview((report).into()))
                } else {
                    Ok(ResponseBody::Preview(preview))
                }
            }
            _ => Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "preview evidence did not match requested source/frame/transforms/generation",
            )),
        }
    }

    fn export(
        &self,
        request: &Request,
        path: &Path,
        fingerprint: &str,
        kind: ExportKind<'_>,
    ) -> PResult<ResponseBody> {
        let selected_axis = match &kind {
            ExportKind::Axis(axis, _) => Some(*axis),
            ExportKind::HandyStroke => Some(Axis::Stroke),
            #[cfg(unix)]
            ExportKind::Stash { .. } => Some(Axis::Stroke),
            _ => None,
        };
        let dynamics = match &kind {
            ExportKind::Device(profile) => Some(*profile),
            _ => None,
        };
        let _artifact_guard = self.artifact_gate.lock().map_err(internal)?;
        let _motion_admission = self.pool.reserve_memory(transfers::VALIDATION_MEMORY)?;
        let session = request.session.as_ref().unwrap();
        let mut _clock_admission: Option<Reservation> = None;
        let mut _action_clock_admission: Option<Reservation> = None;
        let (bytes, revision, adaptation_receipt) = {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            self.authorize(&tx, request, session)?;
            if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
                return Ok(body);
            }
            let existing: Option<String> = tx
                .query_row(
                    "SELECT state FROM exports WHERE session=?1 AND request=?2",
                    params![session.as_str(), request.request_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            if existing.is_some() {
                return Err(ProtocolError::new(ErrorCode::UnknownOutcome,"previous export dispatch has an unresolved outcome; inspect destination, do not blindly replay"));
            }
            check_revision(&tx, request)?;
            if let ExportKind::Axis(_, Some(identity)) = &kind {
                artifacts::preflight_pinned_export(path, identity).map_err(|error| {
                    ProtocolError::new(ErrorCode::RequestConflict, error.to_string())
                })?;
            }
            claim_request_identity(&tx, session.as_str(), request, fingerprint)?;
            let snapshot = project_state(&tx, project(request)?)?;
            let mut source_clock: Option<SourceClockMotion> = None;
            #[cfg(unix)]
            let mut action_ancestry = None;
            let mut clock_details: Option<(
                SourcePlacement,
                CandidateId,
                ArtifactIdentity,
                ArtifactIdentity,
                Vec<MotionRetimeReceipt>,
            )> = None;
            let mut archive_details: Option<(
                archived_generation::PublicationClocks,
                Vec<MotionRetimeReceipt>,
                Vec<ArchivedMotionRetimeReceipt>,
            )> = None;
            #[cfg(unix)]
            if let ExportKind::Stash {
                capture,
                candidate,
                job,
                archived_origin,
                ..
            } = &kind
            {
                let raw:Option<String>=tx.query_row("SELECT receipt FROM stash_sources WHERE project=?1 AND version=?2 AND scene=?3 AND file=?4",params![project(request)?.as_str(),capture.source_version.as_str(),capture.scene_id,capture.file_id],|row|row.get(0)).optional().map_err(internal)?;
                if raw.as_deref() != Some(encode(capture)?.as_str()) {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "Stash source association changed before publication",
                    ));
                }
                if let Some(reference) = archived_origin {
                    let source_time = matches!(
                        &request.command,
                        Command::StashExportScript {
                            source_time: true,
                            ..
                        }
                    );
                    let ready = archived_generation::publication_clocks(
                        &tx,
                        archived_generation::PublicationSelection {
                            project: project(request)?,
                            candidate,
                            job,
                            selected: reference,
                            revision: snapshot.revision,
                            source_time,
                        },
                        &self.pool,
                        &self.import_store,
                    )?;
                    if ready.context.origin.source_version != capture.source_version
                        || ready.context.origin.source_identity != capture.identity
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "archival generation differs from freshly bound source",
                        ));
                    }
                    let mut native = Vec::new();
                    let mut archived = Vec::new();
                    if let Some(plan) = &ready.plan {
                        let track = snapshot.program.track(Axis::Stroke).ok_or_else(|| {
                            ProtocolError::invalid(
                                "archival source-time publication requires stroke motion",
                            )
                        })?;
                        let mut entries = plan.samples.len().max(track.actions().len());
                        for revision in &ready.context.master.retiming_revisions {
                            let local:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM motion_retime_receipts WHERE project=?1 AND revision=?2)",params![project(request)?.as_str(),rev_sql(*revision)?],|r|r.get(0)).map_err(internal)?;
                            let base = if local {
                                let receipt = motion_retime_receipts::load_verified(
                                    &tx,
                                    project(request)?,
                                    *revision,
                                    &self.pool,
                                )?;
                                let base = receipt.base_revision;
                                native.push(receipt);
                                base
                            } else {
                                let receipt = archived_motion_retimes::load_verified(
                                    &tx,
                                    project(request)?,
                                    *revision,
                                    &self.pool,
                                    &self.import_store,
                                )?;
                                let base = receipt.base_revision;
                                archived.push(receipt);
                                base
                            };
                            let descriptor =
                                motion_state::revision_descriptor(&tx, project(request)?, base)?;
                            if let Some(axis) =
                                descriptor.axes.iter().find(|a| a.axis == Axis::Stroke)
                            {
                                entries = entries
                                    .max(usize::try_from(axis.action_count).map_err(internal)?);
                            }
                        }
                        _clock_admission = Some(
                            self.pool.reserve_memory(
                                (entries as u64)
                                    .checked_mul(512)
                                    .and_then(|n| n.checked_add(16 * 1024 * 1024))
                                    .ok_or_else(|| {
                                        ProtocolError::new(
                                            ErrorCode::ResourceExhausted,
                                            "archival clock carrier memory overflow",
                                        )
                                    })?,
                            )?,
                        );
                        let derived = stash_action_ancestry::map_archived(
                            &tx,
                            project(request)?,
                            snapshot.revision,
                            &ready.context.origin,
                            track,
                            &self.pool,
                            &self.import_store,
                        )?;
                        let (motion, ancestry, admission) = derived.into_parts();
                        source_clock = Some(motion);
                        action_ancestry = Some(ancestry);
                        _action_clock_admission = Some(admission);
                    }
                    archive_details = Some((ready, native, archived));
                } else {
                    let source_time = matches!(
                        &request.command,
                        Command::StashExportScript {
                            source_time: true,
                            ..
                        }
                    );
                    let retimings = if source_time {
                        Vec::new()
                    } else {
                        stash_lineage::verified_source_retimings(
                            &tx,
                            project(request)?,
                            snapshot.revision,
                            &capture.source_version,
                            candidate,
                            job,
                            &self.pool,
                        )?
                    };
                    let native_manifest = source_clock_origins::manifest(
                        &tx,
                        project(request)?,
                        &capture.source_version,
                        job,
                        &self.pool,
                    )?;
                    let origin = native_manifest.request();
                    if !source_time && !retimings.is_empty() {
                        return Err(ProtocolError::unsupported(
                            "retimed Stash motion requires explicit source-time export",
                        ));
                    }
                    if source_time
                        && !matches!(&origin.operation,WorkerOperation::Generate{settings,..} if settings.source_placement.is_some())
                    {
                        return Err(ProtocolError::invalid(
                            "explicit source-time export requires a placed generation origin",
                        ));
                    }
                    if let WorkerOperation::Generate { settings, .. } = &origin.operation {
                        if let Some(placement) = &settings.source_placement {
                            if !source_time
                                && !placement.spec.preserves_source_clock().map_err(internal)?
                            {
                                return Err(ProtocolError::unsupported("Stash sibling scripts require original source-clock motion; placed generation needs an explicit source-time export"));
                            }
                            let native_clocks = source_clock_origins::clocks(
                                &tx,
                                &native_manifest,
                                candidate,
                                &self.pool,
                            )?;
                            let original = native_clocks.original_candidate.clone();
                            let program = native_clocks.program.clone();
                            let verified = &native_clocks.verified;
                            if source_time {
                                let track =
                                    snapshot.program.track(Axis::Stroke).ok_or_else(|| {
                                        ProtocolError::invalid(
                                            "source-time export requires stroke motion",
                                        )
                                    })?;
                                let derived = stash_action_ancestry::map_native(
                                    &tx,
                                    project(request)?,
                                    &capture.source_version,
                                    snapshot.revision,
                                    track,
                                    &self.pool,
                                )?;
                                let revisions = stash_action_ancestry::selected_native_retimings(
                                    &derived.ancestry,
                                    job,
                                    &native_clocks,
                                )?;
                                let mut receipts = Vec::with_capacity(revisions.len());
                                for revision in revisions {
                                    receipts.push(motion_retime_receipts::load_verified(
                                        &tx,
                                        project(request)?,
                                        revision,
                                        &self.pool,
                                    )?);
                                }
                                let (motion, ancestry, memory) = derived.into_parts();
                                source_clock = Some(motion);
                                action_ancestry = Some(ancestry);
                                _action_clock_admission = Some(memory);
                                clock_details = Some((
                                    placement.clone(),
                                    original,
                                    verified.identity.clone(),
                                    program,
                                    receipts,
                                ));
                            }
                        } else if source_time {
                            return Err(ProtocolError::invalid(
                                "explicit source-time export requires a placed generation origin",
                            ));
                        }
                    }
                }
            }
            let adaptation = dynamics
                .map(|profile| {
                    adapt_device_motion(&snapshot.program, profile)
                        .map_err(|e| ProtocolError::invalid(e.to_string()))
                })
                .transpose()?;
            let profile_hash = dynamics
                .map(|profile| {
                    encode(profile).map(|raw| format!("{:x}", Sha256::digest(raw.as_bytes())))
                })
                .transpose()?;
            let handy = if matches!(kind, ExportKind::HandyStroke) {
                Some(crate::handy_script::prepare(&snapshot.program)?)
            } else {
                None
            };
            let (handy_bytes, handy_summary) = match handy {
                Some((bytes, summary)) => (Some(bytes), Some(summary)),
                None => (None, None),
            };
            let bytes = if let Some(bytes) = handy_bytes {
                bytes
            } else if let Some(mapped) = &source_clock {
                let program = MotionProgram::new(vec![mapped.track.clone()])
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                neutral_export_axis(&program, Axis::Stroke)?
            } else if let Some(adapted) = &adaptation {
                crate::device_motion::proposal_bytes(
                    &snapshot.project_id,
                    snapshot.revision,
                    &snapshot.motion.program.sha256,
                    profile_hash.as_deref().expect("profile hash computed"),
                    dynamics.expect("profile supplied"),
                    adapted,
                    OUTPUT_LIMIT as usize,
                )?
            } else {
                match selected_axis {
                    Some(axis) => neutral_export_axis(&snapshot.program, axis)?,
                    None => neutral_export(&snapshot.program)?,
                }
            };
            if bytes.len() as u64 > OUTPUT_LIMIT {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "device/motion export exceeds output limit",
                ));
            }
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let needed = self
                .pool
                .reserved_storage()
                .checked_add(bytes.len() as u64)
                .and_then(|v| v.checked_add(16 * 1024 * 1024))
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "export storage reservation overflow",
                    )
                })?;
            let available = match &kind {
                #[cfg(unix)]
                ExportKind::Stash { publisher, .. } => {
                    publisher.available_space().map_err(unavailable)?
                }
                _ => fs2::available_space(parent).map_err(unavailable)?,
            };
            if available < needed {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "insufficient unreserved export storage",
                ));
            }
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let mut timing_proof: Option<StashTimingProof> = None;
            #[cfg(unix)]
            if let (
                Some(mapped),
                Some((placement, original, receipt, program, retimings)),
                ExportKind::Stash {
                    candidate,
                    job,
                    capture,
                    ..
                },
            ) = (&source_clock, &clock_details, &kind)
            {
                let script = ArtifactIdentity {
                    artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                    sha256: hash.clone(),
                    byte_len: bytes.len() as u64,
                };
                timing_proof = Some(stash_clock_proofs::prepare(
                    self,
                    stash_clock_proofs::Context {
                        publication_request: &request.request_id,
                        source: capture,
                        project: &snapshot.project_id,
                        revision: snapshot.revision,
                        candidate,
                        job,
                        original_candidate: original,
                        original_receipt: receipt,
                        original_program: program,
                        master_sha256: &snapshot.motion.program.sha256,
                        script: &script,
                        placement,
                        retimings,
                        piecewise: action_ancestry
                            .as_ref()
                            .is_some_and(|a| a.carriers.len() > 1),
                        action_ancestry: action_ancestry.as_ref().ok_or_else(|| {
                            ProtocolError::invalid("verified native action ancestry absent")
                        })?,
                    },
                    mapped,
                )?);
            }
            #[cfg(unix)]
            if let (
                Some((ready, native, archived)),
                ExportKind::Stash {
                    candidate,
                    job,
                    capture,
                    ..
                },
            ) = (&archive_details, &kind)
            {
                let script = ArtifactIdentity {
                    artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                    sha256: hash.clone(),
                    byte_len: bytes.len() as u64,
                };
                timing_proof = Some(stash_clock_proofs::prepare_archived(
                    self,
                    stash_clock_proofs::ArchivedContext {
                        publication_request: &request.request_id,
                        source: capture,
                        project: &snapshot.project_id,
                        revision: snapshot.revision,
                        candidate,
                        job,
                        master_sha256: &snapshot.motion.program.sha256,
                        script: &script,
                        archived_context: &ready.context,
                        native_retimings: native,
                        archived_retimings: archived,
                        action_ancestry: action_ancestry.as_ref(),
                    },
                    source_clock.as_ref(),
                )?);
                // Atomically protect pending and completed archival proof subjects
                // from readers that cannot distinguish historical jobs.
                if crate::inference_equivalence_reader::version(&tx).map_err(internal)? < 10 {
                    crate::inference_equivalence_reader::publish(&tx, 10).map_err(internal)?;
                }
            }
            tx.execute(
                "INSERT INTO exports(session,request,project,revision,path,sha256,state,axis) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7)",
                params![
                    session.as_str(),
                    request.request_id.as_str(),
                    snapshot.project_id.as_str(),
                    rev_sql(snapshot.revision)?,
                    path.to_string_lossy(),
                    hash,
                    if dynamics.is_some() { "device-motion" } else { selected_axis.unwrap_or(Axis::Stroke).as_str() }
                ],
            )
            .map_err(internal)?;
            if let Some(proof) = &timing_proof {
                tx.execute(
                    "INSERT INTO stash_clock_proofs(session,request,proof) VALUES(?1,?2,?3)",
                    params![
                        session.as_str(),
                        request.request_id.as_str(),
                        encode(proof)?
                    ],
                )
                .map_err(internal)?;
            }
            #[cfg(unix)]
            let stash_review = if matches!(&kind, ExportKind::Stash { .. }) {
                Some(lineage::state(&tx, project(request)?, snapshot.revision)?)
            } else {
                None
            };
            let handy_receipt = handy_summary
                .map(|quantization| -> PResult<ResponseBody> {
                    let review = lineage::state(&tx, project(request)?, snapshot.revision)?;
                    let report = HandyStrokeScript {
                        request_id: request.request_id.clone(),
                        project_id: snapshot.project_id.clone(),
                        revision: snapshot.revision,
                        path: path.to_path_buf(),
                        master_sha256: snapshot.motion.program.sha256.clone(),
                        identity: ArtifactIdentity {
                            artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                            sha256: hash.clone(),
                            byte_len: bytes.len() as u64,
                        },
                        quantization,
                        unresolved_review_count: u32::try_from(review.unresolved()?.len())
                            .map_err(internal)?,
                        review_state_unknown: review.unknown,
                        release_qualified: false,
                    };
                    report.validate()?;
                    Ok(ResponseBody::HandyStrokeScript(Box::new(report)))
                })
                .transpose()?;
            tx.commit().map_err(internal)?;
            let receipt = adaptation.map(|adapted| ResponseBody::DeviceMotionExported {
                path: path.to_path_buf(),
                revision: snapshot.revision,
                gain: adapted.gain,
                profile_sha256: profile_hash.expect("profile hash computed"),
                master_sha256: snapshot.motion.program.sha256,
            });
            let receipt = handy_receipt.or(receipt);
            #[cfg(unix)]
            let receipt = if let ExportKind::Stash {
                capture,
                candidate,
                job,
                ..
            } = &kind
            {
                let review = stash_review.expect("Stash review captured in reservation");
                Some(ResponseBody::StashScriptExported(
                    (StashScriptExport {
                        timing_proof,
                        publication_request_id: Some(request.request_id.clone()),
                        project_id: snapshot.project_id.clone(),
                        revision: snapshot.revision,
                        candidate_id: (*candidate).clone(),
                        job_id: (*job).clone(),
                        source: (*capture).clone(),
                        path: path.to_path_buf(),
                        identity: ArtifactIdentity {
                            artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                            sha256: hash,
                            byte_len: bytes.len() as u64,
                        },
                        unresolved_review_count: review.unresolved()?.len() as u32,
                        review_state_unknown: review.unknown,
                        release_qualified: false,
                    })
                    .into(),
                ))
            } else {
                receipt
            };
            (bytes, snapshot.revision, receipt)
        };
        // The export reservation above is the authorization linearization point.
        // Revocation cannot erase already disclosed bytes or reverse a committed dispatch.
        let result = match &kind {
            #[cfg(unix)]
            ExportKind::Stash { publisher, .. } => {
                publisher.publish(&bytes).map_err(anyhow::Error::from)
            }
            ExportKind::Axis(_, Some(identity)) => {
                artifacts::publish_export_pinned(path, &bytes, identity)
            }
            _ => artifacts::publish_export(path, &bytes),
        };
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        match result {
            Ok(()) => {
                let body = if let Some(receipt) = adaptation_receipt {
                    receipt
                } else {
                    match selected_axis {
                        Some(axis) => ResponseBody::AxisExported {
                            path: path.to_path_buf(),
                            revision,
                            axis,
                        },
                        None => ResponseBody::Exported {
                            path: path.to_path_buf(),
                            revision,
                        },
                    }
                };
                tx.execute(
                    "UPDATE exports SET state='complete' WHERE session=?1 AND request=?2",
                    params![session.as_str(), request.request_id.as_str()],
                )
                .map_err(internal)?;
                remember(&tx, session.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            Err(error) => {
                tx.execute(
                    "UPDATE exports SET state='failed_or_unknown' WHERE session=?1 AND request=?2",
                    params![session.as_str(), request.request_id.as_str()],
                )
                .map_err(internal)?;
                tx.commit().map_err(internal)?;
                Err(ProtocolError::new(
                    ErrorCode::UnknownOutcome,
                    error.to_string(),
                ))
            }
        }
    }
}

fn project(request: &Request) -> PResult<&ProjectId> {
    request
        .project
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("project scope required"))
}
fn check_revision(db: &Connection, request: &Request) -> PResult<()> {
    if let Some(expected) = request.expected_revision {
        let actual = project_snapshot(db, project(request)?)?.revision;
        if expected != actual {
            return Err(ProtocolError::new(
                ErrorCode::RevisionConflict,
                "expected revision is stale",
            ));
        }
    }
    Ok(())
}
fn actor(db: &Connection, session: &SessionId) -> PResult<String> {
    db.query_row(
        "SELECT actor FROM sessions WHERE id=?1",
        [session.as_str()],
        |r| r.get(0),
    )
    .optional()
    .map_err(internal)?
    .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "unknown pairing session"))
}
fn require_grant(
    db: &Connection,
    session: &SessionId,
    project: &ProjectId,
    scope: Scope,
) -> PResult<()> {
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM grants WHERE session=?1 AND project=?2 AND scope=?3)",
            params![session.as_str(), project.as_str(), scope_name(scope)],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if !exists {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "client lacks required project capability",
        ));
    }
    Ok(())
}
fn check_request_identity(
    db: &Connection,
    session: &str,
    request: &Request,
    fingerprint: &str,
) -> PResult<()> {
    let old: Option<String> = db
        .query_row(
            "SELECT fingerprint FROM request_identities WHERE session=?1 AND request=?2",
            params![session, request.request_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    if old.is_some_and(|old| old != fingerprint) {
        return Err(ProtocolError::new(
            ErrorCode::RequestConflict,
            "request identity reused across commands or transfer namespaces",
        ));
    }
    Ok(())
}
fn claim_request_identity(
    db: &Connection,
    session: &str,
    request: &Request,
    fingerprint: &str,
) -> PResult<()> {
    check_request_identity(db, session, request, fingerprint)?;
    db.execute(
        "INSERT OR IGNORE INTO request_identities VALUES(?1,?2,?3)",
        params![session, request.request_id.as_str(), fingerprint],
    )
    .map_err(internal)?;
    Ok(())
}
fn replay(
    db: &Connection,
    session: &str,
    request: &Request,
    fingerprint: &str,
) -> PResult<Option<ResponseBody>> {
    check_request_identity(db, session, request, fingerprint)?;
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT fingerprint,response FROM requests WHERE session=?1 AND request=?2",
            params![session, request.request_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    match row {
        Some((old, body)) if old == fingerprint => Ok(Some(decode(&body)?)),
        Some(_) => Err(ProtocolError::new(
            ErrorCode::RequestConflict,
            "request identity reused with different content",
        )),
        None => Ok(None),
    }
}
fn remember(
    tx: &Transaction<'_>,
    session: &str,
    request: &Request,
    fingerprint: &str,
    body: &ResponseBody,
) -> PResult<()> {
    claim_request_identity(tx, session, request, fingerprint)?;
    crate::live_cpu_history::remember(tx, session, request, fingerprint, body).map_err(internal)?;
    tx.execute(
        "INSERT INTO requests VALUES(?1,?2,?3,?4)",
        params![
            session,
            request.request_id.as_str(),
            fingerprint,
            encode(body)?
        ],
    )
    .map_err(internal)?;
    Ok(())
}

/// Explicit rebinding requires a currently catalogued immutable media identity.
/// No original/archive path, endpoint or receipt becomes an active resolver.
fn check_stash_source_binding(db: &Connection, request: &Request) -> PResult<Option<u64>> {
    let Command::StashImportSource {
        expected_source_version: Some(version),
        ..
    } = &request.command
    else {
        return Ok(None);
    };
    let project = project(request)?;
    require_grant(
        db,
        request.session.as_ref().expect("authenticated"),
        project,
        Scope::Read,
    )?;
    let source: Option<(i64, bool)> = db
        .query_row(
            "SELECT bytes,sha256=?2 AND kind='media' FROM sources WHERE project=?1 AND version=?2",
            params![project.as_str(), version.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let (bytes, bound) = source.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "selected Stash rebinding source is absent from this project",
        )
    })?;
    if !bound
        || version.as_str().len() != 64
        || !version
            .as_str()
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || bytes < 0
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "selected Stash rebinding source is not immutable media",
        ));
    }
    Ok(Some(bytes as u64))
}

struct ProjectState {
    project_id: ProjectId,
    revision: RevisionId,
    program: MotionProgram,
    motion: MotionDescriptor,
}
fn project_state(db: &Connection, id: &ProjectId) -> PResult<ProjectState> {
    // Motion mutation kernels need the committed motion/revision, not a client
    // catalog. Placement membership may be staged within this same transaction
    // and is validated only after its new project revision has been recorded.
    let row: Option<(i64, String)> = db
        .query_row(
            "SELECT revision,program FROM projects WHERE id=?1",
            [id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let (revision, encoded) =
        row.ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "project not found"))?;
    let revision = RevisionId::new(u64::try_from(revision).map_err(internal)?);
    let motion = MotionDescriptor {
        program: motion_state::descriptor(db, &encoded)?,
        binding: MotionBinding::ProjectRevision {
            project_id: id.clone(),
            revision,
        },
    };
    let program = motion_state::store_for(db)?.read(&motion.program)?;
    Ok(ProjectState {
        project_id: id.clone(),
        revision,
        program,
        motion,
    })
}

struct CandidateState {
    base_revision: RevisionId,
    program: MotionProgram,
    review: Vec<ReviewFlag>,
}
fn candidate_state(
    db: &Connection,
    id: &CandidateId,
    project: &ProjectId,
) -> PResult<CandidateState> {
    let snapshot = candidate_snapshot(db, id, project)?;
    let program = motion_state::store_for(db)?.read(&snapshot.motion.program)?;
    Ok(CandidateState {
        base_revision: snapshot.base_revision,
        program,
        review: snapshot.review,
    })
}

fn project_snapshot(db: &Connection, id: &ProjectId) -> PResult<ProjectSnapshot> {
    let row: Option<(String, i64, String)> = db
        .query_row(
            "SELECT name,revision,program FROM projects WHERE id=?1",
            [id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(internal)?;
    let (name, revision, program) =
        row.ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "project not found"))?;
    let mut stmt = db
        .prepare("SELECT version,label,kind FROM sources WHERE project=?1 ORDER BY version")
        .map_err(internal)?;
    let rows = stmt
        .query_map([id.as_str()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(internal)?;
    let mut sources = Vec::new();
    for row in rows {
        let (version, label, kind) = row.map_err(internal)?;
        sources.push(SourceSummary {
            source_version: SourceVersionId::new(version).map_err(internal)?,
            label,
            kind: source_kinds::parse(&kind)?,
        });
    }
    Ok(ProjectSnapshot {
        project_id: id.clone(),
        revision: RevisionId::new(revision as u64),
        name,
        motion: MotionDescriptor {
            program: motion_state::descriptor(db, &program)?,
            binding: MotionBinding::ProjectRevision {
                project_id: id.clone(),
                revision: RevisionId::new(revision as u64),
            },
        },
        sources,
        placements: source_placements::snapshot(db, id)?,
    })
}
fn job_snapshot(db: &Connection, id: &JobId, project: &ProjectId) -> PResult<JobSnapshot> {
    let row:Option<JobRow>=db.query_row("SELECT attempt,base_revision,state,candidate,error FROM jobs WHERE id=?1 AND project=?2",params![id.as_str(),project.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(internal)?;
    let (attempt, revision, state, candidate, error) =
        row.ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "job not found in project"))?;
    Ok(JobSnapshot {
        job_id: id.clone(),
        attempt_id: AttemptId::new(attempt).map_err(internal)?,
        project_id: project.clone(),
        base_revision: RevisionId::new(revision as u64),
        state,
        candidate_id: candidate
            .map(CandidateId::new)
            .transpose()
            .map_err(internal)?,
        error,
    })
}
fn candidate_generation_interval(
    db: &Connection,
    id: &CandidateId,
    project: &ProjectId,
) -> PResult<Option<TimeRange>> {
    let (job,raw,bytes):(Option<String>,Option<String>,Option<i64>)=db.query_row(
        "SELECT c.job, CASE WHEN length(CAST(j.manifest AS BLOB))<=?3 THEN j.manifest ELSE NULL END, length(CAST(j.manifest AS BLOB)) FROM candidates c LEFT JOIN jobs j ON j.id=c.job AND j.project=c.project WHERE c.id=?1 AND c.project=?2",
        params![id.as_str(),project.as_str(),PROGRAM_LIMIT as i64],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(internal)?;
    let Some(job) = job else {
        return imported_candidate_intervals::load(db, id, project);
    };
    if bytes.is_some_and(|n| n > PROGRAM_LIMIT as i64) {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "candidate job manifest exceeds bounded metadata",
        ));
    }
    let raw = raw.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "native candidate job manifest is missing",
        )
    })?;
    let manifest: WorkerRequest = decode(&raw)?;
    manifest.validate()?;
    if manifest.project_id != *project || manifest.job_id.as_str() != job {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "candidate job manifest identity differs from project or job",
        ));
    }
    Ok(match manifest.operation {
        WorkerOperation::Generate { settings, .. } => settings.project_interval,
        _ => None,
    })
}

fn candidate_snapshot(
    db: &Connection,
    id: &CandidateId,
    project: &ProjectId,
) -> PResult<CandidateSnapshot> {
    let row: Option<(i64, String, Option<String>, String)> = db
        .query_row(
            "SELECT base_revision,program,job,review FROM candidates WHERE id=?1 AND project=?2",
            params![id.as_str(), project.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(internal)?;
    let (revision, program, job, review) = row
        .ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "candidate not found in project"))?;
    let review: Vec<ReviewFlag> = decode(&review)?;
    validate_reviews(&review)?;
    Ok(CandidateSnapshot {
        project_interval: candidate_generation_interval(db, id, project)?,
        candidate_id: id.clone(),
        project_id: project.clone(),
        base_revision: RevisionId::new(revision as u64),
        motion: MotionDescriptor {
            program: motion_state::descriptor(db, &program)?,
            binding: MotionBinding::Candidate {
                project_id: project.clone(),
                candidate_id: id.clone(),
                base_revision: RevisionId::new(revision as u64),
            },
        },
        job_id: job.map(JobId::new).transpose().map_err(internal)?,
        review,
    })
}
fn event(
    tx: &Transaction<'_>,
    project: &ProjectId,
    revision: RevisionId,
    body: EventBody,
) -> PResult<()> {
    tx.execute(
        "INSERT INTO events(project,revision,body) VALUES(?1,?2,?3)",
        params![project.as_str(), rev_sql(revision)?, encode(&body)?],
    )
    .map_err(internal)?;
    Ok(())
}
fn validate_program(program: &MotionProgram) -> PResult<()> {
    let actions = program
        .tracks()
        .iter()
        .try_fold(0_u64, |sum, track| {
            sum.checked_add(track.actions().len() as u64)
        })
        .ok_or_else(|| ProtocolError::invalid("motion action count overflow"))?;
    let gaps = program
        .tracks()
        .iter()
        .try_fold(0_u64, |sum, track| {
            sum.checked_add(track.gaps().len() as u64)
        })
        .ok_or_else(|| ProtocolError::invalid("motion gap count overflow"))?;
    if actions > MAX_MOTION_ACTIONS || gaps > MAX_MOTION_GAPS {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "motion count exceeds artifact admission",
        ));
    }
    Ok(())
}
fn commit_program(
    tx: &Transaction<'_>,
    request: &Request,
    session: &SessionId,
    program: &MotionProgram,
    kind: &str,
    label: &str,
    append: bool,
) -> PResult<RevisionId> {
    validate_program(program)?;
    let id = project(request)?;
    let current = project_state(tx, id)?;
    if request.expected_revision != Some(current.revision) {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "expected revision does not match project",
        ));
    }
    let protected: String = tx
        .query_row(
            "SELECT protected FROM projects WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    let regions: Vec<ProtectedRegion> = decode(&protected)?;
    let authored_candidate = if let Command::CommitCandidate { candidate_id }
    | Command::MergeCandidate { candidate_id, .. } = &request.command
    {
        tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM edit_candidate_lineage WHERE candidate=?1)",
            [candidate_id.as_str()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(internal)?
    } else {
        false
    };
    let preserved = if authored_candidate
        || matches!(kind, "candidate_merge" | "source_retime" | "undo" | "redo")
    {
        pulsar_core::ensure_protected_segments_unchanged(&current.program, program, &regions)
    } else {
        ensure_protected_regions_unchanged(&current.program, program, &regions)
    };
    if let Err(error) = preserved {
        if matches!(request.command, Command::Undo | Command::Redo)
            && matches!(kind, "undo" | "redo")
        {
            // An explicit authorized history restore can unwind automatic manual
            // protection and its motion together; ordinary edits cannot bypass it.
            require_grant(tx, session, id, Scope::ManageProtection)?;
        } else {
            return Err(ProtocolError::new(ErrorCode::Forbidden, error.to_string()));
        }
    }
    let protected = if authored_candidate {
        let changed = pulsar_core::changed_motion_regions(&current.program, program)
            .map_err(|error| ProtocolError::new(ErrorCode::ResourceExhausted, error.to_string()))?;
        let policy = pulsar_core::protect_changed_motion(&regions, &changed, 1024)
            .map_err(|error| ProtocolError::new(ErrorCode::ResourceExhausted, error.to_string()))?;
        encode(&policy)?
    } else {
        protected
    };
    let next = current.revision.checked_next().map_err(internal)?;
    let encoded = motion_state::save(tx, program)?;
    let actor = actor(tx, session)?;
    tx.execute(
        "UPDATE projects SET revision=?2,program=?3,protected=?4 WHERE id=?1",
        params![id.as_str(), rev_sql(next)?, encoded, protected],
    )
    .map_err(internal)?;
    tx.execute(
        "INSERT INTO revisions VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            id.as_str(),
            rev_sql(next)?,
            actor,
            kind,
            label,
            encoded,
            protected
        ],
    )
    .map_err(internal)?;
    source_placements::capture_revision(tx, id, next)?;
    if append {
        append_history(tx, id, &encoded, &protected)?;
    }
    lineage::record(
        tx,
        request,
        next,
        current.revision,
        &current.program,
        program,
        append,
    )?;
    event(tx, id, next, EventBody::ProjectChanged)?;
    Ok(next)
}
fn append_history(
    tx: &Transaction<'_>,
    id: &ProjectId,
    program: &str,
    protected: &str,
) -> PResult<()> {
    let cursor: i64 = tx
        .query_row(
            "SELECT history_cursor FROM projects WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    let next_position = cursor
        .checked_add(1)
        .ok_or_else(|| ProtocolError::invalid("history overflow"))?;
    tx.execute(
        "DELETE FROM edit_states WHERE project=?1 AND position>?2",
        params![id.as_str(), cursor],
    )
    .map_err(internal)?;
    tx.execute(
        "INSERT INTO edit_states VALUES(?1,?2,?3,?4)",
        params![id.as_str(), next_position, program, protected],
    )
    .map_err(internal)?;
    source_placements::capture_history(tx, id, next_position)?;
    tx.execute(
        "UPDATE projects SET history_cursor=?2 WHERE id=?1",
        params![id.as_str(), next_position],
    )
    .map_err(internal)?;
    Ok(())
}
fn neutral_export(program: &MotionProgram) -> PResult<Vec<u8>> {
    if program
        .tracks()
        .iter()
        .any(|track| track.axis() != Axis::Stroke)
    {
        return Err(ProtocolError::unsupported(
            "ambiguous multi-axis export; use ExportAxis with an explicit axis",
        ));
    }
    neutral_export_axis(program, Axis::Stroke)
}

fn neutral_export_axis(program: &MotionProgram, axis: Axis) -> PResult<Vec<u8>> {
    let track = program.track(axis).ok_or_else(|| {
        ProtocolError::invalid(format!(
            "selected export axis {} is absent from the project",
            axis.as_str()
        ))
    })?;
    if !track.gaps().is_empty() {
        return Err(ProtocolError::unsupported(
            "standard export of selected-axis unresolved evidence gaps; explicit resolution required",
        ));
    }
    if track.actions().is_empty() {
        return Err(ProtocolError::invalid(
            "selected export axis contains no motion actions",
        ));
    }
    let mut actions = Vec::with_capacity(track.actions().len());
    let mut last = None;
    for action in track.actions() {
        let ns = action.time().as_nanos();
        if ns < 0 {
            return Err(ProtocolError::invalid(
                "standard funscript cannot encode negative project time",
            ));
        }
        let ms = ns
            .checked_add(500_000)
            .ok_or_else(|| ProtocolError::invalid("export timestamp overflow"))?
            / 1_000_000;
        if last.is_some_and(|previous| previous >= ms) {
            return Err(ProtocolError::invalid(
                "millisecond quantization merges ordered actions; explicit resampling required",
            ));
        }
        last = Some(ms);
        actions.push(serde_json::json!({
            "at": ms,
            "pos": (action.position().value() * 100.0).round() as u32,
        }));
    }
    serde_json::to_vec_pretty(
        &serde_json::json!({"version":"1.0","inverted":false,"range":90,"actions":actions}),
    )
    .map_err(internal)
}

#[cfg(test)]
pub(crate) mod tests {
    mod profile_ram {
        include!("profile_ram_engine_tests.rs");
    }
    include!("model_pack_withdrawal_tests.rs");
    use super::*;
    include!("live_recording_candidate_tests.rs");
    include!("engine_live_capture_clock_tests.rs");
    include!("engine_live_capture_delivery_tests.rs");
    #[cfg(target_os = "linux")]
    include!("annotated_engine_generation_tests.rs");
    #[cfg(target_os = "linux")]
    include!("admitted_engine_crash_tests.rs");
    #[cfg(target_os = "linux")]
    include!("native_capture_engine_tests.rs");
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    include!("engine_cuda_generation_tests.rs");
    include!("engine_cpu_generation_tests.rs");
    include!("engine_inference_fallback_tests.rs");
    include!("engine_inference_fallback_native_tests.rs");
    include!("engine_live_cpu_native_tests.rs");
    include!("engine_live_cpu_browser_native_tests.rs");
    #[cfg(target_os = "linux")]
    include!("engine_native_preparation_db_tests.rs");
    #[cfg(target_os = "linux")]
    include!("engine_native_preparation_control_tests.rs");
    include!("engine_media_probe_tests.rs");
    include!("engine_handy_registration_tests.rs");
    include!("engine_handy_session_tests.rs");
    include!("engine_handy_script_tests.rs");
    include!("engine_device_archive_tests.rs");
    include!("engine_handy_hosting_tests.rs");
    include!("engine_handy_setup_tests.rs");
    include!("engine_handy_program_tests.rs");
    include!("engine_handy_preparation_tests.rs");
    include!("engine_handy_controller_tests.rs");
    include!("engine_handy_server_clock_tests.rs");
    include!("engine_handy_device_timing_tests.rs");
    include!("engine_handy_clock_sync_tests.rs");
    include!("engine_handy_playback_clock_tests.rs");
    include!("engine_handy_continuous_timing_tests.rs");
    include!("engine_handy_player_guard_tests.rs");
    include!("engine_browser_media_tests.rs");
    include!("engine_browser_media_capture_tests.rs");
    include!("engine_browser_timing_tests.rs");
    include!("engine_handy_execution_tests.rs");
    include!("engine_handy_hsp_execution_tests.rs");
    include!("engine_handy_hsp_continuity_control_tests.rs");
    include!("engine_handy_hsp_public_preparation_io_tests.rs");
    include!("engine_handy_emergency_tests.rs");
    include!("engine_handy_playback_tests.rs");
    include!("engine_shutdown_tests.rs");
    include!("engine_inference_attempt_native_tests.rs");
    include!("engine_inference_equivalence_tests.rs");
    include!("engine_handy_setup_status_tests.rs");
    include!("engine_handy_setup_send_tests.rs");
    include!("engine_handy_setup_cancel_tests.rs");
    include!("engine_handy_qualification_tests.rs");
    include!("engine_handy_browser_start_qualification_tests.rs");
    include!("engine_handy_hsp_source_qualification_tests.rs");
    include!("engine_handy_browser_playback_tests.rs");
    include!("engine_handy_reconciliation_tests.rs");
    struct Fixture {
        engine: Arc<Engine>,
        session: SessionId,
        project: ProjectId,
        dir: tempfile::TempDir,
    }
    thread_local! {static TOKENS:std::cell::RefCell<HashMap<String,String>>=std::cell::RefCell::new(HashMap::new());}
    fn request(
        session: &SessionId,
        project: Option<&ProjectId>,
        revision: Option<u64>,
        command: Command,
    ) -> Request {
        Request {
            version: PROTOCOL_VERSION,
            request_id: id!(RequestId),
            session: Some(session.clone()),
            auth_token: TOKENS.with(|tokens| tokens.borrow().get(session.as_str()).cloned()),
            project: project.cloned(),
            expected_revision: revision.map(RevisionId::new),
            command,
        }
    }
    fn pair(engine: &Arc<Engine>, name: &str) -> SessionId {
        let r = Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: name.into(),
                pairing_token: engine.token.clone(),
            },
        );
        match engine.handle(r).result.unwrap() {
            ResponseBody::Paired {
                session,
                auth_token,
            } => {
                TOKENS.with(|tokens| tokens.borrow_mut().insert(session.to_string(), auth_token));
                session
            }
            _ => panic!("pairing response"),
        }
    }
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("nonexecuted-worker-fixture");
        fs::write(&binary, b"authority-test executable identity").unwrap();
        let engine =
            Engine::open(EngineConfig::new(dir.path().join("private-state"), binary)).unwrap();
        let session = pair(&engine, "test client");
        let r = request(
            &session,
            None,
            None,
            Command::CreateProject {
                name: "Test project".into(),
            },
        );
        let project = match engine.handle(r).result.unwrap() {
            ResponseBody::Project(p) => p.project_id,
            _ => panic!("project response"),
        };
        Fixture {
            engine,
            session,
            project,
            dir,
        }
    }

    #[test]
    fn captured_model_path_cannot_be_rebound_to_different_bytes() {
        let f = fixture();
        let source = f.dir.path().join("model.onnx");
        fs::write(&source, b"original model bytes").unwrap();
        let model = f.engine.snapshot_model(&source).unwrap();
        let repeated = f.engine.snapshot_model(&model.path).unwrap();
        assert_eq!(repeated.identity.sha256, model.identity.sha256);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&model.path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(not(unix))]
        {
            let mut permissions = fs::metadata(&model.path).unwrap().permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&model.path, permissions).unwrap();
        }
        fs::write(&model.path, b"changed captured model bytes").unwrap();
        assert_eq!(
            f.engine.snapshot_model(&model.path).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
    }

    #[test]
    fn unsupported_visual_tiers_refuse_before_job_or_attempt_creation() {
        let f = fixture();
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "INSERT OR IGNORE INTO grants VALUES(?1,?2,?3)",
                params![
                    f.session.as_str(),
                    f.project.as_str(),
                    scope_name(Scope::Generate)
                ],
            )
            .unwrap();
        for (preset, settings) in [
            ("maximum", GenerationSettings::default()),
            (
                "default",
                GenerationSettings {
                    axis: Axis::Yaw,
                    ..Default::default()
                },
            ),
        ] {
            let result = f.engine.handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::Generate {
                    source_version: SourceVersionId::new("not-needed-for-tier-refusal").unwrap(),
                    preset: preset.into(),
                    settings: Some(settings),
                    model_path: None,
                },
            ));
            assert_eq!(result.result.unwrap_err().code, ErrorCode::Unsupported);
        }
        let db = f.engine.db.lock().unwrap();
        for table in ["jobs", "attempts"] {
            let count: i64 = db
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0);
        }
        assert_eq!(
            fs::read_dir(f.engine.config.state_dir.join("attempts"))
                .unwrap()
                .count(),
            0
        );
        #[cfg(unix)]
        assert!(!f.engine.worker_fence.has_pending_worker_custody().unwrap());
    }

    #[test]
    fn auxiliary_schema_failure_rolls_back_every_member() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE sources(project TEXT,version TEXT,PRIMARY KEY(project,version));CREATE TABLE candidates(id TEXT PRIMARY KEY);PRAGMA user_version=8;").unwrap();
        assert!(initialize_auxiliary_schema(&mut db).is_err());
        let created:i64=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT IN ('sources','candidates')",[],|r|r.get(0)).unwrap();
        assert_eq!(
            created, 0,
            "a failed auxiliary migration must not publish earlier tables"
        );
        let columns: Vec<String> = db
            .prepare("PRAGMA table_info(candidates)")
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(columns, vec!["id"]);
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            8
        );
        assert!(db.is_autocommit());
    }

    #[test]
    fn auxiliary_schema_preserves_rows_defaults_and_reopens_idempotently() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE sources(project TEXT,version TEXT,PRIMARY KEY(project,version));CREATE TABLE candidates(id TEXT PRIMARY KEY);CREATE TABLE exports(session TEXT,request TEXT,PRIMARY KEY(session,request));INSERT INTO candidates VALUES('retained');INSERT INTO exports VALUES('session','request');PRAGMA user_version=8;").unwrap();
        initialize_auxiliary_schema(&mut db).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT review FROM candidates WHERE id='retained'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "[]"
        );
        assert_eq!(
            db.query_row(
                "SELECT axis FROM exports WHERE session='session' AND request='request'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "stroke"
        );
        db.execute(
            "UPDATE candidates SET review='retained review' WHERE id='retained'",
            [],
        )
        .unwrap();
        db.execute(
            "UPDATE exports SET axis='roll' WHERE session='session' AND request='request'",
            [],
        )
        .unwrap();
        initialize_auxiliary_schema(&mut db).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT review FROM candidates WHERE id='retained'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "retained review"
        );
        assert_eq!(
            db.query_row(
                "SELECT axis FROM exports WHERE session='session' AND request='request'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "roll"
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            8
        );
        assert!(db.is_autocommit());
    }

    #[cfg(unix)]
    fn verify_mixed_archive_clone(
        f: &Fixture,
        source: &SourceArtifact,
        master: &ProjectSnapshot,
        candidate: &CandidateId,
        job: &JobId,
        retimed: bool,
    ) {
        use crate::project_package_format::{write_package, PackageCodecLimits};
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "INSERT OR IGNORE INTO grants VALUES(?1,?2,?3)",
                params![
                    f.session.as_str(),
                    f.project.as_str(),
                    scope_name(Scope::PackageProject)
                ],
            )
            .unwrap();
        }
        let token = TOKENS
            .with(|tokens| tokens.borrow().get(f.session.as_str()).cloned())
            .unwrap();
        let master = if retimed {
            let spec = SourcePlacementSpec {
                source_version: source.source_version.clone(),
                source_start: SourceTimestamp::new(0, 1).unwrap(),
                source_end: SourceTimestamp::new(750_000_001, 1_000_000_000).unwrap(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                rate_numerator: 3,
                rate_denominator: 1,
            };
            let ResponseBody::Project(placed) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(master.revision.value()),
                    Command::SetSourcePlacement {
                        spec,
                        replace: None,
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("retime base placement");
            };
            retime_mixed_fixture(f, &f.project, *placed, 2)
        } else {
            master.clone()
        };
        let plan =
            project_packages::capture(&f.engine, &f.session, &token, &f.project, master.revision)
                .unwrap();
        assert_eq!(plan.manifest.retiming_receipts.len(), usize::from(retimed));
        let mut bytes = Vec::new();
        write_package(
            &plan.manifest,
            &mut bytes,
            |object| plan.open_object(object),
            PackageCodecLimits {
                max_package_bytes: DEFAULT_MAX_PROJECT_PACKAGE_BYTES,
            },
        )
        .unwrap();
        drop(plan);
        fs::rename(
            f.engine.config.state_dir.join("attempts"),
            f.dir.path().join("parked-attempts"),
        )
        .unwrap();
        let package = PackageUploadDeclaration {
            format_version: package_format_version(&bytes[..52]).unwrap(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            byte_len: bytes.len() as u64,
        };
        let ResponseBody::PackageImport(initial) = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::StartProjectImport { package },
            ))
            .result
            .unwrap()
        else {
            panic!("import admission");
        };
        let ResponseBody::PackageUpload(lease) = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::BeginProjectPackageUpload {
                    operation_id: initial.operation_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("upload lease");
        };
        let handshake = PackageUploadHandshake {
            version: PROTOCOL_VERSION,
            channel: PackageUploadChannel::ProjectPackageUpload,
            session: f.session.clone(),
            auth_token: token,
            lease_id: lease.lease_id.clone(),
            engine_epoch: lease.engine_epoch.clone(),
            operation_id: lease.operation_id.clone(),
            generation: lease.generation,
            package_sha256: lease.package.sha256.clone(),
        };
        let (connection, _) = f.engine.import_bulk_open(&handshake).unwrap();
        let mut offset = 0;
        for chunk in bytes.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
            offset = f
                .engine
                .import_bulk_append(&connection, &handshake, offset, chunk)
                .unwrap();
        }
        assert_eq!(offset, bytes.len() as u64);
        drop(connection);
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::SealProjectImport {
                    operation_id: initial.operation_id.clone(),
                    upload_generation: lease.generation,
                },
            ))
            .result
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        let cloned = loop {
            let ResponseBody::PackageImport(status) = f
                .engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::ProjectImportStatus {
                        operation_id: initial.operation_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("import status");
            };
            if status.state.is_terminal() {
                assert_eq!(
                    status.state,
                    PackageImportOperationState::Completed,
                    "{:?}",
                    status.error
                );
                break status.receipt.unwrap();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "authored archive import observation deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let alias = {
            let db = f.engine.db.lock().unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM jobs WHERE project=?1",
                    [cloned.project_id.as_str()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            let raw: String = db
                .query_row(
                    "SELECT manifest FROM project_import_archives WHERE project=?1",
                    [cloned.project_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            let m: serde_json::Value = serde_json::from_str(&raw).unwrap();
            let edge = m["imported_origins"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["container"]["project_id"] == f.project.as_str())
                .unwrap();
            let id = edge["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["origin"] == candidate.as_str())
                .unwrap()["local"]
                .as_str()
                .unwrap();
            CandidateId::new(id).unwrap()
        };
        let report = {
            let db = f.engine.db.lock().unwrap();
            archived_generation::load(
                &db,
                &cloned.project_id,
                &alias,
                &f.engine.pool,
                &f.engine.import_store,
            )
            .unwrap()
        };
        assert_eq!(report.origins.len(), 1);
        let origin = &report.origins[0];
        assert_eq!(&origin.job_id, job);
        {
            let db = f.engine.db.lock().unwrap();
            assert_eq!(
                stash_lineage::verified_archived_master(
                    &db,
                    &cloned.project_id,
                    cloned.revision,
                    &report,
                    &f.engine.pool,
                    &f.engine.import_store
                )
                .unwrap_err()
                .code,
                ErrorCode::DependencyMismatch
            );
        }

        let media = f.dir.path().join("fresh-archive-media.mkv");
        fs::copy(&source.path, &media).unwrap();
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&cloned.project_id),
                Some(cloned.revision.value()),
                Command::ImportSource {
                    path: media.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("fresh clone binding");
        };
        assert_eq!(source_version, origin.source_version);
        let current = if retimed {
            let base = project_snapshot(&f.engine.db.lock().unwrap(), &cloned.project_id).unwrap();
            retime_mixed_fixture(f, &cloned.project_id, base, 4)
        } else {
            project_snapshot(&f.engine.db.lock().unwrap(), &cloned.project_id).unwrap()
        };
        let capture = StashSourceCapture {
            endpoint: Some("http://127.0.0.1:1/graphql".into()),
            source_version,
            identity: source.identity.clone(),
            scene_id: "archive-scene".into(),
            file_id: "archive-file".into(),
            server_path: "/authored/fresh-archive-media.mkv".into(),
            local_source: media.clone(),
            script_destination: media.with_extension("funscript"),
        };
        {
            let db = f.engine.db.lock().unwrap();
            db.execute("INSERT INTO stash_sources(project,version,scene,file,receipt) VALUES(?1,?2,?3,?4,?5)",params![cloned.project_id.as_str(),capture.source_version.as_str(),capture.scene_id,capture.file_id,encode(&capture).unwrap()]).unwrap();
        }
        let export = request(
            &f.session,
            Some(&cloned.project_id),
            Some(current.revision.value()),
            Command::StashExportScript {
                connection: StashConnection {
                    endpoint: capture.endpoint.clone().unwrap(),
                    credential_env: None,
                },
                source_version: capture.source_version.clone(),
                scene_id: capture.scene_id.clone(),
                file_id: capture.file_id.clone(),
                candidate_id: alias.clone(),
                job_id: job.clone(),
                archived_origin: Some(origin.origin_manifest.clone()),
                source_time: true,
                owner_proof: f.engine.stash_owner_token.clone(),
            },
        );
        let publisher = crate::stash_files::Publication::prepare(&capture).unwrap();
        let ResponseBody::StashScriptExported(receipt) = f
            .engine
            .export(
                &export,
                &capture.script_destination,
                "authored-archived-mixed-clock-export",
                ExportKind::Stash {
                    capture: &capture,
                    candidate: &alias,
                    job,
                    archived_origin: Some(&origin.origin_manifest),
                    publisher: &publisher,
                },
            )
            .unwrap()
        else {
            panic!("archival publication");
        };
        let proof = receipt.timing_proof.as_ref().unwrap();
        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(&proof.path).unwrap()).unwrap();
        assert_eq!(raw["schema_version"], 7);
        assert_eq!(
            raw["context"]["action_ancestry"]["carriers"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        if retimed {
            assert_eq!(
                raw["context"]["native_retimings"].as_array().unwrap().len(),
                1
            );
            assert_eq!(
                raw["context"]["archived_retimings"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                raw["context"]["archived_context"]["master"]["retiming_revisions"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            for carrier in raw["context"]["action_ancestry"]["carriers"]
                .as_array()
                .unwrap()
            {
                assert_eq!(carrier["retimings"].as_array().unwrap().len(), 2);
            }
        }
        let project_times = if retimed {
            [124_999_999, 156_249_999]
        } else {
            [166_666_666, 208_333_333]
        };
        for (project_time, frame, ns) in [
            (project_times[0], 4, 500_000_000),
            (project_times[1], 5, 625_000_000),
        ] {
            let m = raw["mappings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["project_time"] == project_time)
                .unwrap();
            assert_eq!(m["original_source_frame"], frame);
            assert_eq!(m["quantization"]["time"], ns);
        }
        let observe = request(
            &f.session,
            Some(&cloned.project_id),
            None,
            Command::StashPublicationStatus {
                publication_request_id: export.request_id.clone(),
                owner_proof: f.engine.stash_owner_token.clone(),
            },
        );
        let status = || {
            let ResponseBody::StashPublication(v) = f
                .engine
                .stash_publication_observation(&observe, &export.request_id)
                .unwrap()
            else {
                panic!("archival observation");
            };
            v.proof_observation
        };
        assert_eq!(status(), StashArtifactObservation::Verified);
        let saved_response = encode(&ResponseBody::StashScriptExported(receipt.clone())).unwrap();
        let faults = if retimed {
            vec![
                "mapping",
                "selection",
                "catalogue",
                "profile",
                "downgrade",
                "native_retime",
                "archived_retime",
                "carrier_chain",
            ]
        } else {
            vec!["mapping", "selection", "catalogue", "profile", "downgrade"]
        };
        for fault in faults {
            let mut forged = raw.clone();
            match fault {
                "mapping" => {
                    forged["mappings"][0]["original_source_frame"] = serde_json::json!(777)
                }
                "selection" => {
                    forged["context"]["action_ancestry"]["selections"][2]["carrier"] =
                        serde_json::json!(0)
                }
                "catalogue" => {
                    forged["context"]["archived_context"]["master"]["retiming_revisions"] =
                        serde_json::json!([0])
                }
                "profile" => {
                    forged["context"]["archived_context"]["origin"]["preset"] =
                        serde_json::json!("forged-preset")
                }
                "native_retime" => {
                    forged["context"]["native_retimings"][0]["to"]["spec"]["rate_numerator"] =
                        serde_json::json!(777)
                }
                "archived_retime" => {
                    forged["context"]["archived_retimings"][0]["origin_receipt"]["to"]["spec"]
                        ["rate_numerator"] = serde_json::json!(777)
                }
                "carrier_chain" => {
                    forged["context"]["action_ancestry"]["carriers"][0]["retimings"] =
                        serde_json::json!([])
                }
                _ => forged["schema_version"] = serde_json::json!(5),
            }
            let bytes = serde_json::to_vec(&forged).unwrap();
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let mut reference = proof.clone();
            reference.identity = ArtifactIdentity {
                artifact_id: ArtifactId::new(hash.clone()).unwrap(),
                sha256: hash.clone(),
                byte_len: bytes.len() as u64,
            };
            reference.path = proof.path.parent().unwrap().join(format!("{hash}.json"));
            reference.archived_context = Some(
                serde_json::from_value(forged["context"]["archived_context"].clone()).unwrap(),
            );
            fs::write(&reference.path, &bytes).unwrap();
            let mut changed = receipt.clone();
            changed.timing_proof = Some(reference.clone());
            {
                let db = f.engine.db.lock().unwrap();
                db.execute(
                    "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                    params![
                        encode(&reference).unwrap(),
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
                db.execute(
                    "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                    params![
                        encode(&ResponseBody::StashScriptExported(changed)).unwrap(),
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
            }
            assert_eq!(status(), StashArtifactObservation::Unverified, "{fault}");
            {
                let db = f.engine.db.lock().unwrap();
                db.execute(
                    "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                    params![
                        encode(proof).unwrap(),
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
                db.execute(
                    "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                    params![
                        saved_response,
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
            }
            fs::remove_file(&reference.path).unwrap();
            assert_eq!(status(), StashArtifactObservation::Verified);
        }
        assert!(!receipt.release_qualified && !proof.archived_context.as_ref().unwrap().active_job);
        if let Some(directory) = std::env::var_os("PULSAR_TEST_CLOCK_EVIDENCE_DIR") {
            let directory = PathBuf::from(directory);
            for (name, path) in [
                ("proof.json", &proof.path),
                ("script.funscript", &capture.script_destination),
            ] {
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(directory.join(name))
                    .unwrap();
                std::io::copy(&mut fs::File::open(path).unwrap(), &mut output).unwrap();
            }
        }
    }

    #[cfg(unix)]
    fn retime_mixed_fixture(
        f: &Fixture,
        project: &ProjectId,
        base: ProjectSnapshot,
        rate: i32,
    ) -> ProjectSnapshot {
        let old = base.placements[0].clone();
        let mut spec = old.spec.clone();
        spec.rate_numerator = rate;
        let ResponseBody::Project(placed) = f
            .engine
            .handle(request(
                &f.session,
                Some(project),
                Some(base.revision.value()),
                Command::SetSourcePlacement {
                    spec,
                    replace: Some(old.placement_id.clone()),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("replacement placement");
        };
        let ResponseBody::Project(retimed) = f
            .engine
            .handle(request(
                &f.session,
                Some(project),
                Some(placed.revision.value()),
                Command::RetimeMotion {
                    from_placement: old.placement_id,
                    to_placement: placed.placements[0].placement_id.clone(),
                    axes: vec![Axis::Stroke],
                },
            ))
            .result
            .unwrap()
        else {
            panic!("retimed master");
        };
        *retimed
    }
    #[cfg(unix)]
    pub(crate) fn verify_archived_retimed_mixed_export_fixture(
        tools: WorkerTools,
        source: SourceArtifact,
    ) {
        verify_mixed_export_fixture(tools, source, true, true);
    }
    #[cfg(unix)]
    pub(crate) fn verify_native_mixed_export_fixture(tools: WorkerTools, source: SourceArtifact) {
        verify_mixed_export_fixture(tools, source, false, false);
    }
    #[cfg(unix)]
    pub(crate) fn verify_archived_mixed_export_fixture(tools: WorkerTools, source: SourceArtifact) {
        verify_mixed_export_fixture(tools, source, true, false);
    }
    #[cfg(unix)]
    fn verify_mixed_export_fixture(
        tools: WorkerTools,
        mut source: SourceArtifact,
        archived: bool,
        retimed: bool,
    ) {
        source.source_version = SourceVersionId::new(source.identity.sha256.clone()).unwrap();
        let f = fixture();
        // This numerical/export fixture is not a crash-durability gate. Keep
        // its authored SQL setup independent of host journal-sync stalls.
        f.engine
            .db
            .lock()
            .unwrap()
            .execute_batch("PRAGMA synchronous=OFF;")
            .unwrap();
        let placement = SourcePlacement {
            placement_id: SourcePlacementId::new("mixed-clock-placement").unwrap(),
            created_revision: RevisionId::new(0),
            spec: SourcePlacementSpec {
                source_version: source.source_version.clone(),
                source_start: SourceTimestamp::new(0, 1).unwrap(),
                source_end: SourceTimestamp::new(750_000_001, 1_000_000_000).unwrap(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::from_nanos(0),
                rate_numerator: 3,
                rate_denominator: 1,
            },
        };
        let mut candidates = Vec::new();
        let mut jobs = Vec::new();
        if archived {
            let ResponseBody::Source { source_version } = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::ImportSource {
                        path: source.path.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("captured archive source");
            };
            assert_eq!(source_version, source.source_version);
            let db = f.engine.db.lock().unwrap();
            source.path = PathBuf::from(
                db.query_row(
                    "SELECT path FROM sources WHERE project=?1 AND version=?2",
                    params![f.project.as_str(), source_version.as_str()],
                    |r| r.get::<_, String>(0),
                )
                .unwrap(),
            );
            let tx = db.unchecked_transaction().unwrap();
            f.engine.arm_worker_fence(&tx).unwrap();
            tx.commit().unwrap();
        } else {
            let db = f.engine.db.lock().unwrap();
            db.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes) VALUES(?1,?2,?3,?3,'authored mixed clock',?4,?5)",params![f.project.as_str(),source.source_version.as_str(),source.path.to_string_lossy(),source.identity.sha256,source.identity.byte_len]).unwrap();
        }
        for stride in [3, 1] {
            let attempt = id!(AttemptId);
            let output = if archived {
                f.engine
                    .config
                    .state_dir
                    .join("attempts")
                    .join(attempt.as_str())
            } else {
                f.dir.path().join(format!("producer-{stride}"))
            };
            fs::create_dir_all(&output).unwrap();
            let manifest = WorkerRequest {
                version: PROTOCOL_VERSION,
                attempt_id: attempt,
                job_id: id!(JobId),
                project_id: f.project.clone(),
                base_revision: RevisionId::new(if stride == 3 { 0 } else { 1 }),
                source: source.clone(),
                output_dir: output.canonicalize().unwrap(),
                operation: WorkerOperation::Generate {
                    preset: "fast".into(),
                    settings: (GenerationSettings {
                        source_placement: Some(placement.clone()),
                        source_sample_stride: stride,
                        analysis_width: 64,
                        analysis_height: 64,
                        ..Default::default()
                    })
                    .into(),
                    model: None,
                },
                budget: ResourceBudget {
                    memory_bytes: 512 * 1024 * 1024,
                    output_bytes: 16 * 1024 * 1024,
                    wall_time_ms: 30_000,
                    cpu_threads: 1,
                },
                tools: tools.clone(),
                dependencies: vec![],
                cuda_execution: None,
                independent_audio: None,
            };
            let WorkerOutput::Generated {
                mut program,
                mut receipt,
                mut lineage,
                review,
            } = crate::worker::execute_manifest(&manifest).result.unwrap()
            else {
                panic!("authored generation");
            };
            let broker_program = output.join("program.json");
            let broker_receipt = output.join("receipt.json");
            fs::rename(&program.path, &broker_program).unwrap();
            program.path = broker_program;
            fs::rename(&receipt.path, &broker_receipt).unwrap();
            receipt.path = broker_receipt;
            let motion: MotionProgram =
                serde_json::from_slice(&fs::read(&program.path).unwrap()).unwrap();
            assert!(
                motion.track(Axis::Stroke).unwrap().gaps().is_empty(),
                "authored stride {stride} must have decoded support through the placement boundary"
            );
            if archived {
                lineage.push(receipt.identity.clone());
            } else {
                lineage = vec![receipt.identity.clone()];
            }
            let candidate = id!(CandidateId);
            let raw = encode(&manifest).unwrap();
            {
                let db = f.engine.db.lock().unwrap();
                db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,?7,?4,?5,'complete',?6)",params![manifest.job_id.as_str(),manifest.attempt_id.as_str(),f.project.as_str(),f.session.as_str(),source.source_version.as_str(),raw,rev_sql(manifest.base_revision).unwrap()]).unwrap();
                db.execute(
                    "INSERT INTO attempts VALUES(?1,?2,?3,?4)",
                    params![
                        manifest.attempt_id.as_str(),
                        manifest.job_id.as_str(),
                        raw,
                        if archived { "completed" } else { "complete" }
                    ],
                )
                .unwrap();
                let reference = motion_state::save(&db, &motion).unwrap();
                db.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?7,?3,?4,?5,?6)",params![candidate.as_str(),f.project.as_str(),reference,manifest.job_id.as_str(),encode(&lineage).unwrap(),encode(&review).unwrap(),rev_sql(manifest.base_revision).unwrap()]).unwrap();
            }
            if stride == 3 {
                assert!(matches!(
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(0),
                            Command::CommitCandidate {
                                candidate_id: candidate.clone()
                            }
                        ))
                        .result
                        .unwrap(),
                    ResponseBody::Project(_)
                ));
            }
            candidates.push(candidate);
            jobs.push(manifest.job_id);
        }
        let merged = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::MergeCandidate {
                    candidate_id: candidates[1].clone(),
                    axes: vec![Axis::Stroke],
                    range: Some(
                        TimeRange::new(
                            ProjectTime::from_nanos(125_000_000),
                            ProjectTime::from_nanos(250_000_000),
                        )
                        .unwrap(),
                    ),
                },
            ))
            .result
            .unwrap();
        let ResponseBody::Project(master) = merged else {
            panic!("merged project");
        };
        if archived {
            verify_mixed_archive_clone(&f, &source, &master, &candidates[0], &jobs[0], retimed);
            return;
        }
        let capture = StashSourceCapture {
            endpoint: Some("http://127.0.0.1:1/graphql".into()),
            source_version: source.source_version.clone(),
            identity: source.identity.clone(),
            scene_id: "authored-scene".into(),
            file_id: "authored-file".into(),
            server_path: "/authored/moving.mkv".into(),
            local_source: source.path.clone(),
            script_destination: source.path.with_extension("funscript"),
        };
        {
            let db = f.engine.db.lock().unwrap();
            db.execute("INSERT INTO stash_sources(project,version,scene,file,receipt) VALUES(?1,?2,?3,?4,?5)",params![f.project.as_str(),capture.source_version.as_str(),capture.scene_id,capture.file_id,encode(&capture).unwrap()]).unwrap();
        }
        let command = Command::StashExportScript {
            connection: StashConnection {
                endpoint: capture.endpoint.clone().unwrap(),
                credential_env: None,
            },
            source_version: source.source_version.clone(),
            scene_id: capture.scene_id.clone(),
            file_id: capture.file_id.clone(),
            candidate_id: candidates[0].clone(),
            job_id: jobs[0].clone(),
            archived_origin: None,
            source_time: true,
            owner_proof: f.engine.stash_owner_token.clone(),
        };
        let export = request(
            &f.session,
            Some(&f.project),
            Some(master.revision.value()),
            command,
        );
        let publisher = crate::stash_files::Publication::prepare(&capture).unwrap();
        // Exercise the production export seam after the independently tested
        // Stash discovery/capture preflight; this fixture contacts no server.
        let ResponseBody::StashScriptExported(receipt) = f
            .engine
            .export(
                &export,
                &capture.script_destination,
                "authored-mixed-clock-export",
                ExportKind::Stash {
                    capture: &capture,
                    candidate: &candidates[0],
                    job: &jobs[0],
                    archived_origin: None,
                    publisher: &publisher,
                },
            )
            .unwrap()
        else {
            panic!("published script");
        };
        let proof = receipt.timing_proof.as_ref().unwrap();
        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(&proof.path).unwrap()).unwrap();
        assert_eq!(raw["schema_version"], 6);
        assert_eq!(
            raw["context"]["action_ancestry"]["carriers"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        for (project_time, frame, source_ns) in
            [(166_666_666, 4, 500_000_000), (208_333_333, 5, 625_000_000)]
        {
            let mapping = raw["mappings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["project_time"] == project_time)
                .unwrap();
            assert_eq!(mapping["original_source_frame"], frame);
            assert_eq!(mapping["quantization"]["time"], source_ns);
        }
        assert!(!receipt.release_qualified);
        let observe = request(
            &f.session,
            Some(&f.project),
            None,
            Command::StashPublicationStatus {
                publication_request_id: export.request_id.clone(),
                owner_proof: f.engine.stash_owner_token.clone(),
            },
        );
        let ResponseBody::StashPublication(observation) = f
            .engine
            .stash_publication_observation(&observe, &export.request_id)
            .unwrap()
        else {
            panic!("publication observation");
        };
        assert_eq!(
            observation.proof_observation,
            StashArtifactObservation::Verified
        );
        let saved_response = encode(&ResponseBody::StashScriptExported(receipt.clone())).unwrap();
        for fault in ["mapping", "selection", "anchor", "legacy_mapping"] {
            let mut forged = raw.clone();
            match fault {
                "mapping" => {
                    forged["mappings"][0]["original_source_frame"] = serde_json::json!(777)
                }
                "selection" => {
                    forged["context"]["action_ancestry"]["selections"][2]["carrier"] =
                        serde_json::json!(0)
                }
                "anchor" => {
                    forged["context"]["original_candidate"] =
                        serde_json::json!("wrong-native-anchor")
                }
                _ => {
                    forged["schema_version"] = serde_json::json!(2);
                    forged["context"]
                        .as_object_mut()
                        .unwrap()
                        .remove("action_ancestry");
                    forged["mappings"][0]["original_source_frame"] = serde_json::json!(777);
                }
            }
            let bytes = serde_json::to_vec(&forged).unwrap();
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let mut reference = proof.clone();
            reference.identity = ArtifactIdentity {
                artifact_id: ArtifactId::new(hash.clone()).unwrap(),
                sha256: hash.clone(),
                byte_len: bytes.len() as u64,
            };
            reference.path = proof.path.parent().unwrap().join(format!("{hash}.json"));
            fs::write(&reference.path, &bytes).unwrap();
            let mut forged_receipt = receipt.clone();
            forged_receipt.timing_proof = Some(reference.clone());
            {
                let db = f.engine.db.lock().unwrap();
                db.execute(
                    "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                    params![
                        encode(&reference).unwrap(),
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
                db.execute(
                    "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                    params![
                        encode(&ResponseBody::StashScriptExported(forged_receipt)).unwrap(),
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
            }
            let ResponseBody::StashPublication(observed) = f
                .engine
                .stash_publication_observation(&observe, &export.request_id)
                .unwrap()
            else {
                panic!("forged observation");
            };
            assert_eq!(
                observed.proof_observation,
                StashArtifactObservation::Unverified,
                "{fault}"
            );
            {
                let db = f.engine.db.lock().unwrap();
                db.execute(
                    "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                    params![
                        encode(proof).unwrap(),
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
                db.execute(
                    "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                    params![
                        saved_response,
                        f.session.as_str(),
                        export.request_id.as_str()
                    ],
                )
                .unwrap();
            }
            fs::remove_file(&reference.path).unwrap();
            let ResponseBody::StashPublication(restored) = f
                .engine
                .stash_publication_observation(&observe, &export.request_id)
                .unwrap()
            else {
                panic!("restored observation");
            };
            assert_eq!(
                restored.proof_observation,
                StashArtifactObservation::Verified
            );
        }
    }

    #[test]
    fn annotated_submission_requires_imported_source_before_model_snapshot_or_job_publication() {
        let f = fixture();
        let manifest: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let WorkerOperation::Generate { mut settings, .. } = manifest.operation else {
            panic!("generate fixture");
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.provider = InferenceProvider::Cpu;
        profile.schema_version = 1;
        profile.cuda_resources = None;
        profile.preprocessing.resize = InferenceResize::AnnotatedTemporalView;
        settings.annotated_view=Some(serde_json::from_value(serde_json::json!({
            "source_sha256":"a".repeat(64),"declaration":{"schema_version":1,
                "source_dimensions":[128,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"authored",
                "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}}}
        })).unwrap());
        settings.validate().unwrap();
        let version: i64 = f
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let snapshots = f.engine.config.state_dir.join("snapshots");
        let before = fs::read_dir(&snapshots).unwrap().count();
        let error = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::Generate {
                    source_version: SourceVersionId::new("absent-source").unwrap(),
                    preset: "fast".into(),
                    settings: Some(*settings),
                    model_path: Some(f.dir.path().join("absent-model.onnx")),
                },
            ))
            .result
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound, "{error}");
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(fs::read_dir(snapshots).unwrap().count(), before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn annotated_storage_reopens_and_bad_membership_refuses_before_recovery_or_wal_write() {
        for corrupt in [
            None,
            Some("UPDATE attempts SET manifest=manifest || ' '"),
            Some("DELETE FROM annotated_generation_attempts"),
            Some("PRAGMA user_version=20"),
        ] {
            let f = fixture();
            let mut manifest = crate::annotated_generation_db::tests::request();
            manifest.project_id = f.project.clone();
            manifest.base_revision = RevisionId::new(0);
            let raw = serde_json::to_string(&manifest).unwrap();
            {
                let mut db = f.engine.db.lock().unwrap();
                let tx = db.transaction().unwrap();
                f.engine.arm_worker_fence(&tx).unwrap();
                tx.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes) VALUES(?1,?2,?3,?3,'authored metadata',?4,?5)",params![f.project.as_str(),manifest.source.source_version.as_str(),manifest.source.path.to_string_lossy(),manifest.source.identity.sha256,manifest.source.identity.byte_len]).unwrap();
                tx.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,'failed',?6)",params![manifest.job_id.as_str(),manifest.attempt_id.as_str(),f.project.as_str(),f.session.as_str(),manifest.source.source_version.as_str(),raw]).unwrap();
                tx.execute(
                    "INSERT INTO attempts VALUES(?1,?2,?3,'failed')",
                    params![manifest.attempt_id.as_str(), manifest.job_id.as_str(), raw],
                )
                .unwrap();
                crate::annotated_generation_db::retain(&tx, &manifest, &f.engine.pool).unwrap();
                tx.commit().unwrap();
            }
            let config = Engine::retire_for_test(f.engine);
            if let Some(sql) = corrupt {
                let observer = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
                observer.execute_batch(sql).unwrap();
                let identities = || -> Vec<(String, String)> {
                    [
                        "projects.sqlite3",
                        "projects.sqlite3-wal",
                        "worker-fence.json",
                    ]
                    .iter()
                    .filter_map(|name| {
                        let path = config.state_dir.join(name);
                        path.is_file().then(|| {
                            (
                                name.to_string(),
                                format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                            )
                        })
                    })
                    .collect()
                };
                let before = identities();
                assert!(
                    reopen_after_transient_leases(config.clone()).is_err(),
                    "{sql}"
                );
                assert_eq!(identities(), before, "{sql}");
                drop(observer);
            } else {
                let engine = reopen_after_transient_leases(config).unwrap();
                let db = engine.db.lock().unwrap();
                assert_eq!(
                    db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    23
                );
                assert_eq!(
                    db.query_row(
                        "SELECT manifest FROM attempts WHERE id=?1",
                        [manifest.attempt_id.as_str()],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    raw
                );
                engine.require_worker_fence(&db).unwrap();
                crate::annotated_generation_db::validate(&db, &engine.pool).unwrap();
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn reopen_after_transient_leases(config: EngineConfig) -> Result<Arc<Engine>> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match Engine::open(config.clone()) {
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                result => return result,
            }
        }
    }
    #[cfg(target_os = "linux")]
    fn arm_fixture(f: &Fixture) {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        // Historical-reader fixtures intentionally retain the preceding format.
        crate::worker_fence_db::arm(&tx, &f.engine.worker_fence.scope().unwrap()).unwrap();
        f.engine.require_worker_fence(&tx).unwrap();
        tx.commit().unwrap();
    }
    #[cfg(target_os = "linux")]
    fn live_cuda_storage_fixture() -> Fixture {
        let f = fixture();
        let source_path = f.dir.path().join("storage-source.bin");
        fs::write(&source_path, b"authored storage fixture").unwrap();
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::ImportSource { path: source_path },
            ))
            .result
            .unwrap()
        else {
            panic!("source")
        };
        let original = crate::live_cuda_evidence_db::tests::evidence(&f.engine.pool);
        let admission =
            crate::live_cuda_evidence::EvidenceAdmission::reserve(&f.engine.pool).unwrap();
        // Authored rebinding tests durable consistency only. It does not prove
        // new native execution or an authorized live finalization.
        let mut manifest = original.manifest().clone();
        manifest.stream.identity.project = f.project.clone();
        manifest.stream.identity.source = source_version;
        let mut summary = original.summary().clone();
        summary.manifest_sha256 = manifest.identity_sha256().unwrap();
        let mut receipt: serde_json::Value = serde_json::from_slice(original.receipt()).unwrap();
        receipt["live_binding"] = serde_json::to_value(&summary).unwrap();
        let mut evidence = admission
            .finished(
                &manifest,
                &summary,
                serde_json::to_vec(&receipt).unwrap(),
                original.trace().to_vec(),
            )
            .unwrap();
        drop(original);
        evidence
            .reserve_storage(fs2::available_space(&f.engine.config.state_dir).unwrap())
            .unwrap();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            f.engine.arm_worker_fence(&tx).unwrap();
            crate::live_cuda_evidence_db::retain(&tx, &evidence).unwrap();
            tx.commit().unwrap();
        }
        f
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn live_cuda_storage_reopens_and_refuses_corruption_before_recovery_or_wal() {
        for bad in [
            "PRAGMA user_version=20",
            "DROP TABLE live_cuda_evidence",
            "UPDATE live_cuda_evidence SET trace=CAST(trace || ' ' AS BLOB)",
        ] {
            let f = live_cuda_storage_fixture();
            let config = f.engine.config.clone();
            drop(f.engine);
            let reopened = reopen_after_transient_leases(config.clone()).unwrap();
            assert_eq!(
                reopened
                    .db
                    .lock()
                    .unwrap()
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                21
            );
            drop(reopened);
            let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
            keeper
                .execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;")
                .unwrap();
            keeper.execute_batch(bad).unwrap();
            let paths = [
                config.state_dir.join("projects.sqlite3"),
                config.state_dir.join("projects.sqlite3-wal"),
                config.state_dir.join("worker-fence.json"),
            ];
            assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
            let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
            assert!(Engine::open(config).is_err(), "{bad}");
            assert_eq!(
                paths
                    .iter()
                    .map(|p| fs::read(p).unwrap())
                    .collect::<Vec<_>>(),
                before,
                "{bad}"
            );
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires explicitly preserved schema-20 application reader; run serially"]
    fn live_cuda_storage_refuses_preceding_reader_without_db_wal_or_ledger_mutation() {
        let reader = std::env::var_os("PULSAR_LIVE_CUDA_PRECEDING_READER")
            .expect("pin preceding schema-20 application");
        let f = live_cuda_storage_fixture();
        let config = f.engine.config.clone();
        drop(f.engine);
        let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' schema21';").unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
        let result = std::process::Command::new("/usr/bin/timeout")
            .args(["--signal=TERM", "5"])
            .arg(&reader)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PULSAR_STATE_DIR", &config.state_dir)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration"));
        assert!(!config.state_dir.join("engine.sock").exists());
        assert_eq!(
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before
        );
        let reopened = reopen_after_transient_leases(config.clone()).unwrap();
        assert_eq!(
            reopened
                .db
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM live_cuda_evidence", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        if let Some(base) = std::env::var_os("PULSAR_LIVE_CUDA_READER_EVIDENCE") {
            let base = PathBuf::from(base);
            assert!(!base.exists());
            artifacts::private_directory(&base).unwrap();
            fs::write(base.join("preceding.stderr"), &result.stderr).unwrap();
            for (path, bytes) in paths.iter().zip(before) {
                fs::write(base.join(path.file_name().unwrap()), bytes).unwrap();
            }
        }
        eprintln!("schema-20 application refused schema21 with unchanged DB/WAL/ledger; current Engine readback reopened one retained row");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn staging_requirement_refuses_live_and_restart_admission_before_mutating_state() {
        let f = fixture();
        arm_fixture(&f);
        let config = f.engine.config.clone();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            crate::worker_fence_db::arm_staging(&tx, &f.engine.worker_fence.scope().unwrap())
                .unwrap();
            assert!(f.engine.require_worker_fence(&tx).is_err());
            assert!(f.engine.arm_worker_fence(&tx).is_err());
            tx.commit().unwrap();
        }
        drop(f.engine);
        let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' staged-reader-test';").unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
        let error = Engine::open(config)
            .err()
            .expect("format-2 ledger admitted below staging requirement");
        assert!(
            error.to_string().contains("below its database requirement"),
            "{error:#}"
        );
        assert_eq!(
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a writable delegated cgroup namespace; run explicitly serially"]
    fn native_creation_requirement_reopens_schema20_with_format4_and_preserves_scope() {
        let f = fixture();
        let config = f.engine.config.clone();
        let scope = f.engine.worker_fence.scope().unwrap();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            f.engine.arm_worker_fence(&tx).unwrap();
            f.engine.require_worker_fence(&tx).unwrap();
            f.engine.arm_staging_fence(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                20
            );
        }
        assert_eq!(f.engine.worker_fence.version().unwrap(), 4);
        let observer = Connection::open_with_flags(
            config.state_dir.join("projects.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            observer
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        assert_eq!(
            crate::worker_fence_db::binding(&observer, 20)
                .unwrap()
                .unwrap()
                .minimum_ledger_version,
            4
        );
        let claim = Arc::new(f.engine.pool.reserve(32 * 1024 * 1024).unwrap());
        let admitted = f
            .engine
            .worker_fence
            .admit_group(claim, 32 * 1024 * 1024, 1)
            .unwrap();
        let mut custody = admitted.into_worker();
        let group = custody.group_handle();
        let path = group.path().to_owned();
        custody
            .retire(std::time::Instant::now() + std::time::Duration::from_secs(3))
            .unwrap();
        drop(group);
        drop(custody);
        assert!(!path.exists());
        drop(observer);
        drop(f.engine);
        let reopened = reopen_after_transient_leases(config).unwrap();
        assert_eq!(reopened.worker_fence.scope().unwrap(), scope);
        assert_eq!(reopened.worker_fence.version().unwrap(), 4);
        let db = reopened.db.lock().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        reopened.require_worker_fence(&db).unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn creation_requirement_refuses_live_and_restart_admission_before_mutating_state() {
        let f = fixture();
        let config = f.engine.config.clone();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            f.engine.arm_staging_fence(&tx).unwrap();
            crate::worker_fence_db::arm_creation(&tx, &f.engine.worker_fence.scope().unwrap())
                .unwrap();
            assert!(f.engine.require_worker_fence(&tx).is_err());
            assert!(f.engine.arm_worker_fence(&tx).is_err());
            assert!(f.engine.arm_staging_fence(&tx).is_err());
            assert_eq!(
                crate::worker_fence_db::binding(&tx, 20)
                    .unwrap()
                    .unwrap()
                    .minimum_ledger_version,
                4
            );
            tx.commit().unwrap();
            source_placements::initialize(&mut db, 20).unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                20
            );
        }
        drop(f.engine);
        let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' creation-reader-test';").unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
        let error = Engine::open(config)
            .err()
            .expect("format-3 ledger admitted below creation requirement");
        assert!(
            error.to_string().contains("below its database requirement"),
            "{error:#}"
        );
        assert_eq!(
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires explicitly preserved format-2 application reader; run serially"]
    fn native_staging_requirement_refuses_preceding_reader_without_db_wal_mutation() {
        exercise_creation_reader_requirement(false);
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires explicitly preserved schema-19 application reader; run serially"]
    fn native_creation_requirement_refuses_schema19_reader_without_db_wal_mutation() {
        exercise_creation_reader_requirement(true);
    }
    #[cfg(target_os = "linux")]
    fn exercise_creation_reader_requirement(creation: bool) {
        use std::os::unix::fs::FileTypeExt;
        struct Process(std::process::Child);
        impl Drop for Process {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let reader = std::env::var_os(if creation {
            "PULSAR_CREATION_PRECEDING_READER"
        } else {
            "PULSAR_STAGING_PRECEDING_READER"
        })
        .expect("preceding reader required");
        let f = fixture();
        arm_fixture(&f);
        let config = f.engine.config.clone();
        if creation {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            f.engine.arm_staging_fence(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                19
            );
        }
        let scope = f.engine.worker_fence.scope().unwrap();
        drop(f.engine);
        let socket = config.state_dir.join("engine.sock");
        assert!(!socket.exists());
        let mut positive = Process(
            std::process::Command::new(&reader)
                .arg("engine")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PULSAR_STATE_DIR", &config.state_dir)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if fs::symlink_metadata(&socket).is_ok_and(|m| m.file_type().is_socket()) {
                break;
            }
            assert!(
                positive.0.try_wait().unwrap().is_none(),
                "preceding reader did not open old binding"
            );
            assert!(
                std::time::Instant::now() < end,
                "preceding reader listener deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        positive.0.kill().unwrap();
        positive.0.wait().unwrap();
        drop(positive);
        fs::remove_file(&socket).unwrap();
        eprintln!("preceding application reader opened schema-{} format-{} state and reached its listener",if creation {19}else{17},if creation {3}else{2});
        let mut keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        keeper
            .execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        let tx = keeper.transaction().unwrap();
        if creation {
            crate::worker_fence_db::arm_creation(&tx, &scope).unwrap();
        } else {
            crate::worker_fence_db::arm_staging(&tx, &scope).unwrap();
        }
        tx.commit().unwrap();
        keeper
            .execute_batch("UPDATE projects SET name=name || ' native staging-reader-test';")
            .unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
        let result = std::process::Command::new("/usr/bin/timeout")
            .args(["--signal=TERM", "5"])
            .arg(&reader)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PULSAR_STATE_DIR", &config.state_dir)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration"));
        assert!(!socket.exists());
        assert_eq!(
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before
        );
        let error = Engine::open(config)
            .err()
            .expect("downgraded ledger admitted");
        assert!(
            error.to_string().contains("below its database requirement"),
            "{error:#}"
        );
        assert_eq!(
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before
        );
        eprintln!("preceding reader refused new binding; current engine refused insufficient ledger; committed DB/WAL and fence bytes unchanged");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn engine_bound_fence_reopens_with_same_scope_and_barrier() {
        let f = fixture();
        assert!(
            crate::worker_fence_db::scope(&f.engine.db.lock().unwrap(), 8)
                .unwrap()
                .is_none()
        );
        arm_fixture(&f);
        let scope = f.engine.worker_fence.scope().unwrap();
        let config = f.engine.config.clone();
        drop(f.engine);
        let reopened = reopen_after_transient_leases(config).unwrap();
        let db = reopened.db.lock().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            17
        );
        assert_eq!(
            crate::worker_fence_db::scope(&db, 17).unwrap().as_deref(),
            Some(scope.as_str())
        );
        assert_eq!(reopened.worker_fence.scope().unwrap(), scope);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn engine_bound_fence_refuses_a_new_scope_or_missing_files() {
        for case in ["scope", "both-files"] {
            let f = fixture();
            arm_fixture(&f);
            let config = f.engine.config.clone();
            drop(f.engine);
            // Observe old-owner release before changing this test's own files.
            let owner = reopen_after_transient_leases(config.clone()).unwrap();
            drop(owner);
            if case == "scope" {
                let path = config.state_dir.join("worker-fence.json");
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["scope"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
                // Preserve the primitive's canonical struct field order.
                let old = fs::read_to_string(&path).unwrap();
                let old_scope = serde_json::from_str::<serde_json::Value>(&old).unwrap()["scope"]
                    .as_str()
                    .unwrap()
                    .to_string();
                fs::write(
                    path,
                    old.replace(&old_scope, value["scope"].as_str().unwrap()),
                )
                .unwrap();
            } else {
                fs::remove_file(config.state_dir.join("worker-fence.json")).unwrap();
                fs::remove_file(config.state_dir.join("worker-fence.lock")).unwrap();
            }
            assert!(
                reopen_after_transient_leases(config.clone()).is_err(),
                "{case}"
            );
            if case == "both-files" {
                assert!(!config.state_dir.join("worker-fence.lock").exists());
            }
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires an owned writable delegated cgroup; run explicitly serially"]
    fn native_engine_restart_refuses_populated_roster_without_signal_then_recovers() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        use std::process::{Command, Stdio};
        let f = fixture();
        arm_fixture(&f);
        let config = f.engine.config.clone();
        let group = crate::cgroup::AttemptCgroup::create(
            &format!("engine-restart-test-{}", uuid::Uuid::new_v4()),
            32 * 1024 * 1024,
            1,
        )
        .unwrap();
        f.engine.worker_fence.register(&group).unwrap();
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        drop(f.engine);
        let ledger = fs::read(config.state_dir.join("worker-fence.json")).unwrap();
        let error = reopen_after_transient_leases(config.clone())
            .err()
            .expect("populated roster admitted");
        assert!(error.to_string().contains("populated"), "{error:#}");
        assert!(child.try_wait().unwrap().is_none());
        assert_eq!(
            fs::read(config.state_dir.join("worker-fence.json")).unwrap(),
            ledger
        );
        group.kill();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() || !group.is_empty().unwrap() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let reopened = reopen_after_transient_leases(config.clone()).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(config.state_dir.join("worker-fence.json")).unwrap())
                .unwrap();
        assert!(value["groups"].as_object().unwrap().is_empty());
        assert_eq!(
            reopened
                .db
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            17
        );
    }
    fn capture_options() -> LiveCaptureOptions {
        LiveCaptureOptions {
            browser_analysis: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::Immediate,
            encoded_source: None,
            inference: None,
            policy: LivePolicy {
                maximum_analysis_age_ns: 200_000_000,
                added_buffer_ns: 0,
            },
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: 30_000,
        }
    }
    fn capture_start(f: &Fixture) -> Request {
        request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StartLiveCapture {
                engine_epoch: AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
                options: capture_options(),
            },
        )
    }
    fn capture_report(body: ResponseBody) -> LiveCaptureReport {
        let ResponseBody::LiveCapture(report) = body else {
            panic!("capture response")
        };
        *report
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn capture_restart_replays_retirement_without_admitting_old_boot() {
        let f = fixture();
        let start = capture_start(&f);
        let initial = capture_report(f.engine.handle(start.clone()).result.unwrap());
        let continuation = capture_continue(&f, &initial);
        let next = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
        let handshake = live_handshake(&f, &next);
        let config = f.engine.config.clone();
        let Fixture {
            engine,
            session,
            project,
            dir: _dir,
        } = f;
        drop(engine);
        let engine = reopen_after_transient_leases(config).unwrap();
        assert_ne!(engine.engine_epoch, initial.engine_epoch.as_str());
        for intent in [&start, &continuation] {
            let retired = capture_report(engine.handle(intent.clone()).result.unwrap());
            retired.validate().unwrap();
            assert!(!retired.active);
            assert_eq!(
                retired.retirement,
                Some(LiveCaptureRetirement::EngineRestarted)
            );
            assert_eq!(retired.admitted_pixel_bytes, 0);
            assert!(retired.server_elapsed.is_none());
        }
        assert!(!engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&initial.identity.epoch));
        assert!(!engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&next.identity.epoch));
        assert!(engine.live_bulk_ready(&handshake, None).is_err());
        let mut stale = start.clone();
        stale.request_id = id!(RequestId);
        assert_eq!(
            engine.handle(stale).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        let mut conflict = start.clone();
        if let Command::StartLiveCapture { options, .. } = &mut conflict.command {
            options.lease_ms = 20_000;
        }
        assert_eq!(
            engine.handle(conflict).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let mut revision = start.clone();
        revision.request_id = id!(RequestId);
        revision.expected_revision = Some(RevisionId::new(1));
        assert_eq!(
            engine.handle(revision).result.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let fresh = request(
            &session,
            Some(&project),
            Some(0),
            Command::StartLiveCapture {
                engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                options: capture_options(),
            },
        );
        let active = capture_report(engine.handle(fresh).result.unwrap());
        assert!(active.active);
        assert_ne!(active.identity.epoch, next.identity.epoch);
        engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: active.identity.epoch,
                },
            ))
            .result
            .unwrap();
        let admin = pair(&engine, "restart replay administrator");
        engine
            .handle(request(
                &session,
                None,
                None,
                Command::Grant {
                    session: admin.clone(),
                    project_id: project.clone(),
                    scopes: vec![Scope::Read, Scope::ManageGrants],
                },
            ))
            .result
            .unwrap();
        engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Revoke {
                    session: session.clone(),
                    project_id: project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            engine.handle(start).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    #[test]
    fn capture_lifecycle_replays_without_rearming_and_preserves_project_revision() {
        let f = fixture();
        let start = capture_start(&f);
        let first = capture_report(f.engine.handle(start.clone()).result.unwrap());
        assert!(first.active);
        assert_eq!(
            first.admitted_pixel_bytes,
            crate::live_frames::LIVE_RGB_ADMISSION_BYTES
        );
        assert_eq!(first.received_frames, 0);
        assert_eq!(
            capture_report(f.engine.handle(start.clone()).result.unwrap()).identity,
            first.identity
        );
        let mut changed = start.clone();
        if let Command::StartLiveCapture { options, .. } = &mut changed.command {
            options.lease_ms = 20_000;
        }
        assert_eq!(
            f.engine.handle(changed).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert_eq!(
            f.engine.handle(capture_start(&f)).result.unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        let stop = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StopLiveCapture {
                epoch: first.identity.epoch.clone(),
            },
        );
        let stopped = capture_report(f.engine.handle(stop.clone()).result.unwrap());
        assert!(!stopped.active);
        assert_eq!(stopped.retirement, Some(LiveCaptureRetirement::Stopped));
        assert_eq!(stopped.admitted_pixel_bytes, 0);
        assert!(!capture_report(f.engine.handle(stop).result.unwrap()).active);
        assert!(!capture_report(f.engine.handle(start).result.unwrap()).active);
        assert_eq!(
            project_snapshot(&f.engine.db.lock().unwrap(), &f.project)
                .unwrap()
                .revision,
            RevisionId::new(0)
        );
        assert!(capture_report(f.engine.handle(capture_start(&f)).result.unwrap()).active);
    }
    fn capture_continue(f: &Fixture, prior: &LiveCaptureReport) -> Request {
        let mut options = prior.options.clone();
        options.source_anchor = SourceTimestamp::new(1, 1).unwrap();
        options.project_anchor = ProjectTime::from_nanos(1_000_000_000);
        options.presentation_anchor = LiveClockTick(0);
        request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::ContinueLiveCapture {
                engine_epoch: prior.engine_epoch.clone(),
                predecessor: prior.identity.epoch.clone(),
                options,
            },
        )
    }
    #[test]
    fn capture_continuation_fences_predecessor_and_replay_never_rearms() {
        let f = fixture();
        let start = capture_start(&f);
        let prior = capture_report(f.engine.handle(start.clone()).result.unwrap());
        let old_channel = live_handshake(&f, &prior);
        f.engine
            .live_bulk_upload(&old_channel, &live_header(0, 0), &vec![0; LIVE_FRAME_BYTES])
            .unwrap();
        let continuation = capture_continue(&f, &prior);
        let next = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
        assert_ne!(prior.identity.epoch, next.identity.epoch);
        assert_eq!(prior.identity.source, next.identity.source);
        assert_eq!(
            next.continuation.as_ref().unwrap().predecessor,
            prior.identity
        );
        assert_eq!(
            next.continuation
                .as_ref()
                .unwrap()
                .predecessor_last_frame
                .as_ref()
                .unwrap()
                .frame,
            FrameId::new(0)
        );
        assert_eq!(next.received_frames, 0);
        assert!(next.last_frame.is_none());
        assert!(f
            .engine
            .live_bulk_upload(&old_channel, &live_header(1, 1), &vec![1; LIVE_FRAME_BYTES])
            .is_err());
        let retired = capture_report(f.engine.handle(start).result.unwrap());
        assert_eq!(retired.retirement, Some(LiveCaptureRetirement::Superseded));
        assert!(retired.queue.is_none());
        let new_channel = live_handshake(&f, &next);
        assert!(f
            .engine
            .live_bulk_upload(&new_channel, &live_header(0, 0), &vec![0; LIVE_FRAME_BYTES])
            .is_err());
        let mut header = live_header(0, 0);
        header.source_time = SourceTimestamp::new(1, 1).unwrap();
        f.engine
            .live_bulk_upload(&new_channel, &header, &vec![0; LIVE_FRAME_BYTES])
            .unwrap();
        let replayed = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
        assert_eq!(replayed.identity, next.identity);
        assert_eq!(replayed.received_frames, 1);
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: next.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!capture_report(f.engine.handle(continuation).result.unwrap()).active);
        assert!(f
            .engine
            .handle(capture_continue(&f, &prior))
            .result
            .is_err());
        assert_eq!(
            project_snapshot(&f.engine.db.lock().unwrap(), &f.project)
                .unwrap()
                .revision,
            RevisionId::new(0)
        );
    }
    #[test]
    fn capture_continuation_admission_and_receipt_failure_preserve_predecessor() {
        let f = fixture();
        let prior = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        for mode in 0..5 {
            let mut continuation = capture_continue(&f, &prior);
            if let Command::ContinueLiveCapture {
                engine_epoch,
                options,
                ..
            } = &mut continuation.command
            {
                match mode {
                    0 => options.source_anchor = SourceTimestamp::new(0, 1).unwrap(),
                    1 => options.project_anchor = ProjectTime::ZERO,
                    2 => options.presentation_anchor = LiveClockTick(1),
                    3 => options.policy.maximum_analysis_age_ns += 1,
                    _ => *engine_epoch = AttemptId::new("retired-boot").unwrap(),
                }
            }
            assert!(f.engine.handle(continuation).result.is_err());
            assert!(f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .active_for_test(&prior.identity.epoch));
        }
        let continuation = capture_continue(&f, &prior);
        f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER deny_capture_continuation BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'authored continuation receipt failure'); END;").unwrap();
        assert!(f.engine.handle(continuation.clone()).result.is_err());
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&prior.identity.epoch));
        let db = f.engine.db.lock().unwrap();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM request_identities WHERE request=?1",
                [continuation.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        db.execute_batch("DROP TRIGGER deny_capture_continuation")
            .unwrap();
        drop(db);
        assert!(capture_report(f.engine.handle(continuation).result.unwrap()).active);
    }
    #[test]
    fn fractional_capture_continuation_derives_exact_phase_and_rejects_equal_floor_substitution() {
        let f = fixture();
        let start = capture_start(&f);
        let prior = capture_report(f.engine.handle(start).result.unwrap());
        let anchor = SourceTimestamp::new(1, 30).unwrap();
        let options = prior.continuation_options(anchor, 30_000).unwrap();
        assert_eq!(
            options.exact_project_anchor,
            Some(ExactSeconds::new(1, 30).unwrap())
        );
        assert_eq!(options.project_anchor, ProjectTime::from_nanos(33_333_333));
        let operation = |options| {
            request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::ContinueLiveCapture {
                    engine_epoch: prior.engine_epoch.clone(),
                    predecessor: prior.identity.epoch.clone(),
                    options,
                },
            )
        };
        let mut changed = options.clone();
        changed.exact_project_anchor = Some(ExactSeconds::new(33_333_333, 1_000_000_000).unwrap());
        changed.validate().unwrap();
        assert!(f.engine.handle(operation(changed)).result.is_err());
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&prior.identity.epoch));
        let next = capture_report(f.engine.handle(operation(options)).result.unwrap());
        let channel = live_handshake(&f, &next);
        for frame in 0..3 {
            let mut header = live_header(frame, frame as u8);
            header.source_time = SourceTimestamp::new(frame as i64 + 1, 30).unwrap();
            let (report, _) = f
                .engine
                .live_bulk_upload(&channel, &header, &vec![frame as u8; LIVE_FRAME_BYTES])
                .unwrap();
            assert_eq!(
                report.clock.map(header.source_time).unwrap().project,
                prior.clock.map(header.source_time).unwrap().project
            );
        }
        assert_eq!(
            next.clock
                .map(SourceTimestamp::new(3, 30).unwrap())
                .unwrap()
                .project
                .time
                .as_nanos(),
            100_000_000
        );
        let twice = next
            .continuation_options(SourceTimestamp::new(4, 30).unwrap(), 30_000)
            .unwrap();
        assert_eq!(
            twice.exact_project_anchor,
            Some(ExactSeconds::new(2, 15).unwrap())
        );
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: next.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        let retired = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: next.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert!(retired
            .continuation_options(SourceTimestamp::new(4, 30).unwrap(), 30_000)
            .is_err());
    }
    #[test]
    fn derived_capture_continuation_replays_after_retirement_and_conflicts_without_rearm() {
        let f = fixture();
        let prior = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let command = Command::ContinueLiveCaptureAt {
            engine_epoch: prior.engine_epoch.clone(),
            predecessor: prior.identity.epoch.clone(),
            source_anchor: SourceTimestamp::new(4, 30).unwrap(),
            lease_ms: 30_000,
        };
        let continuation = request(&f.session, Some(&f.project), Some(0), command.clone());
        let next = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
        assert_eq!(
            next.options.exact_project_anchor,
            Some(ExactSeconds::new(2, 15).unwrap())
        );
        assert_eq!(
            next.options.project_anchor,
            ProjectTime::from_nanos(133_333_333)
        );
        assert_eq!(
            next.clock
                .map(SourceTimestamp::new(6, 30).unwrap())
                .unwrap()
                .project
                .time
                .as_nanos(),
            200_000_000
        );
        assert_eq!(
            capture_report(f.engine.handle(continuation.clone()).result.unwrap()).identity,
            next.identity
        );
        let mut changed = continuation.clone();
        if let Command::ContinueLiveCaptureAt { source_anchor, .. } = &mut changed.command {
            *source_anchor = SourceTimestamp::new(5, 30).unwrap();
        }
        assert_eq!(
            f.engine.handle(changed).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let bad = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::ContinueLiveCaptureAt {
                engine_epoch: next.engine_epoch.clone(),
                predecessor: next.identity.epoch.clone(),
                source_anchor: SourceTimestamp::new(4, 30).unwrap(),
                lease_ms: 0,
            },
        );
        assert!(f.engine.handle(bad).result.is_err());
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&next.identity.epoch));
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: next.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!capture_report(f.engine.handle(continuation).result.unwrap()).active);
        assert!(f
            .engine
            .handle(request(&f.session, Some(&f.project), Some(0), command))
            .result
            .is_err());
    }
    #[test]
    fn capture_continuation_requires_owner_current_revision_and_active_authority() {
        let f = fixture();
        let prior = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let other = pair(&f.engine, "foreign continuation actor");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: other.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
                },
            ))
            .result
            .unwrap();
        let command = capture_continue(&f, &prior).command;
        assert_eq!(
            f.engine
                .handle(request(&other, Some(&f.project), Some(0), command.clone()))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            f.engine
                .handle(request(&f.session, Some(&f.project), Some(1), command))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .expire_for_test(&prior.identity.epoch);
        assert_eq!(
            f.engine
                .handle(capture_continue(&f, &prior))
                .result
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        assert!(!f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&prior.identity.epoch));
    }
    #[test]
    fn capture_owner_revision_and_expiration_are_fenced() {
        let f = fixture();
        let first = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let other = pair(&f.engine, "foreign capture reader");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: other.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &other,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: first.identity.epoch.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .expire_for_test(&first.identity.epoch);
        let expired = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: first.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert_eq!(expired.retirement, Some(LiveCaptureRetirement::Expired));
        let next = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::SetProtectedRegions {
                    regions: vec![ProtectedRegion {
                        axis: None,
                        range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1))
                            .unwrap(),
                    }],
                },
            ))
            .result
            .unwrap();
        let retired = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: next.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        assert_eq!(
            retired.retirement,
            Some(LiveCaptureRetirement::RevisionChanged)
        );
        assert!(!retired.active);
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(1),
                    Command::LiveCaptureStatus {
                        epoch: next.identity.epoch
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    #[test]
    fn capture_revocation_retires_state_before_restored_read_grant() {
        let f = fixture();
        let first = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let admin = pair(&f.engine, "capture grant administrator");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: admin.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::ManageGrants],
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: first.identity.epoch.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Grant {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        let retired = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: first.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert_eq!(
            retired.retirement,
            Some(LiveCaptureRetirement::AuthorityLost)
        );
        assert!(!retired.active);
        assert_eq!(
            f.engine.handle(capture_start(&f)).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    #[test]
    fn capture_timer_expires_without_another_client_request() {
        let f = fixture();
        let mut start = capture_start(&f);
        if let Command::StartLiveCapture { options, .. } = &mut start.command {
            options.lease_ms = 20;
        }
        let live = capture_report(f.engine.handle(start).result.unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&live.identity.epoch)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "capture timer did not retire expired lease"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    #[test]
    fn capture_receipt_failure_publishes_no_lease_and_rolls_back_request_identity() {
        let f = fixture();
        let start = capture_start(&f);
        f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER deny_capture_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'authored capture receipt failure'); END;").unwrap();
        assert!(f.engine.handle(start.clone()).result.is_err());
        let db = f.engine.db.lock().unwrap();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM request_identities WHERE request=?1",
                [start.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        db.execute_batch("DROP TRIGGER deny_capture_receipt")
            .unwrap();
        drop(db);
        let live = capture_report(f.engine.handle(start).result.unwrap());
        assert!(live.active);
    }
    #[test]
    fn capture_restart_refuses_unknown_old_boot_start_and_restores_no_live_state() {
        let f = fixture();
        let start = capture_start(&f);
        let first = capture_report(f.engine.handle(start.clone()).result.unwrap());
        let config = f.engine.config.clone();
        let Fixture {
            engine,
            session,
            project,
            dir,
        } = f;
        drop(engine);
        let reopened = Engine::open(config).unwrap();
        let mut unknown = start;
        unknown.request_id = id!(RequestId);
        assert_eq!(
            reopened.handle(unknown).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        assert_eq!(
            reopened
                .handle(request(
                    &session,
                    Some(&project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: first.identity.epoch
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        drop(dir);
    }
    fn live_handshake(f: &Fixture, report: &LiveCaptureReport) -> LiveBulkHandshake {
        LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveCapture,
            session: f.session.clone(),
            auth_token: TOKENS.with(|tokens| tokens.borrow()[f.session.as_str()].clone()),
            engine_epoch: report.engine_epoch.clone(),
            identity: report.identity.clone(),
        }
    }
    fn live_header(frame: u64, byte: u8) -> LiveFrameHeader {
        LiveFrameHeader {
            frame: FrameId::new(frame),
            source_time: SourceTimestamp::new(frame as i64, 1000).unwrap(),
            capture_tick: LiveClockTick(frame),
            sha256: format!("{:x}", Sha256::digest(vec![byte; LIVE_FRAME_BYTES])),
        }
    }
    #[test]
    fn live_ingress_preserves_latest_pair_and_exact_retry_without_durable_frame_rows() {
        let f = fixture();
        let report = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let handshake = live_handshake(&f, &report);
        let before: i64 = f
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
            .unwrap();
        for i in 0..100 {
            let header = live_header(i, i as u8);
            f.engine.live_bulk_ready(&handshake, Some(&header)).unwrap();
            let (received, replayed) = f
                .engine
                .live_bulk_upload(&handshake, &header, &vec![i as u8; LIVE_FRAME_BYTES])
                .unwrap();
            assert!(!replayed);
            assert_eq!(received.received_frames, i + 1);
            assert_eq!(received.last_frame, Some(header));
        }
        let (retry, replayed) = f
            .engine
            .live_bulk_upload(
                &handshake,
                &live_header(99, 99),
                &vec![99; LIVE_FRAME_BYTES],
            )
            .unwrap();
        assert!(replayed);
        assert_eq!(retry.received_frames, 100);
        assert!(f
            .engine
            .live_bulk_ready(&handshake, Some(&live_header(98, 98)))
            .is_err());
        assert_eq!(
            f.engine
                .live_bulk_ready(&handshake, Some(&live_header(99, 0)))
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        let work = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .take_for_test(&report.identity.epoch);
        assert_eq!(work.previous(), vec![98; LIVE_FRAME_BYTES]);
        assert_eq!(work.current(), vec![99; LIVE_FRAME_BYTES]);
        assert_eq!(work.declaration().previous_frame, FrameId::new(98));
        let after: i64 = f
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before, after);
    }
    #[test]
    fn live_ingress_rejects_bad_identity_hash_and_clocks_without_mutating_capture() {
        let f = fixture();
        let report = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let handshake = live_handshake(&f, &report);
        let mut wrong = handshake.clone();
        wrong.auth_token = "wrong".into();
        assert_eq!(
            f.engine.live_bulk_ready(&wrong, None).unwrap_err().code,
            ErrorCode::Unauthorized
        );
        wrong = handshake.clone();
        wrong.identity.source = SourceVersionId::new("wrong-source").unwrap();
        assert!(f.engine.live_bulk_ready(&wrong, None).is_err());
        wrong = handshake.clone();
        wrong.engine_epoch = AttemptId::new("wrong-boot").unwrap();
        assert!(f.engine.live_bulk_ready(&wrong, None).is_err());
        let mut header = live_header(0, 0);
        assert!(f
            .engine
            .live_bulk_upload(&handshake, &header, &vec![1; LIVE_FRAME_BYTES])
            .is_err());
        assert!(f.engine.live_bulk_upload(&handshake, &header, &[]).is_err());
        header.capture_tick = LiveClockTick(u64::MAX);
        assert!(f.engine.live_bulk_ready(&handshake, Some(&header)).is_err());
        assert_eq!(
            f.engine
                .live_bulk_ready(&handshake, None)
                .unwrap()
                .received_frames,
            0
        );
        f.engine
            .live_bulk_upload(&handshake, &live_header(1, 1), &vec![1; LIVE_FRAME_BYTES])
            .unwrap();
        header = live_header(2, 2);
        header.source_time = SourceTimestamp::new(0, 1).unwrap();
        assert!(f
            .engine
            .live_bulk_upload(&handshake, &header, &vec![2; LIVE_FRAME_BYTES])
            .is_err());
        assert_eq!(
            f.engine
                .live_bulk_ready(&handshake, None)
                .unwrap()
                .last_frame,
            Some(live_header(1, 1))
        );
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: report.identity.epoch,
                },
            ))
            .result
            .unwrap();
        assert!(f
            .engine
            .live_bulk_upload(&handshake, &live_header(2, 2), &vec![2; LIVE_FRAME_BYTES])
            .is_err());
    }
    #[test]
    fn live_ingress_rechecks_revocation_and_expiry_between_header_and_pixels() {
        let f = fixture();
        let report = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let handshake = live_handshake(&f, &report);
        let header = live_header(0, 0);
        f.engine.live_bulk_ready(&handshake, Some(&header)).unwrap();
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .expire_for_test(&report.identity.epoch);
        assert!(f
            .engine
            .live_bulk_upload(&handshake, &header, &vec![0; LIVE_FRAME_BYTES])
            .is_err());
        let next = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let handshake = live_handshake(&f, &next);
        f.engine.live_bulk_ready(&handshake, Some(&header)).unwrap();
        let admin = pair(&f.engine, "ingress admin");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: admin.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::ManageGrants],
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .live_bulk_upload(&handshake, &header, &vec![0; LIVE_FRAME_BYTES])
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert!(!f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&next.identity.epoch));
    }
    fn analysis_work(
        f: &Fixture,
    ) -> (
        LiveCaptureReport,
        AttemptId,
        crate::live_frames::LiveRgbWork,
        LiveInferenceReply,
    ) {
        let capture = capture_report(f.engine.handle(capture_start(f)).result.unwrap());
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 10,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact.clone(),
            runtime: artifact.clone(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::NearestStretch,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0 / 255.0; 6],
                bias: vec![0.0; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 134217728,
            minimum_output_bytes: 1048576,
            cpu_threads: 2,
            cuda_resources: None,
        };
        let manifest = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("authored-analysis").unwrap(),
            identity: capture.identity.clone(),
            clock: capture.clock.clone(),
            model: ArtifactReference {
                path: "/authored/model".into(),
                identity: artifact.clone(),
            },
            runtime: ArtifactReference {
                path: "/authored/runtime".into(),
                identity: artifact,
            },
            worker: f.engine.binary_identity.clone(),
            profile,
            budget: ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: 1048576,
                wall_time_ms: 30000,
                cpu_threads: 2,
            },
        };
        manifest.validate().unwrap();
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .arm_for_test(&capture.identity.epoch, &manifest);
        let handshake = live_handshake(f, &capture);
        for frame in 0..2 {
            f.engine
                .live_bulk_upload(
                    &handshake,
                    &live_header(frame, frame as u8),
                    &vec![frame as u8; LIVE_FRAME_BYTES],
                )
                .unwrap();
        }
        let work = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .take_for_test(&capture.identity.epoch);
        let pixels = |bytes: &[u8]| {
            let sha256 = format!("{:x}", Sha256::digest(bytes));
            ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len: bytes.len() as u64,
            }
        };
        let result = LiveInferenceReply::Estimated {
            attempt_id: manifest.attempt_id.clone(),
            identity: manifest.identity.clone(),
            profile_sha256: crate::inference_profiles::profile_identity(&manifest.profile)
                .unwrap()
                .sha256,
            input: LiveInferenceInput {
                sequence: 1,
                pair: work.declaration(),
                previous: pixels(work.previous()),
                current: pixels(work.current()),
            },
            position: NormalizedPosition::new(0.25).unwrap(),
            pair_analysis_elapsed_ns: 1,
        };
        (capture, manifest.attempt_id, work, result)
    }
    #[test]
    fn prepared_ready_preserves_prior_output_then_switches_atomically() {
        let f = fixture();
        let (prior, attempt, work, result) = analysis_work(&f);
        let (next, next_attempt) = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .prepare_for_test(&f.engine, &prior.identity.epoch);
        assert!(f
            .engine
            .live_analysis_complete(&prior.identity.epoch, &attempt, work, result)
            .unwrap());
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&prior.identity.epoch));
        assert!(!f
            .engine
            .live_analysis_ready_for_test(&next, &AttemptId::new("foreign-ready").unwrap())
            .unwrap());
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&prior.identity.epoch));
        assert!(f
            .engine
            .live_analysis_ready_for_test(&next, &next_attempt)
            .unwrap());
        let old = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: prior.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert!(!old.active);
        assert_eq!(old.retirement, Some(LiveCaptureRetirement::Superseded));
        let new = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus { epoch: next },
                ))
                .result
                .unwrap(),
        );
        assert!(new.active);
        assert_eq!(
            new.inference.unwrap().state,
            LiveCaptureInferenceState::Running
        );
    }
    #[test]
    fn prepared_admission_refuses_second_pending_successor_without_retiring_prior() {
        let f = fixture();
        let (prior, _, _, _) = analysis_work(&f);
        let (next, _) = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .prepare_for_test(&f.engine, &prior.identity.epoch);
        let status = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: next.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        let start = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StartLiveCapture {
                engine_epoch: prior.engine_epoch.clone(),
                options: status.options.clone(),
            },
        );
        assert!(f
            .engine
            .handle(start)
            .result
            .unwrap_err()
            .message
            .contains("cannot start without a predecessor"));
        let intent = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::ContinueLiveCapture {
                engine_epoch: prior.engine_epoch.clone(),
                predecessor: prior.identity.epoch.clone(),
                options: status.options,
            },
        );
        assert_eq!(
            f.engine.handle(intent).result.unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        let registry = f.engine.live_captures.lock().unwrap();
        assert!(registry.active_for_test(&prior.identity.epoch));
        assert!(registry.active_for_test(&next));
    }
    #[test]
    fn prepared_control_remains_actor_owned_despite_full_project_grants() {
        let f = fixture();
        let (prior, _, _, _) = analysis_work(&f);
        let foreign = pair(&f.engine, "prepared foreign client");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: foreign.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &foreign,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureLimits,
            ))
            .result
            .unwrap();
        let current = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: prior.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        let mut options = current
            .continuation_options(SourceTimestamp::new(1, 1).unwrap(), 30_000)
            .unwrap();
        options.continuation_mode = LiveContinuationMode::WhenReady;
        let ResponseBody::Project(other_project) = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::CreateProject {
                    name: "prepared other project".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("project response")
        };
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&other_project.project_id),
                    Some(0),
                    Command::ContinueLiveCapture {
                        engine_epoch: prior.engine_epoch.clone(),
                        predecessor: prior.identity.epoch.clone(),
                        options: options.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            f.engine
                .handle(request(
                    &foreign,
                    Some(&f.project),
                    Some(0),
                    Command::ContinueLiveCapture {
                        engine_epoch: prior.engine_epoch.clone(),
                        predecessor: prior.identity.epoch.clone(),
                        options
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        let (next, attempt) = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .prepare_for_test(&f.engine, &prior.identity.epoch);
        for epoch in [&prior.identity.epoch, &next] {
            for command in [
                Command::LiveCaptureStatus {
                    epoch: epoch.clone(),
                },
                Command::StopLiveCapture {
                    epoch: epoch.clone(),
                },
                Command::FinishLiveCapture {
                    epoch: epoch.clone(),
                },
                Command::ContinueLiveCaptureAt {
                    engine_epoch: prior.engine_epoch.clone(),
                    predecessor: epoch.clone(),
                    source_anchor: SourceTimestamp::new(2, 1).unwrap(),
                    lease_ms: 30_000,
                },
            ] {
                assert_eq!(
                    f.engine
                        .handle(request(&foreign, Some(&f.project), Some(0), command))
                        .result
                        .unwrap_err()
                        .code,
                    ErrorCode::Forbidden
                );
            }
            let owned = capture_report(
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::LiveCaptureStatus {
                            epoch: epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            );
            let mut handshake = live_handshake(&f, &owned);
            handshake.session = foreign.clone();
            handshake.auth_token = TOKENS.with(|tokens| tokens.borrow()[foreign.as_str()].clone());
            assert_eq!(
                f.engine.live_bulk_ready(&handshake, None).unwrap_err().code,
                ErrorCode::Forbidden
            );
            assert!(f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .active_for_test(epoch));
        }
        assert!(f
            .engine
            .live_analysis_ready_for_test(&next, &attempt)
            .unwrap());
        let new = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus { epoch: next },
                ))
                .result
                .unwrap(),
        );
        assert!(new.active);
        assert_eq!(
            new.inference.unwrap().state,
            LiveCaptureInferenceState::Running
        );
        assert!(!f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&prior.identity.epoch));
    }
    #[test]
    fn public_grant_revocation_fences_prepared_ready_and_restoring_grants_cannot_rearm() {
        let f = fixture();
        let (prior, _, _, _) = analysis_work(&f);
        let (next, attempt) = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .prepare_for_test(&f.engine, &prior.identity.epoch);
        let admin = pair(&f.engine, "prepared grant administrator");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: admin.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![
                        Scope::Read,
                        Scope::ManageGrants,
                        Scope::Generate,
                        Scope::ImportSource,
                    ],
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!f
            .engine
            .live_analysis_ready_for_test(&next, &attempt)
            .unwrap());
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Grant {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
                },
            ))
            .result
            .unwrap();
        assert!(!f
            .engine
            .live_analysis_ready_for_test(&next, &attempt)
            .unwrap());
        for epoch in [&prior.identity.epoch, &next] {
            let report = capture_report(
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::LiveCaptureStatus {
                            epoch: epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            );
            assert!(!report.active);
            report.validate().unwrap();
        }
    }
    #[test]
    fn prepared_failure_preserves_prior_and_prior_termination_fences_ready() {
        for case in ["failure", "stop", "expiry", "revision", "revoke"] {
            let f = fixture();
            let (prior, _, _, _) = analysis_work(&f);
            let (next, attempt) = f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .prepare_for_test(&f.engine, &prior.identity.epoch);
            match case {
                "failure" => f
                    .engine
                    .live_analysis_failure_for_test(&next, &attempt)
                    .unwrap(),
                "stop" => {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(0),
                            Command::StopLiveCapture {
                                epoch: prior.identity.epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                }
                "expiry" => f
                    .engine
                    .live_captures
                    .lock()
                    .unwrap()
                    .expire_for_test(&prior.identity.epoch),
                "revision" => {
                    f.engine
                        .db
                        .lock()
                        .unwrap()
                        .execute(
                            "UPDATE projects SET revision=1 WHERE id=?1",
                            [f.project.as_str()],
                        )
                        .unwrap();
                }
                "revoke" => {
                    f.engine
                        .db
                        .lock()
                        .unwrap()
                        .execute(
                            "DELETE FROM grants WHERE session=?1 AND project=?2",
                            [f.session.as_str(), f.project.as_str()],
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                !f.engine
                    .live_analysis_ready_for_test(&next, &attempt)
                    .unwrap(),
                "{case}"
            );
            let registry = f.engine.live_captures.lock().unwrap();
            assert!(!registry.active_for_test(&next), "{case}");
            registry.validate_report_for_test(&next);
            registry.validate_report_for_test(&prior.identity.epoch);
            assert_eq!(
                registry.active_for_test(&prior.identity.epoch),
                case == "failure",
                "{case}"
            );
        }
    }
    #[test]
    fn live_analysis_acceptance_and_status_age_are_bound_to_original_capture() {
        let f = fixture();
        let (capture, attempt, work, result) = analysis_work(&f);
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        let status = || {
            capture_report(
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::LiveCaptureStatus {
                            epoch: capture.identity.epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            )
        };
        let current = status();
        let info = current.inference.unwrap();
        assert_eq!(info.accepted, 1);
        assert!(info.latest.unwrap().fresh);
        assert_eq!(current.queue.unwrap().accepted_completions, 1);
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .age_for_test(&capture.identity.epoch);
        let aged = status();
        assert!(!aged.inference.unwrap().latest.unwrap().fresh);
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        let stopped = status();
        assert!(stopped.inference.unwrap().latest.is_none());
        assert!(stopped.queue.is_none());
    }
    #[test]
    fn live_analysis_late_completion_is_counted_stale_without_acceptance() {
        let f = fixture();
        let (capture, attempt, work, result) = analysis_work(&f);
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .age_for_test(&capture.identity.epoch);
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        let report = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: capture.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        let info = report.inference.unwrap();
        assert_eq!(info.completed, 1);
        assert_eq!(info.stale, 1);
        assert_eq!(info.accepted, 0);
        assert!(info.latest.is_none());
    }
    #[test]
    fn live_analysis_wrong_attempt_retires_without_publishing_or_rearming() {
        let f = fixture();
        let (capture, attempt, work, mut result) = analysis_work(&f);
        if let LiveInferenceReply::Estimated { attempt_id, .. } = &mut result {
            *attempt_id = AttemptId::new("foreign-attempt").unwrap();
        }
        assert_eq!(
            f.engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        let report = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: capture.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert!(!report.active);
        assert_eq!(report.retirement, Some(LiveCaptureRetirement::WorkerFailed));
        let info = report.inference.unwrap();
        assert_eq!(info.state, LiveCaptureInferenceState::Failed);
        assert_eq!(info.accepted, 0);
        assert!(info.latest.is_none());
        assert!(info.error.is_some());
        assert_eq!(report.admitted_pixel_bytes, 0);
    }
    #[test]
    fn live_analysis_revision_and_revocation_discard_issued_results() {
        for revoke in [false, true] {
            let f = fixture();
            let (capture, attempt, work, result) = analysis_work(&f);
            if revoke {
                let admin = pair(&f.engine, "analysis administrator");
                f.engine
                    .handle(request(
                        &f.session,
                        None,
                        None,
                        Command::Grant {
                            session: admin.clone(),
                            project_id: f.project.clone(),
                            scopes: vec![Scope::Read, Scope::ManageGrants],
                        },
                    ))
                    .result
                    .unwrap();
                f.engine
                    .handle(request(
                        &admin,
                        None,
                        None,
                        Command::Revoke {
                            session: f.session.clone(),
                            project_id: f.project.clone(),
                        },
                    ))
                    .result
                    .unwrap();
            } else {
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::SetProtectedRegions {
                            regions: vec![ProtectedRegion {
                                axis: None,
                                range: TimeRange::new(
                                    ProjectTime::ZERO,
                                    ProjectTime::from_nanos(1),
                                )
                                .unwrap(),
                            }],
                        },
                    ))
                    .result
                    .unwrap();
            }
            assert!(!f
                .engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap());
            assert!(!f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .active_for_test(&capture.identity.epoch));
        }
    }
    fn cuda_analysis_work(
        f: &Fixture,
    ) -> (
        LiveCaptureReport,
        AttemptId,
        crate::live_frames::LiveRgbWork,
        LiveInferenceReply,
    ) {
        let (capture, attempt, work, result) = analysis_work(f);
        let profile_sha256 = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .cuda_for_test(&capture.identity.epoch);
        let LiveInferenceReply::Estimated {
            attempt_id,
            identity,
            input,
            position,
            pair_analysis_elapsed_ns,
            ..
        } = result
        else {
            unreachable!()
        };
        (
            capture,
            attempt,
            work,
            LiveInferenceReply::CudaProvisional {
                attempt_id,
                identity,
                profile_sha256,
                manifest_sha256: "b".repeat(64),
                input,
                position,
                pair_analysis_elapsed_ns,
            },
        )
    }
    #[test]
    fn live_analysis_cuda_provisional_completion_uses_capture_clock_and_stop_clears_latest() {
        for aged in [false, true] {
            let f = fixture();
            let (capture, attempt, work, result) = cuda_analysis_work(&f);
            if aged {
                f.engine
                    .live_captures
                    .lock()
                    .unwrap()
                    .age_for_test(&capture.identity.epoch);
            }
            assert!(f
                .engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap());
            let status = || {
                capture_report(
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(0),
                            Command::LiveCaptureStatus {
                                epoch: capture.identity.epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap(),
                )
            };
            let report = status();
            report.validate().unwrap();
            let info = report.inference.unwrap();
            assert_eq!(info.completed, 1);
            assert_eq!(info.stale, u64::from(aged));
            assert_eq!(info.accepted, u64::from(!aged));
            assert_eq!(info.cuda_manifest_sha256, Some("b".repeat(64)));
            if let Some(latest) = info.latest {
                assert!(!latest.qualified);
                assert!(matches!(
                    latest.result,
                    LiveInferenceReply::CudaProvisional { .. }
                ));
            }
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::StopLiveCapture {
                        epoch: capture.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap();
            let stopped = status();
            assert!(!stopped.active);
            assert!(stopped.inference.unwrap().latest.is_none());
        }
    }
    #[test]
    fn live_analysis_cuda_rejects_foreign_manifest_and_cpu_disguise() {
        for refusal in ["manifest", "cpu-disguise", "sequence"] {
            let f = fixture();
            let (capture, attempt, work, result) = cuda_analysis_work(&f);
            let LiveInferenceReply::CudaProvisional {
                attempt_id,
                identity,
                profile_sha256,
                mut input,
                position,
                pair_analysis_elapsed_ns,
                ..
            } = result
            else {
                unreachable!()
            };
            if refusal == "sequence" {
                input.sequence = 2;
            }
            let result = if refusal == "cpu-disguise" {
                LiveInferenceReply::Estimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    position,
                    pair_analysis_elapsed_ns,
                }
            } else {
                LiveInferenceReply::CudaProvisional {
                    attempt_id,
                    identity,
                    profile_sha256,
                    manifest_sha256: if refusal == "manifest" { "c" } else { "b" }.repeat(64),
                    input,
                    position,
                    pair_analysis_elapsed_ns,
                }
            };
            assert_eq!(
                f.engine
                    .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                    .unwrap_err()
                    .code,
                ErrorCode::DependencyMismatch
            );
            let report = capture_report(
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::LiveCaptureStatus {
                            epoch: capture.identity.epoch,
                        },
                    ))
                    .result
                    .unwrap(),
            );
            assert!(!report.active);
            assert_eq!(report.retirement, Some(LiveCaptureRetirement::WorkerFailed));
            assert!(report.inference.unwrap().latest.is_none());
        }
    }
    #[test]
    fn live_analysis_cuda_revision_revocation_and_expiry_discard_issued_results() {
        for reason in ["revision", "revoke", "expiry"] {
            let f = fixture();
            let (capture, attempt, work, result) = cuda_analysis_work(&f);
            match reason {
                "revision" => {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(0),
                            Command::SetProtectedRegions {
                                regions: vec![ProtectedRegion {
                                    axis: None,
                                    range: TimeRange::new(
                                        ProjectTime::ZERO,
                                        ProjectTime::from_nanos(1),
                                    )
                                    .unwrap(),
                                }],
                            },
                        ))
                        .result
                        .unwrap();
                }
                "revoke" => {
                    let admin = pair(&f.engine, "CUDA reporting administrator");
                    f.engine
                        .handle(request(
                            &f.session,
                            None,
                            None,
                            Command::Grant {
                                session: admin.clone(),
                                project_id: f.project.clone(),
                                scopes: vec![Scope::Read, Scope::ManageGrants],
                            },
                        ))
                        .result
                        .unwrap();
                    f.engine
                        .handle(request(
                            &admin,
                            None,
                            None,
                            Command::Revoke {
                                session: f.session.clone(),
                                project_id: f.project.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                }
                "expiry" => f
                    .engine
                    .live_captures
                    .lock()
                    .unwrap()
                    .expire_for_test(&capture.identity.epoch),
                _ => unreachable!(),
            }
            assert!(!f
                .engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap());
            assert!(!f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .active_for_test(&capture.identity.epoch));
        }
    }
    #[test]
    fn live_cuda_finish_request_drains_issued_work_and_replays_current_terminal_state() {
        let f = fixture();
        let (capture, attempt, work, result) = cuda_analysis_work(&f);
        let finish = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::FinishLiveCapture {
                epoch: capture.identity.epoch.clone(),
            },
        );
        let requested = capture_report(f.engine.handle(finish.clone()).result.unwrap());
        requested.validate().unwrap();
        assert_eq!(
            requested.inference.as_ref().unwrap().state,
            LiveCaptureInferenceState::FinishRequested
        );
        assert!(requested.inference.as_ref().unwrap().latest.is_none());
        let handshake = live_handshake(&f, &capture);
        assert!(f.engine.live_bulk_ready(&handshake, None).is_err());
        assert!(f
            .engine
            .live_analysis_take(&capture.identity.epoch, &attempt)
            .unwrap()
            .is_none());
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        let replayed = capture_report(f.engine.handle(finish.clone()).result.unwrap());
        replayed.validate().unwrap();
        assert_eq!(replayed.inference.as_ref().unwrap().completed, 1);
        assert!(replayed.inference.as_ref().unwrap().latest.is_none());
        let mut conflict = finish.clone();
        conflict.command = Command::StopLiveCapture {
            epoch: capture.identity.epoch.clone(),
        };
        assert_eq!(
            f.engine.handle(conflict).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        let replayed = capture_report(f.engine.handle(finish).result.unwrap());
        assert_eq!(replayed.retirement, Some(LiveCaptureRetirement::Stopped));
        assert!(replayed.inference.unwrap().cuda_evidence.is_none());
    }
    #[test]
    fn live_cuda_finish_requires_cuda_owner_revision_and_generation_authority() {
        let f = fixture();
        let (capture, _, _, _) = analysis_work(&f);
        let finish = || {
            request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::FinishLiveCapture {
                    epoch: capture.identity.epoch.clone(),
                },
            )
        };
        assert_eq!(
            f.engine.handle(finish()).result.unwrap_err().code,
            ErrorCode::Unsupported
        );
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .cuda_for_test(&capture.identity.epoch);
        let reader = pair(&f.engine, "Finish reader");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        let foreign = || {
            request(
                &reader,
                Some(&f.project),
                Some(0),
                Command::FinishLiveCapture {
                    epoch: capture.identity.epoch.clone(),
                },
            )
        };
        assert_eq!(
            f.engine.handle(foreign()).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine.handle(foreign()).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let mut wrong = finish();
        wrong.expected_revision = Some(RevisionId::new(1));
        assert_eq!(
            f.engine.handle(wrong).result.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let mut unauth = finish();
        unauth.auth_token = None;
        assert_eq!(
            f.engine.handle(unauth).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&capture.identity.epoch));
    }
    #[test]
    fn live_analysis_cuda_finishing_freezes_intake_take_and_continuation() {
        let f = fixture();
        let (capture, attempt, work, result) = cuda_analysis_work(&f);
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        let cancel = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .finishing_for_test(&capture.identity.epoch);
        assert!(!cancel.load(Ordering::Acquire));
        let handshake = live_handshake(&f, &capture);
        assert!(f.engine.live_bulk_ready(&handshake, None).is_err());
        assert!(f
            .engine
            .live_bulk_upload(&handshake, &live_header(2, 2), &vec![2; LIVE_FRAME_BYTES])
            .is_err());
        assert!(f
            .engine
            .live_analysis_take(&capture.identity.epoch, &attempt)
            .unwrap()
            .is_none());
        assert!(f
            .engine
            .handle(capture_continue(&f, &capture))
            .result
            .is_err());
        let status = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: capture.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        status.validate().unwrap();
        assert!(status.active);
        assert_eq!(
            status.inference.as_ref().unwrap().state,
            LiveCaptureInferenceState::Finishing
        );
        assert!(status.inference.as_ref().unwrap().latest.is_none());
        assert_eq!(status.received_frames, 2);
        assert!(!cancel.load(Ordering::Acquire));
        let stopped = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::StopLiveCapture {
                        epoch: capture.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert_eq!(stopped.retirement, Some(LiveCaptureRetirement::Stopped));
        assert!(cancel.load(Ordering::Acquire));
        assert!(stopped.inference.unwrap().cuda_evidence.is_none());
    }
    #[test]
    fn live_analysis_cuda_finishing_failure_retires_without_database_lock() {
        let f = fixture();
        let (capture, attempt, work, result) = cuda_analysis_work(&f);
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        let cancel = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .finishing_for_test(&capture.identity.epoch);
        let db = f.engine.db.lock().unwrap();
        f.engine
            .live_analysis_failure_for_test(&capture.identity.epoch, &attempt)
            .unwrap();
        drop(db);
        assert!(cancel.load(Ordering::Acquire));
        let report = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: capture.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert_eq!(report.retirement, Some(LiveCaptureRetirement::WorkerFailed));
        let info = report.inference.unwrap();
        assert_eq!(info.state, LiveCaptureInferenceState::Failed);
        assert!(info.cuda_evidence.is_none());
        assert!(info.latest.is_none());
        assert!(info.error.is_some());
    }
    #[test]
    fn live_analysis_cuda_finishing_remains_under_revision_revocation_and_expiry() {
        for cause in ["revision", "revoke", "expiry"] {
            let f = fixture();
            let (capture, attempt, work, result) = cuda_analysis_work(&f);
            assert!(f
                .engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap());
            let cancel = f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .finishing_for_test(&capture.identity.epoch);
            match cause {
                "revision" => {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(0),
                            Command::SetProtectedRegions { regions: vec![] },
                        ))
                        .result
                        .unwrap();
                }
                "revoke" => {
                    let admin = pair(&f.engine, "finishing administrator");
                    f.engine
                        .handle(request(
                            &f.session,
                            None,
                            None,
                            Command::Grant {
                                session: admin.clone(),
                                project_id: f.project.clone(),
                                scopes: vec![Scope::Read, Scope::ManageGrants],
                            },
                        ))
                        .result
                        .unwrap();
                    f.engine
                        .handle(request(
                            &admin,
                            None,
                            None,
                            Command::Revoke {
                                session: f.session.clone(),
                                project_id: f.project.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                }
                "expiry" => f
                    .engine
                    .live_captures
                    .lock()
                    .unwrap()
                    .expire_for_test(&capture.identity.epoch),
                _ => unreachable!(),
            }
            let _ = f
                .engine
                .live_analysis_take(&capture.identity.epoch, &attempt);
            assert!(
                !f.engine
                    .live_captures
                    .lock()
                    .unwrap()
                    .active_for_test(&capture.identity.epoch),
                "{cause}"
            );
            assert!(cancel.load(Ordering::Acquire), "{cause}");
        }
    }
    #[test]
    fn annotated_preview_requires_import_authority_before_any_source_or_model_access() {
        let f = fixture();
        let reader = pair(&f.engine, "annotated-preview reader");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        let view:AnnotatedGenerationView=serde_json::from_value(serde_json::json!({"source_sha256":"a".repeat(64),"declaration":{"schema_version":1,"source_dimensions":[64,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"authored","video_view":{"layout":"mono","eye":"left","projection":{"kind":"flat"}}}})).unwrap();
        let context = FrameContext {
            source_version: SourceVersionId::new("missing").unwrap(),
            source_placement: SourcePlacementId::new("missing").unwrap(),
            frame: FrameId::new(0),
            transform: annotated_preview_transform(&view).unwrap(),
            seek_generation: 0,
            request_generation: 0,
        };
        let result = f
            .engine
            .handle(request(
                &reader,
                Some(&f.project),
                None,
                Command::PreviewAnnotatedFrame {
                    context,
                    time: ModelPackPreviewTime::Source {
                        source_time: SourceTimestamp::new(0, 1).unwrap(),
                        time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    },
                    spec: AnnotatedPreviewSpec {
                        view,
                        source_sample_stride: 1,
                    },
                    model: AnnotatedPreviewModel::Pack {
                        choice: ModelPackChoice {
                            manifest_sha256: "a".repeat(64),
                            selection_revision: 1,
                        },
                    },
                },
            ))
            .result;
        assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
        assert!(!f.engine.config.state_dir.join("model-packs").exists());
        assert!(f.engine.workers.lock().unwrap().is_empty());
    }
    #[test]
    fn pack_preview_requires_import_authority_before_any_source_or_pack_access() {
        let f = fixture();
        let reader = pair(&f.engine, "pack-preview reader");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        let context = FrameContext {
            source_version: SourceVersionId::new("missing-source").unwrap(),
            source_placement: SourcePlacementId::new("missing-placement").unwrap(),
            frame: FrameId::new(0),
            transform: video_view_transform(None).unwrap(),
            seek_generation: 0,
            request_generation: 0,
        };
        let result = f
            .engine
            .handle(request(
                &reader,
                Some(&f.project),
                None,
                Command::PreviewModelPackFrame {
                    context,
                    time: ModelPackPreviewTime::Source {
                        source_time: SourceTimestamp::new(0, 1).unwrap(),
                        time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    },
                    video_view: None,
                    choice: ModelPackChoice {
                        manifest_sha256: "a".repeat(64),
                        selection_revision: 1,
                    },
                },
            ))
            .result;
        assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
        assert!(!f.engine.config.state_dir.join("model-packs").exists());
        assert!(f.engine.workers.lock().unwrap().is_empty());
    }
    #[test]
    fn pack_preview_completion_fence_refuses_deactivation_even_for_retained_inputs() {
        let f = fixture();
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 100,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: identity.clone(),
            runtime: identity,
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 134217728,
            minimum_output_bytes: 1048576,
            cpu_threads: 2,
            cuda_resources: None,
        };
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        let selected = crate::model_pack_selection::select(
            &tx,
            0,
            Some(("c".repeat(64), profile)),
            &f.session,
            &RequestId::new("preview-selection").unwrap(),
        )
        .unwrap();
        tx.commit().unwrap();
        let choice = selected.choice.unwrap();
        f.engine.require_current_pack_choice(&db, &choice).unwrap();
        let tx = db.transaction().unwrap();
        crate::model_pack_selection::select(
            &tx,
            1,
            None,
            &f.session,
            &RequestId::new("preview-deactivation").unwrap(),
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            f.engine
                .require_current_pack_choice(&db, &choice)
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
    }
    #[test]
    fn catalog_commands_require_fresh_session_and_separate_owner_before_storage() {
        let f = fixture();
        for owner_proof in [
            "wrong-catalog-owner".into(),
            f.engine.stash_owner_token.clone(),
            f.engine.handy_owner_token.clone(),
        ] {
            let command = Command::RefreshModelCatalog {
                source: "/nonexistent-catalog-source".into(),
                bootstrap: "/nonexistent-catalog-root".into(),
                bootstrap_sha256: "a".repeat(64),
                expected_catalog_revision: 0,
                approve_bootstrap: true,
                owner_proof,
            };
            assert_eq!(
                f.engine
                    .handle(request(&f.session, None, None, command))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        let mut status = request(
            &f.session,
            None,
            None,
            Command::ModelCatalogStatus {
                bootstrap_sha256: "a".repeat(64),
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        );
        status.auth_token = None;
        assert_eq!(
            f.engine.handle(status).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        assert!(!f.engine.config.state_dir.join("model-catalogs").exists());
    }
    #[test]
    fn model_pack_commands_require_separate_owner_capability_and_session() {
        let f = fixture();
        for owner_proof in [
            "incorrect-owner-capability".into(),
            f.engine.handy_owner_token.clone(),
            f.engine.stash_owner_token.clone(),
            f.engine.package_owner_token.clone(),
        ] {
            let result = f
                .engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::ImportModelPack {
                        source: "/nonexistent-pack-fixture".into(),
                        owner_proof,
                    },
                ))
                .result;
            assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
        }
        assert!(!f.engine.config.state_dir.join("model-packs").exists());
        let mut command = request(
            &f.session,
            None,
            None,
            Command::ModelPackStatus {
                manifest_sha256: "a".repeat(64),
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        );
        command.auth_token = None;
        assert_eq!(
            f.engine.handle(command).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        let wrong = request(
            &f.session,
            None,
            None,
            Command::ModelPackStagingStatus {
                owner_proof: f.engine.stash_owner_token.clone(),
            },
        );
        assert_eq!(
            f.engine.handle(wrong).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let forbidden = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::DiscardModelPackStage {
                    basename: ".pack-stage-authored".into(),
                    manifest_sha256: "a".repeat(64),
                    owner_proof: f.engine.stash_owner_token.clone(),
                },
            ))
            .result;
        assert_eq!(forbidden.unwrap_err().code, ErrorCode::Forbidden);
        assert!(!f
            .engine
            .config
            .state_dir
            .join("model-pack-discards")
            .exists());
        let body = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::ModelPackStagingStatus {
                    owner_proof: f.engine.model_pack_owner_token.clone(),
                },
            ))
            .result
            .unwrap();
        let ResponseBody::ModelPackStaging(report) = body else {
            panic!("missing staging report")
        };
        assert!(report.stages.is_empty());
        assert!(!f.engine.config.state_dir.join("model-packs").exists());
    }
    #[test]
    fn model_pack_selection_authority_replay_and_conflict_are_independent_of_project_grants() {
        let f = fixture();
        let wrong = request(
            &f.session,
            None,
            None,
            Command::SelectModelPack {
                manifest_sha256: None,
                expected_selection_revision: 0,
                approve_unqualified_native_execution: false,
                owner_proof: f.engine.stash_owner_token.clone(),
            },
        );
        assert_eq!(
            f.engine.handle(wrong).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let mut command = request(
            &f.session,
            None,
            None,
            Command::SelectModelPack {
                manifest_sha256: None,
                expected_selection_revision: 0,
                approve_unqualified_native_execution: false,
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        );
        let first = f.engine.handle(command.clone()).result.unwrap();
        assert_eq!(
            encode(&first).unwrap(),
            encode(&f.engine.handle(command.clone()).result.unwrap()).unwrap()
        );
        if let Command::SelectModelPack {
            expected_selection_revision,
            ..
        } = &mut command.command
        {
            *expected_selection_revision = 1;
        }
        assert_eq!(
            f.engine.handle(command).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let report = crate::model_pack_selection::status(&f.engine.db.lock().unwrap()).unwrap();
        assert_eq!(report.selection_revision, 1);
        let mut read = request(
            &f.session,
            None,
            None,
            Command::ModelPackSelectionStatus {
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        );
        read.auth_token = None;
        assert_eq!(
            f.engine.handle(read).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
    }
    fn program(position: f64) -> MotionProgram {
        MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![
                MotionAction::new(
                    ProjectTime::ZERO,
                    NormalizedPosition::new(position).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(1_000_000_000),
                    NormalizedPosition::new(position).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap(),
            ],
        )
        .unwrap()])
        .unwrap()
    }
    #[test]
    fn handy_inspection_requires_its_own_local_capability_before_credentials() {
        let f = fixture();
        let inspection = HandyInspectionRequest {
            api_key_env: "HANDY_ABSENT_TEST_API".into(),
            connection_key_env: "HANDY_ABSENT_TEST_DEVICE".into(),
            allow_online_inspection: true,
        };
        for proof in [
            String::new(),
            "incorrect-owner-capability".into(),
            f.engine.stash_owner_token.clone(),
            f.engine.package_owner_token.clone(),
        ] {
            let result = f
                .engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::HandyInspect {
                        inspection: inspection.clone(),
                        owner_proof: proof,
                    },
                ))
                .result;
            assert!(
                matches!(result,Err(error) if matches!(error.code,ErrorCode::Forbidden|ErrorCode::InvalidRequest))
            );
        }
        let result = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::HandyInspect {
                    inspection,
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ))
            .result;
        assert!(
            matches!(result,Err(error) if error.code==ErrorCode::Unavailable && error.message.contains("credential is absent"))
        );
        assert!(f
            .dir
            .path()
            .join("private-state/handy-owner.token")
            .is_file());
    }
    #[test]
    fn handy_inspection_requires_session_authentication_and_consent() {
        let f = fixture();
        let command = Command::HandyInspect {
            inspection: HandyInspectionRequest {
                api_key_env: "HANDY_ABSENT_TEST_API".into(),
                connection_key_env: "HANDY_ABSENT_TEST_DEVICE".into(),
                allow_online_inspection: true,
            },
            owner_proof: f.engine.handy_owner_token.clone(),
        };
        let mut r = request(&f.session, None, None, command.clone());
        r.auth_token = None;
        assert_eq!(
            f.engine.handle(r).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        let Command::HandyInspect {
            mut inspection,
            owner_proof,
        } = command
        else {
            unreachable!()
        };
        inspection.allow_online_inspection = false;
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::HandyInspect {
                        inspection,
                        owner_proof
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    // Stage only a proposal; the public command still checks all authority and revision rules.
    fn commit_program(f: &Fixture, revision: u64, program: MotionProgram) -> Command {
        let candidate_id = id!(CandidateId);
        let db = f.engine.db.lock().unwrap();
        let reference = crate::authority::motion_state::save(&db, &program).unwrap();
        db.execute(
            "INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,NULL,'[]','[]')",
            params![candidate_id.as_str(), f.project.as_str(), revision as i64, reference],
        ).unwrap();
        Command::CommitCandidate { candidate_id }
    }
    #[test]
    fn duplicate_edit_is_durable_replay_not_second_revision() {
        let f = fixture();
        let r = request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.3)),
        );
        let first = f.engine.handle(r.clone()).result.unwrap();
        let second = f.engine.handle(r.clone()).result.unwrap();
        assert_eq!(encode(&first).unwrap(), encode(&second).unwrap());
        let config = f.engine.config.clone();
        drop(f.engine);
        let reopened = Engine::open(config).unwrap();
        assert_eq!(
            encode(&first).unwrap(),
            encode(&reopened.handle(r).result.unwrap()).unwrap()
        );
        let db = reopened.db.lock().unwrap();
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM revisions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
    #[test]
    fn stale_revision_and_reused_request_identity_reject() {
        let f = fixture();
        let r = request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.3)),
        );
        assert!(f.engine.handle(r.clone()).result.is_ok());
        let mut changed = r.clone();
        changed.command = commit_program(&f, 0, program(0.9));
        assert_eq!(
            f.engine.handle(changed).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let mut stale = r;
        stale.request_id = id!(RequestId);
        assert_eq!(
            f.engine.handle(stale).result.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
    }
    #[test]
    fn independently_paired_client_has_no_existing_project_authority() {
        let f = fixture();
        let other = pair(&f.engine, "PulsarDesktop");
        let r = request(
            &other,
            None,
            None,
            Command::OpenProject {
                project_id: f.project.clone(),
            },
        );
        assert_eq!(
            f.engine.handle(r).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let grant = request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: other.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        );
        assert!(f.engine.handle(grant).result.is_ok());
        assert!(f
            .engine
            .handle(request(
                &other,
                Some(&f.project),
                None,
                Command::GetSnapshot
            ))
            .result
            .is_ok());
        assert_eq!(
            f.engine
                .handle(request(
                    &other,
                    Some(&f.project),
                    Some(0),
                    commit_program(&f, 0, program(0.2))
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    #[test]
    fn revocation_denies_cached_queries_and_undo_preserves_fresh_revisions() {
        let f = fixture();
        let other = pair(&f.engine, "reviewer");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: other.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        let query = request(&other, Some(&f.project), None, Command::GetSnapshot);
        f.engine.handle(query.clone()).result.unwrap();
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: other,
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine.handle(query).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                commit_program(&f, 0, program(0.2)),
            ))
            .result
            .unwrap();
        let undo = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::Undo,
            ))
            .result
            .unwrap();
        match undo {
            ResponseBody::Project(p) => {
                assert_eq!(p.revision.value(), 2);
                assert_eq!(
                    f.engine.motion_store.read(&p.motion.program).unwrap(),
                    MotionProgram::default()
                );
            }
            _ => panic!(),
        }
        let redo = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(2),
                Command::Redo,
            ))
            .result
            .unwrap();
        match redo {
            ResponseBody::Project(p) => {
                assert_eq!(p.revision.value(), 3);
                assert_eq!(
                    f.engine.motion_store.read(&p.motion.program).unwrap(),
                    program(0.2)
                );
            }
            _ => panic!(),
        }
    }
    #[test]
    fn protected_regions_cover_replacement_and_undo() {
        let f = fixture();
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                commit_program(&f, 0, program(0.2)),
            ))
            .result
            .unwrap();
        let regions = vec![ProtectedRegion {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000_001))
                .unwrap(),
        }];
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::SetProtectedRegions { regions },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(2),
                    commit_program(&f, 2, program(0.8))
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        let editor = pair(&f.engine, "ordinary editor");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: editor.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Edit],
                },
            ))
            .result
            .unwrap();
        for command in [
            Command::Undo,
            Command::SetProtectedRegions { regions: vec![] },
        ] {
            assert_eq!(
                f.engine
                    .handle(request(&editor, Some(&f.project), Some(2), command))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(2),
                Command::Undo,
            ))
            .result
            .unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            let protected: String = db
                .query_row("SELECT protected FROM projects", [], |r| r.get(0))
                .unwrap();
            assert_eq!(protected, "[]");
        }
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(3),
                Command::Redo,
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(4),
                    commit_program(&f, 4, program(0.8))
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    #[test]
    fn physical_effects_fail_closed() {
        let f = fixture();
        let result = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::Arm {
                    device_id: id!(DeviceId),
                    revision: RevisionId::new(0),
                },
            ))
            .result;
        assert_eq!(result.unwrap_err().code, ErrorCode::Unsupported);
        assert!(serde_json::from_str::<Request>(r#"{"version":1,"request_id":"unknown","session":null,"project":null,"expected_revision":null,"command":{"operation":"shell","arguments":{"command":"rm"}}}"#).is_err());
    }

    #[test]
    fn inference_catalog_reports_implemented_coverage_without_qualification() {
        let f = fixture();
        let query = request(&f.session, None, None, Command::InferenceCatalog);
        assert!(!query.command.is_mutating());
        let ResponseBody::InferenceCatalog(catalog) = f.engine.handle(query).result.unwrap() else {
            panic!("catalog response required");
        };
        assert_eq!(catalog.providers, [InferenceProvider::Cpu]);
        assert!(catalog.qualified_profiles.is_empty());
        assert!(!catalog.qualified_fallback_available);
        let capabilities = f.engine.capabilities();
        assert_eq!(
            capabilities
                .operations
                .iter()
                .any(|operation| operation == "generate_with_inference_fallback"),
            cfg!(all(target_os = "linux", target_arch = "x86_64")),
        );
        assert!(!capabilities.physical_playback);
        assert_eq!(catalog.memory_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(catalog.cpu_threads, 2);
        assert_eq!(catalog.output_bytes, OUTPUT_LIMIT);
        assert!(f
            .engine
            .handle(Request::new(id!(RequestId), Command::InferenceCatalog))
            .result
            .is_err());
    }
    #[test]
    fn export_has_no_provenance_and_replays_without_overwrite() {
        let f = fixture();
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                commit_program(&f, 0, program(0.2)),
            ))
            .result
            .unwrap();
        let path = f.dir.path().join("neutral.funscript");
        let r = request(
            &f.session,
            Some(&f.project),
            None,
            Command::Export { path: path.clone() },
        );
        f.engine.handle(r.clone()).result.unwrap();
        f.engine.handle(r).result.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["actions"][0]["pos"], 20);
        assert!(value.get("source").is_none());
        assert!(value.get("lineage").is_none());
        let db = f.engine.db.lock().unwrap();
        let state: String = db
            .query_row("SELECT state FROM exports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(state, "complete");
    }
    #[test]
    fn explicit_gaps_block_standard_export() {
        let gap = TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1000)).unwrap();
        let p = MotionProgram::new(vec![MotionTrack::with_gaps(
            Axis::Stroke,
            vec![],
            vec![gap],
        )
        .unwrap()])
        .unwrap();
        assert_eq!(neutral_export(&p).unwrap_err().code, ErrorCode::Unsupported);
    }
    #[test]
    fn imported_snapshot_pin_and_evict_are_scoped() {
        let f = fixture();
        let original = f.dir.path().join("original.mp4");
        fs::write(&original, b"immutable test").unwrap();
        let source = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource {
                    path: original.clone(),
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Source { source_version } => source_version,
            _ => panic!(),
        };
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::PinSource {
                    source_version: source.clone(),
                    pinned: true,
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::EvictSource {
                        source_version: source.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::PinSource {
                    source_version: source.clone(),
                    pinned: false,
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::EvictSource {
                    source_version: source,
                },
            ))
            .result
            .unwrap();
        assert_eq!(fs::read(original).unwrap(), b"immutable test");
    }
    #[test]
    fn invalid_credentials_never_create_session() {
        let f = fixture();
        let r = Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: "test".into(),
                pairing_token: "incorrect".into(),
            },
        );
        assert_eq!(
            f.engine.handle(r).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        let db = f.engine.db.lock().unwrap();
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
    #[test]
    fn full_durability_is_selected() {
        let f = fixture();
        let db = f.engine.db.lock().unwrap();
        let synchronous: i64 = db
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(synchronous, 2);
    }
    #[test]
    fn public_session_identity_is_not_a_bearer_credential() {
        let f = fixture();
        let other = pair(&f.engine, "other client");
        let mut stolen = request(&other, Some(&f.project), None, Command::GetSnapshot);
        stolen.session = Some(f.session.clone());
        assert_eq!(
            f.engine.handle(stolen).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        let mut absent = request(&f.session, Some(&f.project), None, Command::GetSnapshot);
        absent.auth_token = None;
        assert_eq!(
            f.engine.handle(absent).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
    }
    #[test]
    fn queries_do_not_accumulate_durable_request_rows() {
        let f = fixture();
        let count = || {
            let db = f.engine.db.lock().unwrap();
            db.query_row("SELECT COUNT(*) FROM requests", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let before = count();
        let query = request(&f.session, Some(&f.project), None, Command::GetSnapshot);
        for _ in 0..20 {
            f.engine.handle(query.clone()).result.unwrap();
        }
        assert_eq!(count(), before);
    }
    #[test]
    fn ordinary_edit_grant_does_not_authorize_filesystem_import() {
        let f = fixture();
        let editor = pair(&f.engine, "editor");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: editor.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Edit],
                },
            ))
            .result
            .unwrap();
        let result = f
            .engine
            .handle(request(
                &editor,
                Some(&f.project),
                None,
                Command::ImportSource {
                    path: f.dir.path().join("not-read"),
                },
            ))
            .result;
        assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
    }
    fn staged_manifest(f: &Fixture) -> (WorkerRequest, SourceVersionId) {
        let import = |name: &str, bytes: &[u8]| {
            let path = f.dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            match f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::ImportSource { path },
                ))
                .result
                .unwrap()
            {
                ResponseBody::Source { source_version } => source_version,
                _ => panic!(),
            }
        };
        let source_version = import("media.fixture", b"media");
        let model_version = import("model.fixture", b"model");
        let db = f.engine.db.lock().unwrap();
        let source = f.engine.source(&db, &f.project, &source_version).unwrap();
        let model = f.engine.source(&db, &f.project, &model_version).unwrap();
        let identity = source.identity.clone();
        let reference = ArtifactReference {
            identity: identity.clone(),
            path: source.path.clone(),
        };
        let tools = WorkerTools {
            ffmpeg: reference.clone(),
            ffprobe: reference,
            onnx_runtime: None,
        };
        let job = id!(JobId);
        let attempt = id!(AttemptId);
        let output_dir = f
            .engine
            .config
            .state_dir
            .join("attempts")
            .join(attempt.as_str());
        artifacts::private_directory(&output_dir).unwrap();
        let manifest = WorkerRequest {
            cuda_execution: None,
            independent_audio: None,
            version: PROTOCOL_VERSION,
            attempt_id: attempt.clone(),
            job_id: job.clone(),
            project_id: f.project.clone(),
            base_revision: RevisionId::new(0),
            source,
            output_dir,
            operation: WorkerOperation::Generate {
                preset: "fast".into(),
                settings: (GenerationSettings {
                    inference_profile: None,
                    model_input: Some(ModelInputContract {
                        width: 64,
                        height: 64,
                        channels: 3,
                        layout: TensorLayout::Nchw,
                        class_count: 1,
                        decoder: DetectorDecoder::YoloChannelMajor,
                    }),
                    ..Default::default()
                })
                .into(),
                model: Some(model.clone()),
            },
            budget: ResourceBudget {
                memory_bytes: 1024,
                output_bytes: 1024,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![model.identity, identity],
            tools,
        };
        db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,'queued',?6)",params![job.as_str(),attempt.as_str(),f.project.as_str(),f.session.as_str(),source_version.as_str(),encode(&manifest).unwrap()]).unwrap();
        db.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![attempt.as_str(), job.as_str(), encode(&manifest).unwrap()],
        )
        .unwrap();
        (manifest, model_version)
    }
    #[test]
    fn active_model_bytes_cannot_be_evicted_through_source_alias() {
        let f = fixture();
        let (manifest, model) = staged_manifest(&f);
        for state in ["queued", "running", "interrupted"] {
            {
                let db = f.engine.db.lock().unwrap();
                db.execute(
                    "UPDATE jobs SET state=?2 WHERE id=?1",
                    params![manifest.job_id.as_str(), state],
                )
                .unwrap();
            }
            let response = f.engine.handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::EvictSource {
                    source_version: model.clone(),
                },
            ));
            assert_eq!(response.result.unwrap_err().code, ErrorCode::Forbidden);
        }
    }
    #[test]
    fn attempt_history_records_late_cancelled_failure_without_publication_or_query_receipts() {
        let f = fixture();
        let (manifest, _) = staged_manifest(&f);
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::CancelJob {
                    job_id: manifest.job_id.clone(),
                },
            ))
            .result
            .unwrap();
        let error = ProtocolError::new(
            ErrorCode::Unavailable,
            "authored settled worker failure; no native execution",
        );
        f.engine
            .finish_job_observed(
                &manifest,
                Err(error.clone()),
                Some(WorkerRetirementObservation {
                    worker_error: Some(JobAttemptDiagnostic::from_error(&error)),
                    provider_unavailable: None,
                }),
            )
            .unwrap();
        let count = || {
            f.engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM requests", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let before = count();
        for _ in 0..3 {
            let response = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::JobAttempts {
                        job_id: manifest.job_id.clone(),
                        after_attempt: None,
                        limit: 8,
                    },
                ))
                .result
                .unwrap();
            let ResponseBody::JobAttempts(page) = response else {
                panic!("history response")
            };
            page.validate().unwrap();
            assert_eq!(page.attempts.len(), 1);
            assert_eq!(page.attempts[0].state, "cancelled");
            let observation = page.attempts[0].outcome.as_ref().unwrap();
            assert_eq!(
                observation.validation_error.as_ref().unwrap().code,
                ErrorCode::Unavailable
            );
            assert!(observation.worker_retirement.is_some());
        }
        assert_eq!(count(), before);
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM candidates WHERE job=?1",
                [manifest.job_id.as_str()],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn attempt_history_requires_project_read_grant_and_never_infers_retirement() {
        let f = fixture();
        let (manifest, _) = staged_manifest(&f);
        let stranger = pair(&f.engine, "history-stranger");
        let command = Command::JobAttempts {
            job_id: manifest.job_id.clone(),
            after_attempt: None,
            limit: 8,
        };
        let error = f
            .engine
            .handle(request(&stranger, Some(&f.project), None, command.clone()))
            .result
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        let ResponseBody::JobAttempts(page) = f
            .engine
            .handle(request(&f.session, Some(&f.project), None, command))
            .result
            .unwrap()
        else {
            panic!()
        };
        assert!(page.attempts[0].outcome.is_none());
        assert!(page.attempts[0].declared_profile.is_none());
        assert!(page.history_only);
    }
    #[test]
    fn attempt_history_queued_host_failure_reaches_terminal_state() {
        let f = fixture();
        let (manifest, _) = staged_manifest(&f);
        f.engine
            .finish_job(
                &manifest,
                Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "authored dispatch failure before worker launch",
                )),
            )
            .unwrap();
        let db = f.engine.db.lock().unwrap();
        let state = job_snapshot(&db, &manifest.job_id, &f.project).unwrap();
        assert_eq!(state.state, "failed");
        let page =
            crate::job_attempt_history::page(&db, &f.project, &manifest.job_id, None, 8).unwrap();
        let outcome = page.attempts[0].outcome.as_ref().unwrap();
        assert!(outcome.worker_retirement.is_none());
        assert_eq!(
            outcome.validation_error.as_ref().unwrap().code,
            ErrorCode::Unavailable
        );
    }
    #[test]
    fn completion_retains_cuda_membership_and_refuses_missing_changed_or_other_epoch_evidence() {
        // Authored consistency fixtures exercise the real completion transaction;
        // they do not establish launch authorization or genuine native provenance.
        for case in ["accepted", "missing", "changed", "other-epoch"] {
            let f = fixture();
            let (staged, _) = staged_manifest(&f);
            let mut manifest: WorkerRequest =
                serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                    .unwrap();
            manifest.job_id = staged.job_id;
            manifest.attempt_id = staged.attempt_id;
            manifest.project_id = f.project.clone();
            manifest.source = staged.source;
            manifest.dependencies.push(manifest.source.identity.clone());
            let (manifest, evidence) =
                crate::cuda_acceptance::authored_evidence_for(manifest, &staged.output_dir);
            let p = staged.output_dir.join("program.json");
            fs::write(&p, serde_json::to_vec(&program(0.3)).unwrap()).unwrap();
            let program = crate::worker::pin_artifact(&p).unwrap();
            let r = staged.output_dir.join("receipt.json");
            fs::write(&r, b"{\"scope\":\"authored consistency fixture\"}").unwrap();
            let receipt = crate::worker::pin_artifact(&r).unwrap();
            {
                let mut db = f.engine.db.lock().unwrap();
                let tx = db.transaction().unwrap();
                let raw = encode(&manifest).unwrap();
                tx.execute(
                    "UPDATE jobs SET manifest=?1 WHERE id=?2",
                    params![raw, manifest.job_id.as_str()],
                )
                .unwrap();
                tx.execute(
                    "UPDATE attempts SET manifest=?1 WHERE id=?2",
                    params![raw, manifest.attempt_id.as_str()],
                )
                .unwrap();
                crate::cuda_attempts::declare(&tx, &manifest, &f.engine.engine_epoch).unwrap();
                tx.execute(
                    "UPDATE jobs SET state='running' WHERE id=?1",
                    [manifest.job_id.as_str()],
                )
                .unwrap();
                tx.execute(
                    "UPDATE attempts SET state='running' WHERE id=?1",
                    [manifest.attempt_id.as_str()],
                )
                .unwrap();
                if case == "other-epoch" {
                    tx.execute(
                        "UPDATE cuda_attempt_intents SET engine_epoch=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
                }
                tx.commit().unwrap();
            }
            if case == "changed" {
                fs::write(&evidence.trace.path, b"[]").unwrap();
            }
            f.engine
                .finish_job(
                    &manifest,
                    Ok(launch::LaunchExecution {
                        output: WorkerOutput::Generated {
                            program,
                            receipt,
                            lineage: vec![],
                            review: vec![],
                        },
                        cuda_artifacts: if case == "missing" {
                            None
                        } else {
                            Some(evidence.clone())
                        },
                        worker_peak_memory_bytes: None,
                    }),
                )
                .unwrap();
            let db = f.engine.db.lock().unwrap();
            let state = job_snapshot(&db, &manifest.job_id, &f.project).unwrap();
            assert_eq!(
                state.state,
                if case == "accepted" {
                    "completed"
                } else {
                    "failed"
                },
                "{case}"
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM candidates", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                i64::from(case == "accepted")
            );
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                if case == "accepted" { 16 } else { 15 }
            );
            if case == "accepted" {
                crate::cuda_acceptance::validate(&db, 16).unwrap();
                assert_eq!(
                    crate::cuda_acceptance::portable_members(&db, &f.project)
                        .unwrap()
                        .1
                        .len(),
                    1
                );
                let lineage: String = db
                    .query_row("SELECT lineage FROM candidates", [], |r| r.get(0))
                    .unwrap();
                let lineage: Vec<ArtifactIdentity> = decode(&lineage).unwrap();
                assert!(
                    lineage.contains(&evidence.receipt.identity)
                        && lineage.contains(&evidence.trace.identity)
                );
            }
        }
    }
    #[test]
    fn accepted_cuda_capture_preserves_dedicated_roles_and_general_receipt() {
        authored_cuda_portable_fixture(None, false);
    }
    #[test]
    #[ignore = "requires an explicitly preserved preceding schema-17 application reader"]
    fn native_portable_cuda_export_refuses_preceding_reader() {
        let old = std::env::var_os("PULSAR_PORTABLE_CUDA_PRECEDING_READER")
            .expect("preceding reader required");
        authored_cuda_portable_fixture(Some(std::path::Path::new(&old)), false);
    }
    fn historical_reader_subject(f: &Fixture) -> tempfile::TempDir {
        fn copy_directory(source: &std::path::Path, destination: &std::path::Path, root: bool) {
            artifacts::private_directory(destination).unwrap();
            for entry in fs::read_dir(source).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name();
                if root
                    && (name.to_string_lossy().starts_with("projects.sqlite3")
                        || name == "worker-fence.json"
                        || name == "worker-fence.lock")
                {
                    continue;
                }
                let destination = destination.join(&name);
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    copy_directory(&entry.path(), &destination, false);
                } else {
                    assert!(
                        kind.is_file(),
                        "private fixture contains a nonregular entry"
                    );
                    fs::copy(entry.path(), destination).unwrap();
                }
            }
        }
        let snapshot = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(snapshot.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        copy_directory(&f.engine.config.state_dir, snapshot.path(), true);
        // Snapshot SQLite's committed view, including WAL, rather than copying
        // live database files. The old reader gets its own unbound host lease.
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "VACUUM INTO ?1",
                [snapshot.path().join("projects.sqlite3").to_str().unwrap()],
            )
            .unwrap();
        snapshot
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn accepted_cuda_capture_from_format3_host_preserves_portable_roles() {
        authored_cuda_portable_fixture(None, true);
    }
    fn authored_cuda_portable_fixture(old_reader: Option<&std::path::Path>, staging_host: bool) {
        // Authored consistency evidence, never a GPU admission or provenance claim.
        let f = fixture();
        let (staged, _) = staged_manifest(&f);
        let mut manifest: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        manifest.job_id = staged.job_id;
        manifest.attempt_id = staged.attempt_id;
        manifest.project_id = f.project.clone();
        manifest.source = staged.source;
        manifest.dependencies.extend(staged.dependencies);
        if let (
            WorkerOperation::Generate {
                model, settings, ..
            },
            WorkerOperation::Generate {
                model: Some(staged_model),
                ..
            },
        ) = (&mut manifest.operation, staged.operation)
        {
            let original = settings.inference_profile.as_ref().unwrap().model.clone();
            manifest
                .dependencies
                .retain(|identity| identity != &original);
            settings.inference_profile.as_mut().unwrap().model = staged_model.identity.clone();
            *model = Some(staged_model);
        } else {
            panic!("generation fixture");
        }
        let (manifest, evidence) =
            crate::cuda_acceptance::authored_evidence_for(manifest, &staged.output_dir);
        let path = staged.output_dir.join("program.json");
        fs::write(&path, serde_json::to_vec(&program(0.3)).unwrap()).unwrap();
        let program = crate::worker::pin_artifact(&path).unwrap();
        let path = staged.output_dir.join("receipt.json");
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version":2,"recipe":"authored-portable-cuda-consistency",
                "project_id":manifest.project_id,"base_revision":manifest.base_revision,
                "job_id":manifest.job_id,"attempt_id":manifest.attempt_id,
                "source_version":manifest.source.source_version,"program":program.identity,
                "dependencies":manifest.dependencies
            }))
            .unwrap(),
        )
        .unwrap();
        let receipt = crate::worker::pin_artifact(&path).unwrap();
        let general = receipt.identity.clone();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            let raw = encode(&manifest).unwrap();
            tx.execute(
                "UPDATE jobs SET manifest=?1 WHERE id=?2",
                params![raw, manifest.job_id.as_str()],
            )
            .unwrap();
            tx.execute(
                "UPDATE attempts SET manifest=?1 WHERE id=?2",
                params![raw, manifest.attempt_id.as_str()],
            )
            .unwrap();
            crate::cuda_attempts::declare(&tx, &manifest, &f.engine.engine_epoch).unwrap();
            tx.execute(
                "UPDATE jobs SET state='running' WHERE id=?1",
                [manifest.job_id.as_str()],
            )
            .unwrap();
            tx.execute(
                "UPDATE attempts SET state='running' WHERE id=?1",
                [manifest.attempt_id.as_str()],
            )
            .unwrap();
            tx.commit().unwrap();
        }
        f.engine
            .finish_job(
                &manifest,
                Ok(launch::LaunchExecution {
                    output: WorkerOutput::Generated {
                        program,
                        receipt,
                        lineage: manifest.dependencies.clone(),
                        review: vec![],
                    },
                    cuda_artifacts: Some(evidence.clone()),
                    worker_peak_memory_bytes: None,
                }),
            )
            .unwrap();
        if let Some(old) = old_reader {
            let subject = historical_reader_subject(&f);
            let result = std::process::Command::new("/usr/bin/timeout")
                .args(["--signal=TERM", "2"])
                .arg(old)
                .arg("engine")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PULSAR_STATE_DIR", subject.path())
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(124),
                "preceding reader must open pre-barrier subject: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                assert!(
                    fs::symlink_metadata(subject.path().join("engine.sock"))
                        .unwrap()
                        .file_type()
                        .is_socket(),
                    "preceding reader must reach its listener after opening schema 16"
                );
            }
        }
        let ResponseBody::PackageExport(initial) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StartProjectExport,
            ))
            .result
            .unwrap()
        else {
            panic!("export response")
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let exported = loop {
            let ResponseBody::PackageExport(status) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::ProjectPackageStatus {
                        operation_id: initial.operation_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("export status")
            };
            if status.state.is_terminal() {
                assert_eq!(
                    status.state,
                    PackageExportOperationState::Ready,
                    "{:?}",
                    status.error
                );
                break status;
            }
            assert!(std::time::Instant::now() < deadline, "CUDA export deadline");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(exported.artifact.as_ref().unwrap().format_version, 10);
        assert_eq!(
            f.engine
                .db
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            18
        );
        if let Some(old) = old_reader {
            let subject = historical_reader_subject(&f);
            let database = subject.path().join("projects.sqlite3");
            let wal = subject.path().join("projects.sqlite3-wal");
            let keeper = Connection::open(&database).unwrap();
            keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' native reader fixture';").unwrap();
            assert!(fs::metadata(&wal).unwrap().len() > 32);
            let identity = |path: &std::path::Path| {
                fs::read(path)
                    .ok()
                    .map(|bytes| (bytes.len(), format!("{:x}", Sha256::digest(&bytes))))
            };
            let before = [identity(&database), identity(&wal)];
            let result = std::process::Command::new("/usr/bin/timeout")
                .args(["--signal=TERM", "5"])
                .arg(old)
                .arg("engine")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PULSAR_STATE_DIR", subject.path())
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(1),
                "preceding reader refusal: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration")
            );
            assert_eq!(
                [identity(&database), identity(&wal)],
                before,
                "preceding reader must preserve committed DB/WAL"
            );
            drop(keeper);
        }
        #[cfg(target_os = "linux")]
        if staging_host {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            f.engine.arm_staging_fence(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                19
            );
            drop(db);
            let claim = f
                .engine
                .pool
                .reserve(1024)
                .unwrap()
                .with_storage(4096, 32 * 1024 * 1024)
                .unwrap();
            let staging = f.engine.worker_fence.create_staging(claim).unwrap();
            let path = staging.path();
            fs::write(path.join("authored"), b"no byte users escaped this fixture").unwrap();
            staging
                .close_after_use_retired()
                .wait(std::time::Instant::now() + std::time::Duration::from_secs(3))
                .unwrap();
            assert!(!path.exists());
            assert_eq!(f.engine.worker_fence.version().unwrap(), 3);
        }
        let token = TOKENS.with(|tokens| tokens.borrow().get(f.session.as_str()).unwrap().clone());
        let plan = project_packages::capture(
            &f.engine,
            &f.session,
            &token,
            &f.project,
            RevisionId::new(0),
        )
        .unwrap();
        assert_eq!(plan.manifest.format_version, 10);
        let origin = &plan.manifest.generated_origins[0];
        let cuda = origin.cuda_evidence.as_ref().unwrap();
        assert_eq!(origin.receipt.as_ref(), Some(&general));
        assert_eq!(cuda.receipt, evidence.receipt.identity);
        assert_eq!(cuda.trace, evidence.trace.identity);
        let candidate = plan
            .manifest
            .candidates
            .iter()
            .find(|c| c.candidate_id == cuda.accepted_candidate)
            .unwrap();
        assert_eq!(cuda.general_receipt(&candidate.lineage).unwrap(), &general);
        for (identity, role) in [
            (&cuda.receipt, PackageObjectRole::CudaExecutionReceipt),
            (&cuda.trace, PackageObjectRole::CudaNodeTrace),
        ] {
            let object = plan
                .manifest
                .objects
                .iter()
                .find(|o| o.sha256 == identity.sha256)
                .unwrap();
            assert!(object.roles.contains(&role));
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut plan.open_object(object).unwrap(), &mut bytes).unwrap();
            assert_eq!(bytes.len() as u64, identity.byte_len);
        }
        let mut bytes = Vec::new();
        crate::project_package_format::write_package(
            &plan.manifest,
            &mut bytes,
            |object| plan.open_object(object),
            Default::default(),
        )
        .unwrap();
        #[cfg(target_os = "linux")]
        if staging_host {
            let scope = f.engine.worker_fence.scope().unwrap();
            for host_only in [
                scope.as_bytes(),
                b"worker_fence_binding",
                b"minimum_ledger_version",
                b"pulsar-stage-",
            ] {
                assert!(
                    !bytes
                        .windows(host_only.len())
                        .any(|window| window == host_only),
                    "host staging ownership leaked into logical CUDA archive"
                );
            }
        }
        project_package_imports::exercise_authored_cuda_portable_lifecycle(
            &bytes,
            &cuda.trace,
            old_reader,
        );
        fs::write(&evidence.trace.path, b"[]").unwrap();
        assert!(project_packages::capture(
            &f.engine,
            &f.session,
            &token,
            &f.project,
            RevisionId::new(0)
        )
        .is_err());
    }
    #[test]
    fn cpu_completion_refuses_cuda_evidence_before_reading_program_or_changing_barrier() {
        let f = fixture();
        let (manifest, _) = staged_manifest(&f);
        let (_, evidence) = crate::cuda_acceptance::authored_evidence(&manifest.output_dir);
        let original: i64 = f
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE jobs SET state='running' WHERE id=?1",
                [manifest.job_id.as_str()],
            )
            .unwrap();
            db.execute(
                "UPDATE attempts SET state='running' WHERE id=?1",
                [manifest.attempt_id.as_str()],
            )
            .unwrap();
        }
        f.engine
            .finish_job(
                &manifest,
                Ok(launch::LaunchExecution {
                    output: WorkerOutput::Generated {
                        program: evidence.trace.clone(),
                        receipt: evidence.receipt.clone(),
                        lineage: vec![],
                        review: vec![],
                    },
                    cuda_artifacts: Some(evidence),
                    worker_peak_memory_bytes: None,
                }),
            )
            .unwrap();
        let db = f.engine.db.lock().unwrap();
        let (state, error): (String, String) = db
            .query_row(
                "SELECT state,error FROM jobs WHERE id=?1",
                [manifest.job_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "failed");
        assert!(error.contains("completion CUDA evidence differs from declared provider"));
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            original
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM candidates", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn independent_audio_dependencies_hold_snapshots_and_require_exact_declaration() {
        let f = fixture();
        let (mut manifest, audio_version) = staged_manifest(&f);
        let audio = f
            .engine
            .source(&f.engine.db.lock().unwrap(), &f.project, &audio_version)
            .unwrap();
        manifest.independent_audio = Some(audio.clone());
        if let WorkerOperation::Generate {
            model, settings, ..
        } = &mut manifest.operation
        {
            *model = None;
            settings.model_input = None;
            settings.mixed_audio = Some(MixedAudioSettings {
                intent: MultimodalIntent::Faithful,
                mapping: AudioMapping::new(
                    Axis::Stroke,
                    0.2,
                    NormalizedPosition::new(0.4).unwrap(),
                )
                .unwrap(),
                independent: Some(IndependentAudio {
                    source_version: audio_version.clone(),
                    alignment: AudioAlignment {
                        rate: None,
                        video_anchor: SourceTimestamp::new(0, 1).unwrap(),
                        audio_anchor: SourceTimestamp::new(1, 8).unwrap(),
                    },
                }),
            });
        }
        manifest.validate().unwrap();
        let mut mismatched = manifest.clone();
        mismatched.independent_audio = None;
        assert!(mismatched.validate().is_err());
        let mut mismatched = manifest.clone();
        mismatched
            .dependencies
            .retain(|identity| identity != &audio.identity);
        assert!(mismatched.validate().is_err());
        for state in ["queued", "running", "interrupted"] {
            f.engine
                .db
                .lock()
                .unwrap()
                .execute(
                    "UPDATE jobs SET state=?2,manifest=?3 WHERE id=?1",
                    params![manifest.job_id.as_str(), state, encode(&manifest).unwrap()],
                )
                .unwrap();
            let result = f.engine.handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::EvictSource {
                    source_version: audio_version.clone(),
                },
            ));
            assert_eq!(result.result.unwrap_err().code, ErrorCode::Forbidden);
            assert!(audio.path.is_file());
        }
    }
    #[test]
    fn cancellation_receipt_failure_does_not_signal_worker_and_exact_retry_commits() {
        for fail_at_commit in [false, true] {
            let f = fixture();
            let (manifest, _) = staged_manifest(&f);
            let flag = Arc::new(AtomicBool::new(false));
            f.engine
                .workers
                .lock()
                .unwrap()
                .insert(manifest.job_id.to_string(), flag.clone());
            let cancel = request(
                &f.session,
                Some(&f.project),
                None,
                Command::CancelJob {
                    job_id: manifest.job_id.clone(),
                },
            );
            let before = {
                let db = f.engine.db.lock().unwrap();
                let states: (String, String) = db.query_row(
                "SELECT jobs.state,attempts.state FROM jobs JOIN attempts ON attempts.id=jobs.attempt WHERE jobs.id=?1",
                [manifest.job_id.as_str()], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
                if fail_at_commit {
                    // Deferred foreign-key refusal occurs in COMMIT, after remember succeeds.
                    assert_eq!(
                        db.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                            .unwrap(),
                        1
                    );
                    db.execute_batch("CREATE TABLE cancel_commit_parent(id INTEGER PRIMARY KEY); CREATE TABLE cancel_commit_child(id INTEGER REFERENCES cancel_commit_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER reject_cancel_receipt AFTER INSERT ON requests BEGIN INSERT INTO cancel_commit_child VALUES(1); END;").unwrap();
                } else {
                    db.execute_batch("CREATE TEMP TRIGGER reject_cancel_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'injected cancellation receipt failure'); END;").unwrap();
                }
                states
            };
            let error = f.engine.handle(cancel.clone()).result.unwrap_err();
            assert_eq!(error.code, ErrorCode::Internal);
            assert!(error.message.contains(if fail_at_commit {
                "FOREIGN KEY constraint failed"
            } else {
                "injected cancellation receipt failure"
            }));
            assert!(
                !flag.load(Ordering::Acquire),
                "rolled-back cancellation signalled the worker"
            );
            {
                let db = f.engine.db.lock().unwrap();
                let after: (String, String) = db.query_row(
                "SELECT jobs.state,attempts.state FROM jobs JOIN attempts ON attempts.id=jobs.attempt WHERE jobs.id=?1",
                [manifest.job_id.as_str()], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
                assert_eq!(after, before);
                let receipts: i64 = db
                    .query_row(
                        "SELECT COUNT(*) FROM requests WHERE session=?1 AND request=?2",
                        params![f.session.as_str(), cancel.request_id.as_str()],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(receipts, 0);
                db.execute_batch("DROP TRIGGER reject_cancel_receipt;")
                    .unwrap();
            }
            let reply = f.engine.handle(cancel.clone()).result.unwrap();
            assert!(matches!(reply,ResponseBody::Job(ref job) if job.state=="cancelled"));
            assert!(flag.load(Ordering::Acquire));
            // Exact replay returns the durable result without signalling a replacement handle.
            let replacement = Arc::new(AtomicBool::new(false));
            f.engine
                .workers
                .lock()
                .unwrap()
                .insert(manifest.job_id.to_string(), replacement.clone());
            assert!(
                matches!(f.engine.handle(cancel).result.unwrap(),ResponseBody::Job(ref job) if job.state=="cancelled")
            );
            assert!(!replacement.load(Ordering::Acquire));
        }
    }

    #[test]
    fn revocation_rollback_preserves_all_worker_classes_and_exact_retry_signals_only_scope() {
        for fail_at_commit in [false, true] {
            let f = fixture();
            let target = pair(&f.engine, "revoked test session");
            f.engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::Grant {
                        session: target.clone(),
                        project_id: f.project.clone(),
                        scopes: vec![Scope::Read, Scope::Generate],
                    },
                ))
                .result
                .unwrap();
            let (manifest, _) = staged_manifest(&f);
            let keys = [
                manifest.job_id.to_string(),
                format!("preview:{}:{}:fixture", f.project, target),
                format!("wasm-edit:{}:{}:fixture", f.project, target),
                format!("probe:{}:{}:source:fixture", f.project, target),
                format!("preview:{}:{}:unrelated", f.project, f.session),
                format!("probe:{}:{}:source:unrelated", f.project, f.session),
            ];
            let flags: Vec<_> = keys
                .iter()
                .map(|key| {
                    let flag = Arc::new(AtomicBool::new(false));
                    f.engine
                        .workers
                        .lock()
                        .unwrap()
                        .insert(key.clone(), flag.clone());
                    flag
                })
                .collect();
            let revoke = request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: target.clone(),
                    project_id: f.project.clone(),
                },
            );
            let before = {
                let db = f.engine.db.lock().unwrap();
                db.execute(
                    "UPDATE jobs SET session=?2 WHERE id=?1",
                    params![manifest.job_id.as_str(), target.as_str()],
                )
                .unwrap();
                // Inert stored invocation exercises rollback of the revocation UPDATE;
                // no guest, process termination or WASM qualification is claimed.
                db.execute("INSERT INTO wasm_edits(id,project,session,base_revision,engine_epoch,request_id,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,state) VALUES('revoke-fixture',?1,?2,0,'fixture','fixture','fixture','{}',X'',X'','{}','{}',1,'fixture','running')",params![f.project.as_str(),target.as_str()]).unwrap();
                let counts:(i64,i64)=db.query_row("SELECT (SELECT COUNT(*) FROM grants WHERE session=?1 AND project=?2),(SELECT COUNT(*) FROM events WHERE project=?2)",params![target.as_str(),f.project.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
                if fail_at_commit {
                    db.execute_batch("CREATE TABLE revoke_commit_parent(id INTEGER PRIMARY KEY); CREATE TABLE revoke_commit_child(id INTEGER REFERENCES revoke_commit_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER reject_revoke_receipt AFTER INSERT ON requests BEGIN INSERT INTO revoke_commit_child VALUES(1); END;").unwrap();
                } else {
                    db.execute_batch("CREATE TEMP TRIGGER reject_revoke_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'injected revocation receipt failure'); END;").unwrap();
                }
                counts
            };
            let error = f.engine.handle(revoke.clone()).result.unwrap_err();
            assert_eq!(error.code, ErrorCode::Internal);
            assert!(error.message.contains(if fail_at_commit {
                "FOREIGN KEY constraint failed"
            } else {
                "injected revocation receipt failure"
            }));
            assert!(
                flags.iter().all(|flag| !flag.load(Ordering::Acquire)),
                "rolled-back revocation signalled a worker"
            );
            {
                let db = f.engine.db.lock().unwrap();
                let after:(i64,i64)=db.query_row("SELECT (SELECT COUNT(*) FROM grants WHERE session=?1 AND project=?2),(SELECT COUNT(*) FROM events WHERE project=?2)",params![target.as_str(),f.project.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
                assert_eq!(before, after);
                assert_eq!(
                    job_snapshot(&db, &manifest.job_id, &f.project)
                        .unwrap()
                        .state,
                    "queued"
                );
                assert_eq!(
                    db.query_row(
                        "SELECT state FROM attempts WHERE id=?1",
                        [manifest.attempt_id.as_str()],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    "queued"
                );
                assert_eq!(
                    db.query_row(
                        "SELECT state FROM wasm_edits WHERE id='revoke-fixture'",
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    "running"
                );
                assert_eq!(
                    db.query_row(
                        "SELECT COUNT(*) FROM requests WHERE session=?1 AND request=?2",
                        params![f.session.as_str(), revoke.request_id.as_str()],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    0
                );
                db.execute_batch("DROP TRIGGER reject_revoke_receipt;")
                    .unwrap();
            }
            assert!(matches!(
                f.engine.handle(revoke.clone()).result.unwrap(),
                ResponseBody::Ack
            ));
            assert!(flags[..4].iter().all(|flag| flag.load(Ordering::Acquire)));
            assert!(flags[4..].iter().all(|flag| !flag.load(Ordering::Acquire)));
            {
                let db = f.engine.db.lock().unwrap();
                assert_eq!(
                    job_snapshot(&db, &manifest.job_id, &f.project)
                        .unwrap()
                        .state,
                    "cancelled"
                );
                assert_eq!(
                    db.query_row(
                        "SELECT state FROM wasm_edits WHERE id='revoke-fixture'",
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    "cancelled"
                );
                assert_eq!(
                    db.query_row(
                        "SELECT COUNT(*) FROM grants WHERE session=?1 AND project=?2",
                        params![target.as_str(), f.project.as_str()],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    0
                );
            }
            let replacements: Vec<_> = keys[..3]
                .iter()
                .map(|key| {
                    let flag = Arc::new(AtomicBool::new(false));
                    f.engine
                        .workers
                        .lock()
                        .unwrap()
                        .insert(key.clone(), flag.clone());
                    flag
                })
                .collect();
            assert!(matches!(
                f.engine.handle(revoke).result.unwrap(),
                ResponseBody::Ack
            ));
            assert!(replacements
                .iter()
                .all(|flag| !flag.load(Ordering::Acquire)));
        }
    }

    #[test]
    fn wasm_cancellation_rollback_preserves_invocation_and_worker_until_exact_retry() {
        for original_state in ["running", "interrupted"] {
            for fail_at_commit in [false, true] {
                let f = fixture();
                let attempt = id!(AttemptId);
                let start_id = id!(RequestId);
                // Typed, validated retained invocation; no guest/process is launched.
                let module=wat::parse_str("(module (memory (export \"memory\") 1) (func (export \"pulsar_alloc\") (param i32) (result i32) i32.const 0) (func (export \"pulsar_process\") (param i32 i32) (result i64) i64.const 0))").unwrap();
                let spec = WasmEditSpec {
                    manifest: ExtensionManifest {
                        schema_version: PROTOCOL_VERSION,
                        name: "Cancellation fixture".into(),
                        role: ExtensionRole::Editor,
                        artifact: ArtifactIdentity {
                            artifact_id: ArtifactId::new("cancel-module").unwrap(),
                            sha256: format!("{:x}", Sha256::digest(&module)),
                            byte_len: module.len() as u64,
                        },
                        abi: WASM_PROCESSING_ABI.into(),
                        requested_scopes: vec![],
                        budget: ResourceBudget {
                            memory_bytes: 256 * 1024 * 1024,
                            output_bytes: WASM_BUFFER_MAX_BYTES as u64,
                            wall_time_ms: 5000,
                            cpu_threads: 2,
                        },
                        confinement: ConfinementRequirement::Wasm,
                    },
                    module,
                    guest_memory_bytes: 1024 * 1024,
                    expected_worker: f.engine.binary_identity.clone(),
                    approve_execution: true,
                    label: "Cancellation fixture".into(),
                };
                spec.validate().unwrap();
                let input =
                    serde_json::to_vec(&pulsar_core::EditValuesProgram::new(vec![]).unwrap())
                        .unwrap();
                let original_error = if original_state == "interrupted" {
                    Some(
                        encode(&ProtocolError::new(
                            ErrorCode::Unavailable,
                            "authored interrupted fixture",
                        ))
                        .unwrap(),
                    )
                } else {
                    None
                };
                let worker_key = format!("wasm-edit:{}:{}:{}", f.project, f.session, attempt);
                let flag = Arc::new(AtomicBool::new(false));
                f.engine
                    .workers
                    .lock()
                    .unwrap()
                    .insert(worker_key.clone(), flag.clone());
                let cancel = request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::CancelWasmEdit {
                        attempt_id: attempt.clone(),
                    },
                );
                {
                    let db = f.engine.db.lock().unwrap();
                    let base =
                        motion_state::revision_descriptor(&db, &f.project, RevisionId::new(0))
                            .unwrap();
                    db.execute("INSERT INTO wasm_edits(id,project,session,base_revision,engine_epoch,request_id,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,state,error) VALUES(?1,?2,?3,0,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",params![attempt.as_str(),f.project.as_str(),f.session.as_str(),f.engine.engine_epoch,start_id.as_str(),"a".repeat(64),encode(&spec.manifest).unwrap(),spec.module,input,encode(&base).unwrap(),encode(&spec.expected_worker).unwrap(),spec.guest_memory_bytes as i64,spec.label,original_state,original_error]).unwrap();
                    wasm_edits::validate(&db).unwrap();
                    if fail_at_commit {
                        db.execute_batch("CREATE TABLE wasm_cancel_parent(id INTEGER PRIMARY KEY); CREATE TABLE wasm_cancel_child(id INTEGER REFERENCES wasm_cancel_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER reject_wasm_cancel AFTER INSERT ON requests BEGIN INSERT INTO wasm_cancel_child VALUES(1); END;").unwrap();
                    } else {
                        db.execute_batch("CREATE TEMP TRIGGER reject_wasm_cancel BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'injected WASM cancellation receipt failure'); END;").unwrap();
                    }
                }
                let error = f.engine.handle(cancel.clone()).result.unwrap_err();
                assert_eq!(error.code, ErrorCode::Internal);
                assert!(error.message.contains(if fail_at_commit {
                    "FOREIGN KEY constraint failed"
                } else {
                    "injected WASM cancellation receipt failure"
                }));
                assert!(
                    !flag.load(Ordering::Acquire),
                    "rolled-back WASM cancellation signalled the worker"
                );
                {
                    let db = f.engine.db.lock().unwrap();
                    let after: (String, Option<String>) = db
                        .query_row(
                            "SELECT state,error FROM wasm_edits WHERE id=?1",
                            [attempt.as_str()],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .unwrap();
                    assert_eq!(after, (original_state.into(), original_error.clone()));
                    assert_eq!(
                        db.query_row(
                            "SELECT COUNT(*) FROM requests WHERE session=?1 AND request=?2",
                            params![f.session.as_str(), cancel.request_id.as_str()],
                            |r| r.get::<_, i64>(0)
                        )
                        .unwrap(),
                        0
                    );
                    wasm_edits::validate(&db).unwrap();
                    db.execute_batch("DROP TRIGGER reject_wasm_cancel;")
                        .unwrap();
                }
                assert!(
                    matches!(f.engine.handle(cancel.clone()).result.unwrap(),ResponseBody::WasmEdit(ref report) if report.state==WasmEditState::Cancelled)
                );
                assert!(flag.load(Ordering::Acquire));
                wasm_edits::validate(&f.engine.db.lock().unwrap()).unwrap();
                let replacement = Arc::new(AtomicBool::new(false));
                f.engine
                    .workers
                    .lock()
                    .unwrap()
                    .insert(worker_key, replacement.clone());
                assert!(
                    matches!(f.engine.handle(cancel).result.unwrap(),ResponseBody::WasmEdit(ref report) if report.state==WasmEditState::Cancelled)
                );
                assert!(!replacement.load(Ordering::Acquire));
                let terminal_cancel = request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::CancelWasmEdit {
                        attempt_id: attempt,
                    },
                );
                assert!(
                    matches!(f.engine.handle(terminal_cancel).result.unwrap(),ResponseBody::WasmEdit(ref report) if report.state==WasmEditState::Cancelled)
                );
                assert!(!replacement.load(Ordering::Acquire));
                assert_eq!(
                    f.engine
                        .db
                        .lock()
                        .unwrap()
                        .query_row("SELECT COUNT(*) FROM candidates", [], |r| r
                            .get::<_, i64>(0))
                        .unwrap(),
                    0
                );
            }
        }
    }

    #[test]
    fn cancelled_and_revoked_work_cannot_publish_late_candidates() {
        let f = fixture();
        let (manifest, _) = staged_manifest(&f);
        let flag = Arc::new(AtomicBool::new(false));
        f.engine
            .workers
            .lock()
            .unwrap()
            .insert(manifest.job_id.to_string(), flag.clone());
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::CancelJob {
                    job_id: manifest.job_id.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(flag.load(Ordering::Acquire));
        f.engine
            .finish_job(
                &manifest,
                Err(ProtocolError::invalid("late malformed result")),
            )
            .unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            assert_eq!(
                job_snapshot(&db, &manifest.job_id, &f.project)
                    .unwrap()
                    .state,
                "cancelled"
            );
            let count: i64 = db
                .query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0);
        }
        // A terminal attempt's observation is immutable, including a different
        // late response. Exercise revocation with a distinct original job.
        assert_eq!(
            f.engine
                .finish_job(
                    &manifest,
                    Err(ProtocolError::invalid("conflicting late result"))
                )
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        let f = fixture();
        let (manifest, _) = staged_manifest(&f);
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE jobs SET state='running' WHERE id=?1",
                [manifest.job_id.as_str()],
            )
            .unwrap();
        }
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        f.engine
            .finish_job(
                &manifest,
                Err(ProtocolError::invalid("another late result")),
            )
            .unwrap();
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            job_snapshot(&db, &manifest.job_id, &f.project)
                .unwrap()
                .state,
            "cancelled"
        );
    }
    #[test]
    fn explicit_motion_retime_is_revision_bound_replayable_protected_and_restorable() {
        let f = fixture();
        let file = f.dir.path().join("retime-source");
        fs::write(&file, b"inert media fixture").unwrap();
        let source = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path: file },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Source { source_version } => source_version,
            _ => panic!("source"),
        };
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                commit_program(&f, 0, program(0.3)),
            ))
            .result
            .unwrap();
        let spec = SourcePlacementSpec {
            source_version: source,
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(10, 1).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            rate_numerator: 1,
            rate_denominator: 1,
        };
        let old = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::SetSourcePlacement {
                    spec: spec.clone(),
                    replace: None,
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p.placements[0].clone(),
            _ => panic!("placement"),
        };
        let mut changed = spec;
        changed.rate_numerator = 2;
        let current = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(2),
                Command::SetSourcePlacement {
                    spec: changed,
                    replace: Some(old.placement_id.clone()),
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p,
            _ => panic!("placement"),
        };
        let command = Command::RetimeMotion {
            from_placement: old.placement_id,
            to_placement: current.placements[0].placement_id.clone(),
            axes: vec![Axis::Stroke],
        };
        let other = pair(&f.engine, "read-only retime fixture");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: other.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(&other, Some(&f.project), Some(3), command.clone()))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(2),
                    command.clone()
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        {
            let db = f.engine.db.lock().unwrap();
            db.execute_batch("CREATE TRIGGER receipt_failure BEFORE INSERT ON motion_retime_receipts BEGIN SELECT RAISE(ABORT,'synthetic receipt failure');END;").unwrap();
        }
        let failed = request(&f.session, Some(&f.project), Some(3), command.clone());
        assert!(f.engine.handle(failed.clone()).result.is_err());
        {
            let db = f.engine.db.lock().unwrap();
            assert_eq!(
                project_state(&db, &f.project).unwrap().revision,
                RevisionId::new(3)
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM motion_retime_receipts", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            db.execute_batch("DROP TRIGGER receipt_failure;").unwrap();
        }
        let mutation = request(&f.session, Some(&f.project), Some(3), command.clone());
        let output = f.engine.handle(mutation.clone()).result.unwrap();
        assert_eq!(
            encode(&output).unwrap(),
            encode(&f.engine.handle(mutation).result.unwrap()).unwrap()
        );
        let ResponseBody::Project(out) = output else {
            panic!("project")
        };
        assert_eq!(
            project_state(&f.engine.db.lock().unwrap(), &f.project)
                .unwrap()
                .program
                .tracks()[0]
                .actions()[1]
                .time()
                .as_nanos(),
            500_000_000
        );
        let status = request(
            &other,
            Some(&f.project),
            None,
            Command::MotionRetimeStatus {
                revision: out.revision,
            },
        );
        let ResponseBody::MotionRetimeReceipt(receipt) =
            f.engine.handle(status.clone()).result.unwrap()
        else {
            panic!("receipt")
        };
        assert_eq!(receipt.after.sha256, out.motion.program.sha256);
        assert_eq!(receipt.before.sha256, current.motion.program.sha256);
        assert_eq!(receipt.base_revision, RevisionId::new(3));
        let token = TOKENS.with(|tokens| tokens.borrow().get(f.session.as_str()).cloned().unwrap());
        let captured =
            project_packages::capture(&f.engine, &f.session, &token, &f.project, out.revision)
                .unwrap();
        assert_eq!(
            captured.manifest.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_RETIMING_RECEIPTS
        );
        assert_eq!(captured.manifest.counts.retiming_receipts, 1);
        assert_eq!(
            encode(&captured.manifest.retiming_receipts[0]).unwrap(),
            encode(&receipt).unwrap()
        );
        let mut forged = captured.manifest.clone();
        forged.retiming_receipts[0].actor = "forged".into();
        assert!(forged.validate().is_err());
        let mut forged = captured.manifest.clone();
        forged.retiming_receipts.push(*receipt.clone());
        forged.counts = forged.record_counts().unwrap();
        assert!(forged.validate().is_err());
        let verify = |manifest: &PortableProjectManifest| {
            project_package_imports::validate_evidence(
                manifest,
                &f.engine.pool,
                |reference| {
                    let object = captured
                        .manifest
                        .objects
                        .iter()
                        .find(|o| o.sha256 == reference.sha256 && o.byte_len == reference.byte_len)
                        .unwrap();
                    let mut bytes = Vec::new();
                    captured
                        .open_object(object)?
                        .read_to_end(&mut bytes)
                        .map_err(unavailable)?;
                    Ok(bytes)
                },
                &mut || Ok(()),
            )
        };
        verify(&captured.manifest).unwrap();
        let mut forged = captured.manifest.clone();
        forged.retiming_receipts[0].axes = vec![Axis::Sway];
        forged.validate().unwrap();
        assert_eq!(
            verify(&forged).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        drop(captured);
        {
            let db = f.engine.db.lock().unwrap();
            let original: String = db
                .query_row(
                    "SELECT receipt FROM motion_retime_receipts WHERE project=?1 AND revision=4",
                    [f.project.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            db.execute(
                "UPDATE motion_retime_receipts SET receipt=?1 WHERE project=?2 AND revision=4",
                params!["x".repeat(16385), f.project.as_str()],
            )
            .unwrap();
            drop(db);
            assert!(f.engine.handle(status.clone()).result.is_err());
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE motion_retime_receipts SET receipt=?1 WHERE project=?2 AND revision=4",
                params![original, f.project.as_str()],
            )
            .unwrap();
        }
        let db = f.engine.db.lock().unwrap();
        let review = lineage::state(&db, &f.project, out.revision).unwrap();
        assert!(
            review.unknown
                && review
                    .flags
                    .iter()
                    .any(|f| f.reason.as_str()
                        == "existing_motion_explicitly_retimed_review_required")
        );
        drop(db);
        let restored = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(4),
                Command::Undo,
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p,
            _ => panic!("undo"),
        };
        assert_eq!(restored.motion.program, current.motion.program);
        let redone = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(5),
                Command::Redo,
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p,
            _ => panic!("redo"),
        };
        assert_eq!(redone.motion.program, out.motion.program);
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(6),
                Command::SetProtectedRegions {
                    regions: vec![ProtectedRegion {
                        axis: Some(Axis::Stroke),
                        range: TimeRange::new(
                            ProjectTime::ZERO,
                            ProjectTime::from_nanos(1_000_000_000),
                        )
                        .unwrap(),
                    }],
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(&f.session, Some(&f.project), Some(7), command))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        let config = f.engine.config.clone();
        drop(f.engine);
        let reopened = Engine::open(config).unwrap();
        let ResponseBody::Project(snapshot) = reopened
            .handle(request(
                &f.session,
                None,
                None,
                Command::OpenProject {
                    project_id: f.project,
                },
            ))
            .result
            .unwrap()
        else {
            panic!("restart")
        };
        assert_eq!(snapshot.motion.program, redone.motion.program);
        assert!(matches!(
            reopened
                .handle(request(
                    &f.session,
                    Some(&snapshot.project_id),
                    None,
                    Command::MotionRetimeStatus {
                        revision: RevisionId::new(4)
                    }
                ))
                .result
                .unwrap(),
            ResponseBody::MotionRetimeReceipt(_)
        ));
    }
    #[test]
    fn source_placements_preserve_motion_replay_history_and_edit_authority() {
        let f = fixture();
        let input = f.dir.path().join("source.bin");
        fs::write(&input, b"declared source fixture").unwrap();
        let source = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path: input },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Source { source_version } => source_version,
            _ => panic!("source"),
        };
        let base = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                commit_program(&f, 0, program(0.3)),
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p,
            _ => panic!("project"),
        };
        let spec = SourcePlacementSpec {
            source_version: source.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(10, 1).unwrap(),
            source_anchor: SourceTimestamp::new(1, 2).unwrap(),
            project_anchor: ProjectTime::from_nanos(2_000_000_000),
            rate_numerator: 2,
            rate_denominator: 3,
        };
        let create = request(
            &f.session,
            Some(&f.project),
            Some(base.revision.value()),
            Command::SetSourcePlacement {
                spec: spec.clone(),
                replace: None,
            },
        );
        let placed = match f.engine.handle(create.clone()).result.unwrap() {
            ResponseBody::Project(p) => p,
            _ => panic!("project"),
        };
        assert_eq!(placed.motion.program, base.motion.program);
        assert_eq!(placed.placements.len(), 1);
        let old = placed.placements[0].clone();
        let replay = match f.engine.handle(create.clone()).result.unwrap() {
            ResponseBody::Project(p) => p,
            _ => panic!("project"),
        };
        assert_eq!(replay.revision, placed.revision);
        assert_eq!(replay.placements, placed.placements);
        let mut changed = create;
        let mut different = spec.clone();
        different.rate_numerator = 5;
        changed.command = Command::SetSourcePlacement {
            spec: different,
            replace: None,
        };
        assert_eq!(
            f.engine.handle(changed).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let mut replacement = spec.clone();
        replacement.rate_numerator = -1;
        let replaced = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(placed.revision.value()),
                Command::SetSourcePlacement {
                    spec: replacement,
                    replace: Some(old.placement_id.clone()),
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p,
            _ => panic!("project"),
        };
        assert_ne!(replaced.placements[0].placement_id, old.placement_id);
        assert_eq!(replaced.motion.program, base.motion.program);
        let restored = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(replaced.revision.value()),
                Command::Undo,
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p,
            _ => panic!("project"),
        };
        assert_eq!(restored.placements, placed.placements);
        let redone = match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(restored.revision.value()),
                Command::Redo,
            ))
            .result
            .unwrap()
        {
            ResponseBody::Project(p) => p,
            _ => panic!("project"),
        };
        assert_eq!(redone.placements, replaced.placements);
        assert_eq!(redone.motion.program, base.motion.program);
        let other = pair(&f.engine, "read only");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: other.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &other,
                    Some(&f.project),
                    Some(redone.revision.value()),
                    Command::SetSourcePlacement {
                        spec,
                        replace: None
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::EvictSource {
                        source_version: source
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM source_placement_definitions",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        let reviews = lineage::state(&db, &f.project, redone.revision).unwrap();
        assert!(reviews
            .flags
            .iter()
            .any(|f| f.reason.as_str() == "source_alignment_changed_motion_not_retimed"));
        drop(db);
        let token = TOKENS.with(|tokens| tokens.borrow().get(f.session.as_str()).cloned().unwrap());
        let capture =
            project_packages::capture(&f.engine, &f.session, &token, &f.project, redone.revision);
        let capture = capture.unwrap();
        assert_eq!(
            capture.manifest.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS
        );
        assert_eq!(
            capture
                .manifest
                .placements
                .as_ref()
                .unwrap()
                .definitions
                .len(),
            2
        );
        capture.manifest.validate().unwrap();
    }

    #[test]
    fn retained_cuda_removal_requires_read_admin_revision_and_identity() {
        let f = fixture();
        let reader = pair(&f.engine, "retention reader");
        let operation = Command::RemoveRetainedLiveCudaEvidence {
            epoch: AttemptId::new("absent-capture").unwrap(),
            expected_manifest_sha256: "0".repeat(64),
        };
        assert!(operation.is_mutating() && operation.requires_revision());
        for scopes in [vec![Scope::Read], vec![Scope::ManageGrants]] {
            f.engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::Revoke {
                        session: reader.clone(),
                        project_id: f.project.clone(),
                    },
                ))
                .result
                .unwrap();
            f.engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::Grant {
                        session: reader.clone(),
                        project_id: f.project.clone(),
                        scopes,
                    },
                ))
                .result
                .unwrap();
            assert_eq!(
                f.engine
                    .handle(request(
                        &reader,
                        Some(&f.project),
                        Some(0),
                        operation.clone()
                    ))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::ManageGrants],
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &reader,
                    Some(&f.project),
                    Some(1),
                    operation.clone()
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            f.engine
                .handle(request(
                    &reader,
                    Some(&f.project),
                    Some(0),
                    operation.clone()
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(&reader, Some(&f.project), Some(0), operation))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    #[test]
    fn retained_cuda_removal_is_durable_replay_and_rechecks_current_authority() {
        let f = fixture();
        let record = {
            let db = f.engine.db.lock().unwrap();
            crate::live_cuda_capture_db::tests::install_removal_fixture(&db, &f.project, &f.session)
        };
        let project = &record.identity.project;
        let revision = record.identity.revision.value();
        let reader = pair(&f.engine, "retention administrator");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: project.clone(),
                    scopes: vec![Scope::Read, Scope::ManageGrants],
                },
            ))
            .result
            .unwrap();
        let operation = Command::RemoveRetainedLiveCudaEvidence {
            epoch: record.identity.epoch.clone(),
            expected_manifest_sha256: record.evidence.manifest.sha256.clone(),
        };
        let mut wrong = request(&reader, Some(project), Some(revision), operation.clone());
        wrong.command = Command::RemoveRetainedLiveCudaEvidence {
            epoch: record.identity.epoch.clone(),
            expected_manifest_sha256: "0".repeat(64),
        };
        assert_eq!(
            f.engine.handle(wrong).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        let original = request(&reader, Some(project), Some(revision), operation.clone());
        let first = f.engine.handle(original.clone()).result.unwrap();
        let ResponseBody::LiveCudaEvidenceRemoval(receipt) = &first else {
            panic!("removal receipt")
        };
        receipt.validate().unwrap();
        assert_eq!(receipt.manifest, record.evidence.manifest);
        assert!(!receipt.physical_erasure);
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(f.engine.handle(original.clone()).result.unwrap()).unwrap()
        );
        let mut conflict = original.clone();
        conflict.command = Command::RemoveRetainedLiveCudaEvidence {
            epoch: record.identity.epoch.clone(),
            expected_manifest_sha256: "0".repeat(64),
        };
        assert_eq!(
            f.engine.handle(conflict).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert_eq!(
            f.engine
                .handle(request(&reader, Some(project), Some(revision), operation))
                .result
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        {
            let db = f.engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM live_cuda_capture_evidence", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                db.query_row(
                    "SELECT revision FROM projects WHERE id=?1",
                    [project.as_str()],
                    |r| r.get::<_, u64>(0)
                )
                .unwrap(),
                revision
            );
            let saved: String = db
                .query_row(
                    "SELECT response FROM requests WHERE session=?1 AND request=?2",
                    params![reader.as_str(), original.request_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(saved, serde_json::to_string(&first).unwrap());
        }
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: reader.clone(),
                    project_id: project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine.handle(original).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    #[test]
    fn retained_cuda_query_requires_current_read_authority_and_revision() {
        for operation in [
            Command::ListLiveCudaEvidence {
                after: None,
                limit: 16,
            },
            Command::InspectLiveCudaEvidence {
                epoch: AttemptId::new("absent-capture").unwrap(),
            },
            Command::BeginLiveCudaEvidenceDownload {
                epoch: AttemptId::new("absent-capture").unwrap(),
                part: LiveCudaEvidencePart::Trace,
            },
        ] {
            let f = fixture();
            let reader = pair(&f.engine, "retained CUDA reader");
            assert_eq!(
                operation.is_mutating(),
                matches!(operation, Command::BeginLiveCudaEvidenceDownload { .. })
            );
            assert!(operation.requires_revision());
            assert_eq!(
                f.engine
                    .handle(request(
                        &reader,
                        Some(&f.project),
                        Some(0),
                        operation.clone()
                    ))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
            f.engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::Grant {
                        session: reader.clone(),
                        project_id: f.project.clone(),
                        scopes: vec![Scope::Read],
                    },
                ))
                .result
                .unwrap();
            assert_eq!(
                f.engine
                    .handle(request(
                        &reader,
                        Some(&f.project),
                        Some(1),
                        operation.clone()
                    ))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            let result = f
                .engine
                .handle(request(
                    &reader,
                    Some(&f.project),
                    Some(0),
                    operation.clone(),
                ))
                .result;
            if matches!(operation, Command::ListLiveCudaEvidence { .. }) {
                let ResponseBody::LiveCudaEvidencePage(page) = result.unwrap() else {
                    panic!("listing response")
                };
                assert!(page.entries.is_empty());
                page.validate().unwrap();
            } else {
                assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
            }
            f.engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::Revoke {
                        session: reader.clone(),
                        project_id: f.project.clone(),
                    },
                ))
                .result
                .unwrap();
            assert_eq!(
                f.engine
                    .handle(request(&reader, Some(&f.project), Some(0), operation))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
    }
}
