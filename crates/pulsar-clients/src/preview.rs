//! Frame/evidence freshness and bounded immutable artifact consumption.

use anyhow::{bail, Result};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::io::Read;

pub const MAX_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;

enum ExpectedPreview {
    Source(FrameContext, SourceTimestamp, PreviewTimeBasis),
    Placed(FrameContext, PlacedPreviewRequest),
}
#[derive(Default)]
pub struct PreviewState {
    expected: Option<ExpectedPreview>,
    annotated: Option<(AnnotatedPreviewSpec, DeclaredInferenceProfile)>,
    displayed: Option<PreviewResult>,
}

impl PreviewState {
    pub fn begin(&mut self, expected: FrameContext, requested_source_time: SourceTimestamp) {
        self.begin_timed(expected, requested_source_time, PreviewTimeBasis::SourcePts);
    }
    pub fn begin_timed(
        &mut self,
        expected: FrameContext,
        requested_source_time: SourceTimestamp,
        time_basis: PreviewTimeBasis,
    ) {
        self.annotated = None;
        self.expected = Some(ExpectedPreview::Source(
            expected,
            requested_source_time,
            time_basis,
        ));
        self.displayed = None;
    }
    pub fn begin_placed(&mut self, context: FrameContext, request: PlacedPreviewRequest) {
        self.annotated = None;
        self.expected = Some(ExpectedPreview::Placed(context, request));
        self.displayed = None;
    }
    /// Bind presentation to the exact requested target and canonical admitted profile.
    pub fn begin_annotated(
        &mut self,
        context: FrameContext,
        time: ModelPackPreviewTime,
        placement: Option<PlacedPreviewRequest>,
        spec: AnnotatedPreviewSpec,
        model: AnnotatedPreviewModel,
        profile: DeclaredInferenceProfile,
    ) -> Result<Command> {
        self.clear();
        spec.validate_profile(&profile)?;
        time.validate(&context)?;
        match &model {
            AnnotatedPreviewModel::File {
                profile: declared, ..
            } => anyhow::ensure!(
                serde_json::to_vec(declared)? == serde_json::to_vec(&profile)?,
                "Annotated preview file profile differs"
            ),
            AnnotatedPreviewModel::Pack { choice } => choice.validate()?,
        }
        anyhow::ensure!(
            context.transform == annotated_preview_transform(&spec.view)?
                && context.source_version.to_string() == spec.view.source_sha256,
            "Annotated preview target differs from the selected source"
        );
        let command = Command::PreviewAnnotatedFrame {
            context: context.clone(),
            time: time.clone(),
            spec: spec.clone(),
            model,
        };
        match (time, placement) {
            (
                ModelPackPreviewTime::Source {
                    source_time,
                    time_basis,
                },
                None,
            ) => self.begin_timed(context, source_time, time_basis),
            (
                ModelPackPreviewTime::Placement {
                    placement_id,
                    project_time,
                },
                Some(request),
            ) => {
                anyhow::ensure!(
                    request.placement.placement_id == placement_id
                        && request.project_time == project_time,
                    "Annotated preview placement differs"
                );
                request.validate(&context)?;
                self.begin_placed(context, request);
            }
            _ => bail!("Annotated preview time and placement differ"),
        }
        self.annotated = Some((spec, profile));
        Ok(command)
    }
    pub fn clear(&mut self) {
        self.expected = None;
        self.annotated = None;
        self.displayed = None;
    }
    pub fn accept(&mut self, result: PreviewResult) -> bool {
        let binding_matches = match (&self.annotated, &result.annotated) {
            (None, None) => true,
            (Some((spec, profile)), Some(evidence)) => {
                evidence.validate(&result).is_ok()
                    && serde_json::to_value(spec).ok() == serde_json::to_value(&evidence.spec).ok()
                    && evidence.model == profile.model
                    && evidence.runtime == profile.runtime
                    && serde_json::to_vec(profile).is_ok_and(|bytes| {
                        evidence.profile.byte_len == bytes.len() as u64
                            && evidence.profile.sha256 == format!("{:x}", Sha256::digest(bytes))
                    })
            }
            _ => false,
        };
        if !binding_matches {
            return false;
        }
        if !self
            .expected
            .as_ref()
            .is_some_and(|expected| match expected {
                ExpectedPreview::Source(context, time, basis) => {
                    result.matches_timed_seek_request(context, time, *basis)
                }
                ExpectedPreview::Placed(context, request) => {
                    result.matches_placed_seek_request(context, request)
                }
            })
        {
            return false;
        }
        self.displayed = Some(result);
        true
    }
    pub fn displayed(&self) -> Option<&PreviewResult> {
        self.displayed.as_ref()
    }
}

