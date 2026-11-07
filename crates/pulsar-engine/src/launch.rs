//! The parent owns confinement, resource limits, deadlines and response fencing.
use crate::{artifacts, EngineConfig};
use pulsar_protocol::*;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

pub fn available() -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/usr/bin/bwrap").is_file() && crate::cgroup::delegated_root().is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}
fn failure(code: ErrorCode, error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(code, error.to_string())
}

pub struct LaunchExecution {
    pub output: WorkerOutput,
    pub cuda_artifacts: Option<WorkerCudaArtifacts>,
    /// Linux cgroup peak for the entire worker process tree, not inference only.
    pub worker_peak_memory_bytes: Option<u64>,
}

/// A failed response from the original worker after confirmed whole-tree
/// retirement and all final response/accounting/cancellation/deadline checks.
/// Private fields prevent host errors from manufacturing this observation.
#[derive(Debug)]
pub(crate) struct SettledWorkerFailure {
    error: ProtocolError,
    provider_unavailable: Option<InferenceProviderUnavailable>,
}
impl SettledWorkerFailure {
    pub(crate) fn error(&self) -> &ProtocolError {
        &self.error
    }
    pub(crate) fn provider_unavailable(&self) -> Option<&InferenceProviderUnavailable> {
        self.provider_unavailable.as_ref()
    }
}

/// The ordinary result stays unchanged. A separate observation distinguishes
/// a settled failed worker from host/transport failure or unconfirmed custody.
pub(crate) struct AttemptReport {
    result: Result<LaunchExecution, ProtocolError>,
    settled_failure: Option<SettledWorkerFailure>,
}
impl AttemptReport {
    pub(crate) fn host_failure(error: ProtocolError) -> Self {
        Self {
            result: Err(error),
            settled_failure: None,
        }
    }
    pub(crate) fn into_parts(
        self,
    ) -> (
        Result<LaunchExecution, ProtocolError>,
        Option<SettledWorkerFailure>,
    ) {
        (self.result, self.settled_failure)
    }
    pub(crate) fn into_result(self) -> Result<LaunchExecution, ProtocolError> {
        self.result
    }
}

pub fn execute_with_metrics(
    config: &EngineConfig,
    manifest: &WorkerRequest,
    cancel: &AtomicBool,
) -> Result<LaunchExecution, ProtocolError> {
    execute_inner(
        config,
        manifest,
        cancel,
        None,
        #[cfg(target_os = "linux")]
        None,
        &mut None,
    )
}

pub(crate) fn execute_reserved(
    config: &EngineConfig,
    manifest: &WorkerRequest,
    cancel: &AtomicBool,
    reservation: Arc<crate::resources::Reservation>,
    #[cfg(target_os = "linux")] fence: Arc<crate::worker_fence::WorkerFence>,
) -> Result<LaunchExecution, ProtocolError> {
    execute_reserved_attempt(
        config,
        manifest,
        cancel,
        reservation,
        #[cfg(target_os = "linux")]
        fence,
    )
    .into_result()
}

pub(crate) fn execute_reserved_attempt(
    config: &EngineConfig,
    manifest: &WorkerRequest,
    cancel: &AtomicBool,
    reservation: Arc<crate::resources::Reservation>,
    #[cfg(target_os = "linux")] fence: Arc<crate::worker_fence::WorkerFence>,
) -> AttemptReport {
    let mut settled_failure = None;
    let result = execute_inner(
        config,
        manifest,
        cancel,
        Some(reservation),
        #[cfg(target_os = "linux")]
        Some(fence),
        &mut settled_failure,
    );
    AttemptReport {
        result,
        settled_failure,
    }
}

