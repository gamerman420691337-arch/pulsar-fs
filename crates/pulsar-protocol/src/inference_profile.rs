//! Declared inference contracts. Validation is not runtime qualification.
use crate::{ArtifactIdentity, AttemptId, ModelInputContract, ProtocolError, ResourceBudget};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceProvider {
    Cpu,
    Cuda,
    TensorRt,
    Rocm,
    CoreMl,
    DirectMl,
    OpenVino,
}

/// Engine-reported implemented adapter and pinned runtime, not model qualification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceCatalog {
    pub schema_version: u16,
    pub worker_available: bool,
    pub runtime: Option<ArtifactIdentity>,
    pub providers: Vec<InferenceProvider>,
    pub maximum_input_dimension: u32,
    pub maximum_class_count: u32,
    pub maximum_model_bytes: u64,
    pub cpu_threads: u16,
    pub memory_bytes: u64,
    pub output_bytes: u64,
    pub qualified_profiles: Vec<DeclaredInferenceProfile>,
    pub qualified_fallback_available: bool,
    pub limitation: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceResize {
    Stretch,
    NearestStretch,
    /// Explicit native eye/projection/region recipe; requires source-bound settings.
    AnnotatedTemporalView,
    Letterbox,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceColor {
    Rgb,
    RgbFramePair,
    Bgr,
    Gray,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferencePreprocessing {
    pub input: ModelInputContract,
    pub resize: InferenceResize,
    pub color: InferenceColor,
    /// Per-channel (byte_value * scale) + bias, before model execution.
    pub scale: Vec<f32>,
    pub bias: Vec<f32>,
    pub padding_byte: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CudaResourceLimits {
    pub device_ordinal: u32,
    pub device_uuid: String,
    /// Native virtual mappings; independent of the physical RAM cgroup.
    pub address_space_bytes: u64,
    /// Cooperative engine reservation, not a hardware-enforced total quota.
    pub device_reservation_bytes: u64,
    /// CUDA EP arena only; other device allocations can exceed it.
    pub arena_limit_bytes: u64,
}

/// Syntax only. Device existence, observed identity and authority belong to the broker.
pub fn validate_cuda_device_identity(ordinal: u32, uuid: &str) -> Result<(), ProtocolError> {
    let uuid = uuid.strip_prefix("GPU-").unwrap_or("");
    if ordinal > 63
        || uuid.len() != 36
        || !uuid.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err(ProtocolError::invalid(
            "invalid CUDA device identity declaration",
        ));
    }
    Ok(())
}

impl CudaResourceLimits {
    pub fn validate(&self, physical_memory_bytes: u64) -> Result<(), ProtocolError> {
        validate_cuda_device_identity(self.device_ordinal, &self.device_uuid)?;
        if physical_memory_bytes == 0
            || self.address_space_bytes < physical_memory_bytes
            || self.address_space_bytes == 0
            || self.address_space_bytes > (1u64 << 40)
            || self.device_reservation_bytes == 0
            || self.device_reservation_bytes > (1u64 << 40)
            || self.arena_limit_bytes == 0
            || self.arena_limit_bytes > self.device_reservation_bytes
        {
            return Err(ProtocolError::invalid(
                "invalid declared CUDA resource limits",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredInferenceProfile {
    pub schema_version: u16,
    pub model: ArtifactIdentity,
    pub runtime: ArtifactIdentity,
    pub preprocessing: InferencePreprocessing,
    /// This attempt has one strict provider. Fallback requires a new attempt.
    pub provider: InferenceProvider,
    pub minimum_memory_bytes: u64,
    pub minimum_output_bytes: u64,
    pub cpu_threads: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda_resources: Option<CudaResourceLimits>,
}

impl DeclaredInferenceProfile {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !matches!(
            (
                self.schema_version,
                self.provider,
                self.cuda_resources.is_some()
            ),
            (1, _, false) | (2, InferenceProvider::Cuda, true)
        ) {
            return Err(ProtocolError::invalid(
                "unsupported inference profile version",
            ));
        }
        for artifact in [&self.model, &self.runtime] {
            artifact.validate()?;
            if artifact.byte_len == 0 {
                return Err(ProtocolError::invalid(
                    "invalid inference artifact identity",
                ));
            }
        }
        let p = &self.preprocessing;
        p.input.validate()?;
        if matches!(p.resize, InferenceResize::AnnotatedTemporalView)
            && (!(self.provider == InferenceProvider::Cpu
                || (self.provider == InferenceProvider::Cuda && self.schema_version == 2))
                || p.input.decoder != crate::DetectorDecoder::TemporalScriptPosition
                || !matches!(p.color, InferenceColor::RgbFramePair)
                || p.padding_byte != 0
                || p.scale != vec![1.0f32 / 255.0; 6]
                || p.bias != vec![0.0; 6])
        {
            return Err(ProtocolError::unsupported("annotated temporal preprocessing requires its exact CPU or resource-declared CUDA frame-pair recipe"));
        }
        let channels = match p.color {
            InferenceColor::Rgb | InferenceColor::Bgr => 3,
            InferenceColor::RgbFramePair => 6,
            InferenceColor::Gray => 1,
        };
        if p.input.channels != channels
            || p.scale.len() != channels as usize
            || p.bias.len() != channels as usize
            || !p.scale.iter().chain(&p.bias).all(|v| v.is_finite())
            || p.scale.contains(&0.0)
        {
            return Err(ProtocolError::invalid(
                "inconsistent inference preprocessing",
            ));
        }
        if self.minimum_memory_bytes == 0 || self.minimum_output_bytes == 0 || self.cpu_threads == 0
        {
            return Err(ProtocolError::invalid(
                "inference resources must be positive",
            ));
        }
        if let Some(cuda) = &self.cuda_resources {
            cuda.validate(self.minimum_memory_bytes)?;
        }
        Ok(())
    }

    /// Structural admission only; engine must reserve aggregate resources.
    pub fn admit(&self, budget: &ResourceBudget) -> Result<(), ProtocolError> {
        self.validate()?;
        if let Some(cuda) = &self.cuda_resources {
            cuda.validate(budget.memory_bytes)?;
        }
        if budget.memory_bytes < self.minimum_memory_bytes
            || budget.output_bytes < self.minimum_output_bytes
            || budget.cpu_threads < self.cpu_threads
            || budget.wall_time_ms == 0
        {
            return Err(ProtocolError::invalid(
                "inference profile exceeds attempt budget",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceExecutionEvidence {
    pub attempt_id: AttemptId,
    /// Digest of the exact serialized profile admitted for this attempt.
    pub profile_sha256: String,
    /// All providers that executed nodes, not merely registered providers.
    pub executed_providers: Vec<InferenceProvider>,
    pub elapsed_ns: u64,
    /// None means unmeasured, never zero consumption.
    pub peak_memory_bytes: Option<u64>,
}

impl InferenceExecutionEvidence {
    pub fn validate_for(
        &self,
        profile: &DeclaredInferenceProfile,
        attempt: &AttemptId,
        profile_sha256: &str,
    ) -> Result<(), ProtocolError> {
        profile.validate()?;
        crate::validate_sha256(profile_sha256)?;
        if &self.attempt_id != attempt || self.profile_sha256 != profile_sha256 {
            return Err(ProtocolError::invalid(
                "inference execution identity mismatch",
            ));
        }
        if self.executed_providers.as_slice() != [profile.provider] {
            return Err(ProtocolError::invalid(
                "strict inference provider pin violated",
            ));
        }
        Ok(())
    }
}

/// A narrow runtime observation, not permission to retry or fall back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceProviderAvailabilityCheck {
    NotListedByRuntime,
}

/// Emitted only when the pinned, initialized runtime reports the selected
/// provider absent. Initialization, registration and inference errors are not
/// this observation. The parent still checks worker custody and the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceProviderUnavailable {
    pub schema_version: u16,
    pub provider: InferenceProvider,
    pub profile_sha256: String,
    pub check: InferenceProviderAvailabilityCheck,
}

impl InferenceProviderUnavailable {
    pub fn validate_for(
        &self,
        profile: &DeclaredInferenceProfile,
        profile_sha256: &str,
    ) -> Result<(), ProtocolError> {
        use sha2::{Digest, Sha256};
        profile.validate()?;
        crate::validate_sha256(profile_sha256)?;
        crate::validate_sha256(&self.profile_sha256)?;
        let bytes = serde_json::to_vec(profile)
            .map_err(|_| ProtocolError::invalid("provider absence profile serialization failed"))?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if self.schema_version != 1
            || self.provider != profile.provider
            || self.profile_sha256 != profile_sha256
            || actual != profile_sha256
        {
            return Err(ProtocolError::invalid(
                "provider absence observation does not match the exact profile",
            ));
        }
        Ok(())
    }
}

/// Engine-resolved qualification reference. Deserialization alone does not
/// authenticate the issuer or grant permission to use this equivalence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedProfileEquivalence {
    pub from_profile_sha256: String,
    pub to_profile_sha256: String,
    pub qualification_record_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceFallbackPolicy {
    Pinned,
    Qualified {
        equivalents: Vec<QualifiedProfileEquivalence>,
        maximum_fallbacks: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceAttemptFailure {
    ProviderUnavailable,
    Cancelled,
    Revoked,
    DependencyMismatch,
    InvalidModel,
    ResourceExhausted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceFallbackTransition {
    pub previous_attempt: AttemptId,
    pub next_attempt: AttemptId,
    pub previous_profile_sha256: String,
    pub next_profile_sha256: String,
    pub qualification_record_sha256: String,
    pub fallback_ordinal: u8,
}

impl InferenceFallbackPolicy {
    /// Structural transition check only. Engine still resolves qualification,
    /// rechecks authority, reserves resources and persists a fresh attempt.
    pub fn admit_transition(
        &self,
        failure: InferenceAttemptFailure,
        transition: &InferenceFallbackTransition,
    ) -> Result<(), ProtocolError> {
        let Self::Qualified {
            equivalents,
            maximum_fallbacks,
        } = self
        else {
            return Err(ProtocolError::invalid(
                "pinned inference backend forbids fallback",
            ));
        };
        if failure != InferenceAttemptFailure::ProviderUnavailable {
            return Err(ProtocolError::invalid(
                "failure is not eligible for backend fallback",
            ));
        }
        if *maximum_fallbacks == 0
            || *maximum_fallbacks > 8
            || equivalents.len() > 32
            || transition.fallback_ordinal == 0
            || transition.fallback_ordinal > *maximum_fallbacks
            || transition.previous_attempt == transition.next_attempt
            || transition.previous_profile_sha256 == transition.next_profile_sha256
        {
            return Err(ProtocolError::invalid(
                "invalid or exhausted fallback transition",
            ));
        }
        for digest in [
            &transition.previous_profile_sha256,
            &transition.next_profile_sha256,
            &transition.qualification_record_sha256,
        ] {
            crate::validate_sha256(digest)?;
        }
        for equivalent in equivalents {
            crate::validate_sha256(&equivalent.from_profile_sha256)?;
            crate::validate_sha256(&equivalent.to_profile_sha256)?;
            crate::validate_sha256(&equivalent.qualification_record_sha256)?;
        }
        if !equivalents.iter().any(|equivalent| {
            equivalent.from_profile_sha256 == transition.previous_profile_sha256
                && equivalent.to_profile_sha256 == transition.next_profile_sha256
                && equivalent.qualification_record_sha256 == transition.qualification_record_sha256
        }) {
            return Err(ProtocolError::invalid(
                "no matching qualified profile equivalence",
            ));
        }
        Ok(())
    }
}
