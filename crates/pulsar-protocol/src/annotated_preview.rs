//! Source-bound temporal preview evidence; never detector observations or qualification.
use crate::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotatedPreviewSpec {
    pub view: AnnotatedGenerationView,
    pub source_sample_stride: u32,
}
impl AnnotatedPreviewSpec {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.view.validate()?;
        if self.source_sample_stride == 0 {
            return Err(ProtocolError::invalid(
                "annotated preview stride must be nonzero",
            ));
        }
        Ok(())
    }
    pub fn validate_profile(
        &self,
        profile: &DeclaredInferenceProfile,
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        if !matches!(
            profile.provider,
            InferenceProvider::Cpu | InferenceProvider::Cuda
        ) || !matches!(
            profile.preprocessing.resize,
            InferenceResize::AnnotatedTemporalView
        ) {
            return Err(ProtocolError::unsupported(
                "annotated preview requires an explicit annotated CPU or CUDA profile",
            ));
        }
        GenerationSettings {
            annotated_view: Some(self.view.clone()),
            source_sample_stride: self.source_sample_stride,
            inference_profile: Some(profile.clone()),
            model_input: Some(profile.preprocessing.input.clone()),
            analysis_width: 64,
            analysis_height: 64,
            ..Default::default()
        }
        .validate()
    }
}
pub fn annotated_preview_transform(
    view: &AnnotatedGenerationView,
) -> Result<TransformId, ProtocolError> {
    view.validate()?;
    let bytes = serde_json::to_vec(&(ANNOTATED_TEMPORAL_RECIPE, view))
        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
    TransformId::new(format!("annotated-preview-v1.{:x}", Sha256::digest(bytes)))
        .map_err(|e| ProtocolError::invalid(e.to_string()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnnotatedPreviewModel {
    File {
        model_path: PathBuf,
        profile: Box<DeclaredInferenceProfile>,
    },
    Pack {
        choice: ModelPackChoice,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotatedPreviewPrevious {
    pub source_frame_index: u64,
    pub source_time: SourceTimestamp,
    pub elapsed_source_time: SourceTimestamp,
    pub frame: FrameArtifact,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotatedPreviewEvidence {
    pub spec: AnnotatedPreviewSpec,
    pub previous: Option<AnnotatedPreviewPrevious>,
    pub position: Option<NormalizedPosition>,
    pub model: ArtifactIdentity,
    pub profile: ArtifactIdentity,
    pub runtime: ArtifactIdentity,
    pub contract: AnnotatedTemporalModelContract,
    pub training_intake_sha256: String,
    pub qualified: bool,
}
fn target_frame(frame: &FrameArtifact) -> Result<(), ProtocolError> {
    frame.validate(64 * 64 * 3)?;
    if frame.width != 64
        || frame.height != 64
        || frame.stride_bytes != 192
        || frame.format != PixelFormat::Rgb8
    {
        return Err(ProtocolError::invalid(
            "annotated preview requires exact 64x64 RGB target pixels",
        ));
    }
    Ok(())
}
fn clock_matches(time: SourceTimestamp, origin: SourceTimestamp, elapsed: SourceTimestamp) -> bool {
    let difference = i128::from(time.numerator()) * i128::from(origin.denominator())
        - i128::from(origin.numerator()) * i128::from(time.denominator());
    matches!((difference.checked_mul(i128::from(elapsed.denominator())),i128::from(elapsed.numerator()).checked_mul(i128::from(origin.denominator())).and_then(|n|n.checked_mul(i128::from(time.denominator())))),(Some(left),Some(right)) if left==right)
}
impl AnnotatedPreviewEvidence {
    pub fn validate(&self, preview: &PreviewResult) -> Result<(), ProtocolError> {
        self.spec.validate()?;
        self.model.validate()?;
        self.profile.validate()?;
        self.runtime.validate()?;
        validate_sha256(&self.training_intake_sha256)?;
        if self.qualified
            || self.contract != AnnotatedTemporalModelContract::expected()
            || preview.context.transform != annotated_preview_transform(&self.spec.view)?
            || !preview
                .source_frame_index
                .is_multiple_of(u64::from(self.spec.source_sample_stride))
            || !preview.observations.is_empty()
            || preview.coordinate_map.is_some()
        {
            return Err(ProtocolError::invalid(
                "annotated preview binding or evidence differs",
            ));
        }
        target_frame(
            preview
                .frame
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("annotated target pixels absent"))?,
        )?;
        let origin = preview
            .source_origin
            .ok_or_else(|| ProtocolError::invalid("annotated source origin absent"))?;
        if let Some(previous) = &self.previous {
            target_frame(&previous.frame)?;
            if previous
                .source_frame_index
                .checked_add(u64::from(self.spec.source_sample_stride))
                != Some(preview.source_frame_index)
                || previous.elapsed_source_time.numerator() < 0
                || !clock_matches(previous.source_time, origin, previous.elapsed_source_time)
            {
                return Err(ProtocolError::invalid(
                    "annotated preview predecessor differs",
                ));
            }
        } else if preview.source_frame_index != 0 {
            return Err(ProtocolError::invalid(
                "annotated preview predecessor absent",
            ));
        }
        let elapsed = preview
            .elapsed_source_time
            .ok_or_else(|| ProtocolError::invalid("annotated elapsed clock absent"))?;
        if (preview.source_frame_index == 0 && elapsed.numerator() != 0)
            || self.previous.as_ref().is_some_and(|p| {
                p.source_frame_index == 0 && p.elapsed_source_time.numerator() != 0
            })
            || elapsed.numerator() < 0
            || !clock_matches(preview.source_time, origin, elapsed)
        {
            return Err(ProtocolError::invalid("annotated current clock differs"));
        }
        if self.position.is_some() {
            let supported = self.previous.as_ref().is_some_and(|p| {
                let delta = elapsed.quantize_nanoseconds().and_then(|q| {
                    q.time
                        .checked_sub(p.elapsed_source_time.quantize_nanoseconds()?.time)
                });
                matches!(delta, Ok(1..=500_000_000))
            });
            if !supported || !matches!(preview.analysis, AnalysisStatus::Available) {
                return Err(ProtocolError::invalid(
                    "annotated position has no supported frame pair",
                ));
            }
        } else {
            let expected = if self.previous.is_none() {
                "no_previous_sample"
            } else {
                "temporal_pair_without_continuous_support"
            };
            if !matches!(&preview.analysis,AnalysisStatus::Unavailable{reason} if reason==expected)
            {
                return Err(ProtocolError::invalid(
                    "absent annotated estimate must retain its support reason",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> PreviewResult {
        let view:AnnotatedGenerationView=serde_json::from_value(serde_json::json!({"source_sha256":"a".repeat(64),"declaration":{"schema_version":1,"source_dimensions":[64,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"authored","video_view":{"layout":"mono","eye":"left","projection":{"kind":"flat"}}}})).unwrap();
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 12288,
        };
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
        PreviewResult {
            annotated: Some(AnnotatedPreviewEvidence {
                spec: AnnotatedPreviewSpec {
                    view: view.clone(),
                    source_sample_stride: 1,
                },
                previous: Some(AnnotatedPreviewPrevious {
                    source_frame_index: 0,
                    source_time: SourceTimestamp::new(2, 1).unwrap(),
                    elapsed_source_time: SourceTimestamp::new(0, 1).unwrap(),
                    frame: frame.clone(),
                }),
                position: Some(NormalizedPosition::new(0.25).unwrap()),
                model: identity.clone(),
                profile: identity.clone(),
                runtime: identity,
                contract: AnnotatedTemporalModelContract::expected(),
                training_intake_sha256: "b".repeat(64),
                qualified: false,
            }),
            placed: None,
            time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
            elapsed_source_time: Some(SourceTimestamp::new(1, 8).unwrap()),
            source_origin: Some(SourceTimestamp::new(2, 1).unwrap()),
            coordinate_map: None,
            context: FrameContext {
                source_version: SourceVersionId::new("source").unwrap(),
                source_placement: SourcePlacementId::new("primary").unwrap(),
                frame: FrameId::new(1),
                transform: annotated_preview_transform(&view).unwrap(),
                seek_generation: 7,
                request_generation: 11,
            },
            requested_source_time: SourceTimestamp::new(1, 8).unwrap(),
            source_time: SourceTimestamp::new(17, 8).unwrap(),
            source_frame_index: 1,
            observations: vec![],
            frame: Some(frame),
            analysis: AnalysisStatus::Available,
        }
    }
    #[test]
    fn annotated_preview_requires_explicit_cpu_or_version2_cuda_recipe() {
        let preview = sample();
        let spec = &preview.annotated.as_ref().unwrap().spec;
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            class_count: 1,
            layout: TensorLayout::Nchw,
            decoder: DetectorDecoder::TemporalScriptPosition,
        };
        let identity = preview.annotated.as_ref().unwrap().model.clone();
        let mut profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: identity.clone(),
            runtime: identity,
            preprocessing: InferencePreprocessing {
                input,
                resize: InferenceResize::AnnotatedTemporalView,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0 / 255.0; 6],
                bias: vec![0.0; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 512 * 1024 * 1024,
            minimum_output_bytes: 65536,
            cpu_threads: 2,
            cuda_resources: None,
        };
        spec.validate_profile(&profile).unwrap();
        profile.provider = InferenceProvider::Cuda;
        assert!(spec.validate_profile(&profile).is_err());
        profile.schema_version = 2;
        profile.cuda_resources = Some(CudaResourceLimits {
            device_ordinal: 0,
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            address_space_bytes: 16 * 1024 * 1024 * 1024,
            device_reservation_bytes: 64 * 1024 * 1024,
            arena_limit_bytes: 64 * 1024 * 1024,
        });
        spec.validate_profile(&profile).unwrap();
        for provider in [
            InferenceProvider::TensorRt,
            InferenceProvider::Rocm,
            InferenceProvider::CoreMl,
            InferenceProvider::DirectMl,
            InferenceProvider::OpenVino,
        ] {
            profile.provider = provider;
            assert!(spec.validate_profile(&profile).is_err());
        }
        profile.provider = InferenceProvider::Cuda;
        profile.preprocessing.resize = InferenceResize::NearestStretch;
        assert!(spec.validate_profile(&profile).is_err());
    }
    #[test]
    fn binding_pair_clocks_and_qualification_cannot_be_substituted() {
        let p = sample();
        assert!(p.matches_request(&p.context));
        for mutation in 0..8 {
            let mut p = sample();
            let e = p.annotated.as_mut().unwrap();
            match mutation {
                0 => e.spec.view.declaration.target_id = "changed".into(),
                1 => e.previous.as_mut().unwrap().source_frame_index = 1,
                2 => e.previous.as_mut().unwrap().source_time = SourceTimestamp::new(1, 1).unwrap(),
                3 => e.previous = None,
                4 => e.qualified = true,
                5 => e.spec.source_sample_stride = 0,
                6 => {
                    p.source_time = SourceTimestamp::new(3, 1).unwrap();
                    p.elapsed_source_time = Some(SourceTimestamp::new(1, 1).unwrap());
                }
                _ => p.annotated = None,
            }
            assert!(!p.matches_request(&p.context), "{mutation}");
        }
    }
    #[test]
    fn unavailable_pair_cannot_be_presented_as_inference() {
        let mut p = sample();
        p.annotated.as_mut().unwrap().position = None;
        assert!(!p.matches_request(&p.context));
        p.analysis = AnalysisStatus::Unavailable {
            reason: "temporal_pair_without_continuous_support".into(),
        };
        assert!(p.matches_request(&p.context));
        p.annotated.as_mut().unwrap().previous = None;
        p.source_frame_index = 0;
        p.context.frame = FrameId::new(0);
        p.source_time = SourceTimestamp::new(2, 1).unwrap();
        p.elapsed_source_time = Some(SourceTimestamp::new(0, 1).unwrap());
        p.analysis = AnalysisStatus::Unavailable {
            reason: "no_previous_sample".into(),
        };
        assert!(p.matches_request(&p.context));
    }
    #[test]
    fn annotated_pack_wrapper_must_match_inner_model_and_profile() {
        let preview = sample();
        let evidence = preview.annotated.as_ref().unwrap();
        let mut report = ModelPackPreviewResult {
            choice: ModelPackChoice {
                manifest_sha256: "c".repeat(64),
                selection_revision: 1,
            },
            model: evidence.model.clone(),
            profile_sha256: evidence.profile.sha256.clone(),
            preview,
            qualified: false,
        };
        report.validate().unwrap();
        report.profile_sha256 = "b".repeat(64);
        assert!(report.validate().is_err());
        report.profile_sha256 = "a".repeat(64);
        report.model.sha256 = "b".repeat(64);
        assert!(report.validate().is_err());
    }
}