fn execute_inner(
    config: &EngineConfig,
    manifest: &WorkerRequest,
    cancel: &AtomicBool,
    reservation: Option<Arc<crate::resources::Reservation>>,
    #[cfg(target_os = "linux")] fence: Option<Arc<crate::worker_fence::WorkerFence>>,
    settled_failure: &mut Option<SettledWorkerFailure>,
) -> Result<LaunchExecution, ProtocolError> {
    if cancel.load(Ordering::Acquire) {
        return Err(failure(
            ErrorCode::Cancelled,
            "worker attempt cancelled before launch",
        ));
    }
    manifest.validate()?;
    if manifest.dependencies.iter().any(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
    }) {
        return Err(ProtocolError::unsupported(
            "captured CPU runtime requires its verified closed worker",
        ));
    }
    let profile = crate::inference_profiles::resolve_profile(manifest)
        .map_err(|error| failure(ErrorCode::InvalidRequest, error))?;
    if config.cpu_owner_policy.is_some() && profile.is_some() {
        return Err(ProtocolError::unsupported(
            "configured CPU runtime requires captured execution",
        ));
    }
    let address_space =
        crate::inference_profiles::address_space_limit(profile.as_ref(), &manifest.budget)
            .map_err(|error| failure(ErrorCode::InvalidRequest, error))?;
    if !available() {
        return Err(ProtocolError::unsupported(
            "native worker confinement on this platform",
        ));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (config, cancel, reservation, settled_failure);
        return Err(ProtocolError::unsupported("worker confinement"));
    }
    #[cfg(target_os = "linux")]
    {
        let reservation = match reservation {
            Some(reservation) => reservation,
            None => Arc::new(
                crate::resources::ResourcePool::new(config.max_ram_bytes, config.max_jobs)
                    .with_cpu_limit(config.max_cpu_threads)?
                    .reserve_worker(manifest.budget.memory_bytes, manifest.budget.cpu_threads)?,
            ),
        };
        let source = manifest
            .source
            .path
            .canonicalize()
            .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?;
        if source != manifest.source.path {
            return Err(ProtocolError::invalid(
                "source manifest path must be canonical",
            ));
        }
        let model = match &manifest.operation {
            WorkerOperation::Generate { model, .. } | WorkerOperation::Preview { model, .. } => {
                model.as_ref()
            }
            WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
        };
        let mut progress_envelope = if crate::generation_progress::eligible(manifest) {
            Some(
                crate::generation_progress::envelope(manifest)
                    .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?,
            )
        } else {
            None
        };
        let mut bindings = vec![(manifest.source.path.clone(), manifest.source.path.clone())];
        if let Some(binding) = progress_envelope.as_mut().and_then(|e| e.resume.as_mut()) {
            let target = std::path::PathBuf::from("/pulsar-generation-resume.json");
            bindings.push((binding.body.path.clone(), target.clone()));
            binding.body.path = target;
        }
        if let Some(audio) = &manifest.independent_audio {
            let path = audio
                .path
                .canonicalize()
                .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?;
            if path != audio.path {
                return Err(ProtocolError::invalid(
                    "independent audio manifest path must be canonical",
                ));
            }
            bindings.push((path.clone(), path));
        }
        if let Some(model) = model {
            bindings.push((model.path.clone(), model.path.clone()));
        }
        for tool in [&manifest.tools.ffmpeg, &manifest.tools.ffprobe]
            .into_iter()
            .chain(manifest.tools.onnx_runtime.iter())
        {
            if !tool.path.starts_with("/usr") && !tool.path.starts_with("/lib") {
                bindings.push((tool.path.clone(), tool.path.clone()));
            }
        }
        let mut custody = match fence {
            Some(fence) => fence
                .admit_group(
                    reservation,
                    manifest.budget.memory_bytes,
                    manifest.budget.cpu_threads,
                )
                .map_err(|e| failure(ErrorCode::Unavailable, e))?
                .into_worker(),
            None => {
                let group = crate::cgroup::AttemptCgroup::create(
                    manifest.attempt_id.as_str(),
                    manifest.budget.memory_bytes,
                    manifest.budget.cpu_threads,
                )
                .map_err(|e| failure(ErrorCode::Unavailable, e))?;
                crate::worker_retirement::WorkerCustody::prepare(group, reservation, None)
                    .map_err(|e| failure(ErrorCode::Unavailable, e))?
            }
        };
        let group = custody.group_handle();
        let command = crate::confinement::command(
            config,
            &manifest.budget,
            address_space,
            &group,
            &bindings,
            &manifest.output_dir,
            if progress_envelope.is_some() {
                "progress-worker"
            } else {
                "worker"
            },
        )
        .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?;
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(manifest.budget.wall_time_ms))
            .ok_or_else(|| ProtocolError::invalid("worker wall deadline overflow"))?;
        if let Some(envelope) = progress_envelope {
            supervise_inner(
                manifest,
                &envelope,
                cancel,
                custody,
                command,
                deadline,
                true,
                settled_failure,
            )
        } else {
            supervise_inner(
                manifest,
                manifest,
                cancel,
                custody,
                command,
                deadline,
                false,
                settled_failure,
            )
        }
    }
}

