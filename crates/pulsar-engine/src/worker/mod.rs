//! Native-worker entry point and media-to-candidate adapter.
//!
//! The engine owns admission, confinement, cancellation, project transactions,
//! and artifact publication. This module executes one immutable attempt, writes
//! only new files in its engine-approved directory, and never commits a project.
mod media_probe;

mod artifacts;
mod audio;
#[cfg(target_os = "linux")]
pub mod cuda_identity;
#[cfg(target_os = "linux")]
pub(crate) mod cuda_memory;
#[cfg(all(target_os = "linux", feature = "cuda"))]
pub mod cuda_session;
pub mod detector;
#[cfg(target_os = "linux")]
mod live_cuda_stream;
mod model_session;
#[cfg(target_os = "linux")]
pub fn run_live_cuda_worker() -> Result<()> {
    live_cuda_stream::run()
}
#[cfg(target_os = "linux")]
pub fn run_live_cuda_worker_exec() -> Result<()> {
    live_cuda_stream::run_exec()
}
mod capture_stream;
mod live_stream;
mod progress;
mod provider_failure;
mod raw_frame_pair;
mod telemetry;
pub(crate) fn run_progress_worker() -> Result<()> {
    progress::run()
}
pub mod temporal_position;
mod timestamped_decode;
pub fn run_capture_worker() -> Result<()> {
    capture_stream::run()
}
pub fn run_live_worker() -> Result<()> {
    live_stream::run()
}
#[cfg(target_os = "linux")]
pub(crate) fn run_closed_cpu_live_worker() -> Result<()> {
    live_stream::run_captured()
}
#[cfg(test)]
mod annotated_generation_tests;
mod annotated_input;
#[cfg(test)]
mod annotated_portable_tests;
mod annotated_preview;
pub(crate) mod annotated_receipt;
#[cfg(test)]
mod good_tests;
mod inputs;
#[cfg(test)]
mod integration_tests;
mod media;
mod mixed_audio;
mod tracking;
mod tracking_plan;
mod training_views;
mod video_views;
pub use training_views::run_training_view_plan;

pub use artifacts::{discover_tools, pin_artifact};
pub type WorkerError = anyhow::Error;

use anyhow::{bail, Context, Result};
use pulsar_protocol::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::{Duration, Instant};

const MAX_SAMPLED_FRAMES: usize = 1_000_000;

/// One framed request and one framed response. Native stdout is never inherited.
/// Parent-enforced limits include the time spent reading an incomplete request.
pub fn run_worker() -> Result<()> {
    let request: WorkerRequest = read_message(&mut std::io::stdin().lock())?;
    let result = execute_manifest(&request);
    emit_result(&request, &result)
}

/// Dedicated fresh child role. Public worker execution stays CPU-only.
#[cfg(target_os = "linux")]
pub fn run_cuda_worker() -> Result<()> {
    crate::cuda_mounts::clear_inherited_descriptors()?;
    #[cfg(not(feature = "cuda"))]
    bail!("CUDA worker is not compiled; no automatic substitution");
    #[cfg(feature = "cuda")]
    {
        let witness: crate::cuda_mounts::CudaMountWitness =
            read_message(&mut std::io::stdin().lock())?;
        crate::cuda_bootstrap::reexec(witness)
    }
}
#[cfg(target_os = "linux")]
pub fn run_cuda_worker_exec() -> Result<()> {
    #[cfg(not(feature = "cuda"))]
    {
        crate::cuda_mounts::clear_inherited_descriptors()?;
        bail!("CUDA worker is not compiled; no automatic substitution");
    }
    #[cfg(feature = "cuda")]
    {
        let relay = crate::cuda_bootstrap::read_relay()?;
        crate::cuda_mounts::clear_inherited_descriptors()?;
        let deadline = crate::cuda_bootstrap::deadline_instant(
            relay.deadline_ns,
            relay.witness.request.budget.wall_time_ms,
        )?;
        let bootstrap = crate::cuda_mounts::verify_worker_bootstrap_at(&relay.witness, deadline)?;
        let request = bootstrap.request();
        let _cudnn = crate::cuda_bootstrap::load_cudnn(&bootstrap)?;
        let result = execute_manifest_inner(request, ExecutionContext::Cuda(&bootstrap));
        emit_result(request, &result)
    }
}

/// A single execution inside the namespace that loaded this process. The parent
/// selected the captured dynamic loader before this role began; this entry never
/// starts a host-loaded intermediate worker or restarts the original deadline.
#[cfg(target_os = "linux")]
pub(crate) fn run_closed_cpu_worker() -> Result<()> {
    crate::cpu_mounts::clear_inherited_descriptors()?;
    let witness: crate::cpu_mounts::CpuMountWitness = read_message(&mut std::io::stdin().lock())?;
    validate_closed_cpu_request_identity(&witness.request, &witness.request_sha256)?;
    let progressive = crate::generation_progress::eligible(&witness.request);
    let bootstrap = match crate::cpu_mounts::verify_bootstrap(&witness) {
        Ok(bootstrap) => bootstrap,
        Err(error) => {
            let result = closed_cpu_failure(&witness.request, error);
            return emit_closed_cpu_result(&witness.request, &result, progressive);
        }
    };
    let request = bootstrap.request();
    if progressive {
        let Some(envelope) = witness.progress.as_ref() else {
            let result = closed_cpu_failure(
                request,
                anyhow::anyhow!("closed CPU progress envelope absent"),
            );
            return emit_closed_cpu_result(request, &result, true);
        };
        progress::run_envelope(envelope, |request| {
            execute_manifest_inner(request, ExecutionContext::ClosedCpu(&bootstrap))
        })
    } else {
        let result = execute_manifest_inner(request, ExecutionContext::ClosedCpu(&bootstrap));
        emit_result(request, &result)
    }
}

#[cfg(target_os = "linux")]
fn validate_closed_cpu_request_identity(request: &WorkerRequest, expected: &str) -> Result<()> {
    request.validate()?;
    validate_sha256(expected)?;
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(serde_json::to_vec(request)?)) == expected,
        "closed CPU request identity mismatch"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn closed_cpu_failure(request: &WorkerRequest, error: anyhow::Error) -> WorkerResult {
    let mut message = format!("closed CPU bootstrap refused: {error:#}");
    if message.len() > 4096 {
        let mut boundary = 4096;
        while !message.is_char_boundary(boundary) {
            boundary -= 1;
        }
        message.truncate(boundary);
    }
    WorkerResult {
        version: PROTOCOL_VERSION,
        attempt_id: request.attempt_id.clone(),
        job_id: request.job_id.clone(),
        cuda_artifacts: None,
        provider_unavailable: None,
        result: Err(ProtocolError::new(ErrorCode::DependencyMismatch, message)),
    }
}

#[cfg(target_os = "linux")]
fn emit_closed_cpu_result(
    request: &WorkerRequest,
    result: &WorkerResult,
    progressive: bool,
) -> Result<()> {
    let mut output = std::io::stdout().lock();
    if progressive {
        progress::emit_finished(request, result, &mut output)
    } else {
        emit_result_to(request, result, &mut output)
    }
}

fn emit_result(request: &WorkerRequest, result: &WorkerResult) -> Result<()> {
    emit_result_to(request, result, &mut std::io::stdout().lock())
}

