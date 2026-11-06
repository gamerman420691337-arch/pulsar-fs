//! Captured RGB pair adapter. No native acquisition, camera access or dispatch.
use super::*;
pub(super) fn generate(
    request: &WorkerRequest,
    preset: &str,
    settings: &GenerationSettings,
    model: Option<&SourceArtifact>,
    deadline: Instant,
) -> Result<WorkerOutput> {
    if !matches!(preset, "default" | "fast" | "economy") {
        return Err(ProtocolError::unsupported("raw frame-pair preset").into());
    }
    let pair = settings
        .raw_frame_pair
        .as_ref()
        .context("raw frame pair declaration missing")?;
    let elapsed = pair.elapsed()?;
    let presentation = pair.presentation_mapping()?;
    let previous_time = presentation
        .as_ref()
        .map_or(ProjectTime::ZERO, |mapping| mapping[0].project.time);
    let current_time = presentation
        .as_ref()
        .map_or(elapsed.time, |mapping| mapping[1].project.time);
    const FRAME: usize = 64 * 64 * 3;
    if request.source.identity.byte_len != (2 * FRAME) as u64
        || request.budget.memory_bytes < 256 * 1024
        || request.budget.output_bytes < 4096
    {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "raw frame-pair bytes or budget differ from admission",
        )
        .into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(&request.source.path)?;
    if !file.metadata()?.is_file() {
        bail!("raw frame pair must be a regular immutable snapshot");
    }
    let mut bytes = Vec::with_capacity(2 * FRAME + 1);
    file.take((2 * FRAME + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() != 2 * FRAME
        || format!("{:x}", Sha256::digest(&bytes)) != request.source.identity.sha256
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "raw frame pair differs from immutable captured bytes",
        )
        .into());
    }
    let mut estimator = temporal_position::TemporalPosition::from_request_at(request, deadline)?;
    let pixels = crate::live_frames::PackedRgbPair::new(&bytes)?;
    let position = estimator.estimate(pixels.previous(), pixels.current())?;
    if Instant::now() >= deadline {
        bail!("raw frame-pair analysis deadline exceeded");
    }
    let program = MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        vec![
            MotionAction::new(
                previous_time,
                NormalizedPosition::new(0.5)?,
                EvidenceKind::Synthesized,
            )?,
            MotionAction::new(current_time, position, EvidenceKind::Inferred)?,
        ],
    )?])?;
    let review = vec![ReviewFlag {
        axis: Some(Axis::Stroke),
        range: TimeRange::new(previous_time, current_time.checked_add(1)?)?,
        reason: ReviewReason::new("captured_pair_script_position_uncalibrated")?,
        evidence: EvidenceKind::Inferred,
        confidence: None,
    }];
    let program = artifacts::write_json(
        &request.output_dir,
        "candidate.json",
        &program,
        request.budget.output_bytes,
    )?;
    let mut lineage = vec![
        request.source.identity.clone(),
        model.expect("checked").identity.clone(),
        request
            .tools
            .onnx_runtime
            .as_ref()
            .expect("checked")
            .identity
            .clone(),
        request.tools.ffmpeg.identity.clone(),
        request.tools.ffprobe.identity.clone(),
    ];
    for identity in &request.dependencies {
        if !lineage.contains(identity) {
            lineage.push(identity.clone());
        }
    }
    let configuration = serde_json::to_vec(&(preset, settings))?;
    let model_execution = estimator.finish()?;
    let receipt = serde_json::json!({"schema_version":1,"recipe":"captured-rgb-pair-script-position-v1","qualification":"unqualified","project_id":request.project_id,"base_revision":request.base_revision,"job_id":request.job_id,"attempt_id":request.attempt_id,"source_version":request.source.source_version,"program":program.identity,"configuration":settings,"configuration_sha256":format!("{:x}",Sha256::digest(configuration)),"preset":preset,"dependencies":lineage,"source_pair":pair,"source_clock_evidence":"producer-declared rational clocks and epoch; not measured capture-to-command","project_time_mapping":if presentation.is_some(){"declared unit-rate source/project/presentation anchors; complete project expression floored once"}else{"relative to previous captured frame; exact rational difference floored once"},"presentation_mapping":presentation,"presentation_clock_evidence":"declared anchors only; no host/player measurement","presentation_buffer_executed":false,"current_quantization":elapsed,"pair_interval_quantization":elapsed,"project_action_times_ns":[previous_time,current_time],"analysis_dimensions":[64,64],"frame_bytes":FRAME,"position":position,"review":review,"inference_execution":model_execution.or_else(telemetry::receipt),"physical_dispatch":false,"continuous_live_session":false,"claim":"One captured-pair inference; no camera, clock synchronization, anatomical tracking, physical-device or live-latency qualification"});
    let remaining = request
        .budget
        .output_bytes
        .checked_sub(program.identity.byte_len)
        .context("raw frame-pair output admission exceeded")?;
    let receipt = artifacts::write_json(&request.output_dir, "lineage.json", &receipt, remaining)?;
    Ok(WorkerOutput::Generated {
        program,
        receipt,
        lineage,
        review,
    })
}