/// Shared bounded request/egress/retirement path for CPU and owned CUDA workers.
/// The caller installs all retained assets before entering this function.
#[cfg(target_os = "linux")]
pub(crate) fn supervise_owned_attempt<T: serde::Serialize>(
    manifest: &WorkerRequest,
    envelope: &T,
    cancel: &AtomicBool,
    custody: crate::worker_retirement::WorkerCustody,
    command: std::process::Command,
    deadline: Instant,
) -> AttemptReport {
    supervise_owned_attempt_with_progress(
        manifest, envelope, cancel, custody, command, deadline, false,
    )
}

/// The same response and retirement checks with the original request's progress
/// framing. The caller derives this flag from the admitted request, never from
/// fields supplied by a later worker response or a malformed bootstrap witness.
#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn supervise_owned_attempt_with_progress<T: serde::Serialize>(
    manifest: &WorkerRequest,
    envelope: &T,
    cancel: &AtomicBool,
    custody: crate::worker_retirement::WorkerCustody,
    command: std::process::Command,
    deadline: Instant,
    progressive: bool,
) -> AttemptReport {
    let mut settled_failure = None;
    let result = supervise_inner(
        manifest,
        envelope,
        cancel,
        custody,
        command,
        deadline,
        progressive,
        &mut settled_failure,
    );
    AttemptReport {
        result,
        settled_failure,
    }
}
#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn supervise_inner<T: serde::Serialize>(
    manifest: &WorkerRequest,
    envelope: &T,
    cancel: &AtomicBool,
    mut custody: crate::worker_retirement::WorkerCustody,
    mut command: std::process::Command,
    deadline: Instant,
    progressive: bool,
    settled_failure: &mut Option<SettledWorkerFailure>,
) -> Result<LaunchExecution, ProtocolError> {
    if cancel.load(Ordering::Acquire) {
        return Err(failure(
            ErrorCode::Cancelled,
            "worker attempt cancelled before spawn",
        ));
    }
    if Instant::now() >= deadline {
        return Err(failure(
            ErrorCode::Unavailable,
            "worker wall deadline exceeded before spawn",
        ));
    }
    custody
        .spawn(&mut command)
        .map_err(|e| failure(ErrorCode::Unavailable, e))?;
    let transport = (|| -> Result<_, ProtocolError> {
        let (stdin, mut stdout, stderr) = {
            let mut child = custody.child();
            (
                child
                    .stdin
                    .take()
                    .ok_or_else(|| failure(ErrorCode::Internal, "worker stdin unavailable"))?,
                child
                    .stdout
                    .take()
                    .ok_or_else(|| failure(ErrorCode::Internal, "worker stdout unavailable"))?,
                child
                    .stderr
                    .take()
                    .ok_or_else(|| failure(ErrorCode::Internal, "worker stderr unavailable"))?,
            )
        };
        crate::launch_pipes::nonblocking(&stdin).map_err(|e| failure(ErrorCode::Unavailable, e))?;
        crate::launch_pipes::nonblocking(&stdout)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        crate::launch_pipes::nonblocking(&stderr)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        let mut monitor = crate::launch_pipes::Monitor::new(deadline, cancel, stderr);
        let response = exchange_messages_inner(
            stdin,
            &mut stdout,
            &mut monitor,
            envelope,
            manifest,
            progressive,
        )?;
        let status = loop {
            monitor
                .tick()
                .map_err(|e| failure(ErrorCode::Unavailable, e))?;
            match custody
                .child()
                .try_wait()
                .map_err(|e| failure(ErrorCode::Unavailable, e))?
            {
                Some(status) => break status,
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        monitor
            .check()
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        Ok((status, response, monitor.diagnostics().to_vec()))
    })();
    let (status, response, diagnostics) = match transport {
        Ok(value) => value,
        Err(error) => {
            // A bounded observation phase; unconfirmed custody transfers
            // to the reaper without releasing the admission reservation.
            let retirement_deadline = Instant::now() + Duration::from_secs(2);
            let _ = custody.retire(retirement_deadline);
            if cancel.load(Ordering::Acquire) {
                return Err(failure(ErrorCode::Cancelled, "worker attempt cancelled"));
            }
            if Instant::now() >= deadline {
                return Err(failure(
                    ErrorCode::Unavailable,
                    "worker wall deadline exceeded",
                ));
            }
            return Err(error);
        }
    };
    // Success is fenced on whole-tree retirement, not direct-child exit.
    custody
        .retire(deadline)
        .map_err(|e| failure(ErrorCode::Unavailable, e))?;
    if !status.success() {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            format!(
                "confined worker exited {status}: {}",
                String::from_utf8_lossy(&diagnostics)
            ),
        ));
    }
    let worker_peak_memory_bytes = custody.tree().group.peak_memory_bytes().ok();
    complete_retired_response(
        manifest,
        response,
        cancel,
        deadline,
        progressive,
        worker_peak_memory_bytes,
        settled_failure,
    )
}