fn emit_result_to<W: std::io::Write>(
    request: &WorkerRequest,
    result: &WorkerResult,
    stdout: &mut W,
) -> Result<()> {
    result.validate_provider_unavailable_for(request)?;
    write_message(stdout, result)?;
    // Files live only in the worker's private, quota-bounded tmpfs. The engine
    // validates this bounded egress and writes its own durable attempt copies.
    // No native worker receives a writable host project/artifact directory.
    if let Ok(output) = &result.result {
        let mut references: Vec<&ArtifactReference> = match output {
            WorkerOutput::MediaProbe(_) => vec![],
            WorkerOutput::Generated {
                program, receipt, ..
            } => vec![program, receipt],
            WorkerOutput::Preview(preview) => preview
                .frame
                .iter()
                .map(|frame| &frame.artifact)
                .chain(
                    preview
                        .annotated
                        .iter()
                        .filter_map(|e| e.previous.as_ref())
                        .map(|p| &p.frame.artifact),
                )
                .collect(),
        };
        if let Some(evidence) = &result.cuda_artifacts {
            evidence.validate()?;
            references.extend([&evidence.receipt, &evidence.trace]);
        }
        let mut total = 0u64;
        let mut buffer = vec![0u8; MAX_ARTIFACT_CHUNK_BYTES];
        for artifact in references {
            total = total
                .checked_add(artifact.identity.byte_len)
                .context("bulk artifact length overflow")?;
            if total > request.budget.output_bytes {
                bail!("bulk artifact egress exceeds attempt admission");
            }
            let mut file = std::fs::File::open(&artifact.path)?;
            if file.metadata()?.len() != artifact.identity.byte_len {
                bail!("artifact changed before egress");
            }
            let mut remaining = artifact.identity.byte_len;
            while remaining > 0 {
                let length = usize::try_from(remaining.min(MAX_ARTIFACT_CHUNK_BYTES as u64))?;
                file.read_exact(&mut buffer[..length])?;
                write_artifact_chunk(stdout, &buffer[..length])?;
                remaining -= length as u64;
            }
        }
    }
    Ok(())
}

/// Execute only after engine confinement has been established. This function
/// does not claim that its caller is sandboxed; public client requests cannot
/// call it or choose this manifest's resource paths.
pub fn execute_manifest(request: &WorkerRequest) -> WorkerResult {
    execute_manifest_inner(request, ExecutionContext::Native)
}

#[derive(Clone, Copy)]
enum ExecutionContext<'a> {
    Native,
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    Cuda(&'a crate::cuda_mounts::VerifiedCudaBootstrap<'a>),
    #[cfg(target_os = "linux")]
    ClosedCpu(&'a crate::cpu_mounts::VerifiedCpuBootstrap<'a>),
    #[cfg(not(target_os = "linux"))]
    #[allow(dead_code)]
    Unsupported(std::marker::PhantomData<&'a ()>),
}

fn execute_manifest_inner(request: &WorkerRequest, context: ExecutionContext<'_>) -> WorkerResult {
    telemetry::clear();
    let execution = execute(request, context);
    // Check the complete captured namespace even after an execution error. A
    // changed alias must not be reported as a meaningful provider failure.
    #[cfg(target_os = "linux")]
    let execution = if let ExecutionContext::ClosedCpu(bootstrap) = context {
        match bootstrap.verify_after_execution() {
            Ok(()) => execution,
            Err(error) => {
                telemetry::clear();
                return closed_cpu_failure(request, error);
            }
        }
    } else {
        execution
    };
    let (result, provider_unavailable) = match execution {
        Ok(output) => (Ok(output), None),
        Err(error) => {
            let (error, evidence) = provider_failure::worker_failure(request, error);
            (Err(error), evidence)
        }
    };
    WorkerResult {
        cuda_artifacts: if result.is_ok() {
            telemetry::take_cuda()
        } else {
            telemetry::clear();
            None
        },
        version: PROTOCOL_VERSION,
        attempt_id: request.attempt_id.clone(),
        job_id: request.job_id.clone(),
        provider_unavailable,
        result,
    }
}

fn execute(request: &WorkerRequest, context: ExecutionContext<'_>) -> Result<WorkerOutput> {
    request.validate()?;
    match context {
        #[cfg(all(target_os = "linux", feature = "cuda"))]
        ExecutionContext::Cuda(bootstrap) => {
            anyhow::ensure!(
                serde_json::to_vec(request)? == serde_json::to_vec(bootstrap.request())?,
                "CUDA bootstrap request changed"
            );
            crate::inference_profiles::admit_cuda_worker_profile(request)?;
        }
        #[cfg(target_os = "linux")]
        ExecutionContext::ClosedCpu(bootstrap) => {
            anyhow::ensure!(
                serde_json::to_vec(request)? == serde_json::to_vec(bootstrap.request())?,
                "closed CPU bootstrap request changed"
            );
            require_explicit_cpu_profile(request)?;
        }
        _ => {
            if request.dependencies.iter().any(|identity| {
                identity
                    .artifact_id
                    .as_str()
                    .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
            }) {
                return Err(ProtocolError::unsupported(
                    "captured CPU runtime requires its verified closed worker",
                )
                .into());
            }
            crate::inference_profiles::resolve_profile(request)?;
        }
    }
    if request.budget.wall_time_ms > 24 * 60 * 60 * 1000 {
        bail!("worker deadline must be at most one day");
    }
    let retained_deadline = match context {
        #[cfg(all(target_os = "linux", feature = "cuda"))]
        ExecutionContext::Cuda(bootstrap) => Some(bootstrap.deadline()),
        #[cfg(target_os = "linux")]
        ExecutionContext::ClosedCpu(bootstrap) => Some(bootstrap.deadline()),
        _ => None,
    };
    let deadline = execution_deadline(request.budget.wall_time_ms, retained_deadline)?;
    let directory = request
        .output_dir
        .canonicalize()
        .context("approved attempt directory is absent")?;
    if directory != request.output_dir || !directory.is_dir() {
        bail!("attempt directory must be a canonical existing directory");
    }
    validate_dependencies(request, deadline)?;
    let result = if matches!(request.operation, WorkerOperation::ProbeMedia) {
        WorkerOutput::MediaProbe(Box::new(media_probe::run(request, deadline)?))
    } else if let WorkerOperation::GenerateInput { input } = &request.operation {
        inputs::generate(request, input, deadline)?
    } else if let WorkerOperation::Generate {
        preset,
        settings,
        model,
    } = &request.operation
    {
        if settings.raw_frame_pair.is_some() {
            raw_frame_pair::generate(request, preset, settings, model.as_ref(), deadline)?
        } else {
            let index = media::probe(&request.tools.ffprobe.path, &request.source.path, deadline)?;

            generate(request, &index, preset, settings, model.as_ref(), deadline)?
        }
    } else {
        let index = media::probe(&request.tools.ffprobe.path, &request.source.path, deadline)?;
        if matches!(
            &request.operation,
            WorkerOperation::Preview {
                still_image: true,
                ..
            }
        ) && index.pts.len() != 1
        {
            return Err(ProtocolError::invalid(
                "assistant image preparation requires exactly one decoded still frame",
            )
            .into());
        }

        match &request.operation {
            WorkerOperation::Generate {
                preset,
                settings,
                model,
            } => generate(request, &index, preset, settings, model.as_ref(), deadline)?,
            WorkerOperation::Preview {
                placement_seek,
                time_basis,
                video_view,
                context,
                source_time,
                max_width,
                max_height,
                model,
                model_input,
                ..
            } => preview(
                request,
                &index,
                context,
                *source_time,
                *time_basis,
                *max_width,
                *max_height,
                model.as_ref(),
                model_input.as_ref(),
                video_view.as_ref(),
                placement_seek.as_deref(),
                deadline,
            )?,
            WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => {
                unreachable!("handled before media probing")
            }
        }
    };
    // Read-only mounts are not immutable copies of mutable backing files.
    // This detects changed inputs and invalidates the result; it does not undo
    // native effects. The launcher therefore also confines effects and snapshots
    // mutable custom model/runtime bytes before calling this worker.
    validate_dependencies(request, deadline)?;
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    if let ExecutionContext::Cuda(bootstrap) = context {
        bootstrap.verify_after()?;
    }
    Ok(result)
}