pub(crate) struct DecodedFrame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Artifact paths come only from an authenticated engine response. Files are
/// not decoded as arbitrary image formats; their checked raw layout is the ABI.
pub(crate) fn decode_frame(frame: &FrameArtifact) -> Result<DecodedFrame> {
    decode_frame_inner(frame, || {})
}

fn decode_frame_inner(frame: &FrameArtifact, before_open: impl FnOnce()) -> Result<DecodedFrame> {
    frame.validate(MAX_PREVIEW_BYTES)?;
    let metadata = std::fs::symlink_metadata(&frame.artifact.path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != frame.artifact.identity.byte_len
    {
        bail!("Preview artifact changed or is not an immutable regular file");
    }
    before_open();
    // The name may change after inspection. Refuse a raced final symlink or
    // special file without waiting for a FIFO publisher on the desktop RPC path.
    #[cfg(unix)]
    let file = std::fs::File::from(rustix::fs::open(
        &frame.artifact.path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )?);
    #[cfg(not(unix))]
    let file = std::fs::File::open(&frame.artifact.path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() != frame.artifact.identity.byte_len {
        bail!("Opened preview artifact changed or is not a regular file");
    }
    let mut bytes = Vec::with_capacity(frame.artifact.identity.byte_len as usize);
    file.take(MAX_PREVIEW_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != frame.artifact.identity.byte_len
        || format!("{:x}", Sha256::digest(&bytes)) != frame.artifact.identity.sha256
    {
        bail!("Preview artifact identity mismatch");
    }
    let width = usize::try_from(frame.width)?;
    let height = usize::try_from(frame.height)?;
    let rgba_len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n as u64 <= MAX_PREVIEW_BYTES)
        .ok_or_else(|| anyhow::anyhow!("Preview RGBA budget exceeded"))?;
    let stride = usize::try_from(frame.stride_bytes)?;
    let channels = frame.format.channels() as usize;
    let mut rgba = Vec::with_capacity(rgba_len);
    for row in bytes.chunks_exact(stride).take(height) {
        for pixel in row[..width * channels].chunks_exact(channels) {
            match frame.format {
                PixelFormat::Rgb8 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
                PixelFormat::Rgba8 => rgba.extend_from_slice(pixel),
                PixelFormat::Gray8 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
            }
        }
    }
    Ok(DecodedFrame {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> FrameContext {
        FrameContext {
            source_version: SourceVersionId::new("source").unwrap(),
            source_placement: SourcePlacementId::new("placement").unwrap(),
            frame: FrameId::new(4),
            transform: TransformId::new("source-plane").unwrap(),
            seek_generation: 3,
            request_generation: 8,
        }
    }

    #[cfg(unix)]
    #[test]
    fn frame_replacement_symlink_between_check_and_open_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("frame.raw");
        let other = root.path().join("other.raw");
        let bytes = [1u8, 2, 3];
        std::fs::write(&path, bytes).unwrap();
        std::fs::write(&other, bytes).unwrap();
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                path: path.clone(),
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new("frame").unwrap(),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                    byte_len: 3,
                },
            },
            width: 1,
            height: 1,
            stride_bytes: 3,
            format: PixelFormat::Rgb8,
        };
        let result = decode_frame_inner(&frame, || {
            std::fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(&other, &path).unwrap();
        });
        assert!(
            result.is_err(),
            "a raced symlink must never supply preview bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn frame_replacement_fifo_between_check_and_open_needs_no_writer() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("frame.raw");
        let bytes = [1u8, 2, 3];
        std::fs::write(&path, bytes).unwrap();
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                path: path.clone(),
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new("frame").unwrap(),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                    byte_len: 3,
                },
            },
            width: 1,
            height: 1,
            stride_bytes: 3,
            format: PixelFormat::Rgb8,
        };
        let result = decode_frame_inner(&frame, || {
            std::fs::remove_file(&path).unwrap();
            rustix::fs::mkfifoat(
                rustix::fs::CWD,
                &path,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .unwrap();
        });
        assert!(
            result.is_err(),
            "a raced FIFO must be refused without a writer"
        );
    }

    fn empty_preview(context: FrameContext) -> PreviewResult {
        PreviewResult {
            annotated: None,
            time_basis: PreviewTimeBasis::SourcePts,
            elapsed_source_time: None,
            source_origin: None,
            placed: None,
            coordinate_map: None,
            context,
            source_time: SourceTimestamp::new(1, 2).unwrap(),
            requested_source_time: SourceTimestamp::new(1, 2).unwrap(),
            source_frame_index: 4,
            observations: vec![],
            frame: None,
            analysis: AnalysisStatus::Available,
        }
    }

    #[test]
    fn seek_source_transform_and_request_changes_reject_late_results() {
        let original = context();
        let mut altered = vec![];
        let mut seek = original.clone();
        seek.seek_generation += 1;
        altered.push(seek);
        let mut request = original.clone();
        request.request_generation += 1;
        altered.push(request);
        let mut transform = original.clone();
        transform.transform = TransformId::new("crop").unwrap();
        altered.push(transform);
        let mut source = original.clone();
        source.source_version = SourceVersionId::new("other-source").unwrap();
        altered.push(source);
        let mut placement = original.clone();
        placement.source_placement = SourcePlacementId::new("other-placement").unwrap();
        altered.push(placement);
        for current in altered {
            let mut state = PreviewState::default();
            state.begin(current, SourceTimestamp::new(1, 2).unwrap());
            assert!(!state.accept(empty_preview(original.clone())));
            assert!(state.displayed().is_none());
        }
    }

    #[test]
    fn absent_detections_clear_previous_observations() {
        let mut state = PreviewState::default();
        state.begin(context(), SourceTimestamp::new(1, 2).unwrap());
        let mut observed = empty_preview(context());
        observed.observations.push(
            DetectionObservation::new(
                context(),
                CoordinateBox::new(CoordinateSpace::ProjectionNormalized, 0.1, 0.2, 0.3, 0.4)
                    .unwrap(),
                None,
                EvidenceKind::Observed,
                Confidence::new(0.8, None).unwrap(),
            )
            .unwrap(),
        );
        assert!(state.accept(observed));
        assert_eq!(state.displayed().unwrap().observations.len(), 1);
        assert!(state.accept(empty_preview(context())));
        assert!(state.displayed().unwrap().observations.is_empty());
        let mut next = context();
        next.request_generation += 1;
        state.begin(next, SourceTimestamp::new(1, 2).unwrap());
        assert!(state.displayed().is_none());
    }

    #[test]
    fn moving_boxes_and_actual_decoder_frame_replace_previous_evidence() {
        let mut state = PreviewState::default();
        let mut request = context();
        request.frame = FrameId::new(0);
        state.begin(request, SourceTimestamp::new(1, 2).unwrap());
        let mut result = empty_preview(context());
        result.observations.push(
            DetectionObservation::new(
                context(),
                CoordinateBox::new(CoordinateSpace::ProjectionNormalized, 0.6, 0.2, 0.8, 0.4)
                    .unwrap(),
                None,
                EvidenceKind::Observed,
                Confidence::new(0.8, None).unwrap(),
            )
            .unwrap(),
        );
        assert!(state.accept(result));
        assert_eq!(state.displayed().unwrap().context.frame.value(), 4);
        assert_eq!(
            state.displayed().unwrap().observations[0].bounds().x_min(),
            0.6
        );
        let mut mismatched = empty_preview(context());
        let mut wrong_frame = context();
        wrong_frame.frame = FrameId::new(999);
        mismatched.observations.push(
            DetectionObservation::new(
                wrong_frame,
                CoordinateBox::new(CoordinateSpace::ProjectionNormalized, 0.1, 0.2, 0.3, 0.4)
                    .unwrap(),
                None,
                EvidenceKind::Observed,
                Confidence::new(0.8, None).unwrap(),
            )
            .unwrap(),
        );
        assert!(!state.accept(mismatched));
    }

    #[test]
    fn raw_artifact_hash_is_checked_before_display() {
        let path =
            std::env::temp_dir().join(format!("pulsar-preview-test-{}", uuid::Uuid::new_v4()));
        let bytes = [1u8, 2, 3];
        std::fs::write(&path, bytes).unwrap();
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                path: path.clone(),
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new("frame").unwrap(),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                    byte_len: 3,
                },
            },
            width: 1,
            height: 1,
            stride_bytes: 3,
            format: PixelFormat::Rgb8,
        };
        assert_eq!(decode_frame(&frame).unwrap().rgba, vec![1, 2, 3, 255]);
        std::fs::write(&path, [3u8, 2, 1]).unwrap();
        assert!(decode_frame(&frame).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn elapsed_seek_rejects_clock_changes_inconsistent_origin_and_future_frame() {
        let ctx = context();
        let wanted = SourceTimestamp::new(300, 1000).unwrap();
        let mut result = empty_preview(ctx.clone());
        result.time_basis = PreviewTimeBasis::ElapsedFromFirstFrame;
        result.requested_source_time = wanted;
        result.source_time = SourceTimestamp::new(5125, 1000).unwrap();
        result.source_origin = Some(SourceTimestamp::new(5, 1).unwrap());
        result.elapsed_source_time = Some(SourceTimestamp::new(1, 8).unwrap());
        let mut state = PreviewState::default();
        state.begin_timed(ctx.clone(), wanted, PreviewTimeBasis::ElapsedFromFirstFrame);
        assert!(state.accept(result.clone()));
        state.begin(ctx.clone(), wanted);
        assert!(!state.accept(result.clone()));
        state.begin_timed(ctx.clone(), wanted, PreviewTimeBasis::ElapsedFromFirstFrame);
        let mut wrong = result.clone();
        wrong.source_origin = Some(SourceTimestamp::new(6, 1).unwrap());
        assert!(!state.accept(wrong));
        let mut future = result.clone();
        future.source_time = SourceTimestamp::new(5500, 1000).unwrap();
        future.elapsed_source_time = Some(SourceTimestamp::new(1, 2).unwrap());
        assert!(!state.accept(future));
        result.elapsed_source_time = None;
        assert!(!state.accept(result));
    }

    #[test]
    fn placed_preview_rejects_replaced_mapping_and_corrupt_project_time_receipts() {
        let ctx = context();
        let spec = SourcePlacementSpec {
            source_version: ctx.source_version.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(2, 1).unwrap(),
            source_anchor: SourceTimestamp::new(1, 2).unwrap(),
            project_anchor: ProjectTime::from_nanos(5_000_000_000),
            rate_numerator: 1,
            rate_denominator: 1,
        };
        let request = PlacedPreviewRequest {
            placement: SourcePlacement {
                placement_id: ctx.source_placement.clone(),
                created_revision: RevisionId::new(1),
                spec,
            },
            project_time: ProjectTime::from_nanos(5_000_000_000),
        };
        let actual = ExactSeconds::new(5, 1).unwrap();
        let mut result = empty_preview(ctx.clone());
        result.time_basis = PreviewTimeBasis::ElapsedFromFirstFrame;
        result.source_origin = Some(SourceTimestamp::new(0, 1).unwrap());
        result.elapsed_source_time = Some(result.source_time);
        result.placed = Some(PlacedPreviewReceipt {
            request: request.clone(),
            actual_project_time: actual,
            quantization: actual.quantize_nanoseconds().unwrap(),
        });
        let mut state = PreviewState::default();
        state.begin_placed(ctx.clone(), request.clone());
        assert!(state.accept(result.clone()));
        assert!(result.matches_request(&ctx));
        let mut corrupt = result.clone();
        corrupt
            .placed
            .as_mut()
            .unwrap()
            .quantization
            .remainder_numerator = "1".into();
        assert!(!state.accept(corrupt.clone()));
        assert!(!corrupt.matches_request(&ctx));
        let mut altered = request;
        altered.placement.spec.project_anchor = ProjectTime::from_nanos(6_000_000_000);
        state.begin_placed(ctx, altered);
        assert!(!state.accept(result));
        assert!(state.displayed().is_none());
    }

    fn annotated_sample() -> (DeclaredInferenceProfile, PreviewResult) {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 12288,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: identity.clone(),
            runtime: identity.clone(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::AnnotatedTemporalView,
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
        let view:AnnotatedGenerationView=serde_json::from_value(serde_json::json!({"source_sha256":"a".repeat(64),"declaration":{"schema_version":1,"source_dimensions":[64,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"authored","video_view":{"layout":"mono","eye":"left","projection":{"kind":"flat"}}}})).unwrap();
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                identity: identity.clone(),
                path: crate::test_host_path("/authored/pixels.rgb").into(),
            },
            width: 64,
            height: 64,
            stride_bytes: 192,
            format: PixelFormat::Rgb8,
        };
        let mut result = empty_preview(context());
        result.context.source_version = SourceVersionId::new("a".repeat(64)).unwrap();
        result.context.transform = annotated_preview_transform(&view).unwrap();
        result.context.frame = FrameId::new(1);
        result.source_frame_index = 1;
        result.time_basis = PreviewTimeBasis::ElapsedFromFirstFrame;
        result.requested_source_time = SourceTimestamp::new(1, 8).unwrap();
        result.source_time = SourceTimestamp::new(17, 8).unwrap();
        result.elapsed_source_time = Some(SourceTimestamp::new(1, 8).unwrap());
        result.source_origin = Some(SourceTimestamp::new(2, 1).unwrap());
        result.frame = Some(frame.clone());
        let bytes = serde_json::to_vec(&profile).unwrap();
        result.annotated = Some(AnnotatedPreviewEvidence {
            spec: AnnotatedPreviewSpec {
                view,
                source_sample_stride: 1,
            },
            previous: Some(AnnotatedPreviewPrevious {
                source_frame_index: 0,
                source_time: SourceTimestamp::new(2, 1).unwrap(),
                elapsed_source_time: SourceTimestamp::new(0, 1).unwrap(),
                frame,
            }),
            position: Some(NormalizedPosition::new(0.25).unwrap()),
            model: identity.clone(),
            profile: ArtifactIdentity {
                artifact_id: ArtifactId::new(format!("{:x}", Sha256::digest(&bytes))).unwrap(),
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                byte_len: bytes.len() as u64,
            },
            runtime: identity,
            contract: AnnotatedTemporalModelContract::expected(),
            training_intake_sha256: "b".repeat(64),
            qualified: false,
        });
        (profile, result)
    }
    fn begin_sample(
        state: &mut PreviewState,
        profile: &DeclaredInferenceProfile,
        result: &PreviewResult,
    ) -> Result<Command> {
        state.begin_annotated(
            result.context.clone(),
            ModelPackPreviewTime::Source {
                source_time: result.requested_source_time,
                time_basis: result.time_basis,
            },
            None,
            result.annotated.as_ref().unwrap().spec.clone(),
            AnnotatedPreviewModel::File {
                model_path: crate::test_host_path("/authored/model.onnx").into(),
                profile: (profile.clone()).into(),
            },
            profile.clone(),
        )
    }
    #[test]
    fn annotated_presentation_binds_model_profile_runtime_stride_and_seek() {
        let (profile, result) = annotated_sample();
        let mut state = PreviewState::default();
        assert!(matches!(
            begin_sample(&mut state, &profile, &result).unwrap(),
            Command::PreviewAnnotatedFrame {
                model: AnnotatedPreviewModel::File { .. },
                ..
            }
        ));
        assert!(state.accept(result.clone()));
        for mutation in 0..7 {
            begin_sample(&mut state, &profile, &result).unwrap();
            let mut wrong = result.clone();
            match mutation {
                0 => wrong.annotated.as_mut().unwrap().model.sha256 = "c".repeat(64),
                1 => wrong.annotated.as_mut().unwrap().runtime.sha256 = "c".repeat(64),
                2 => wrong.annotated.as_mut().unwrap().profile.sha256 = "c".repeat(64),
                3 => wrong.annotated.as_mut().unwrap().spec.source_sample_stride = 2,
                4 => wrong.context.seek_generation += 1,
                5 => wrong.annotated = None,
                _ => wrong.annotated.as_mut().unwrap().qualified = true,
            }
            assert!(!state.accept(wrong), "mutation {mutation}");
            assert!(state.displayed().is_none());
        }
        state.begin_timed(
            result.context.clone(),
            result.requested_source_time,
            result.time_basis,
        );
        assert!(!state.accept(result.clone()));
        begin_sample(&mut state, &profile, &result).unwrap();
        assert!(state.accept(result.clone()));
        let mut invalid = result;
        invalid.context.source_version = SourceVersionId::new("c".repeat(64)).unwrap();
        assert!(begin_sample(&mut state, &profile, &invalid).is_err());
        assert!(state.displayed().is_none());
    }
    #[test]
    fn annotated_absence_replaces_estimate_without_inventing_position() {
        let (profile, result) = annotated_sample();
        let mut state = PreviewState::default();
        begin_sample(&mut state, &profile, &result).unwrap();
        assert!(state.accept(result.clone()));
        let mut gap = result;
        gap.annotated.as_mut().unwrap().position = None;
        gap.analysis = AnalysisStatus::Unavailable {
            reason: "temporal_pair_without_continuous_support".into(),
        };
        assert!(state.accept(gap));
        assert!(state
            .displayed()
            .unwrap()
            .annotated
            .as_ref()
            .unwrap()
            .position
            .is_none());
        state.clear();
        assert!(state.displayed().is_none());
    }
    #[test]
    fn annotated_request_retains_pack_choice_and_refuses_conflicting_file_profile() {
        let (profile, result) = annotated_sample();
        let mut state = PreviewState::default();
        let choice = ModelPackChoice {
            manifest_sha256: "d".repeat(64),
            selection_revision: 9,
        };
        let time = ModelPackPreviewTime::Source {
            source_time: result.requested_source_time,
            time_basis: result.time_basis,
        };
        let command = state
            .begin_annotated(
                result.context.clone(),
                time.clone(),
                None,
                result.annotated.as_ref().unwrap().spec.clone(),
                AnnotatedPreviewModel::Pack {
                    choice: choice.clone(),
                },
                profile.clone(),
            )
            .unwrap();
        assert!(
            matches!(command,Command::PreviewAnnotatedFrame {model:AnnotatedPreviewModel::Pack {choice:retained},..} if retained==choice)
        );
        assert!(state.accept(result.clone()));
        let mut changed = profile.clone();
        changed.cpu_threads += 1;
        assert!(state
            .begin_annotated(
                result.context.clone(),
                time,
                None,
                result.annotated.as_ref().unwrap().spec.clone(),
                AnnotatedPreviewModel::File {
                    model_path: crate::test_host_path("/authored/model.onnx").into(),
                    profile: Box::new(changed)
                },
                profile
            )
            .is_err());
        assert!(state.displayed().is_none());
    }
    #[test]
    #[ignore = "requires retained authored native preview campaign via PULSAR_ANNOTATED_PREVIEW_REPLAY"]
    fn authored_native_annotated_preview_presentation() {
        let root = std::path::PathBuf::from(
            std::env::var_os("PULSAR_ANNOTATED_PREVIEW_REPLAY").expect("native fixture path"),
        );
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
        let profile: DeclaredInferenceProfile =
            serde_json::from_slice(&std::fs::read(root.join("canonical-profile.json")).unwrap())
                .unwrap();
        let mut results = fixture["previews"].as_array().unwrap().clone();
        results.extend(
            fixture["placedPreviews"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        for name in ["first", "gap", "cut", "restarted", "source_pts_preview"] {
            results.push(fixture[name].clone());
        }
        for value in results {
            let result: PreviewResult = serde_json::from_value(value).unwrap();
            let mut state = PreviewState::default();
            let placed = result.placed.as_ref().map(|p| p.request.clone());
            let time = if let Some(p) = &placed {
                ModelPackPreviewTime::Placement {
                    placement_id: p.placement.placement_id.clone(),
                    project_time: p.project_time,
                }
            } else {
                ModelPackPreviewTime::Source {
                    source_time: result.requested_source_time,
                    time_basis: result.time_basis,
                }
            };
            state
                .begin_annotated(
                    result.context.clone(),
                    time,
                    placed,
                    result.annotated.as_ref().unwrap().spec.clone(),
                    AnnotatedPreviewModel::File {
                        model_path: root.join("authored-annotated-pack/model.onnx"),
                        profile: (profile.clone()).into(),
                    },
                    profile.clone(),
                )
                .unwrap();
            assert!(state.accept(result.clone()));
            let current = decode_frame(result.frame.as_ref().unwrap()).unwrap();
            assert_eq!((current.width, current.height), (64, 64));
            if let Some(p) = &result.annotated.as_ref().unwrap().previous {
                let previous = decode_frame(&p.frame).unwrap();
                assert_eq!((previous.width, previous.height), (64, 64));
            }
        }
        // The desktop request builder and SessionClient also execute a fresh
        // confined Preview against the owned fixture's restarted Engine.
        use crate::session::{EngineApi, SessionClient};
        use std::{
            process::{Command as ProcessCommand, Stdio},
            time::{Duration, Instant},
        };
        struct OwnedEngine(std::process::Child);
        impl Drop for OwnedEngine {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let binary = fixture["binary"]["path"].as_str().unwrap();
        let mut executable = std::fs::File::open(binary).unwrap();
        let mut hash = Sha256::new();
        let mut chunk = [0u8; 65536];
        loop {
            let count = executable.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&chunk[..count]);
        }
        assert_eq!(
            format!("{:x}", hash.finalize()),
            fixture["binary"]["sha256"].as_str().unwrap(),
            "campaign executable changed"
        );
        let state_dir = root.join("engine");
        let log = std::fs::File::create(root.join("desktop-controller-engine.log")).unwrap();
        let mut engine = OwnedEngine(
            ProcessCommand::new(binary)
                .arg("engine")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PULSAR_STATE_DIR", &state_dir)
                .env(
                    "PULSAR_ORT_DYLIB",
                    std::env::var_os("PULSAR_ORT_DYLIB").expect("pinned runtime"),
                )
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        let endpoint = state_dir.join("engine.sock");
        let deadline = Instant::now() + Duration::from_secs(15);
        while LocalClient::connect(&endpoint, Duration::from_millis(100)).is_err() {
            assert!(
                engine.0.try_wait().unwrap().is_none(),
                "owned Engine exited"
            );
            assert!(Instant::now() < deadline, "owned Engine startup timeout");
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut client =
            SessionClient::connect(&endpoint, &state_dir.join("pairing.token")).unwrap();
        let project = ProjectId::new(fixture["retained"]["job"][1].as_str().unwrap()).unwrap();
        let mut observed = Vec::new();
        for value in fixture["previews"].as_array().unwrap().iter().chain([
            &fixture["first"],
            &fixture["gap"],
            &fixture["restarted"],
            &fixture["source_pts_preview"],
        ]) {
            let previous: PreviewResult = serde_json::from_value(value.clone()).unwrap();
            let mut state = PreviewState::default();
            let command = state
                .begin_annotated(
                    previous.context.clone(),
                    ModelPackPreviewTime::Source {
                        source_time: previous.requested_source_time,
                        time_basis: previous.time_basis,
                    },
                    None,
                    previous.annotated.as_ref().unwrap().spec.clone(),
                    AnnotatedPreviewModel::File {
                        model_path: root.join("authored-annotated-pack/model.onnx"),
                        profile: (profile.clone()).into(),
                    },
                    profile.clone(),
                )
                .unwrap();
            let crate::motion::ResponseBody::Preview(result) = client
                .execute(command, Some(project.clone()), None)
                .unwrap()
            else {
                panic!("annotated response kind differs")
            };
            assert_eq!(
                result.annotated.as_ref().unwrap().position,
                previous.annotated.as_ref().unwrap().position
            );
            assert_eq!(
                decode_frame(result.frame.as_ref().unwrap()).unwrap().rgba,
                decode_frame(previous.frame.as_ref().unwrap()).unwrap().rgba
            );
            assert!(state.accept(*result.clone()));
            observed.push(result);
        }
        std::fs::write(
            root.join("desktop-controller-results.json"),
            serde_json::to_vec_pretty(&observed).unwrap(),
        )
        .unwrap();
    }
}