/// Only the successfully retired supervision path calls this final boundary.
/// Unit tests exercise its decisions, not the native retirement prerequisite.
#[cfg(target_os = "linux")]
fn complete_retired_response(
    manifest: &WorkerRequest,
    response: WorkerResult,
    cancel: &AtomicBool,
    deadline: Instant,
    progressive: bool,
    worker_peak_memory_bytes: Option<u64>,
    settled_failure: &mut Option<SettledWorkerFailure>,
) -> Result<LaunchExecution, ProtocolError> {
    if response.version != PROTOCOL_VERSION
        || response.job_id != manifest.job_id
        || response.attempt_id != manifest.attempt_id
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "late or mismatched worker response identity",
        ));
    }
    // The guest has no writable host path. A size-limited tmpfs caps guest
    // output; only bounded, validated egress is published by the engine.
    let mut total = 0u64;
    let mut checkpoint_total = 0u64;
    for entry in
        std::fs::read_dir(&manifest.output_dir).map_err(|e| failure(ErrorCode::Unavailable, e))?
    {
        let entry = entry.map_err(|e| failure(ErrorCode::Unavailable, e))?;
        let meta = entry
            .file_type()
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        if !meta.is_file() {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "worker published non-regular output",
            ));
        }
        let checkpoint =
            progressive && crate::generation_progress::is_checkpoint_member(&entry.file_name());
        let (counter, limit) = if checkpoint {
            (
                &mut checkpoint_total,
                manifest
                    .budget
                    .output_bytes
                    .saturating_mul(3)
                    .saturating_add(16 * 1024),
            )
        } else {
            (&mut total, manifest.budget.output_bytes)
        };
        *counter = counter
            .checked_add(
                entry
                    .metadata()
                    .map_err(|e| failure(ErrorCode::Unavailable, e))?
                    .len(),
            )
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "output byte accounting overflow",
                )
            })?;
        if *counter > limit {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "aggregate worker output exceeded reservation",
            ));
        }
    }
    artifacts::sync_dir(&manifest.output_dir).map_err(|e| failure(ErrorCode::Unavailable, e))?;
    response.validate_provider_unavailable_for(manifest)?;
    if response.result.is_err() && response.cuda_artifacts.is_some() {
        return Err(ProtocolError::invalid(
            "unexpected CUDA evidence on CPU or failed output",
        ));
    }
    if cancel.load(Ordering::Acquire) {
        return Err(failure(ErrorCode::Cancelled, "worker attempt cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(failure(
            ErrorCode::Unavailable,
            "worker wall deadline exceeded",
        ));
    }
    match response.result {
        Ok(output) => Ok(LaunchExecution {
            output,
            cuda_artifacts: response.cuda_artifacts,
            worker_peak_memory_bytes,
        }),
        Err(error) => {
            *settled_failure = Some(SettledWorkerFailure {
                error: error.clone(),
                provider_unavailable: response.provider_unavailable,
            });
            Err(error)
        }
    }
}

#[cfg(target_os = "linux")]
#[cfg(test)]
fn exchange_worker_messages<
    I: std::io::Write,
    O: std::io::Read,
    R: std::io::Read,
    T: serde::Serialize,