fn execution_deadline(budget_ms: u64, retained: Option<Instant>) -> Result<Instant> {
    let deadline = match retained {
        Some(deadline) => deadline,
        None => Instant::now()
            .checked_add(Duration::from_millis(budget_ms))
            .context("worker deadline overflow")?,
    };
    anyhow::ensure!(
        Instant::now() < deadline,
        "worker deadline exhausted before execution"
    );
    Ok(deadline)
}

#[cfg(target_os = "linux")]
fn require_explicit_cpu_profile(request: &WorkerRequest) -> Result<()> {
    let profile = match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
    }
    .context("closed CPU execution requires an explicit inference profile")?;
    anyhow::ensure!(
        profile.provider == InferenceProvider::Cpu && request.cuda_execution.is_none(),
        "closed CPU execution cannot substitute a provider"
    );
    let resolved = crate::inference_profiles::resolve_profile(request)?;
    anyhow::ensure!(
        serde_json::to_vec(&resolved)? == serde_json::to_vec(&Some(profile))?,
        "closed CPU profile changed"
    );
    Ok(())
}

fn validate_dependencies(request: &WorkerRequest, deadline: Instant) -> Result<()> {
    artifacts::verify_artifact(&request.source.path, &request.source.identity, deadline)?;
    if let Some(audio) = &request.independent_audio {
        artifacts::verify_artifact(&audio.path, &audio.identity, deadline)?;
    }
    for tool in [&request.tools.ffmpeg, &request.tools.ffprobe] {
        artifacts::verify_artifact(&tool.path, &tool.identity, deadline)?;
    }
    if let Some(runtime) = &request.tools.onnx_runtime {
        artifacts::verify_artifact(&runtime.path, &runtime.identity, deadline)?;
    }
    let model = match &request.operation {
        WorkerOperation::Generate { model, .. } | WorkerOperation::Preview { model, .. } => {
            model.as_ref()
        }
        WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
    };
    if let Some(model) = model {
        artifacts::verify_artifact(&model.path, &model.identity, deadline)?;
    }
    Ok(())
}

fn create_detector(
    request: &WorkerRequest,
    model: Option<&SourceArtifact>,
    contract: Option<&ModelInputContract>,
    deadline: Instant,
) -> Result<Option<detector::Detector>> {
    match (model, contract) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => {
            request.tools.onnx_runtime.as_ref().ok_or_else(||
                ProtocolError::new(ErrorCode::Unavailable, "pinned local ONNX runtime unavailable; no download or implicit backend fallback"))?;
            detector::Detector::from_request_at(request, deadline).map(Some)
        }
        _ => Err(ProtocolError::invalid(
            "model and declared decoder contract must be supplied together",
        )
        .into()),
    }
}

pub(crate) fn validate_annotated_preview_pair(preview: &PreviewResult) -> Result<()> {
    annotated_preview::validate_pair(preview)
}

