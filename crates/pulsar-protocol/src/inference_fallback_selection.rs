//! Explicit fallback intent and scoped semantic correspondence. Engine admission
//! still owns accepted evidence, source kind, current authority, environment,
//! retirement, deadlines and fresh resource custody. These values grant none.
use crate::*;
#[path = "browser_inference_semantics.rs"]
mod browser_semantics;
pub use browser_semantics::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Component, PathBuf},
};

pub const MAX_INFERENCE_FALLBACKS: usize = 8;
pub const MAX_INFERENCE_FALLBACK_REFERENCES: usize = 32;
pub const INFERENCE_FALLBACK_SELECTION_ID_PREFIX: &str = "inference-fallback-selection-";

fn check(condition: bool, message: &str) -> Result<(), ProtocolError> {
    if condition {
        Ok(())
    } else {
        Err(ProtocolError::invalid(message))
    }
}
fn encoded<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| ProtocolError::invalid("fallback policy serialization failed"))?;
    check(
        bytes.len() as u64 <= INFERENCE_EQUIVALENCE_MANIFEST_LIMIT,
        "fallback policy exceeds its encoding bound",
    )?;
    Ok(bytes)
}
fn identity<T: Serialize>(value: &T) -> Result<ArtifactIdentity, ProtocolError> {
    let bytes = encoded(value)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())
            .map_err(|error| ProtocolError::invalid(error.to_string()))?,
        sha256,
        byte_len: bytes.len() as u64,
    })
}
fn artifact_list(values: &[ArtifactIdentity]) -> Result<(), ProtocolError> {
    check(
        !values.is_empty() && values.len() <= INFERENCE_EQUIVALENCE_MAX_CASES,
        "fallback source allowlist must be nonempty and bounded",
    )?;
    for value in values {
        value.validate()?;
        check(
            value.byte_len > 0
                && value.byte_len <= i64::MAX as u64
                && value.artifact_id.as_str() == value.sha256,
            "fallback source requires a positive content-derived identity",
        )?;
    }
    check(
        values
            .windows(2)
            .all(|pair| pair[0].sha256 < pair[1].sha256),
        "fallback source allowlist must be uniquely sorted by digest",
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceFallbackModel {
    Path { path: PathBuf },
    ModelPack { choice: ModelPackChoice },
}
impl InferenceFallbackModel {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Path { path } => check(
                path.is_absolute()
                    && path
                        .to_str()
                        .is_some_and(|value| value.len() <= 4096 && !value.contains('\0'))
                    && !path
                        .components()
                        .any(|part| matches!(part, Component::ParentDir)),
                "fallback model path must be bounded, absolute and without traversal",
            ),
            Self::ModelPack { choice } => choice.validate(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceFallbackReference {
    pub dossier_id: ArtifactId,
    pub expected_revision: u64,
    pub expected_report_sha256: String,
}
impl InferenceFallbackReference {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.expected_report_sha256)?;
        check(
            self.expected_revision > 0 && self.expected_revision <= i64::MAX as u64,
            "fallback equivalence revision must be positive and bounded",
        )
    }
    /// Exact historical binding only. Pending/withdrawn reports can match;
    /// Engine must separately require a live accepted decision and intact evidence.
    pub fn matches_report(
        &self,
        report: &InferenceEquivalenceReport,
    ) -> Result<bool, ProtocolError> {
        self.validate()?;
        report.validate()?;
        let bytes = serde_json::to_vec(report)
            .map_err(|_| ProtocolError::invalid("fallback report serialization failed"))?;
        check(
            bytes.len() <= INFERENCE_EQUIVALENCE_PAGE_LIMIT,
            "fallback referenced report exceeds its bound",
        )?;
        Ok(self.dossier_id == report.dossier_id
            && self.expected_revision == report.revision
            && self.expected_report_sha256 == format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceFallbackSourceKind {
    /// Interpretation policy; Engine must check its retained source-kind record.
    DecodedMedia,
}

/// Domain separation from decoded-file fallback policy. This value is declared
/// applicability only, never a native browser source or an execution grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserInferenceFallbackSourceNamespace {
    BrowserMediaCapture,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceFallbackSourcePolicy {
    pub schema_version: u16,
    pub namespace: BrowserInferenceFallbackSourceNamespace,
    /// Exact content allowlist, independently of the current capture epoch.
    /// None permits any explicitly bound browser-media source, not raw cameras
    /// or arbitrary producer-declared RGB streams.
    pub sources: Option<Vec<ArtifactIdentity>>,
}
impl BrowserInferenceFallbackSourcePolicy {
    pub fn any_bound_media() -> Self {
        Self {
            schema_version: 1,
            namespace: BrowserInferenceFallbackSourceNamespace::BrowserMediaCapture,
            sources: None,
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1,
            "unsupported browser fallback source policy version",
        )?;
        if let Some(sources) = &self.sources {
            artifact_list(sources)?;
        }
        encoded(self).map(|_| ())
    }
    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        identity(self)
    }
    /// Metadata correspondence only. Engine still resolves original native
    /// media/timing ownership, accepted evidence, cancellation and expiry.
    pub fn matches_manifest(
        &self,
        engine_epoch: &AttemptId,
        options: &LiveCaptureOptions,
        manifest: &LiveInferenceManifest,
    ) -> Result<bool, ProtocolError> {
        self.validate()?;
        options.validate()?;
        manifest.validate_metadata()?;
        let (Some(media), Some(_analysis), Some(inference)) = (
            &options.browser_media,
            &options.browser_analysis,
            &options.inference,
        ) else {
            return Ok(false);
        };
        Ok(
            options.source_time_basis == LiveSourceTimeBasis::SourceMediaPresentation
                && options.encoded_source.is_none()
                && media.engine_epoch == *engine_epoch
                && media.project_id == manifest.identity.project
                && media.revision == manifest.identity.revision
                && media.selection.source_version == manifest.identity.source
                && manifest.clock == options.clock(manifest.identity.epoch.clone())
                && manifest.budget.wall_time_ms == u64::from(options.lease_ms)
                && manifest.model.path == inference.model_path
                && encoded(&manifest.profile)? == encoded(&inference.profile)?
                && self
                    .sources
                    .as_ref()
                    .is_none_or(|sources| sources.contains(&media.selection.source)),
        )
    }
}

impl LiveInferenceManifest {
    /// Recheck the browser namespace using retained options and the original
    /// boot identity. Absence of browser analysis keeps ordinary capture modes.
    /// Success is not proof of source acquisition or permission to execute.
    pub fn validate_browser_source(
        &self,
        engine_epoch: &AttemptId,
        options: &LiveCaptureOptions,
    ) -> Result<(), ProtocolError> {
        if options.browser_analysis.is_some() {
            check(
                BrowserInferenceFallbackSourcePolicy::any_bound_media().matches_manifest(
                    engine_epoch,
                    options,
                    self,
                )?,
                "browser inference source namespace, clock, profile or lifetime differs",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceFallbackIndependentAudio {
    Forbidden,
    Any,
    Exact { sources: Vec<ArtifactIdentity> },
}
impl InferenceFallbackIndependentAudio {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Exact { sources } => artifact_list(sources),
            Self::Forbidden | Self::Any => Ok(()),
        }
    }
    fn matches(&self, source: Option<&SourceArtifact>) -> bool {
        match (self, source) {
            (_, None) | (Self::Any, Some(_)) => true,
            (Self::Forbidden, Some(_)) => false,
            (Self::Exact { sources }, Some(source)) => sources.contains(&source.identity),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceFallbackSourcePolicy {
    pub schema_version: u16,
    pub kind: InferenceFallbackSourceKind,
    /// None deliberately permits any decoded-media source. Some is an exact
    /// content allowlist, never an inferred training or evaluation restriction.
    pub sources: Option<Vec<ArtifactIdentity>>,
    pub independent_audio: InferenceFallbackIndependentAudio,
}
impl InferenceFallbackSourcePolicy {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1,
            "unsupported fallback source policy version",
        )?;
        if let Some(sources) = &self.sources {
            artifact_list(sources)?;
        }
        self.independent_audio.validate()?;
        encoded(self).map(|_| ())
    }
    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        identity(self)
    }
    /// Correspondence only. The source-kind field is not proof of decoded media.
    pub fn matches_worker_request(&self, request: &WorkerRequest) -> Result<bool, ProtocolError> {
        self.validate()?;
        generation(request)?;
        Ok(self
            .sources
            .as_ref()
            .is_none_or(|sources| sources.contains(&request.source.identity))
            && self
                .independent_audio
                .matches(request.independent_audio.as_ref()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceFallbackViewApplicability {
    /// Explicitly broad applicability. A successor must still preserve all
    /// original view, target, source-clock, quality-tier and generation semantics.
    PreserveRequestedSemantics,
    ExactSettings {
        sha256: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceFallbackViewPolicy {
    pub schema_version: u16,
    pub applicability: InferenceFallbackViewApplicability,
}
impl InferenceFallbackViewPolicy {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_view_applicability(self.schema_version, &self.applicability)?;
        encoded(self).map(|_| ())
    }
    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        identity(self)
    }
    pub fn matches_worker_request(&self, request: &WorkerRequest) -> Result<bool, ProtocolError> {
        self.validate()?;
        let (preset, settings, _) = generation(request)?;
        let basis = inference_fallback_semantic_settings_identity(preset, settings)?;
        Ok(match &self.applicability {
            InferenceFallbackViewApplicability::PreserveRequestedSemantics => true,
            InferenceFallbackViewApplicability::ExactSettings { sha256 } => {
                sha256.contains(&basis.sha256)
            }
        })
    }
}

fn validate_view_applicability(
    schema_version: u16,
    applicability: &InferenceFallbackViewApplicability,
) -> Result<(), ProtocolError> {
    check(
        schema_version == 1,
        "unsupported fallback view policy version",
    )?;
    if let InferenceFallbackViewApplicability::ExactSettings { sha256 } = applicability {
        check(
            !sha256.is_empty() && sha256.len() <= INFERENCE_EQUIVALENCE_MAX_CASES,
            "fallback settings allowlist must be nonempty and bounded",
        )?;
        for hash in sha256 {
            validate_sha256(hash)?;
        }
        check(
            sha256.windows(2).all(|pair| pair[0] < pair[1]),
            "fallback settings allowlist must be uniquely sorted",
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceFallbackSelection {
    pub schema_version: u16,
    /// Order is deliberate; implementations must not reorder candidate preference.
    pub equivalents: Vec<InferenceFallbackReference>,
    pub maximum_fallbacks: u8,
    pub source_policy: InferenceFallbackSourcePolicy,
    pub view_policy: InferenceFallbackViewPolicy,
}
impl InferenceFallbackSelection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1,
            "unsupported fallback selection version",
        )?;
        check(
            !self.equivalents.is_empty()
                && self.equivalents.len() <= MAX_INFERENCE_FALLBACK_REFERENCES
                && (1..=MAX_INFERENCE_FALLBACKS).contains(&usize::from(self.maximum_fallbacks))
                && usize::from(self.maximum_fallbacks) <= self.equivalents.len(),
            "fallback selection count or attempt limit is invalid",
        )?;
        let mut seen = BTreeSet::new();
        for reference in &self.equivalents {
            reference.validate()?;
            check(
                seen.insert(&reference.dossier_id),
                "fallback dossier may appear only once",
            )?;
        }
        self.source_policy.validate()?;
        self.view_policy.validate()?;
        encoded(self).map(|_| ())
    }
    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        let mut identity = identity(self)?;
        identity.artifact_id = ArtifactId::new(format!(
            "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}",
            identity.sha256
        ))
        .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        Ok(identity)
    }
    pub fn matches_worker_request(&self, request: &WorkerRequest) -> Result<bool, ProtocolError> {
        self.validate()?;
        Ok(self.source_policy.matches_worker_request(request)?
            && self.view_policy.matches_worker_request(request)?)
    }
    /// Exact canonical policy bindings, not a claim that scope has been accepted.
    pub fn matches_scope(&self, scope: &InferenceEquivalenceScope) -> Result<bool, ProtocolError> {
        self.validate()?;
        scope.validate()?;
        Ok(self.source_policy.identity()? == scope.source_policy
            && self.view_policy.identity()? == scope.view_policy)
    }
}

/// The projection mirrors equivalence paired-settings comparison: remove only
/// profile and model-input declarations. Cloning the complete typed settings
/// automatically retains any future setting; no caller-supplied label is trusted.
pub fn inference_fallback_semantic_settings_identity(
    preset: &str,
    settings: &GenerationSettings,
) -> Result<ArtifactIdentity, ProtocolError> {
    settings.validate_execution_preset(preset, true)?;
    check(
        settings.raw_frame_pair.is_none(),
        "fallback policy requires decoded-media generation",
    )?;
    #[derive(Serialize)]
    struct Basis {
        schema_version: u16,
        tier: InferenceEquivalenceTier,
        settings: GenerationSettings,
    }
    let mut preserved = settings.clone();
    preserved.inference_profile = None;
    preserved.model_input = None;
    identity(&Basis {
        schema_version: 1,
        tier: InferenceEquivalenceTier::from_preset(preset)?,
        settings: preserved,
    })
}

fn generation(
    request: &WorkerRequest,
) -> Result<(&str, &GenerationSettings, &SourceArtifact), ProtocolError> {
    request.validate()?;
    let WorkerOperation::Generate {
        preset,
        settings,
        model: Some(model),
    } = &request.operation
    else {
        return Err(ProtocolError::unsupported(
            "fallback requires model-backed decoded-media generation",
        ));
    };
    settings.validate_execution_preset(preset, true)?;
    check(
        settings.raw_frame_pair.is_none(),
        "fallback policy requires decoded-media generation",
    )?;
    let profile = settings
        .inference_profile
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("fallback requires an explicit initial profile"))?;
    profile.admit(&request.budget)?;
    check(
        profile.model == model.identity
            && request
                .tools
                .onnx_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.identity == profile.runtime)
            && settings.model_input.as_ref().is_some_and(|input| {
                serde_json::to_value(input).ok()
                    == serde_json::to_value(&profile.preprocessing.input).ok()
            }),
        "fallback manifest model, input or runtime differs from its explicit profile",
    )?;
    Ok((preset, settings, model))
}

/// Validate a fresh attempt's semantic preservation, not equivalence approval.
/// New native dependencies, runtime, resources and output custody are expected;
/// Engine must admit them freshly without importing a failed attempt's checkpoint.
pub fn validate_inference_fallback_successor(
    original: &WorkerRequest,
    next: &WorkerRequest,
) -> Result<(), ProtocolError> {
    let (from_preset, from_settings, from_model) = generation(original)?;
    let (to_preset, to_settings, to_model) = generation(next)?;
    check(
        original.attempt_id != next.attempt_id
            && original.job_id == next.job_id
            && original.project_id == next.project_id
            && original.base_revision == next.base_revision,
        "fallback requires a fresh attempt in the original job/project/revision",
    )?;
    check(
        original.source.source_version == next.source.source_version
            && original.source.identity == next.source.identity
            && original
                .independent_audio
                .as_ref()
                .map(|source| (&source.source_version, &source.identity))
                == next
                    .independent_audio
                    .as_ref()
                    .map(|source| (&source.source_version, &source.identity)),
        "fallback source or independent audio basis changed",
    )?;
    check(
        from_model.source_version == to_model.source_version
            && from_model.identity == to_model.identity
            && original.tools.ffmpeg.identity == next.tools.ffmpeg.identity
            && original.tools.ffprobe.identity == next.tools.ffprobe.identity,
        "fallback model or media decoder/probe identity changed",
    )?;
    check(inference_fallback_semantic_settings_identity(from_preset, from_settings)?
        == inference_fallback_semantic_settings_identity(to_preset, to_settings)?,
        "fallback changed source/view/target/timing/tier/generation settings beyond profile/model_input")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 1,
        }
    }
    fn reference() -> InferenceFallbackReference {
        InferenceFallbackReference {
            dossier_id: ArtifactId::new("dossier-one").unwrap(),
            expected_revision: 2,
            expected_report_sha256: "a".repeat(64),
        }
    }
    fn selection() -> InferenceFallbackSelection {
        InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![reference()],
            maximum_fallbacks: 1,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: None,
                independent_audio: InferenceFallbackIndependentAudio::Forbidden,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        }
    }
    fn request() -> WorkerRequest {
        let model = SourceArtifact {
            source_version: SourceVersionId::new("model-source").unwrap(),
            path: crate::test_host_path("model").into(),
            identity: artifact(2),
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
            model: model.identity.clone(),
            runtime: artifact(3),
            preprocessing: InferencePreprocessing {
                input: input.clone(),
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        };
        WorkerRequest {
            cuda_execution: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("original-attempt").unwrap(),
            job_id: JobId::new("original-job").unwrap(),
            project_id: ProjectId::new("project").unwrap(),
            base_revision: RevisionId::new(1),
            source: SourceArtifact {
                source_version: SourceVersionId::new("video-source").unwrap(),
                path: crate::test_host_path("source").into(),
                identity: artifact(1),
            },
            independent_audio: None,
            output_dir: crate::test_host_path("output").into(),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                model: Some(model),
                settings: Box::new(GenerationSettings {
                    model_input: Some(input),
                    inference_profile: Some(profile),
                    ..GenerationSettings::default()
                }),
            },
            budget: ResourceBudget {
                memory_bytes: 128,
                output_bytes: 128,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![artifact(1), artifact(2), artifact(3)],
            tools: WorkerTools {
                ffmpeg: ArtifactReference {
                    identity: artifact(4),
                    path: crate::test_host_path("ffmpeg").into(),
                },
                ffprobe: ArtifactReference {
                    identity: artifact(5),
                    path: crate::test_host_path("ffprobe").into(),
                },
                onnx_runtime: Some(ArtifactReference {
                    identity: artifact(3),
                    path: crate::test_host_path("ort").into(),
                }),
            },
        }
    }
    fn settings(request: &mut WorkerRequest) -> &mut GenerationSettings {
        match &mut request.operation {
            WorkerOperation::Generate { settings, .. } => settings,
            _ => unreachable!(),
        }
    }
    fn next(original: &WorkerRequest) -> WorkerRequest {
        let mut next = original.clone();
        next.attempt_id = AttemptId::new("next-attempt").unwrap();
        next.output_dir = crate::test_host_path("fresh-output").into();
        next.tools.onnx_runtime.as_mut().unwrap().identity = artifact(8);
        settings(&mut next)
            .inference_profile
            .as_mut()
            .unwrap()
            .runtime = artifact(8);
        next.dependencies.push(artifact(8));
        next.budget.memory_bytes += 1;
        next
    }
    fn audio(request: &mut WorkerRequest) {
        let source = SourceArtifact {
            source_version: SourceVersionId::new("audio-source").unwrap(),
            path: crate::test_host_path("independent-audio").into(),
            identity: artifact(9),
        };
        settings(request).mixed_audio = Some(MixedAudioSettings {
            intent: MultimodalIntent::Faithful,
            mapping: AudioMapping::new(Axis::Stroke, 0.5, NormalizedPosition::new(0.0).unwrap())
                .unwrap(),
            independent: Some(IndependentAudio {
                source_version: source.source_version.clone(),
                alignment: AudioAlignment {
                    video_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    audio_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    rate: None,
                },
            }),
        });
        request.dependencies.push(source.identity.clone());
        request.independent_audio = Some(source);
    }

    #[test]
    fn source_policy_explicitly_supports_broad_exact_and_independent_audio_scopes() {
        let original = request();
        let mut policy = selection().source_policy;
        assert!(policy.matches_worker_request(&original).unwrap());
        let broad = policy.identity().unwrap();
        policy.sources = Some(vec![original.source.identity.clone()]);
        assert!(policy.matches_worker_request(&original).unwrap());
        assert_ne!(broad, policy.identity().unwrap());
        policy.sources = Some(vec![artifact(20)]);
        assert!(!policy.matches_worker_request(&original).unwrap());
        policy.sources = None;
        let mut with_audio = original.clone();
        audio(&mut with_audio);
        with_audio.validate().unwrap();
        assert!(!policy.matches_worker_request(&with_audio).unwrap());
        policy.independent_audio = InferenceFallbackIndependentAudio::Any;
        assert!(policy.matches_worker_request(&with_audio).unwrap());
        policy.independent_audio = InferenceFallbackIndependentAudio::Exact {
            sources: vec![artifact(9)],
        };
        assert!(policy.matches_worker_request(&with_audio).unwrap());
        assert!(policy.matches_worker_request(&original).unwrap());
        policy.independent_audio = InferenceFallbackIndependentAudio::Exact {
            sources: vec![artifact(20)],
        };
        assert!(!policy.matches_worker_request(&with_audio).unwrap());
    }

    #[test]
    fn source_policy_rejects_unsorted_duplicate_empty_or_oversized_allowlists() {
        for sources in [
            vec![],
            vec![artifact(2), artifact(1)],
            vec![artifact(1), artifact(1)],
            (0..=INFERENCE_EQUIVALENCE_MAX_CASES)
                .map(|i| artifact(i as u8))
                .collect(),
        ] {
            let mut policy = selection().source_policy;
            policy.sources = Some(sources.clone());
            assert!(policy.validate().is_err());
            policy.sources = None;
            policy.independent_audio = InferenceFallbackIndependentAudio::Exact { sources };
            assert!(policy.validate().is_err());
        }
        for value in [
            ArtifactIdentity {
                byte_len: 0,
                ..artifact(1)
            },
            ArtifactIdentity {
                artifact_id: ArtifactId::new("alias").unwrap(),
                ..artifact(1)
            },
        ] {
            let mut policy = selection().source_policy;
            policy.sources = Some(vec![value]);
            assert!(policy.validate().is_err());
        }
    }

    #[test]
    fn view_allowlist_is_computed_from_actual_settings_and_preserves_broad_opt_in() {
        let original = request();
        let (preset, source_settings, _) = generation(&original).unwrap();
        let basis = inference_fallback_semantic_settings_identity(preset, source_settings).unwrap();
        let mut policy = selection().view_policy;
        assert!(policy.matches_worker_request(&original).unwrap());
        let broad = policy.identity().unwrap();
        policy.applicability = InferenceFallbackViewApplicability::ExactSettings {
            sha256: vec![basis.sha256],
        };
        assert!(policy.matches_worker_request(&original).unwrap());
        assert!(policy.matches_worker_request(&next(&original)).unwrap());
        assert_ne!(broad, policy.identity().unwrap());
        let mut changed = original.clone();
        settings(&mut changed).source_sample_stride += 1;
        assert!(!policy.matches_worker_request(&changed).unwrap());
        for hashes in [
            vec![],
            vec!["a".repeat(64), "a".repeat(64)],
            vec!["b".repeat(64), "a".repeat(64)],
            vec!["invalid".into()],
        ] {
            policy.applicability =
                InferenceFallbackViewApplicability::ExactSettings { sha256: hashes };
            assert!(policy.validate().is_err());
        }
    }

    #[test]
    fn ordered_reference_search_allows_32_candidates_but_at_most_8_fallbacks() {
        let mut value = selection();
        value.equivalents = (0..MAX_INFERENCE_FALLBACK_REFERENCES)
            .map(|index| {
                let mut candidate = reference();
                candidate.dossier_id = ArtifactId::new(format!("candidate-{index}")).unwrap();
                candidate
            })
            .collect();
        value.maximum_fallbacks = 8;
        value.validate().unwrap();
        value.maximum_fallbacks = 9;
        assert!(value.validate().is_err());
        value.maximum_fallbacks = 8;
        let mut extra = reference();
        extra.dossier_id = ArtifactId::new("candidate-extra").unwrap();
        value.equivalents.push(extra);
        assert!(value.validate().is_err());
    }

    #[test]
    fn selection_binds_order_bounds_and_exact_scope_policy_identities() {
        let mut value = selection();
        value.validate().unwrap();
        let mut other = reference();
        other.dossier_id = ArtifactId::new("dossier-two").unwrap();
        value.equivalents.push(other);
        let ordered = value.identity().unwrap();
        value.equivalents.reverse();
        assert_ne!(ordered, value.identity().unwrap());
        let mut scope = InferenceEquivalenceScope {
            tier: InferenceEquivalenceTier::Default,
            recipe: InferenceEquivalenceRecipe::YoloTracking,
            axes: vec![Axis::Stroke],
            source_policy: value.source_policy.identity().unwrap(),
            view_policy: value.view_policy.identity().unwrap(),
            limitations: "explicit authored scope".into(),
        };
        assert!(value.matches_scope(&scope).unwrap());
        scope.source_policy = artifact(20);
        assert!(!value.matches_scope(&scope).unwrap());
        for limit in [0, 3, 9] {
            value.maximum_fallbacks = limit;
            assert!(value.validate().is_err());
        }
        value.maximum_fallbacks = 1;
        value.equivalents[1] = value.equivalents[0].clone();
        assert!(value.validate().is_err());
        let mut unknown = serde_json::to_value(selection()).unwrap();
        unknown["accepted"] = true.into();
        assert!(serde_json::from_value::<InferenceFallbackSelection>(unknown).is_err());
        let bytes = serde_json::to_vec(&selection()).unwrap();
        assert_eq!(
            selection().identity().unwrap().sha256,
            format!("{:x}", Sha256::digest(bytes))
        );
        let identity = selection().identity().unwrap();
        assert_eq!(
            identity.artifact_id.as_str(),
            format!(
                "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}",
                identity.sha256
            )
        );
        assert_eq!(
            selection()
                .source_policy
                .identity()
                .unwrap()
                .artifact_id
                .as_str(),
            selection().source_policy.identity().unwrap().sha256
        );
    }

    #[test]
    fn successor_allows_fresh_provider_inputs_but_preserves_every_other_semantic_field() {
        let original = request();
        let valid = next(&original);
        validate_inference_fallback_successor(&original, &valid).unwrap();
        let mut resized = valid.clone();
        settings(&mut resized).model_input.as_mut().unwrap().width = 128;
        settings(&mut resized)
            .inference_profile
            .as_mut()
            .unwrap()
            .preprocessing
            .input
            .width = 128;
        validate_inference_fallback_successor(&original, &resized).unwrap();
        for mutate in [
            |r: &mut WorkerRequest| r.attempt_id = AttemptId::new("original-attempt").unwrap(),
            |r: &mut WorkerRequest| r.job_id = JobId::new("other-job").unwrap(),
            |r: &mut WorkerRequest| r.project_id = ProjectId::new("other-project").unwrap(),
            |r: &mut WorkerRequest| r.base_revision = RevisionId::new(2),
            |r: &mut WorkerRequest| {
                r.source.source_version = SourceVersionId::new("other-source").unwrap()
            },
            |r: &mut WorkerRequest| r.source.identity = artifact(22),
            |r: &mut WorkerRequest| r.tools.ffmpeg.identity = artifact(22),
            |r: &mut WorkerRequest| r.tools.ffprobe.identity = artifact(22),
            |r: &mut WorkerRequest| settings(r).analysis_width += 1,
            |r: &mut WorkerRequest| settings(r).source_sample_stride += 1,
            |r: &mut WorkerRequest| settings(r).detector_score_threshold = Some(0.7),
            |r: &mut WorkerRequest| {
                settings(r).target_seed = Some(AnalysisTargetSeed {
                    source_frame_index: 0,
                    x: 0.4,
                    y: 0.5,
                    class_id: Some(0),
                })
            },
            |r: &mut WorkerRequest| {
                settings(r).project_interval =
                    Some(TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(10)).unwrap())
            },
            |r: &mut WorkerRequest| {
                if let WorkerOperation::Generate { preset, .. } = &mut r.operation {
                    *preset = "fast".into();
                }
            },
        ] {
            let mut changed = valid.clone();
            mutate(&mut changed);
            assert!(validate_inference_fallback_successor(&original, &changed).is_err());
        }
        let mut inconsistent = valid.clone();
        settings(&mut inconsistent)
            .model_input
            .as_mut()
            .unwrap()
            .width += 1;
        assert!(validate_inference_fallback_successor(&original, &inconsistent).is_err());
        let mut unbound = valid.clone();
        settings(&mut unbound).inference_profile = None;
        assert!(validate_inference_fallback_successor(&original, &unbound).is_err());
        let mut different = valid.clone();
        different.tools.onnx_runtime.as_mut().unwrap().identity = artifact(30);
        assert!(validate_inference_fallback_successor(&original, &different).is_err());
    }

    #[test]
    fn successor_keeps_independent_audio_identity_and_exact_clock_correspondence() {
        let mut original = request();
        audio(&mut original);
        let valid = next(&original);
        validate_inference_fallback_successor(&original, &valid).unwrap();
        let mut different = valid.clone();
        let changed = artifact(30);
        different.independent_audio.as_mut().unwrap().identity = changed.clone();
        different.dependencies.push(changed);
        assert!(validate_inference_fallback_successor(&original, &different).is_err());
        let mut clock = valid.clone();
        settings(&mut clock)
            .mixed_audio
            .as_mut()
            .unwrap()
            .independent
            .as_mut()
            .unwrap()
            .alignment
            .audio_anchor = SourceTimestamp::new(1, 1).unwrap();
        assert!(validate_inference_fallback_successor(&original, &clock).is_err());
    }

    #[test]
    fn model_request_choice_is_explicit_and_strictly_validated() {
        InferenceFallbackModel::Path {
            path: crate::test_host_path("model.onnx").into(),
        }
        .validate()
        .unwrap();
        for path in [
            "relative.onnx".into(),
            PathBuf::from(crate::test_host_path("child/../model")),
            PathBuf::from(format!("/{}", "a".repeat(4096))),
        ] {
            assert!(InferenceFallbackModel::Path { path }.validate().is_err());
        }
        let choice = ModelPackChoice {
            manifest_sha256: "a".repeat(64),
            selection_revision: 1,
        };
        InferenceFallbackModel::ModelPack {
            choice: choice.clone(),
        }
        .validate()
        .unwrap();
        assert!(InferenceFallbackModel::ModelPack {
            choice: ModelPackChoice {
                selection_revision: 0,
                ..choice
            }
        }
        .validate()
        .is_err());
        let bad = serde_json::json!({"kind":"path","path":crate::test_host_path("model"),"implicit":true});
        assert!(serde_json::from_value::<InferenceFallbackModel>(bad).is_err());
    }

    #[test]
    fn report_reference_is_exact_historical_binding_without_promoting_pending_evidence() {
        let request = request();
        let profile = generation(&request)
            .unwrap()
            .1
            .inference_profile
            .clone()
            .unwrap();
        let mut alternative = profile.clone();
        alternative.runtime = artifact(8);
        let environment = |runtime: ArtifactIdentity| InferenceEquivalenceEnvironment {
            engine: artifact(20),
            worker: artifact(21),
            os: "linux".into(),
            architecture: "x86_64".into(),
            platform_build: artifact(22),
            hardware: artifact(23),
            dependencies: vec![runtime],
        };
        let selection = selection();
        let manifest = InferenceEquivalenceManifest {
            subject: InferenceEquivalenceSubject {
                schema_version: 1,
                origin: InferenceEquivalenceOrigin::Synthetic,
                from_environment: environment(profile.runtime.clone()),
                to_environment: environment(alternative.runtime.clone()),
                from_profile: profile,
                to_profile: alternative,
                scope: InferenceEquivalenceScope {
                    tier: InferenceEquivalenceTier::Default,
                    recipe: InferenceEquivalenceRecipe::YoloTracking,
                    axes: vec![Axis::Stroke],
                    source_policy: selection.source_policy.identity().unwrap(),
                    view_policy: selection.view_policy.identity().unwrap(),
                    limitations: "authored binding test only".into(),
                },
                not_before_unix_ms: 1,
                expires_unix_ms: 1000,
            },
            evidence: InferenceEquivalenceEvidenceKind::ALL
                .into_iter()
                .enumerate()
                .map(|(index, kind)| InferenceEquivalenceEvidence {
                    kind,
                    identity: artifact(30 + index as u8),
                })
                .collect(),
        };
        let report = InferenceEquivalenceReport {
            dossier_id: manifest.id().unwrap(),
            revision: 1,
            state: InferenceEquivalenceState::Pending,
            manifest,
            decision: None,
            report_only: true,
        };
        let mut reference = InferenceFallbackReference {
            dossier_id: report.dossier_id.clone(),
            expected_revision: 1,
            expected_report_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&report).unwrap())
            ),
        };
        assert!(reference.matches_report(&report).unwrap());
        assert_eq!(report.state, InferenceEquivalenceState::Pending);
        reference.expected_report_sha256 = "a".repeat(64);
        assert!(!reference.matches_report(&report).unwrap());
        reference.expected_revision = 0;
        assert!(reference.validate().is_err());
        reference.expected_revision = 1;
        reference.expected_report_sha256 = "not-a-hash".into();
        assert!(reference.validate().is_err());
    }

    #[test]
    fn decoded_media_policies_refuse_raw_frame_pair_and_implicit_profile_requests() {
        let policy = selection();
        let mut raw = request();
        let values = settings(&mut raw);
        values.analysis_width = 64;
        values.analysis_height = 64;
        values.raw_frame_pair = Some(RawFramePair {
            source_epoch: AttemptId::new("authored-raw-epoch").unwrap(),
            previous_frame: FrameId::new(0),
            current_frame: FrameId::new(1),
            previous_time: SourceTimestamp::new(0, 30).unwrap(),
            current_time: SourceTimestamp::new(1, 30).unwrap(),
            presentation_clock: None,
        });
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::TemporalScriptPosition,
        };
        values.model_input = Some(input.clone());
        let p = values.inference_profile.as_mut().unwrap();
        p.preprocessing.input = input;
        p.preprocessing.resize = InferenceResize::NearestStretch;
        p.preprocessing.color = InferenceColor::RgbFramePair;
        p.preprocessing.scale = vec![1.0 / 255.0; 6];
        p.preprocessing.bias = vec![0.0; 6];
        raw.validate().unwrap();
        assert!(policy.matches_worker_request(&raw).is_err());
        let mut implicit = request();
        settings(&mut implicit).inference_profile = None;
        implicit.validate().unwrap();
        assert!(policy.matches_worker_request(&implicit).is_err());
    }

    #[test]
    fn canonical_tier_aliases_preserve_semantics_without_lowering_quality() {
        let mut original = request();
        if let WorkerOperation::Generate { preset, .. } = &mut original.operation {
            *preset = "fast".into();
        }
        let mut successor = next(&original);
        if let WorkerOperation::Generate { preset, .. } = &mut successor.operation {
            *preset = "economy".into();
        }
        validate_inference_fallback_successor(&original, &successor).unwrap();
        if let WorkerOperation::Generate { preset, .. } = &mut successor.operation {
            *preset = "default".into();
        }
        assert!(validate_inference_fallback_successor(&original, &successor).is_err());
    }
}

#[cfg(test)]
#[path = "browser_inference_source_tests.rs"]
mod browser_source_tests;