>(
    stdin: I,
    stdout: &mut O,
    monitor: &mut crate::launch_pipes::Monitor<'_, R>,
    envelope: &T,
    manifest: &WorkerRequest,
) -> Result<WorkerResult, ProtocolError> {
    exchange_messages_inner(stdin, stdout, monitor, envelope, manifest, false)
}
#[cfg(target_os = "linux")]
fn exchange_messages_inner<I: Write, O: Read, R: Read, T: serde::Serialize>(
    mut stdin: I,
    stdout: &mut O,
    monitor: &mut crate::launch_pipes::Monitor<'_, R>,
    envelope: &T,
    manifest: &WorkerRequest,
    progressive: bool,
) -> Result<WorkerResult, ProtocolError> {
    let result = (|| {
        write_message(&mut monitor.write(&mut stdin), envelope)
            .map_err(|e| failure(ErrorCode::InvalidRequest, e))?;
        drop(stdin);
        if progressive {
            crate::generation_progress::receive(&mut monitor.read(stdout), manifest)
        } else {
            receive_artifacts(&mut monitor.read(stdout), manifest)
        }
    })();
    result.map_err(|mut error: ProtocolError| {
        // EOF can precede our last stderr read. Two nonblocking reads fill at
        // most the monitor's existing 16 KiB cap, without waiting for exit or
        // extending the attempt deadline. Retirement remains the caller's job.
        for _ in 0..2 {
            if monitor.tick().is_err() {
                break;
            }
        }
        if !monitor.diagnostics().is_empty() {
            error.message.push_str(": worker diagnostics: ");
            error
                .message
                .push_str(&String::from_utf8_lossy(monitor.diagnostics()));
        }
        error
    })
}

