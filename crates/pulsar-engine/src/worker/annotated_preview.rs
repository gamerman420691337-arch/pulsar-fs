//! Read-only target pixels and one explicit source-clock temporal pair.
use super::*;
use std::io::Read;

pub(super) fn run(
    request: &WorkerRequest,
    index: &media::MediaIndex,
    deadline: Instant,
) -> Result<WorkerOutput> {
    let WorkerOperation::Preview {
        annotated: Some(spec),
        context,
        source_time,
        time_basis,
        placement_seek,
        ..
    } = &request.operation
    else {
        bail!("annotated preview operation absent")
    };
    let plan = annotated_input::plan(
        &spec.view,
        &request.source,
        [index.width, index.height],
        request.budget.memory_bytes,
    )?;
    if context.source_version != request.source.source_version
        || context.transform != annotated_preview_transform(&spec.view)?
    {
        bail!("annotated preview context differs from source/target");
    }
    let selected = if let Some(placed) = placement_seek {
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
        )?
    } else {
        seek_source_frames(
            &index.pts,
            index.time_base_num,
            index.time_base_den,
            *source_time,
            *time_basis,
        )?
    };
    let stride = usize::try_from(spec.source_sample_stride)?;
    let current = selected.frame_index / stride * stride;
    let clock = |i: usize| -> Result<SourceTimestamp> {
        Ok(SourceTimestamp::new(
            i64::try_from(i128::from(index.pts[i]) * i128::from(index.time_base_num))?,
            index.time_base_den,
        )?)
    };
    let decode = |i| {
        media::extract_filtered(
            &request.tools.ffmpeg.path,
            &request.source.path,
            media::SnapshotFrame {
                index: i,
                source_time: clock(i)?,
            },
            media::FrameTransform {
                width: 64,
                height: 64,
                filter: &plan.native_plan.spatial_filter,
            },
            request.budget.cpu_threads,
            deadline,
        )
    };
    let rgb = decode(current)?;
    let mut estimator = temporal_position::TemporalPosition::from_request_at(request, deadline)?;
    let contract = estimator
        .annotated_contract()
        .context("annotated model contract absent")?
        .clone();
    let training_intake_sha256 = estimator
        .training_intake_sha256()
        .context("annotated intake absent")?
        .to_owned();
    let write_frame = |name: &str, bytes: &[u8]| -> Result<FrameArtifact> {
        Ok(FrameArtifact {
            artifact: artifacts::write_bytes(
                &request.output_dir,
                name,
                bytes,
                request.budget.output_bytes,
            )?,
            width: 64,
            height: 64,
            stride_bytes: 192,
            format: PixelFormat::Rgb8,
        })
    };
    let mut position = None;
    let previous = if current >= stride {
        let i = current - stride;
        let prior = decode(i)?;
        let delta = project_time(index, current)?
            .quantize_nanoseconds()?
            .time
            .checked_sub(project_time(index, i)?.quantize_nanoseconds()?.time)?;
        if delta > 0
            && delta <= 500_000_000
            && !photometric_cut(&grayscale(&prior), &grayscale(&rgb))
        {
            position = Some(estimator.estimate(&prior, &rgb)?);
        }
        Some(AnnotatedPreviewPrevious {
            source_frame_index: i as u64,
            source_time: clock(i)?,
            elapsed_source_time: project_time(index, i)?,
            frame: write_frame("preview-previous.rgb", &prior)?,
        })
    } else {
        None
    };
    estimator.finish()?;
    let profile = match &request.operation {
        WorkerOperation::Preview {
            inference_profile: Some(profile),
            ..
        } => profile,
        _ => bail!("annotated profile absent"),
    };
    let model = match &request.operation {
        WorkerOperation::Preview {
            model: Some(model), ..
        } => model,
        _ => bail!("annotated model absent"),
    };
    let elapsed = project_time(index, current)?;
    let placed = placement_seek
        .as_ref()
        .map(|seek| -> Result<PlacedPreviewReceipt> {
            let actual_project_time = seek
                .placement
                .spec
                .project_at_source(ExactSeconds::from_source(elapsed))?;
            Ok(PlacedPreviewReceipt {
                request: *seek.clone(),
                quantization: actual_project_time.quantize_nanoseconds()?,
                actual_project_time,
            })
        })
        .transpose()?;
    let analysis = if position.is_some() {
        AnalysisStatus::Available
    } else {
        AnalysisStatus::Unavailable {
            reason: if previous.is_none() {
                "no_previous_sample"
            } else {
                "temporal_pair_without_continuous_support"
            }
            .into(),
        }
    };
    let preview = PreviewResult {
        annotated: Some(AnnotatedPreviewEvidence {
            spec: *spec.clone(),
            previous,
            position,
            model: model.identity.clone(),
            profile: crate::inference_profiles::profile_identity(profile)?,
            runtime: request
                .tools
                .onnx_runtime
                .as_ref()
                .context("runtime absent")?
                .identity
                .clone(),
            contract,
            training_intake_sha256,
            qualified: false,
        }),
        placed,
        time_basis: *time_basis,
        elapsed_source_time: Some(elapsed),
        source_origin: Some(clock(0)?),
        coordinate_map: None,
        context: FrameContext {
            frame: FrameId::new(current as u64),
            ..context.clone()
        },
        requested_source_time: *source_time,
        source_time: clock(current)?,
        source_frame_index: current as u64,
        observations: vec![],
        frame: Some(write_frame("preview.rgb", &rgb)?),
        analysis,
    };
    preview
        .annotated
        .as_ref()
        .expect("annotated evidence")
        .validate(&preview)?;
    Ok(WorkerOutput::Preview((preview).into()))
}

pub(super) fn validate_pair(preview: &PreviewResult) -> Result<()> {
    let evidence = preview
        .annotated
        .as_ref()
        .context("annotated preview evidence absent")?;
    let supported = if let Some(previous) = &evidence.previous {
        let current = read_pixels(preview.frame.as_ref().context("current pixels absent")?)?;
        let prior = read_pixels(&previous.frame)?;
        if current.len() != 64 * 64 * 3 || prior.len() != 64 * 64 * 3 {
            bail!("annotated preview pixel count differs");
        }
        let delta = preview
            .elapsed_source_time
            .context("elapsed clock absent")?
            .quantize_nanoseconds()?
            .time
            .checked_sub(previous.elapsed_source_time.quantize_nanoseconds()?.time)?;
        delta > 0
            && delta <= 500_000_000
            && !photometric_cut(&grayscale(&prior), &grayscale(&current))
    } else {
        false
    };
    if supported != evidence.position.is_some() {
        bail!("annotated preview availability differs from original pair support");
    }
    Ok(())
}

fn read_pixels(frame: &FrameArtifact) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(64 * 64 * 3);
    std::fs::File::open(&frame.artifact.path)?
        .take(64 * 64 * 3 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() != 64 * 64 * 3
        || format!("{:x}", Sha256::digest(&bytes)) != frame.artifact.identity.sha256
    {
        bail!("annotated preview pair pixels changed after artifact verification");
    }
    Ok(bytes)
}
