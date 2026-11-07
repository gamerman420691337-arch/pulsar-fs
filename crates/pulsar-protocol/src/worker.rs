use crate::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIdentity {
    pub artifact_id: ArtifactId,
    pub sha256: String,
    pub byte_len: u64,
}

impl ArtifactIdentity {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ProtocolError::invalid(
                "artifact SHA-256 must be 64 lowercase hexadecimal bytes",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReference {
    pub identity: ArtifactIdentity,
    pub path: PathBuf,
}

impl ArtifactReference {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.identity.validate()?;
        if !self.path.is_absolute() {
            return Err(ProtocolError::invalid("artifact path must be absolute"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceArtifact {
    pub source_version: SourceVersionId,
    pub path: PathBuf,
    pub identity: ArtifactIdentity,
}

impl SourceArtifact {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.identity.validate()?;
        if !self.path.is_absolute() {
            return Err(ProtocolError::invalid(
                "source snapshot path must be absolute",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceBudget {
    pub memory_bytes: u64,
    pub output_bytes: u64,
    pub wall_time_ms: u64,
    pub cpu_threads: u16,
}

impl ResourceBudget {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.memory_bytes == 0
            || self.output_bytes == 0
            || self.wall_time_ms == 0
            || self.cpu_threads == 0
        {
            return Err(ProtocolError::invalid(
                "worker budgets must be finite positive limits",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerTools {
    pub ffmpeg: ArtifactReference,
    pub ffprobe: ArtifactReference,
    pub onnx_runtime: Option<ArtifactReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda_execution: Option<WorkerCudaExecution>,
    pub version: u16,
    pub attempt_id: AttemptId,
    pub job_id: JobId,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub source: SourceArtifact,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent_audio: Option<SourceArtifact>,
    pub output_dir: PathBuf,
    pub operation: WorkerOperation,
    pub budget: ResourceBudget,
    pub dependencies: Vec<ArtifactIdentity>,
    pub tools: WorkerTools,
}

impl WorkerRequest {
    /// These checks do not verify bytes, authorize filesystem paths, enforce
    /// operating-system resource limits, or attest to worker confinement.
    /// The engine and worker both enforce those effectful obligations.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("worker protocol version"));
        }
        self.source.validate()?;
        if let WorkerOperation::Preview {
            still_image: true,
            context,
            source_time,
            time_basis,
            model,
            model_input,
            inference_profile,
            video_view,
            annotated,
            placement_seek,
            max_width,
            max_height,
        } = &self.operation
        {
            if model.is_some()
                || model_input.is_some()
                || inference_profile.is_some()
                || video_view.is_some()
                || annotated.is_some()
                || placement_seek.is_some()
                || *max_width != 512
                || *max_height != 512
                || context.frame != FrameId::new(0)
                || *time_basis != PreviewTimeBasis::ElapsedFromFirstFrame
                || source_time.numerator() != 0
            {
                return Err(ProtocolError::invalid("still-image preparation requires bounded unprojected first-frame pixels without inference"));
            }
        }
        let declared_audio = match &self.operation {
            WorkerOperation::Generate { settings, .. } => settings
                .mixed_audio
                .as_ref()
                .and_then(|audio| audio.independent.as_ref()),
            _ => None,
        };
        match (&self.independent_audio, declared_audio) {
            (None, None) => {}
            (Some(source), Some(declaration)) => {
                source.validate()?;
                if source.source_version != declaration.source_version
                    || source.source_version == self.source.source_version
                    || !self.dependencies.contains(&source.identity)
                {
                    return Err(ProtocolError::invalid(
                        "independent audio must match a distinct captured source dependency",
                    ));
                }
            }
            _ => {
                return Err(ProtocolError::invalid(
                    "independent audio declaration and captured source must agree",
                ))
            }
        }
        self.budget.validate()?;
        if !self.output_dir.is_absolute() {
            return Err(ProtocolError::invalid(
                "worker output directory must be absolute",
            ));
        }
        self.tools.ffmpeg.validate()?;
        self.tools.ffprobe.validate()?;
        if let Some(runtime) = &self.tools.onnx_runtime {
            runtime.validate()?;
        }
        if self.dependencies.len() > 128 {
            return Err(ProtocolError::invalid("too many execution dependencies"));
        }
        for dependency in &self.dependencies {
            dependency.validate()?;
        }
        match &self.operation {
            WorkerOperation::ProbeMedia => {
                if self.independent_audio.is_some()
                    || self.cuda_execution.is_some()
                    || self.tools.onnx_runtime.is_some()
                {
                    return Err(ProtocolError::invalid(
                        "metadata probe cannot carry inference or audio alignment",
                    ));
                }
            }
            WorkerOperation::Generate {
                preset,
                settings,
                model,
            } => {
                bounded_text(preset, 64, "preset")?;
                settings.validate()?;
                if settings
                    .annotated_view
                    .as_ref()
                    .is_some_and(|view| view.source_sha256 != self.source.identity.sha256)
                {
                    return Err(ProtocolError::invalid(
                        "annotated view differs from captured source content",
                    ));
                }
                if settings.source_placement.as_ref().is_some_and(|p| {
                    p.spec.source_version != self.source.source_version
                        || p.created_revision > self.base_revision
                }) {
                    return Err(ProtocolError::invalid(
                        "generation placement differs from source or base revision",
                    ));
                }
                if let Some(model) = model {
                    model.validate()?;
                }
                if model.is_some() != settings.model_input.is_some() {
                    return Err(ProtocolError::invalid(
                        "model and input contract must be supplied together",
                    ));
                }
            }
            WorkerOperation::GenerateInput { input } => {
                input.validate()?;
                if input
                    .source_version()
                    .is_some_and(|version| version != &self.source.source_version)
                {
                    return Err(ProtocolError::invalid(
                        "modality input does not match its immutable source snapshot",
                    ));
                }
            }
            WorkerOperation::Preview {
                annotated,
                context,
                video_view,
                source_time,
                time_basis,
                placement_seek,
                max_width,
                max_height,
                model,
                model_input,
                inference_profile,
                ..
            } => {
                if let Some(placed) = placement_seek {
                    placed.validate(context)?;
                    if placed.placement.created_revision > self.base_revision {
                        return Err(ProtocolError::invalid(
                            "placement definition is newer than preview revision",
                        ));
                    }
                    if *time_basis != PreviewTimeBasis::ElapsedFromFirstFrame
                        || *source_time != SourceTimestamp::new(0, 1).expect("constant")
                    {
                        return Err(ProtocolError::invalid(
                            "placed preview has ambiguous legacy seek fields",
                        ));
                    }
                }
                if *time_basis == PreviewTimeBasis::ElapsedFromFirstFrame
                    && source_time.numerator() < 0
                {
                    return Err(ProtocolError::invalid(
                        "elapsed preview time must be nonnegative",
                    ));
                }
                validate_dimensions(*max_width, *max_height)?;
                if let Some(spec) = annotated {
                    spec.validate_profile(inference_profile.as_ref().ok_or_else(|| {
                        ProtocolError::invalid("annotated preview profile absent")
                    })?)?;
                    if model.is_none()
                        || spec.view.source_sha256 != self.source.identity.sha256
                        || video_view.is_some()
                        || context.transform != annotated_preview_transform(&spec.view)?
                    {
                        return Err(ProtocolError::invalid(
                            "annotated preview model/source/view binding differs",
                        ));
                    }
                } else if context.transform != video_view_transform(video_view.as_ref())? {
                    return Err(ProtocolError::invalid(
                        "preview context transform differs from declared video view",
                    ));
                }
                if let Some(model) = model {
                    model.validate()?;
                }
                if let Some(input) = model_input {
                    input.validate()?;
                }
                if let Some(profile) = inference_profile {
                    profile.validate()?;
                    if model.is_none() {
                        return Err(ProtocolError::invalid("inference profile requires a model"));
                    }
                }
                if model.is_some() != model_input.is_some() {
                    return Err(ProtocolError::invalid(
                        "preview model and input contract must be supplied together",
                    ));
                }
            }
        }
        let (profile, model, input) = match &self.operation {
            WorkerOperation::Generate {
                settings, model, ..
            } => (
                settings.inference_profile.as_ref(),
                model.as_ref(),
                settings.model_input.as_ref(),
            ),
            WorkerOperation::Preview {
                inference_profile,
                model,
                model_input,
                ..
            } => (
                inference_profile.as_deref(),
                model.as_ref(),
                model_input.as_ref(),
            ),
            WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => {
                (None, None, None)
            }
        };
        match (&self.cuda_execution, profile) {
            (Some(execution), Some(profile)) if profile.provider == InferenceProvider::Cuda => {
                execution.validate(profile, &self.budget, &self.dependencies)?;
                if model.is_none_or(|m| m.identity != profile.model)
                    || self
                        .tools
                        .onnx_runtime
                        .as_ref()
                        .is_none_or(|r| r.identity != profile.runtime)
                    || !self.dependencies.contains(&profile.model)
                    || input.is_none_or(|input| {
                        serde_json::to_value(input).ok()
                            != serde_json::to_value(&profile.preprocessing.input).ok()
                    })
                {
                    return Err(ProtocolError::invalid(
                        "CUDA worker model/runtime binding differs",
                    ));
                }
            }
            (None, Some(profile)) if profile.provider == InferenceProvider::Cuda => {
                return Err(ProtocolError::invalid(
                    "CUDA worker requires captured execution declaration",
                ))
            }
            (None, _) => {}
            (Some(_), _) => {
                return Err(ProtocolError::invalid(
                    "CUDA execution requires an explicit CUDA profile",
                ))
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkerOperation {
    ProbeMedia,
    Generate {
        preset: String,
        settings: Box<GenerationSettings>,
        model: Option<SourceArtifact>,
    },
    GenerateInput {
        input: GenerationInput,
    },
    Preview {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        still_image: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        annotated: Option<Box<AnnotatedPreviewSpec>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement_seek: Option<Box<PlacedPreviewRequest>>,
        #[serde(default, skip_serializing_if = "PreviewTimeBasis::is_source_pts")]
        time_basis: PreviewTimeBasis,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        video_view: Option<VideoView>,
        context: FrameContext,
        source_time: SourceTimestamp,
        max_width: u32,
        max_height: u32,
        model: Option<SourceArtifact>,
        model_input: Option<ModelInputContract>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inference_profile: Option<Box<DeclaredInferenceProfile>>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationSettings {
    /// Optional minimum uncalibrated detector score. Missing legacy requests use 0.25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detector_score_threshold: Option<f32>,
    /// Explicit native view recipe for indexed file temporal inference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotated_view: Option<AnnotatedGenerationView>,
    /// Half-open project-time coverage; clocks and the source stride stay unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_interval: Option<TimeRange>,
    /// A point in the decoded analysis view at one exact original source ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_seed: Option<AnalysisTargetSeed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_frame_pair: Option<RawFramePair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixed_audio: Option<MixedAudioSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_placement: Option<SourcePlacement>,
    pub analysis_width: u32,
    pub analysis_height: u32,
    pub source_sample_stride: u32,
    pub axis: Axis,
    pub vr_mode: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_view: Option<VideoView>,
    pub model_input: Option<ModelInputContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference_profile: Option<DeclaredInferenceProfile>,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            detector_score_threshold: None,
            annotated_view: None,
            project_interval: None,
            target_seed: None,
            raw_frame_pair: None,
            mixed_audio: None,
            source_placement: None,
            analysis_width: 320,
            analysis_height: 180,
            source_sample_stride: 1,
            axis: Axis::Stroke,
            vr_mode: false,
            video_view: None,
            model_input: None,
            inference_profile: None,
        }
    }
}

#[cfg(test)]
mod detector_score_settings_tests {
    use super::*;
    #[test]
    fn legacy_serialization_and_checked_detector_score_roundtrip() {
        let legacy = GenerationSettings::default();
        let old = serde_json::to_value(&legacy).unwrap();
        assert!(old.get("detector_score_threshold").is_none());
        let restored: GenerationSettings = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(restored.effective_detector_score_threshold(), 0.25);
        assert_eq!(serde_json::to_value(restored).unwrap(), old);
        let mut settings = GenerationSettings {
            model_input: Some(ModelInputContract {
                width: 64,
                height: 64,
                channels: 3,
                class_count: 1,
                layout: TensorLayout::Nchw,
                decoder: DetectorDecoder::YoloChannelMajor,
            }),
            ..Default::default()
        };
        for score in [0.0, 0.25, 0.5, 1.0] {
            settings.detector_score_threshold = Some(score);
            settings.validate().unwrap();
            let roundtrip: GenerationSettings =
                serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
            assert_eq!(roundtrip.effective_detector_score_threshold(), score);
        }
        for score in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
            settings.detector_score_threshold = Some(score);
            assert!(settings.validate().is_err());
        }
        settings.detector_score_threshold = Some(0.5);
        settings.model_input.as_mut().unwrap().decoder = DetectorDecoder::TemporalScriptPosition;
        assert!(settings
            .validate()
            .unwrap_err()
            .to_string()
            .contains("declared detector"));
        settings.model_input = None;
        assert!(settings.validate().is_err());
    }
}

impl GenerationSettings {
    /// Executable visual recipe support, separate from archival structural validity.
    /// Economy remains a stored-worker alias; public authority admission controls new names.
    pub fn validate_execution_preset(
        &self,
        preset: &str,
        has_model: bool,
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        if !matches!(
            preset,
            "fast" | "default" | "economy" | "maximum" | "quality"
        ) {
            return Err(ProtocolError::unsupported("generation preset"));
        }
        if self.axis != Axis::Stroke {
            return Err(ProtocolError::unsupported(
                "generation projection or non-stroke recipe",
            ));
        }
        if has_model != self.model_input.is_some() {
            return Err(ProtocolError::invalid(
                "model and input contract must be supplied together",
            ));
        }
        if matches!(preset, "maximum" | "quality")
            && !self
                .model_input
                .as_ref()
                .is_some_and(|input| input.decoder != DetectorDecoder::TemporalScriptPosition)
        {
            return Err(ProtocolError::unsupported("Maximum requires a declared visual detector model; temporal and model-free Maximum are not implemented"));
        }
        Ok(())
    }

    pub fn effective_detector_score_threshold(&self) -> f32 {
        self.detector_score_threshold.unwrap_or(0.25)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(score) = self.detector_score_threshold {
            if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                return Err(ProtocolError::invalid(
                    "detector score threshold must be finite and in 0..=1",
                ));
            }
            if !self
                .model_input
                .as_ref()
                .is_some_and(|input| input.decoder != DetectorDecoder::TemporalScriptPosition)
            {
                return Err(ProtocolError::unsupported(
                    "detector score threshold requires a declared detector",
                ));
            }
        }
        let annotated_profile = self.inference_profile.as_ref().is_some_and(|profile| {
            matches!(
                profile.preprocessing.resize,
                InferenceResize::AnnotatedTemporalView
            )
        });
        if annotated_profile != self.annotated_view.is_some() {
            return Err(ProtocolError::invalid(
                "annotated view and explicit annotated profile must be supplied together",
            ));
        }
        if let Some(view) = &self.annotated_view {
            view.validate()?;
            if !self
                .model_input
                .as_ref()
                .is_some_and(|input| input.decoder == DetectorDecoder::TemporalScriptPosition)
                || self.video_view.is_some()
                || self.raw_frame_pair.is_some()
                || self.target_seed.is_some()
                || self.vr_mode
            {
                return Err(ProtocolError::unsupported("annotated view requires indexed temporal video without a competing view or target declaration"));
            }
        }
        if let Some(interval) = self.project_interval {
            if interval.start() < ProjectTime::ZERO {
                return Err(ProtocolError::invalid(
                    "generation interval must be nonnegative",
                ));
            }
            if self.raw_frame_pair.is_some() {
                return Err(ProtocolError::unsupported(
                    "project intervals require indexed finite video",
                ));
            }
        }
        if let Some(seed) = &self.target_seed {
            seed.validate()?;
            if !self
                .model_input
                .as_ref()
                .is_some_and(|input| input.decoder != DetectorDecoder::TemporalScriptPosition)
                || self.raw_frame_pair.is_some()
            {
                return Err(ProtocolError::unsupported(
                    "target seed requires a declared detector on indexed video",
                ));
            }
            if self
                .source_placement
                .as_ref()
                .is_some_and(|p| p.spec.rate_numerator < 0)
            {
                return Err(ProtocolError::unsupported(
                    "target seed requires forward source placement",
                ));
            }
            if seed.source_frame_index % u64::from(self.source_sample_stride.max(1)) != 0 {
                return Err(ProtocolError::invalid(
                    "target seed frame must be on the sampled source lattice",
                ));
            }
            if seed.class_id.is_some_and(|class| {
                self.model_input
                    .as_ref()
                    .is_some_and(|input| class >= input.class_count)
            }) {
                return Err(ProtocolError::invalid(
                    "target seed class exceeds detector contract",
                ));
            }
        }

        if let Some(pair) = &self.raw_frame_pair {
            pair.elapsed()?;
            pair.presentation_mapping()?;
            if !self
                .model_input
                .as_ref()
                .is_some_and(|input| input.decoder == DetectorDecoder::TemporalScriptPosition)
                || self.source_placement.is_some()
                || self.video_view.is_some()
                || self.mixed_audio.is_some()
                || self.source_sample_stride != 1
                || self.axis != Axis::Stroke
            {
                return Err(ProtocolError::invalid("raw frame pair requires temporal stroke model without file placement, view, audio or stride interpretation"));
            }
        }
        if self
            .mixed_audio
            .as_ref()
            .is_some_and(|audio| audio.mapping.axis() != self.axis)
        {
            return Err(ProtocolError::invalid(
                "mixed audio axis differs from visual generation axis",
            ));
        }
        if self
            .model_input
            .as_ref()
            .is_some_and(|input| input.decoder == DetectorDecoder::TemporalScriptPosition)
            && (self.analysis_width != 64
                || self.analysis_height != 64
                || self.video_view.is_some())
        {
            return Err(ProtocolError::unsupported(
                "temporal script-position requires declared 64x64 original packed-source analysis",
            ));
        }
        validate_dimensions(self.analysis_width, self.analysis_height)?;
        if let Some(placement) = &self.source_placement {
            placement
                .spec
                .validate()
                .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        }
        if let Some(view) = &self.video_view {
            view.validate()
                .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        }
        if self.vr_mode && self.video_view.is_none() {
            return Err(ProtocolError::unsupported(
                "legacy VR flag requires an explicit video_view declaration",
            ));
        }
        if self.source_sample_stride == 0 {
            return Err(ProtocolError::invalid("sample stride must be positive"));
        }
        if let Some(input) = &self.model_input {
            input.validate()?;
        }
        if let Some(profile) = &self.inference_profile {
            profile.validate()?;
            if self.model_input.is_none() {
                return Err(ProtocolError::invalid(
                    "inference profile requires an input contract",
                ));
            }
        }
        Ok(())
    }
}

/// Explicit initial target selection; coordinates are normalized image edges in
/// the decoded/resized/projected analysis view, not original packed-source pixels.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisTargetSeed {
    pub source_frame_index: u64,
    pub x: f64,
    pub y: f64,
    pub class_id: Option<u32>,
}
impl AnalysisTargetSeed {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if ![self.x, self.y]
            .iter()
            .all(|v| v.is_finite() && (0.0..1.0).contains(v))
        {
            return Err(ProtocolError::invalid(
                "analysis target point must be finite and in [0,1) on both axes",
            ));
        }
        Ok(())
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), ProtocolError> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(ProtocolError::invalid(
            "dimensions must be within 1..=16384",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelInputContract {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub layout: TensorLayout,
    pub class_count: u32,
    pub decoder: DetectorDecoder,
}

impl ModelInputContract {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_dimensions(self.width, self.height)?;
        if self.decoder == DetectorDecoder::TemporalScriptPosition {
            if self.channels != 6
                || self.width != 64
                || self.height != 64
                || self.layout != TensorLayout::Nchw
                || self.class_count != 1
            {
                return Err(ProtocolError::invalid("temporal script-position contract is NCHW [1,6,64,64] -> [1,1]; legacy class_count must be 1"));
            }
        } else if !(1..=4).contains(&self.channels)
            || self.class_count == 0
            || self.class_count > 65536
        {
            return Err(ProtocolError::invalid(
                "invalid model channel or class contract",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TensorLayout {
    Nchw,
    Nhwc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDecoder {
    YoloChannelMajor,
    YoloEndToEnd,
    TemporalScriptPosition,
}
/// Source-compatible name for the original detector-only API.
pub type DetectorDecoder = ModelDecoder;

#[cfg(test)]
mod temporal_contract_tests {
    use super::*;
    #[test]
    fn temporal_contract_is_fixed_and_does_not_guess_detector_layouts() {
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: ModelDecoder::TemporalScriptPosition,
        };
        input.validate().unwrap();
        let mut bad = input.clone();
        bad.channels = 3;
        assert!(bad.validate().is_err());
        let mut bad = input.clone();
        bad.decoder = ModelDecoder::YoloChannelMajor;
        assert!(bad.validate().is_err());
        let mut bad = input.clone();
        bad.class_count = 2;
        assert!(bad.validate().is_err());
        let settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            model_input: Some(input),
            ..Default::default()
        };
        settings.validate().unwrap();
        let mut bad = settings;
        bad.analysis_width = 320;
        assert!(bad.validate().is_err());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda_artifacts: Option<WorkerCudaArtifacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_unavailable: Option<InferenceProviderUnavailable>,
    pub version: u16,
    pub attempt_id: AttemptId,
    pub job_id: JobId,
    pub result: Result<WorkerOutput, ProtocolError>,
}

impl WorkerResult {
    /// Validate optional provider-absence metadata against an explicit original
    /// profile. This does not settle the worker or authorize another attempt.
    /// Legacy responses without this metadata require no profile resolution.
    pub fn validate_provider_unavailable_for(
        &self,
        request: &WorkerRequest,
    ) -> Result<(), ProtocolError> {
        use sha2::{Digest, Sha256};
        let Some(evidence) = &self.provider_unavailable else {
            return Ok(());
        };
        request.validate()?;
        if self.version != PROTOCOL_VERSION
            || self.version != request.version
            || self.attempt_id != request.attempt_id
            || self.job_id != request.job_id
            || self.cuda_artifacts.is_some()
            || !matches!(&self.result, Err(error) if error.code == ErrorCode::Unavailable && !error.retryable)
        {
            return Err(ProtocolError::invalid(
                "provider absence requires the original unavailable worker result without artifacts",
            ));
        }
        let (profile, model, input) = match &request.operation {
            WorkerOperation::Generate {
                model, settings, ..
            } => (
                settings.inference_profile.as_ref(),
                model.as_ref(),
                settings.model_input.as_ref(),
            ),
            WorkerOperation::Preview {
                model,
                model_input,
                inference_profile,
                ..
            } => (
                inference_profile.as_deref(),
                model.as_ref(),
                model_input.as_ref(),
            ),
            WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => {
                (None, None, None)
            }
        };
        let profile = profile.ok_or_else(|| {
            ProtocolError::invalid("provider absence requires an explicit manifest profile")
        })?;
        profile.admit(&request.budget)?;
        if model.is_none_or(|model| model.identity != profile.model)
            || request
                .tools
                .onnx_runtime
                .as_ref()
                .is_none_or(|runtime| runtime.identity != profile.runtime)
            || input.is_none_or(|input| {
                serde_json::to_value(input).ok()
                    != serde_json::to_value(&profile.preprocessing.input).ok()
            })
        {
            return Err(ProtocolError::invalid(
                "provider absence profile model/runtime/input differs from the manifest",
            ));
        }
        let bytes = serde_json::to_vec(profile)
            .map_err(|_| ProtocolError::invalid("provider absence profile serialization failed"))?;
        evidence.validate_for(profile, &format!("{:x}", Sha256::digest(&bytes)))
    }
}

/// Original bounded execution evidence, transported after the output artifacts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCudaArtifacts {
    pub receipt: ArtifactReference,
    pub trace: ArtifactReference,
}
impl WorkerCudaArtifacts {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.receipt.validate()?;
        self.trace.validate()?;
        if !(1..=65536).contains(&self.receipt.identity.byte_len)
            || !(1..=8 * 1024 * 1024).contains(&self.trace.identity.byte_len)
            || self.receipt.path == self.trace.path
        {
            return Err(ProtocolError::invalid(
                "CUDA receipt/trace artifact admission",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod cuda_egress_tests {
    use super::*;
    fn evidence() -> WorkerCudaArtifacts {
        let artifact = |path: &str, len| ArtifactReference {
            path: crate::test_host_path(path).into(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new("authored-evidence").unwrap(),
                sha256: "a".repeat(64),
                byte_len: len,
            },
        };
        WorkerCudaArtifacts {
            receipt: artifact("/output/inference.json", 1),
            trace: artifact("/output/trace.json", 1),
        }
    }
    #[test]
    fn cuda_egress_refuses_unbounded_empty_alias_and_unknown_declarations() {
        evidence().validate().unwrap();
        let mut bad = evidence();
        bad.receipt.identity.byte_len = 65537;
        assert!(bad.validate().is_err());
        let mut bad = evidence();
        bad.trace.identity.byte_len = 8 * 1024 * 1024 + 1;
        assert!(bad.validate().is_err());
        let mut bad = evidence();
        bad.trace.identity.byte_len = 0;
        assert!(bad.validate().is_err());
        let mut bad = evidence();
        bad.trace.path = bad.receipt.path.clone();
        assert!(bad.validate().is_err());
        let mut bad = serde_json::to_value(evidence()).unwrap();
        bad["authorized"] = true.into();
        assert!(serde_json::from_value::<WorkerCudaArtifacts>(bad).is_err());
    }
    #[test]
    fn cpu_response_bytes_omit_cuda_transport_declarations() {
        let result = WorkerResult {
            cuda_artifacts: None,
            provider_unavailable: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            job_id: JobId::new("job").unwrap(),
            result: Err(ProtocolError::invalid("authored refusal")),
        };
        let encoded = serde_json::to_value(result).unwrap();
        assert!(encoded.get("cuda_artifacts").is_none());
        assert!(encoded.get("provider_unavailable").is_none());
        assert!(serde_json::from_value::<WorkerResult>(encoded)
            .unwrap()
            .cuda_artifacts
            .is_none());
    }
}

#[cfg(test)]
mod provider_unavailable_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn artifact(name: &str) -> ArtifactReference {
        ArtifactReference {
            path: crate::test_host_path(&format!("/fixture/{name}")).into(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(name).unwrap(),
                sha256: "a".repeat(64),
                byte_len: 12,
            },
        }
    }

    fn manifest() -> WorkerRequest {
        let source = |name: &str| {
            let artifact = artifact(name);
            SourceArtifact {
                source_version: SourceVersionId::new(name).unwrap(),
                path: artifact.path,
                identity: artifact.identity,
            }
        };
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact("model").identity,
            runtime: artifact("runtime").identity,
            preprocessing: InferencePreprocessing {
                input: input.clone(),
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 4096,
            minimum_output_bytes: 1024,
            cpu_threads: 1,
            cuda_resources: None,
        };
        let request = WorkerRequest {
            cuda_execution: None,
            independent_audio: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            job_id: JobId::new("job").unwrap(),
            project_id: ProjectId::new("project").unwrap(),
            base_revision: RevisionId::new(0),
            source: source("source"),
            output_dir: crate::test_host_path("/fixture/output").into(),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                model: Some(source("model")),
                settings: Box::new(GenerationSettings {
                    model_input: Some(input),
                    inference_profile: Some(profile),
                    ..GenerationSettings::default()
                }),
            },
            budget: ResourceBudget {
                memory_bytes: 4096,
                output_bytes: 1024,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![artifact("model").identity],
            tools: WorkerTools {
                ffmpeg: artifact("ffmpeg"),
                ffprobe: artifact("ffprobe"),
                onnx_runtime: Some(artifact("runtime")),
            },
        };
        request.validate().unwrap();
        request
    }

    fn profile(request: &WorkerRequest) -> &DeclaredInferenceProfile {
        let WorkerOperation::Generate { settings, .. } = &request.operation else {
            unreachable!()
        };
        settings.inference_profile.as_ref().unwrap()
    }

    fn response(request: &WorkerRequest) -> WorkerResult {
        // Authored wire validation only; no runtime observation is claimed.
        let selected = profile(request);
        WorkerResult {
            cuda_artifacts: None,
            provider_unavailable: Some(InferenceProviderUnavailable {
                schema_version: 1,
                provider: selected.provider,
                profile_sha256: format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(selected).unwrap())
                ),
                check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
            }),
            version: PROTOCOL_VERSION,
            attempt_id: request.attempt_id.clone(),
            job_id: request.job_id.clone(),
            result: Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "authored absent-provider observation",
            )),
        }
    }

    #[test]
    fn exact_explicit_profile_and_original_unavailable_result_match() {
        let request = manifest();
        let result = response(&request);
        result.validate_provider_unavailable_for(&request).unwrap();
        let encoded = serde_json::to_vec(&result).unwrap();
        serde_json::from_slice::<WorkerResult>(&encoded)
            .unwrap()
            .validate_provider_unavailable_for(&request)
            .unwrap();
    }

    #[test]
    fn absent_metadata_preserves_old_serialization_without_resolving_a_profile() {
        let mut request = manifest();
        let mut value = serde_json::to_value(response(&request)).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("provider_unavailable");
        let decoded: WorkerResult = serde_json::from_value(value.clone()).unwrap();
        assert!(decoded.provider_unavailable.is_none());
        assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
        if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
            settings.inference_profile = None;
        }
        decoded.validate_provider_unavailable_for(&request).unwrap();
    }

    #[test]
    fn success_wrong_errors_artifacts_or_original_identity_refuse_metadata() {
        let request = manifest();
        for case in 0..8 {
            let mut result = response(&request);
            match case {
                0 => {
                    result.result = Ok(WorkerOutput::Generated {
                        program: artifact("program"),
                        receipt: artifact("receipt"),
                        lineage: vec![],
                        review: vec![],
                    })
                }
                1 => {
                    result.result = Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "authored",
                    ))
                }
                2 => result.result.as_mut().unwrap_err().retryable = true,
                3 => {
                    result.cuda_artifacts = Some(WorkerCudaArtifacts {
                        receipt: artifact("receipt"),
                        trace: artifact("trace"),
                    })
                }
                4 => result.attempt_id = AttemptId::new("other-attempt").unwrap(),
                5 => result.job_id = JobId::new("other-job").unwrap(),
                6 => result.version = PROTOCOL_VERSION.saturating_add(1),
                _ => {
                    result.provider_unavailable.as_mut().unwrap().provider =
                        InferenceProvider::OpenVino
                }
            }
            assert!(
                result.validate_provider_unavailable_for(&request).is_err(),
                "case {case}"
            );
        }
    }

    #[test]
    fn changed_model_runtime_input_budget_or_implicit_profile_refuses_metadata() {
        for case in 0..6 {
            let mut request = manifest();
            let result = response(&request);
            match case {
                0 => {
                    let WorkerOperation::Generate { model, .. } = &mut request.operation else {
                        unreachable!()
                    };
                    model.as_mut().unwrap().identity.sha256 = "b".repeat(64);
                }
                1 => request.tools.onnx_runtime.as_mut().unwrap().identity.sha256 = "b".repeat(64),
                2 => {
                    let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
                        unreachable!()
                    };
                    settings.model_input.as_mut().unwrap().width = 32;
                }
                3 => request.budget.memory_bytes = 1,
                4 => {
                    let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
                        unreachable!()
                    };
                    settings.inference_profile = None;
                }
                _ => request.operation = WorkerOperation::ProbeMedia,
            }
            assert!(
                result.validate_provider_unavailable_for(&request).is_err(),
                "case {case}"
            );
        }
    }

    #[test]
    fn profile_observation_requires_exact_typed_bytes_and_version() {
        let request = manifest();
        let selected = profile(&request);
        let evidence = response(&request).provider_unavailable.unwrap();
        for case in 0..4 {
            let mut changed = selected.clone();
            let mut observation = evidence.clone();
            match case {
                0 => changed.model.sha256 = "b".repeat(64),
                1 => changed.preprocessing.padding_byte = 3,
                2 => observation.schema_version = 2,
                _ => observation.profile_sha256 = "not-a-digest".into(),
            }
            assert!(
                observation
                    .validate_for(&changed, &evidence.profile_sha256)
                    .is_err(),
                "case {case}"
            );
        }
        let mut unknown = serde_json::to_value(&evidence).unwrap();
        unknown["accepted"] = true.into();
        assert!(serde_json::from_value::<InferenceProviderUnavailable>(unknown).is_err());
        let mut unknown = serde_json::to_value(&evidence).unwrap();
        unknown["check"] = "registration_failed".into();
        assert!(serde_json::from_value::<InferenceProviderUnavailable>(unknown).is_err());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkerOutput {
    MediaProbe(Box<MediaProbeReport>),
    Generated {
        program: ArtifactReference,
        receipt: ArtifactReference,
        lineage: Vec<ArtifactIdentity>,
        #[serde(default)]
        review: Vec<ReviewFlag>,
    },
    Preview(Box<PreviewResult>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameArtifact {
    pub artifact: ArtifactReference,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u64,
    pub format: PixelFormat,
}

impl FrameArtifact {
    pub fn validate(&self, max_bytes: u64) -> Result<(), ProtocolError> {
        self.artifact.validate()?;
        validate_dimensions(self.width, self.height)?;
        let row = u64::from(self.width)
            .checked_mul(self.format.channels())
            .ok_or_else(|| ProtocolError::invalid("frame row overflow"))?;
        let len = self
            .stride_bytes
            .checked_mul(u64::from(self.height))
            .ok_or_else(|| ProtocolError::invalid("frame length overflow"))?;
        if self.stride_bytes < row || len != self.artifact.identity.byte_len || len > max_bytes {
            return Err(ProtocolError::invalid(
                "frame layout does not match bounded artifact",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    Rgb8,
    Rgba8,
    Gray8,
}
impl PixelFormat {
    pub fn channels(self) -> u64 {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
            Self::Gray8 => 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointManifest {
    pub schema_version: u16,
    pub attempt_id: AttemptId,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub source: ArtifactIdentity,
    pub dependencies: Vec<ArtifactIdentity>,
    pub configuration_sha256: String,
    pub checkpoint: ArtifactReference,
}

impl CheckpointManifest {
    /// Equality of declared identities is necessary, not sufficient. The engine
    /// must also validate each retained artifact's bytes before resuming.
    pub fn matches_dependencies(
        &self,
        request: &WorkerRequest,
        configuration_sha256: &str,
    ) -> bool {
        self.schema_version == PROTOCOL_VERSION
            && self.project_id == request.project_id
            && self.base_revision == request.base_revision
            && self.source == request.source.identity
            && self.dependencies == request.dependencies
            && self.configuration_sha256 == configuration_sha256
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransfer {
    pub artifact: ArtifactIdentity,
    pub session: SessionId,
    pub project: ProjectId,
    pub byte_offset: u64,
    pub byte_len: u64,
    pub max_chunk_bytes: u32,
}

impl ArtifactTransfer {
    pub fn validate(&self, admission_limit: u64) -> Result<(), ProtocolError> {
        self.artifact.validate()?;
        let end = self
            .byte_offset
            .checked_add(self.byte_len)
            .ok_or_else(|| ProtocolError::invalid("artifact transfer range overflow"))?;
        if end > self.artifact.byte_len
            || self.byte_len > admission_limit
            || self.max_chunk_bytes == 0
            || self.max_chunk_bytes as usize > MAX_ARTIFACT_CHUNK_BYTES
        {
            return Err(ProtocolError::invalid(
                "artifact transfer exceeds range or admission limit",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TensorDescriptor {
    pub artifact: ArtifactIdentity,
    pub shape: Vec<u64>,
    pub element_bytes: u8,
    pub byte_offset: u64,
    pub byte_len: u64,
}

impl TensorDescriptor {
    /// Only contiguous tensors are admitted in this ABI. Arbitrary strides,
    /// aliasing and writable host buffers require a future explicit contract.
    pub fn validate(&self, admission_limit: u64) -> Result<(), ProtocolError> {
        self.artifact.validate()?;
        if self.shape.is_empty()
            || self.shape.len() > 8
            || !matches!(self.element_bytes, 1 | 2 | 4 | 8)
        {
            return Err(ProtocolError::invalid(
                "unsupported tensor rank or element size",
            ));
        }
        let mut size = u64::from(self.element_bytes);
        for dimension in &self.shape {
            if *dimension == 0 {
                return Err(ProtocolError::invalid("zero tensor dimension"));
            }
            size = size
                .checked_mul(*dimension)
                .ok_or_else(|| ProtocolError::invalid("tensor shape overflow"))?;
        }
        let end = self
            .byte_offset
            .checked_add(size)
            .ok_or_else(|| ProtocolError::invalid("tensor range overflow"))?;
        if size != self.byte_len || size > admission_limit || end > self.artifact.byte_len {
            return Err(ProtocolError::invalid(
                "tensor layout exceeds artifact or admission",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionRole {
    Processor,
    Editor,
    Runtime,
    Driver,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfinementRequirement {
    Wasm,
    NativeSandbox,
    UnsandboxedExactBuild { approval_id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    pub schema_version: u16,
    pub name: String,
    pub role: ExtensionRole,
    pub artifact: ArtifactIdentity,
    pub abi: String,
    pub requested_scopes: Vec<Scope>,
    pub budget: ResourceBudget,
    pub confinement: ConfinementRequirement,
}

impl ExtensionManifest {
    /// Manifest declarations never create grants, qualification, build approval,
    /// or assistant invocation approval. Those records are engine authority.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("extension schema"));
        }
        bounded_text(&self.name, 256, "extension name")?;
        bounded_text(&self.abi, 128, "extension ABI")?;
        self.artifact.validate()?;
        self.budget.validate()?;
        if self.requested_scopes.len() > 16 {
            return Err(ProtocolError::invalid("too many extension scopes"));
        }
        if let ConfinementRequirement::UnsandboxedExactBuild { approval_id } = &self.confinement {
            bounded_text(approval_id, 256, "approval identity")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(byte_len: u64) -> ArtifactIdentity {
        ArtifactIdentity {
            artifact_id: ArtifactId::new("artifact").unwrap(),
            sha256: "a".repeat(64),
            byte_len,
        }
    }

    #[test]
    fn malformed_digests_rejected() {
        for sha256 in ["A".repeat(64), "f".repeat(63), "g".repeat(64)] {
            assert!(ArtifactIdentity {
                sha256,
                ..identity(1)
            }
            .validate()
            .is_err());
        }
    }

    #[test]
    fn tensor_overflow_zero_and_out_of_bounds_rejected() {
        for shape in [vec![u64::MAX, 2], vec![0, 1], vec![4, 4]] {
            assert!(TensorDescriptor {
                artifact: identity(8),
                shape,
                element_bytes: 4,
                byte_offset: 0,
                byte_len: 8
            }
            .validate(1024)
            .is_err());
        }
        assert!(TensorDescriptor {
            artifact: identity(8),
            shape: vec![2],
            element_bytes: 4,
            byte_offset: 0,
            byte_len: 8
        }
        .validate(8)
        .is_ok());
    }

    #[test]
    fn transfer_cannot_escape_artifact_or_budget() {
        let transfer = ArtifactTransfer {
            artifact: identity(100),
            session: SessionId::new("session").unwrap(),
            project: ProjectId::new("project").unwrap(),
            byte_offset: 95,
            byte_len: 10,
            max_chunk_bytes: 256,
        };
        assert!(transfer.validate(100).is_err());
        assert!(ArtifactTransfer {
            byte_offset: 0,
            ..transfer
        }
        .validate(5)
        .is_err());
    }

    #[test]
    fn invalid_frame_layout_rejected() {
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                identity: identity(8),
                path: std::env::temp_dir().join("frame"),
            },
            width: 2,
            height: 2,
            stride_bytes: 4,
            format: PixelFormat::Rgb8,
        };
        assert!(frame.validate(8).is_err());
        assert!(FrameArtifact {
            format: PixelFormat::Gray8,
            ..frame
        }
        .validate(8)
        .is_ok());
    }

    #[test]
    fn zero_budget_and_stride_rejected() {
        assert!(ResourceBudget {
            memory_bytes: 1,
            output_bytes: 1,
            wall_time_ms: 0,
            cpu_threads: 1
        }
        .validate()
        .is_err());
        assert!(GenerationSettings {
            source_sample_stride: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}

/// Stable normalized-view identity shared by clients, engine and worker.
pub fn video_view_transform(view: Option<&VideoView>) -> Result<TransformId, ProtocolError> {
    use sha2::Digest;
    let Some(view) = view else {
        return TransformId::new("source-identity")
            .map_err(|e| ProtocolError::invalid(e.to_string()));
    };
    view.validate()
        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
    let bytes = serde_json::to_vec(&("pulsar-selected-eye-view-v1", view))
        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
    TransformId::new(format!("video-view-v1.{:x}", sha2::Sha256::digest(bytes)))
        .map_err(|e| ProtocolError::invalid(e.to_string()))
}

#[cfg(test)]
mod execution_preset_tests {
    use super::*;
    #[test]
    fn executable_tier_matrix_does_not_change_archival_shape_or_settings() {
        let legacy = GenerationSettings::default();
        let saved = serde_json::to_value(&legacy).unwrap();
        for name in ["fast", "default", "economy"] {
            legacy.validate_execution_preset(name, false).unwrap();
        }
        for name in ["maximum", "quality", "unknown"] {
            assert_eq!(
                legacy
                    .validate_execution_preset(name, false)
                    .unwrap_err()
                    .code,
                ErrorCode::Unsupported
            );
        }
        let mut detector = GenerationSettings {
            model_input: Some(ModelInputContract {
                width: 64,
                height: 64,
                channels: 3,
                layout: TensorLayout::Nchw,
                class_count: 1,
                decoder: DetectorDecoder::YoloChannelMajor,
            }),
            ..Default::default()
        };
        for name in ["fast", "default", "economy", "maximum", "quality"] {
            detector.validate_execution_preset(name, true).unwrap();
        }
        assert_eq!(
            detector
                .validate_execution_preset("maximum", false)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        detector.model_input.as_mut().unwrap().decoder = DetectorDecoder::TemporalScriptPosition;
        detector.model_input.as_mut().unwrap().channels = 6;
        detector.analysis_width = 64;
        detector.analysis_height = 64;
        detector.validate().unwrap();
        detector.validate_execution_preset("default", true).unwrap();
        assert_eq!(
            detector
                .validate_execution_preset("maximum", true)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        let non_stroke = GenerationSettings {
            axis: Axis::Yaw,
            ..Default::default()
        };
        non_stroke.validate().unwrap(); // Structural records remain readable.
        assert_eq!(
            non_stroke
                .validate_execution_preset("default", false)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(serde_json::to_value(&legacy).unwrap(), saved);
    }
}