pub(crate) fn receive_artifacts(
    reader: &mut impl Read,
    manifest: &WorkerRequest,
) -> Result<WorkerResult, ProtocolError> {
    use sha2::{Digest, Sha256};
    let mut response: WorkerResult =
        read_message(reader).map_err(|e| failure(ErrorCode::InvalidRequest, e))?;
    if response.version != PROTOCOL_VERSION
        || response.job_id != manifest.job_id
        || response.attempt_id != manifest.attempt_id
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "worker response identity mismatch before artifact publication",
        ));
    }
    if matches!(manifest.operation, WorkerOperation::ProbeMedia)
        && response
            .result
            .as_ref()
            .is_ok_and(|v| !matches!(v, WorkerOutput::MediaProbe(_)))
    {
        return Err(ProtocolError::invalid(
            "metadata probe returned another output kind",
        ));
    }
    let succeeded = response.result.is_ok();
    let mut references: Vec<(&mut ArtifactReference, &str)> = match &mut response.result {
        Ok(WorkerOutput::MediaProbe(report)) => {
            report.validate()?;
            if !matches!(manifest.operation, WorkerOperation::ProbeMedia)
                || report.project_id != manifest.project_id
                || report.revision != manifest.base_revision
                || report.source_version != manifest.source.source_version
                || report.source != manifest.source.identity
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "media probe response scope differs",
                ));
            }
            vec![]
        }
        Ok(WorkerOutput::Generated {
            program, receipt, ..
        }) => vec![(program, "program.json"), (receipt, "receipt.json")],
        Ok(WorkerOutput::Preview(preview)) => {
            if preview.annotated.is_some()
                != matches!(
                    &manifest.operation,
                    WorkerOperation::Preview {
                        annotated: Some(_),
                        ..
                    }
                )
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "unexpected or absent annotated preview egress",
                ));
            }
            let mut frames = preview
                .frame
                .as_mut()
                .map(|f| vec![(&mut f.artifact, "frame.raw")])
                .unwrap_or_default();
            if let Some(previous) = preview.annotated.as_mut().and_then(|e| e.previous.as_mut()) {
                frames.push((&mut previous.frame.artifact, "previous-frame.raw"));
            }
            frames
        }
        Err(_) => vec![],
    };
    match (
        &mut response.cuda_artifacts,
        manifest.cuda_execution.as_ref(),
        succeeded,
    ) {
        (Some(evidence), Some(_), true) => {
            evidence.validate()?;
            references.extend([
                (&mut evidence.receipt, "cuda-inference.json"),
                (&mut evidence.trace, "cuda-trace.json"),
            ]);
        }
        (None, Some(_), true) => {
            return Err(ProtocolError::invalid(
                "CUDA success requires original execution evidence",
            ))
        }
        (None, _, _) => {}
        _ => {
            return Err(ProtocolError::invalid(
                "unexpected CUDA evidence on CPU or failed output",
            ))
        }
    }
    let total = references
        .iter()
        .try_fold(0u64, |sum, (reference, _)| {
            sum.checked_add(reference.identity.byte_len)
        })
        .ok_or_else(|| {
            ProtocolError::new(ErrorCode::ResourceExhausted, "artifact total overflow")
        })?;
    if total > manifest.budget.output_bytes {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "worker declared aggregate output exceeds admitted budget",
        ));
    }
    for (reference, name) in &mut references {
        reference.identity.validate()?;
        let mut temporary = tempfile::NamedTempFile::new_in(&manifest.output_dir)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        let mut remaining = reference.identity.byte_len;
        let mut digest = Sha256::new();
        while remaining > 0 {
            let bytes =
                read_artifact_chunk(reader).map_err(|e| failure(ErrorCode::InvalidRequest, e))?;
            if bytes.len() as u64 > remaining {
                return Err(ProtocolError::invalid(
                    "worker chunk exceeds declared artifact length",
                ));
            }
            temporary
                .write_all(&bytes)
                .map_err(|e| failure(ErrorCode::Unavailable, e))?;
            digest.update(&bytes);
            remaining -= bytes.len() as u64;
        }
        if format!("{:x}", digest.finalize()) != reference.identity.sha256 {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "worker artifact egress digest mismatch",
            ));
        }
        temporary
            .as_file()
            .sync_all()
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        let destination = manifest.output_dir.join(*name);
        let published = temporary
            .persist_noclobber(&destination)
            .map_err(|e| failure(ErrorCode::Unavailable, e.error))?;
        let mut permissions = published
            .metadata()
            .map_err(|e| failure(ErrorCode::Unavailable, e))?
            .permissions();
        permissions.set_readonly(true);
        published
            .set_permissions(permissions)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        reference.path = destination;
    }
    // Fixed egress sequence ends at EOF; extra undeclared bytes are malformed.
    let mut extra = [0u8; 1];
    if reader
        .read(&mut extra)
        .map_err(|e| failure(ErrorCode::InvalidRequest, e))?
        != 0
    {
        return Err(ProtocolError::invalid("unexpected trailing worker egress"));
    }
    artifacts::sync_dir(&manifest.output_dir).map_err(|e| failure(ErrorCode::Unavailable, e))?;
    if let Some(evidence) = &response.cuda_artifacts {
        match &response.result {
            Ok(WorkerOutput::Preview(preview)) => {
                crate::cuda_result::verify_preview_files(manifest, evidence, preview)
            }
            _ => crate::cuda_result::verify_files(manifest, evidence),
        }
        .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?;
    }
    Ok(response)
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    fn cpu_request() -> WorkerRequest {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.cuda_execution = None;
        let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
            unreachable!()
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        request
    }

    #[test]
    fn public_ordinary_launch_refuses_reserved_cpu_image_before_dependencies_or_resources() {
        let root = tempfile::tempdir().unwrap();
        let mut config = EngineConfig::new(
            root.path().join("absent-state"),
            root.path().join("absent-worker"),
        );
        config.max_ram_bytes = 0;
        config.max_cpu_threads = 0;
        config.max_jobs = 0;
        let mut request = cpu_request();
        request.output_dir = root.path().join("absent-output");
        request.source.path = root.path().join("absent-source");
        request.dependencies.push(ArtifactIdentity {
            artifact_id: ArtifactId::new(format!(
                "{CPU_RUNTIME_IMAGE_ID_PREFIX}{}",
                "a".repeat(64)
            ))
            .unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        });
        let error = execute_with_metrics(&config, &request, &AtomicBool::new(false))
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error
            .message
            .contains("captured CPU runtime requires its verified closed worker"));
        assert!(!request.output_dir.exists());
        assert!(!config.state_dir.exists());
    }

    #[test]
    fn public_ordinary_launch_refuses_configured_cpu_capture_for_explicit_and_implicit_models() {
        let root = tempfile::tempdir().unwrap();
        let mut config = EngineConfig::new(
            root.path().join("absent-state"),
            root.path().join("absent-worker"),
        );
        config.cpu_owner_policy = Some(root.path().join("absent-policy"));
        config.max_ram_bytes = 0;
        config.max_cpu_threads = 0;
        config.max_jobs = 0;
        for implicit in [false, true] {
            let mut request = cpu_request();
            request.output_dir = root.path().join("absent-output");
            request.source.path = root.path().join("absent-source");
            if implicit {
                let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
                    unreachable!()
                };
                settings.inference_profile = None;
            }
            let error = execute_with_metrics(&config, &request, &AtomicBool::new(false))
                .err()
                .unwrap();
            assert_eq!(error.code, ErrorCode::Unsupported);
            assert!(error
                .message
                .contains("configured CPU runtime requires captured execution"));
            assert!(!request.output_dir.exists());
            assert!(!config.state_dir.exists());
        }
    }

    #[test]
    fn already_cancelled_launch_does_not_access_dependencies_or_create_output() {
        let root = tempfile::tempdir().unwrap();
        let config = EngineConfig::new(
            root.path().join("state"),
            root.path().join("missing-worker"),
        );
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.output_dir = root.path().join("output");
        let error = execute_with_metrics(&config, &request, &AtomicBool::new(true))
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(!request.output_dir.exists());
        assert!(!config.state_dir.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn truncated_worker_reply_retains_bounded_bootstrap_diagnostics() {
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let cancel = AtomicBool::new(false);
        let stderr = std::io::Cursor::new(b"authored bootstrap invariant failed\n");
        let mut monitor = crate::launch_pipes::Monitor::new(
            Instant::now() + Duration::from_secs(2),
            &cancel,
            stderr,
        );
        let mut truncated = std::io::Cursor::new(vec![0u8, 0u8]);
        let error = exchange_worker_messages(
            Vec::new(),
            &mut truncated,
            &mut monitor,
            &"private-envelope",
            &request,
        )
        .err()
        .unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("failed to fill whole buffer"));
        assert!(error
            .message
            .contains("authored bootstrap invariant failed"));
        assert!(!error.message.contains("private-envelope"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn transport_failure_caps_stderr_and_preserves_empty_diagnostic_errors() {
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let cancel = AtomicBool::new(false);
        for size in [0, 65536] {
            let stderr = std::io::Cursor::new(vec![b'x'; size]);
            let mut monitor = crate::launch_pipes::Monitor::new(
                Instant::now() + Duration::from_secs(2),
                &cancel,
                stderr,
            );
            let mut truncated = std::io::Cursor::new(Vec::<u8>::new());
            let error = exchange_worker_messages(
                Vec::new(),
                &mut truncated,
                &mut monitor,
                &"private-envelope",
                &request,
            )
            .err()
            .unwrap();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert!(error
                .message
                .starts_with("transport I/O: failed to fill whole buffer"));
            if size == 0 {
                assert!(!error.message.contains("worker diagnostics:"));
            } else {
                let diagnostic = error
                    .message
                    .split_once(": worker diagnostics: ")
                    .unwrap()
                    .1;
                assert_eq!(diagnostic.len(), 16384);
                assert!(diagnostic.bytes().all(|b| b == b'x'));
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn broken_request_pipe_retains_worker_diagnostics() {
        struct BrokenPipe;
        impl std::io::Write for BrokenPipe {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "authored request pipe closed",
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let cancel = AtomicBool::new(false);
        let mut monitor = crate::launch_pipes::Monitor::new(
            Instant::now() + Duration::from_secs(2),
            &cancel,
            std::io::Cursor::new(b"authored early worker exit"),
        );
        let mut stdout = std::io::Cursor::new(Vec::<u8>::new());
        let error = exchange_worker_messages(
            BrokenPipe,
            &mut stdout,
            &mut monitor,
            &"private-envelope",
            &request,
        )
        .err()
        .unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("authored request pipe closed"));
        assert!(error.message.contains("authored early worker exit"));
        assert!(!error.message.contains("private-envelope"));
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "launch_attempt_tests.rs"]
mod attempt_tests;