#[allow(clippy::too_many_arguments)]
fn preview(
    request: &WorkerRequest,
    index: &media::MediaIndex,
    context: &FrameContext,
    requested_time: SourceTimestamp,
    time_basis: PreviewTimeBasis,
    max_width: u32,
    max_height: u32,
    model: Option<&SourceArtifact>,
    contract: Option<&ModelInputContract>,
    video_view: Option<&VideoView>,
    placement_seek: Option<&PlacedPreviewRequest>,
    deadline: Instant,
) -> Result<WorkerOutput> {
    if matches!(
        &request.operation,
        WorkerOperation::Preview {
            annotated: Some(_),
            ..
        }
    ) {
        return annotated_preview::run(request, index, deadline);
    }
    if context.source_version != request.source.source_version {
        return Err(ProtocolError::invalid(
            "preview context does not name the input source version",
        )
        .into());
    }
    if contract.is_some_and(|c| c.decoder == DetectorDecoder::TemporalScriptPosition) {
        return Err(ProtocolError::unsupported("temporal script-position is a generation recipe; it supplies no preview detector boxes").into());
    }
    if context.transform != video_view_transform(video_view)? {
        return Err(ProtocolError::unsupported(
            "preview projection without a declared transform adapter",
        )
        .into());
    }
    let selected = if let Some(placed) = placement_seek {
        placed.validate(context)?;
        let target = placed
            .placement
            .spec
            .source_at_project(placed.project_time)?;
        seek_source_frames_exact(
            &index.pts,
            index.time_base_num,
            index.time_base_den,
            target,
            PreviewTimeBasis::ElapsedFromFirstFrame,
        )
    } else {
        seek_source_frames(
            &index.pts,
            index.time_base_num,
            index.time_base_den,
            requested_time,
            time_basis,
        )
    }
    .map_err(|error| ProtocolError::invalid(error.to_string()))?;
    let placed = placement_seek
        .map(|request| -> Result<PlacedPreviewReceipt> {
            let actual_project_time = request
                .placement
                .spec
                .project_at_source(ExactSeconds::from_source(selected.elapsed_time))
                .map_err(|error| ProtocolError::invalid(error.to_string()))?;
            Ok(PlacedPreviewReceipt {
                request: request.clone(),
                quantization: actual_project_time.quantize_nanoseconds()?,
                actual_project_time,
            })
        })
        .transpose()?;
    let frame_index = selected.frame_index;
    let (width, height, view_filter, _) =
        video_views::plan(video_view, index, max_width, max_height)?;
    let frame_bytes = media::frame_bytes(width, height)? as u64;
    if frame_bytes > request.budget.memory_bytes / 8 || frame_bytes > request.budget.output_bytes {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "preview buffer exceeds memory admission",
        )
        .into());
    }
    let rgb = media::extract_filtered(
        &request.tools.ffmpeg.path,
        &request.source.path,
        media::SnapshotFrame {
            index: frame_index,
            source_time: selected.source_time,
        },
        media::FrameTransform {
            width,
            height,
            filter: &view_filter,
        },
        request.budget.cpu_threads,
        deadline,
    )?;
    let actual_context = FrameContext {
        frame: FrameId::new(frame_index as u64),
        ..context.clone()
    };
    let mut detector = create_detector(request, model, contract, deadline)?;
    let (observations, analysis) = if let Some(detector) = detector.as_mut() {
        let raw = detector.detect(&rgb, width, height)?;
        let observations = raw
            .into_iter()
            .map(|raw| {
                DetectionObservation::new(
                    actual_context.clone(),
                    CoordinateBox::new(
                        CoordinateSpace::ProjectionNormalized,
                        raw.bbox[0] as f64,
                        raw.bbox[1] as f64,
                        raw.bbox[2] as f64,
                        raw.bbox[3] as f64,
                    )?,
                    None,
                    EvidenceKind::Observed,
                    Confidence::new(raw.score as f64, None)?,
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        // Empty real inference is absence, not pending work or a fabricated ROI.
        (observations, AnalysisStatus::Available)
    } else {
        (
            Vec::new(),
            AnalysisStatus::Unavailable {
                reason:
                    "No model selected. Preview pixels are decoded; no detector observations exist."
                        .into(),
            },
        )
    };
    let artifact = artifacts::write_bytes(
        &request.output_dir,
        "preview.rgb",
        &rgb,
        request.budget.output_bytes,
    )?;
    let frame = FrameArtifact {
        artifact,
        width,
        height,
        stride_bytes: u64::from(width) * 3,
        format: PixelFormat::Rgb8,
    };
    frame.validate(request.budget.output_bytes)?;
    if let Some(detector) = detector {
        // ModelSession finalization publishes original CUDA receipt/trace into bounded egress.
        detector.finish()?;
    }
    Ok(WorkerOutput::Preview(
        (PreviewResult {
            annotated: None,
            placed,
            time_basis,
            elapsed_source_time: Some(selected.elapsed_time),
            source_origin: Some(selected.source_origin),
            coordinate_map: video_view
                .map(|view| {
                    pulsar_core::ViewCoordinateMap::new(
                        view.clone(),
                        index.width,
                        index.height,
                        width,
                        height,
                    )
                })
                .transpose()?,
            requested_source_time: match &request.operation {
                WorkerOperation::Preview { source_time, .. } => *source_time,
                _ => unreachable!("preview operation"),
            },
            context: actual_context,
            observations,
            frame: Some(frame),
            analysis,
            source_time: selected.source_time,
            source_frame_index: frame_index as u64,
        })
        .into(),
    ))
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleLineage {
    #[serde(skip_serializing_if = "Option::is_none")]
    motion_separation: Option<pulsar_core::perception::MotionSeparation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    flow_search: Option<[pulsar_core::perception::PyramidFlowSummary; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_sampling_points: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background_sampling_points: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mixed_audio: Option<mixed_audio::MixedSample>,
    source_frame_index: u64,
    source_pts: i64,
    project_time_ns: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    quantization_remainder: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quantization_denominator: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    placed: Option<pulsar_core::PlacedGenerationSample>,
    evidence: EvidenceKind,
    observed_boxes: usize,
    selected_class: Option<u32>,
    selected_track: Option<EntityTrackId>,
}

fn generate(
    request: &WorkerRequest,
    index: &media::MediaIndex,
    preset: &str,
    settings: &GenerationSettings,
    model: Option<&SourceArtifact>,
    deadline: Instant,
) -> Result<WorkerOutput> {
    settings.validate_execution_preset(preset, model.is_some())?;
    let tracking_plan = tracking_plan::TrackingPlan::resolve(preset)?;
    let placed_plan = settings
        .source_placement
        .as_ref()
        .map(|placement| {
            pulsar_core::plan_placed_generation(
                placement,
                &index.pts,
                index.time_base_num,
                index.time_base_den,
                settings.source_sample_stride,
                MAX_SAMPLED_FRAMES
                    .min(usize::try_from(request.budget.memory_bytes / 1024).unwrap_or(usize::MAX)),
            )
        })
        .transpose()?;
    let interval_plan = settings
        .project_interval
        .map(|interval| {
            pulsar_core::plan_generation_interval(
                &index.pts,
                index.time_base_num,
                index.time_base_den,
                settings.source_sample_stride,
                placed_plan.as_ref(),
                interval,
                MAX_SAMPLED_FRAMES
                    .min(usize::try_from(request.budget.memory_bytes / 1024).unwrap_or(usize::MAX)),
            )
        })
        .transpose()?;
    if let Some(seed) = &settings.target_seed {
        if seed.source_frame_index >= index.pts.len() as u64
            || placed_plan.as_ref().is_some_and(|plan| {
                !plan
                    .samples
                    .iter()
                    .any(|sample| sample.source_frame_index == seed.source_frame_index)
            })
            || interval_plan.as_ref().is_some_and(|plan| {
                !plan
                    .samples
                    .iter()
                    .any(|sample| sample.source_frame_index == seed.source_frame_index)
            })
        {
            return Err(ProtocolError::invalid(
                "target seed is outside the selected decoded source support",
            )
            .into());
        }
    }
    let temporal = settings
        .model_input
        .as_ref()
        .is_some_and(|c| c.decoder == DetectorDecoder::TemporalScriptPosition);
    let annotated_input = settings
        .annotated_view
        .as_ref()
        .map(|binding| {
            annotated_input::plan(
                binding,
                &request.source,
                [index.width, index.height],
                request.budget.memory_bytes,
            )
        })
        .transpose()?;
    let (width, height, view_filter, view_transform) = if let Some(input) = &annotated_input {
        (
            64,
            64,
            input.native_plan.spatial_filter.clone(),
            input.transform.clone(),
        )
    } else if temporal {
        (
            64,
            64,
            "scale=64:64:flags=neighbor".into(),
            TransformId::new("source-identity")?,
        )
    } else {
        video_views::plan(
            settings.video_view.as_ref(),
            index,
            settings.analysis_width,
            settings.analysis_height,
        )?
    };
    let bytes = media::frame_bytes(width, height)? as u64;
    let selected_count = index
        .pts
        .len()
        .div_ceil(settings.source_sample_stride as usize);
    if selected_count > MAX_SAMPLED_FRAMES
        || bytes > request.budget.memory_bytes / 16
        || selected_count as u64 > request.budget.memory_bytes / 1024
    {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "generation buffers exceed immutable attempt admission",
        )
        .into());
    }
    let mut decoder = media::Decoder::new_filtered(
        &request.tools.ffmpeg.path,
        &request.source.path,
        media::FrameTransform {
            width,
            height,
            filter: &view_filter,
        },
        settings.source_sample_stride,
        request.budget.cpu_threads,
        deadline,
    )?;
    let mut detector = if temporal {
        None
    } else {
        create_detector(request, model, settings.model_input.as_ref(), deadline)?
    };
    let mut temporal_estimator = if temporal {
        Some(temporal_position::TemporalPosition::from_request_at(
            request, deadline,
        )?)
    } else {
        None
    };
    let mixed_audio = settings
        .mixed_audio
        .as_ref()
        .map(|settings| mixed_audio::MixedAudio::prepare(request, settings, deadline))
        .transpose()?;
    let mut previous_gray: Option<Vec<u8>> = None;
    let mut previous_rgb: Option<Vec<u8>> = None;
    let mut tracker = tracking::StrokeTracker::with_plan(
        &request.attempt_id,
        settings.target_seed.clone(),
        tracking_plan,
    )?;
    let mut review_spans = std::collections::BTreeMap::new();
    let mut position = NormalizedPosition::new(0.5)?;
    let mut actions = Vec::new();
    let mut gaps = interval_plan
        .as_ref()
        .map(|p| p.uncovered_boundaries.clone())
        .unwrap_or_else(|| {
            placed_plan
                .as_ref()
                .map(|p| p.uncovered_boundaries.clone())
                .unwrap_or_default()
        });
    let mut gap_start: Option<ProjectTime> = None;
    let mut samples = Vec::with_capacity(
        interval_plan
            .as_ref()
            .map(|p| p.samples.len())
            .unwrap_or(selected_count),
    );
    for gap in &gaps {
        record_review(
            &mut review_spans,
            if interval_plan.is_some() {
                "interval_boundary_without_decoded_support"
            } else {
                "source_trim_boundary_without_decoded_support"
            }
            .into(),
            gap.start(),
            gap.end(),
            EvidenceKind::Unavailable,
        )?;
    }
    let mut last_time = ProjectTime::ZERO;
    let mut resume_cursor = 0;
    let mut prefix_last = None;
    let mut reused_samples = 0;
    if let Some(saved) = progress::load(request, index, [width, height], deadline)? {
        let mut retained: Vec<SampleLineage> = serde_json::from_value(saved.samples)?;
        for sample in &retained {
            let frame = sample.source_frame_index as usize;
            let placed = placed_plan
                .as_ref()
                .map(|plan| {
                    plan.samples
                        .binary_search_by_key(&sample.source_frame_index, |s| s.source_frame_index)
                        .map(|i| &plan.samples[i])
                        .map_err(|_| anyhow::anyhow!("progress sample outside placement"))
                })
                .transpose()?;
            if let Some(plan) = &interval_plan {
                anyhow::ensure!(
                    plan.samples
                        .binary_search_by_key(&sample.source_frame_index, |s| s.source_frame_index)
                        .is_ok(),
                    "progress sample outside interval"
                );
            }
            let expected_time = placed
                .map(|s| s.quantization.time)
                .unwrap_or(project_time(index, frame)?.quantize_nanoseconds()?.time);
            anyhow::ensure!(
                sample.project_time_ns == expected_time.as_nanos()
                    && serde_json::to_vec(&sample.placed)? == serde_json::to_vec(&placed.cloned())?,
                "progress placement/time correspondence mismatch"
            );
        }
        let context = retained.last().map(|sample| FrameContext {
            source_version: request.source.source_version.clone(),
            source_placement: settings
                .source_placement
                .as_ref()
                .map(|p| p.placement_id.clone())
                .unwrap_or(SourcePlacementId::new("worker-primary-source").unwrap()),
            frame: FrameId::new(sample.source_frame_index),
            transform: view_transform.clone(),
            seek_generation: 0,
            request_generation: sample.source_frame_index,
        });
        tracker = tracking::StrokeTracker::from_checkpoint(
            &saved.tracker,
            &saved.origin,
            &request.attempt_id,
            if detector.is_some() {
                context.as_ref()
            } else {
                None
            },
            ([width, height], saved.previous_gray.as_deref()),
            settings.target_seed.clone(),
            tracking_plan,
        )?;
        for sample in &mut retained {
            if let Some(id) = &sample.selected_track {
                sample.selected_track = Some(saved.tracker.rebind(id, &request.attempt_id)?);
            }
        }
        prefix_last = retained.last().map(|s| s.source_frame_index as usize);
        resume_cursor = saved.next_source_frame;
        reused_samples = retained.len();
        samples = retained;
        previous_gray = saved.previous_gray;
        previous_rgb = saved.previous_rgb;
        position = NormalizedPosition::new(f64::from_bits(saved.position_bits))?;
        actions = saved
            .actions
            .into_iter()
            .map(progress::SavedAction::restore)
            .collect::<Result<_>>()?;
        gaps = saved.gaps;
        gap_start = saved.gap_start;
        review_spans = saved.reviews;
        last_time = saved.last_time;
    }
    let cadence = selected_count.div_ceil(126).max(16);
    let mut last_checkpoint_cursor = resume_cursor;
    for frame_index in (0..index.pts.len()).step_by(settings.source_sample_stride as usize) {
        if Instant::now() >= deadline {
            bail!("generation deadline exceeded");
        }
        let decoded = decoder
            .next()?
            .context("decoder ended before the indexed source frame")?;
        let indexed_time = ExactSeconds::new(
            i128::from(index.pts[frame_index]) * i128::from(index.time_base_num),
            u64::from(index.time_base_den),
        )?;
        if ExactSeconds::from_source(decoded.source_time).checked_cmp(indexed_time)?
            != std::cmp::Ordering::Equal
            || decoded.ordinal != (frame_index / settings.source_sample_stride as usize) as u64
            || decoded.delivered > Instant::now()
        {
            bail!("decoded pixels differ from indexed source clock/order");
        }
        if (frame_index as u64) < resume_cursor {
            if prefix_last == Some(frame_index) {
                anyhow::ensure!(
                    previous_gray.as_ref() == Some(&grayscale(&decoded.pixels))
                        && (!temporal || previous_rgb.as_ref() == Some(&decoded.pixels)),
                    "progress support pixels differ from original decoded frame"
                );
            }
            continue;
        }
        let interval_sample = if let Some(plan) = &interval_plan {
            let Ok(i) = plan
                .samples
                .binary_search_by_key(&(frame_index as u64), |s| s.source_frame_index)
            else {
                continue;
            };
            Some((i, &plan.samples[i]))
        } else {
            None
        };
        let rgb = decoded.pixels;
        let placed_sample = if let Some(plan) = &placed_plan {
            let Ok(i) = plan
                .samples
                .binary_search_by_key(&(frame_index as u64), |s| s.source_frame_index)
            else {
                continue;
            };
            Some((i, &plan.samples[i]))
        } else {
            None
        };
        let gray = grayscale(&rgb);
        let (time, legacy_quantization) = match placed_sample {
            Some((_, sample)) => (sample.quantization.time, None),
            None => {
                let q = project_time(index, frame_index)?.quantize_nanoseconds()?;
                (q.time, Some(q))
            }
        };
        let is_cut = previous_gray
            .as_ref()
            .is_some_and(|previous| photometric_cut(previous, &gray));
        let mut observed_boxes = 0;
        let mut selected_class = None;
        let mut selected_track = None;
        let mut motion_separation = None;
        let mut flow_search = None;
        let mut target_sampling_points = None;
        let mut background_sampling_points = None;
        let next_frame = frame_index.saturating_add(settings.source_sample_stride as usize);
        let end = if let (Some(plan), Some((_i, _))) = (&interval_plan, interval_sample) {
            let position = plan
                .project_order
                .binary_search_by_key(&time, |&index| plan.samples[index].project_time)
                .map_err(|_| anyhow::anyhow!("interval sample absent from project order"))?;
            if let Some(next) = plan.project_order.get(position + 1) {
                plan.samples[*next].project_time
            } else {
                time.checked_add(1)?
            }
        } else if let (Some(plan), Some((i, _))) = (&placed_plan, placed_sample) {
            let project_position = if plan.placement.spec.rate_numerator > 0 {
                i
            } else {
                plan.samples.len() - 1 - i
            };
            if let Some(next) = plan.project_order.get(project_position + 1) {
                plan.samples[*next].quantization.time
            } else {
                time.checked_add(1)?
            }
        } else if next_frame < index.pts.len() {
            project_time(index, next_frame)?
                .quantize_nanoseconds()?
                .time
        } else {
            time.checked_add(1)?
        };
        let mut reasons = Vec::new();
        let mut evidence = EvidenceKind::Unavailable;
        if let Some(estimator) = temporal_estimator.as_mut() {
            reasons.push("learned_reference_script_position_uncalibrated".into());
            let delta = if frame_index >= settings.source_sample_stride as usize {
                project_time(index, frame_index)?
                    .quantize_nanoseconds()?
                    .time
                    .checked_sub(
                        project_time(index, frame_index - settings.source_sample_stride as usize)?
                            .quantize_nanoseconds()?
                            .time,
                    )?
            } else {
                0
            };
            if let Some(previous) = previous_rgb.as_ref() {
                if !is_cut && delta > 0 && delta <= 500_000_000 {
                    position = estimator.estimate(previous, &rgb)?;
                    evidence = EvidenceKind::Inferred;
                } else {
                    reasons.push("temporal_pair_without_continuous_support".into());
                }
            } else {
                evidence = EvidenceKind::Synthesized;
                reasons.push("temporal_neutral_start_anchor".into());
            }
        } else if let Some(detector) = detector.as_mut() {
            let detections = detector.detect(&rgb, width, height)?;
            let frame = tracker.update(
                FrameContext {
                    source_version: request.source.source_version.clone(),
                    source_placement: settings
                        .source_placement
                        .as_ref()
                        .map(|p| p.placement_id.clone())
                        .unwrap_or(SourcePlacementId::new("worker-primary-source")?),
                    frame: FrameId::new(frame_index as u64),
                    transform: view_transform.clone(),
                    seek_generation: 0,
                    request_generation: frame_index as u64,
                },
                &gray,
                width,
                height,
                &detections,
                is_cut,
            )?;
            observed_boxes = frame.observed_boxes.len();
            selected_class = frame.selected_class;
            selected_track = frame.selected_track;
            motion_separation = frame.motion_separation;
            flow_search = frame.flow_search;
            target_sampling_points = frame.target_sampling_points;
            background_sampling_points = frame.background_sampling_points;
            reasons = frame.reviews;
            if let Some(value) = frame.position {
                position = value;
                evidence = frame.evidence;
            }
        } else if let Some(previous) = previous_gray.as_ref() {
            if !is_cut {
                let estimate = estimate_vertical_translation(previous, &gray, width, height, 8)?;
                if estimate.has_evidence {
                    position =
                        integrate_vertical_motion(position, estimate.displacement_pixels, height)?;
                    evidence = EvidenceKind::Inferred;
                }
            }
        } else {
            // An explicit neutral starting anchor, not an observed stroke.
            evidence = EvidenceKind::Synthesized;
        }
        let mut output_position = position;
        let mixed_sample = if let Some(audio) = &mixed_audio {
            let (selected, receipt) =
                audio.select(index.pts[frame_index], index, position, evidence)?;
            if receipt.audio_window_index.is_none() {
                reasons.push("mixed_audio_outside_decoded_support".into());
            }
            match selected {
                Some((value, kind)) => {
                    output_position = value;
                    evidence = kind;
                }
                None => {
                    evidence = EvidenceKind::Unavailable;
                }
            }
            if receipt.contribution == MotionContribution::Audio {
                reasons.push("mixed_audio_synthesized_uncalibrated".into());
            }
            Some(receipt)
        } else {
            None
        };
        if model.is_none() {
            reasons.push("global_motion_without_target_identity".to_owned());
        }
        if is_cut {
            reasons.push("scene_cut".to_owned());
        }
        if evidence == EvidenceKind::Unavailable {
            reasons.push("missing_motion_evidence".to_owned());
        }
        for reason in reasons {
            record_review(&mut review_spans, reason, time, end, evidence)?;
        }
        if placed_plan.is_some() {
            if evidence == EvidenceKind::Unavailable {
                gaps.push(TimeRange::new(time, end)?);
            } else {
                actions.push(MotionAction::new(time, output_position, evidence)?);
            }
        } else if evidence == EvidenceKind::Unavailable {
            gap_start.get_or_insert(time);
        } else {
            if let Some(start) = gap_start.take() {
                if start < time {
                    gaps.push(TimeRange::new(start, time)?);
                }
            }
            actions.push(MotionAction::new(time, output_position, evidence)?);
        }
        samples.push(SampleLineage {
            motion_separation,
            flow_search,
            target_sampling_points,
            background_sampling_points,
            mixed_audio: mixed_sample,
            source_frame_index: frame_index as u64,
            source_pts: index.pts[frame_index],
            project_time_ns: time.as_nanos(),
            quantization_remainder: legacy_quantization.as_ref().map(|q| q.remainder_numerator),
            quantization_denominator: legacy_quantization.as_ref().map(|q| q.denominator),
            placed: placed_sample.map(|(_, s)| s.clone()),
            evidence,
            observed_boxes,
            selected_class,
            selected_track,
        });
        last_time = time;
        previous_gray = Some(gray);
        if temporal {
            previous_rgb = Some(rgb);
        }
        if progress::active()
            && (samples.len() == reused_samples + 1 || samples.len() % cadence == 0)
        {
            let next = ((frame_index as u64) + u64::from(settings.source_sample_stride))
                .min(index.pts.len() as u64);
            progress::emit(
                request,
                progress::Snapshot {
                    schema: 1,
                    basis_sha256: crate::generation_progress::basis(request)?,
                    origin: request.attempt_id.clone(),
                    next_source_frame: next,
                    tracker: tracker.checkpoint()?,
                    previous_gray: previous_gray.clone(),
                    previous_rgb: previous_rgb.clone(),
                    position_bits: position.value().to_bits(),
                    reviews: review_spans.clone(),
                    actions: actions.iter().map(progress::SavedAction::save).collect(),
                    gaps: gaps.clone(),
                    gap_start,
                    samples: progress::pack_floats(serde_json::to_value(&samples)?),
                    last_time,
                },
            )?;
            last_checkpoint_cursor = next;
        }
    }
    if progress::active() && !samples.is_empty() && last_checkpoint_cursor < index.pts.len() as u64
    {
        progress::emit(
            request,
            progress::Snapshot {
                schema: 1,
                basis_sha256: crate::generation_progress::basis(request)?,
                origin: request.attempt_id.clone(),
                next_source_frame: index.pts.len() as u64,
                tracker: tracker.checkpoint()?,
                previous_gray: previous_gray.clone(),
                previous_rgb: previous_rgb.clone(),
                position_bits: position.value().to_bits(),
                reviews: review_spans.clone(),
                actions: actions.iter().map(progress::SavedAction::save).collect(),
                gaps: gaps.clone(),
                gap_start,
                samples: progress::pack_floats(serde_json::to_value(&samples)?),
                last_time,
            },
        )?;
    }
    if decoder.next()?.is_some() {
        bail!("decoder produced frames absent from its source index");
    }
    if let Some(start) = gap_start {
        gaps.push(TimeRange::new(start, last_time.checked_add(1)?)?);
    }
    if placed_plan.is_some() || interval_plan.is_some() {
        actions.sort_by_key(|a| a.time());
        gaps.sort_by_key(|g| g.start());
        let mut joined: Vec<TimeRange> = Vec::new();
        for gap in gaps {
            if let Some(last) = joined.last_mut() {
                if last.end() >= gap.start() {
                    *last = TimeRange::new(last.start(), last.end().max(gap.end()))?;
                    continue;
                }
            }
            joined.push(gap);
        }
        gaps = joined;
    }
    let program = MotionProgram::new(vec![MotionTrack::with_gaps(Axis::Stroke, actions, gaps)?])?;
    let review = finish_reviews(review_spans, Some(Axis::Stroke))?;
    let program_artifact = artifacts::write_json(
        &request.output_dir,
        "candidate.json",
        &program,
        request.budget.output_bytes,
    )?;
    let mut lineage = vec![
        request.source.identity.clone(),
        request.tools.ffmpeg.identity.clone(),
        request.tools.ffprobe.identity.clone(),
    ];
    if let Some(model) = model {
        lineage.push(model.identity.clone());
    }
    if let Some(runtime) = &request.tools.onnx_runtime {
        lineage.push(runtime.identity.clone());
    }
    lineage.extend(
        request
            .dependencies
            .iter()
            .filter(|item| !lineage.contains(item))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let configuration = serde_json::to_vec(&(preset, settings))?;
    let config_sha256 = format!("{:x}", Sha256::digest(&configuration));
    let annotated_model=temporal_estimator.as_ref().and_then(|estimator|estimator.annotated_contract().map(|contract|
        serde_json::json!({"contract":contract,"training_intake_sha256":estimator.training_intake_sha256()})));
    let model_execution = if let Some(detector) = detector {
        detector.finish()?
    } else if let Some(estimator) = temporal_estimator {
        estimator.finish()?
    } else {
        None
    };
    let mut receipt = serde_json::json!({
        "schema_version": if annotated_input.is_some(){3}else if placed_plan.is_some(){2}else{1}, "recipe": if annotated_input.is_some(){ANNOTATED_TEMPORAL_RECIPE}else if temporal {"temporal-reference-script-position-v1"}else if model.is_some() { if preset == "default" {"tracked-relative-image-flow-stroke-v4"} else {"tracked-relative-image-flow-stroke-v5"} } else { "vertical-translation-stroke-v1" },
        "qualification": "unqualified", "project_id": request.project_id, "base_revision": request.base_revision,
        "job_id": request.job_id, "attempt_id": request.attempt_id, "source_version": request.source.source_version,
        "program": program_artifact.identity, "configuration_sha256": config_sha256, "configuration": settings,
        "preset": preset, "dependencies": lineage, "coordinate_projection": view_transform, "analysis_coordinate_basis": if annotated_input.is_some(){"declared native eye/projection, static integer target region, nearest 64x64"}else if settings.video_view.is_some() {"decoded selected view; not original packed-source pixels"}else{"decoded resized source view"},
        "coordinate_map": settings.video_view.as_ref().map(|view| pulsar_core::ViewCoordinateMap::new(view.clone(), index.width, index.height, width, height)).transpose()?,
        "coordinate_map_contract": "normalized image edges; pixel centers at (i+0.5)/size; nominal sampling centers; no bounding-box, depth, or placement inference",
        "source_dimensions": [index.width, index.height], "analysis_dimensions": [width, height],
        "source_time_base": {"numerator": index.time_base_num, "denominator": index.time_base_den},
        "source_origin_pts": index.pts[0], "project_time_mapping": if placed_plan.is_some(){"exact elapsed source PTS through immutable placement; floor nanoseconds once; source/decode order retained; project order explicit"}else{"source PTS minus first PTS, floor nanoseconds; exact remainder retained"},
        "mixed_audio": mixed_audio.as_ref().map(|audio|serde_json::json!({"settings":settings.mixed_audio,"independent_source":request.independent_audio.as_ref().map(|source|serde_json::json!({"source_version":source.source_version,"identity":source.identity})),"audio_timeline":audio.timeline,"window_samples":160,"sample_rate":16000,"recipe":"full-band-rms-on-video-sample-lattice-v1","alignment":if request.independent_audio.is_some(){"producer-declared audio_anchor + rate * (original video PTS - video_anchor), rate defaults to one, then original decoded audio origin; no alignment measurement"}else{"original video PTS minus original decoded audio origin; exact containing 10ms window; no stream-zero alignment or audio normalization"},"faithful_precedence":"preserve every available visual sample; audio only assists unavailable visual samples","creative_precedence":"explicit audio replacement; missing audio remains unavailable","qualification":"unqualified"})),
        "source_placement": settings.source_placement, "placed_generation_plan": placed_plan,
        "project_interval_plan": interval_plan, "decoded_sample_count": selected_count,
        "analysed_sample_count": samples.len(),
        "interval_contract": "analyse only the selected original lattice; decoder clock/hash validation still traverses the indexed container; fresh tracker/flow context, explicit unavailable boundary coverage, no clock reset or whole-source inference speed claim",
        "detector_score_threshold": if model.is_some() && !temporal {Some(settings.effective_detector_score_threshold())}else{None},
        "confidence_calibration": "uncalibrated", "target_selection": if annotated_input.is_some(){"declared static region temporal regression; no tracked target identity, boxes, depth or physical-motion claim"}else if temporal {"whole packed-source temporal regression against supplied script labels; no anatomical target, boxes, depth or physical-motion claim"}else if model.is_some() {
            "stable observed entity identity; target/background independently checked image flow; neutral synthesized anchors; missing support remains gaps"
        } else { "global analysis-view pixel vertical translation; not anatomical target identification" },
        "manual_target_seed": settings.target_seed,
        "motion_separation_contract": "analysis-view pixel translations and accepted point counts; checked image-relative subtraction, not physical target motion or camera pose; absent diagnostics mean the flow stage was not reached; null translations mean unavailable support",
        "manual_target_contract": "exact original source ordinal and analysis-view normalized point select exactly one observed class-compatible box; follow only that issued entity identity; missing/ambiguous seed or lost identity never selects another target automatically",
        "samples": samples, "review": review, "review_aggregation": "same-reason covering spans; may include intervening supported frames; uncalibrated", "unresolved_gaps": program.track(Axis::Stroke).map(|track| track.gaps()),
        "claim": "Unqualified best-effort generation; not a stroke-quality, speed, model, or hardware qualification",
        "inference_execution": model_execution.or_else(telemetry::receipt)
    });
    if let Some(mut recovery) = progress::recovery_receipt() {
        recovery["reused_analysed_samples"] = serde_json::json!(reused_samples);
        recovery["replayed_decode_prefix_samples"] =
            serde_json::json!(resume_cursor.div_ceil(u64::from(settings.source_sample_stride)));
        receipt["generation_progress"] = recovery;
    }
    if model.is_some() && !temporal {
        receipt["visual_tracking_plan"] = serde_json::to_value(tracking_plan)?;
    }
    if let Some(input) = &annotated_input {
        receipt["annotated_input"] = serde_json::to_value(input)?;
        receipt["annotated_model"] =
            annotated_model.context("annotated model binding absent at publication")?;
    }
    let remaining = request
        .budget
        .output_bytes
        .checked_sub(program_artifact.identity.byte_len)
        .context("attempt artifact budget exceeded")?;
    let receipt = artifacts::write_json(&request.output_dir, "lineage.json", &receipt, remaining)?;
    Ok(WorkerOutput::Generated {
        program: program_artifact,
        receipt,
        lineage,
        review,
    })
}

type ReviewSpans = std::collections::BTreeMap<String, (ProjectTime, ProjectTime, EvidenceKind)>;

fn record_review(
    spans: &mut ReviewSpans,
    code: String,
    start: ProjectTime,
    end: ProjectTime,
    evidence: EvidenceKind,
) -> Result<()> {
    ReviewReason::new(code.clone())?;
    if end <= start {
        bail!("review interval lost timestamp precision");
    }
    spans
        .entry(code)
        .and_modify(|value| {
            value.0 = value.0.min(start);
            value.1 = value.1.max(end);
            if value.2 != evidence {
                value.2 = EvidenceKind::Inferred;
            }
        })
        .or_insert((start, end, evidence));
    if spans.len() > MAX_REVIEW_FLAGS {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "review reason budget exhausted",
        )
        .into());
    }
    Ok(())
}

fn finish_reviews(spans: ReviewSpans, axis: Option<Axis>) -> Result<Vec<ReviewFlag>> {
    let review = spans
        .into_iter()
        .map(|(code, (start, end, evidence))| {
            Ok(ReviewFlag {
                axis,
                range: TimeRange::new(start, end)?,
                reason: ReviewReason::new(code)?,
                evidence,
                confidence: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    validate_reviews(&review)?;
    Ok(review)
}

fn project_time(index: &media::MediaIndex, frame: usize) -> Result<SourceTimestamp> {
    let delta = index.pts[frame]
        .checked_sub(index.pts[0])
        .context("source PTS difference overflow")?;
    SourceTimestamp::new(
        delta
            .checked_mul(i64::from(index.time_base_num))
            .context("source/project timestamp mapping overflow")?,
        index.time_base_den,
    )
    .map_err(Into::into)
}

fn grayscale(rgb: &[u8]) -> Vec<u8> {
    rgb.as_chunks::<3>()
        .0
        .iter()
        .map(|pixel| {
            ((u32::from(pixel[0]) * 77 + u32::from(pixel[1]) * 150 + u32::from(pixel[2]) * 29) >> 8)
                as u8
        })
        .collect()
}

fn photometric_cut(previous: &[u8], current: &[u8]) -> bool {
    previous.len() != current.len()
        || previous.is_empty()
        || previous
            .iter()
            .zip(current)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum::<u64>() as f64
            / previous.len() as f64
            > 65.0
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn cpu_request() -> WorkerRequest {
        let mut request: WorkerRequest = serde_json::from_str(include_str!(
            "../../tests/fixtures/cuda-intent-request.json"
        ))
        .unwrap();
        request.cuda_execution = None;
        let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
            panic!("authored generation fixture required")
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        request.validate().unwrap();
        request
    }

    #[test]
    fn native_worker_refuses_reserved_cpu_image_before_opening_any_inputs() {
        let root = tempfile::tempdir().unwrap();
        let mut request = cpu_request();
        request.source.path = root.path().join("absent-source");
        request.output_dir = root.path().join("absent-output");
        request.dependencies.push(ArtifactIdentity {
            artifact_id: ArtifactId::new(format!(
                "{CPU_RUNTIME_IMAGE_ID_PREFIX}{}",
                "a".repeat(64)
            ))
            .unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        });
        let result = execute_manifest(&request);
        assert_eq!(result.job_id, request.job_id);
        assert_eq!(result.attempt_id, request.attempt_id);
        assert!(result.cuda_artifacts.is_none());
        assert!(result.provider_unavailable.is_none());
        let error = result.result.unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error
            .message
            .contains("captured CPU runtime requires its verified closed worker"));
        assert!(!request.output_dir.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn closed_cpu_requires_original_identity_and_explicit_cpu_selection() {
        let request = cpu_request();
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&request).unwrap())
        );
        validate_closed_cpu_request_identity(&request, &digest).unwrap();
        assert!(validate_closed_cpu_request_identity(&request, &"0".repeat(64)).is_err());
        assert!(validate_closed_cpu_request_identity(&request, "not-a-digest").is_err());
        require_explicit_cpu_profile(&request).unwrap();
        let mut implicit = request.clone();
        let WorkerOperation::Generate { settings, .. } = &mut implicit.operation else {
            unreachable!()
        };
        settings.inference_profile = None;
        assert!(crate::inference_profiles::resolve_profile(&implicit)
            .unwrap()
            .is_some());
        assert!(require_explicit_cpu_profile(&implicit).is_err());
        let cuda: WorkerRequest = serde_json::from_str(include_str!(
            "../../tests/fixtures/cuda-intent-request.json"
        ))
        .unwrap();
        assert!(require_explicit_cpu_profile(&cuda).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn closed_cpu_witness_uses_bounded_strict_control_framing() {
        for length in [0, MAX_CONTROL_BYTES as u32 + 1] {
            let mut bytes = std::io::Cursor::new(length.to_be_bytes());
            assert!(read_message::<_, crate::cpu_mounts::CpuMountWitness>(&mut bytes).is_err());
        }
        let mut bytes = Vec::new();
        write_message(&mut bytes, &serde_json::json!({"unexpected": true})).unwrap();
        assert!(
            read_message::<_, crate::cpu_mounts::CpuMountWitness>(&mut std::io::Cursor::new(bytes))
                .is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn closed_cpu_bootstrap_error_is_bounded_original_attempt_without_provider_claim() {
        let request = cpu_request();
        let result = closed_cpu_failure(&request, anyhow::anyhow!("{}", "界".repeat(3000)));
        assert_eq!(result.attempt_id, request.attempt_id);
        assert_eq!(result.job_id, request.job_id);
        assert_eq!(result.version, PROTOCOL_VERSION);
        assert!(result.cuda_artifacts.is_none());
        assert!(result.provider_unavailable.is_none());
        result.validate_provider_unavailable_for(&request).unwrap();
        let error = result.result.as_ref().unwrap_err();
        assert_eq!(error.code, ErrorCode::DependencyMismatch);
        assert!(error.message.len() <= 4096);
        assert!(!error.retryable);
    }

    #[test]
    fn retained_deadline_is_never_restarted_after_bootstrap() {
        let retained = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            execution_deadline(86_400_000, Some(retained)).unwrap(),
            retained
        );
        let expired = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        assert!(execution_deadline(86_400_000, Some(expired)).is_err());
    }

    #[test]
    fn rational_source_pts_and_project_origin_stay_distinct() {
        let index = media::MediaIndex {
            width: 32,
            height: 32,
            time_base_num: 1,
            time_base_den: 90_000,
            pts: vec![9_000, 12_003, 15_006],
        };
        assert_eq!(
            seek_source_frames(
                &index.pts,
                index.time_base_num,
                index.time_base_den,
                SourceTimestamp::new(12_003, index.time_base_den).unwrap(),
                PreviewTimeBasis::SourcePts
            )
            .unwrap()
            .source_time
            .numerator(),
            12_003
        );
        let project = project_time(&index, 1)
            .unwrap()
            .quantize_nanoseconds()
            .unwrap();
        assert_eq!(project.time.as_nanos(), 33_366_666);
        assert_ne!(project.remainder_numerator, 0);
    }

    #[test]
    fn grayscale_and_cut_are_real_pixel_functions() {
        assert_eq!(grayscale(&[255, 255, 255, 0, 0, 0]), [255, 0]);
        assert!(!photometric_cut(&[128; 9], &[128; 9]));
        assert!(photometric_cut(&[0; 9], &[255; 9]));
    }
}
