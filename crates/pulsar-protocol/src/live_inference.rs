//! Private engine/worker stream contracts, not client authority or device motion.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveInferenceManifest {
    pub version: u16,
    pub attempt_id: AttemptId,
    pub identity: LiveIdentity,
    pub clock: LivePresentationClock,
    pub model: ArtifactReference,
    pub runtime: ArtifactReference,
    pub worker: ArtifactIdentity,
    pub profile: DeclaredInferenceProfile,
    pub budget: ResourceBudget,
}
impl LiveInferenceManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.validate_metadata()?;
        if self.profile.provider != InferenceProvider::Cpu {
            return Err(ProtocolError::invalid(
                "ordinary live worker requires a CPU profile",
            ));
        }
        Ok(())
    }
    /// GPU metadata admission only. Actual native launch still requires Engine ownership.
    pub fn validate_cuda_metadata(&self) -> Result<(), ProtocolError> {
        self.validate_metadata()?;
        if self.profile.provider != InferenceProvider::Cuda {
            return Err(ProtocolError::invalid(
                "GPU live metadata requires a CUDA profile",
            ));
        }
        Ok(())
    }
    // Shared metadata is not launch authority; the ordinary worker stays CPU-only.
    pub(crate) fn validate_metadata(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("live inference version"));
        }
        self.model.validate()?;
        self.runtime.validate()?;
        self.validate_metadata_with_guest_paths()
    }
    /// The enclosing private execution declaration must validate its exact guest
    /// coordinates. These shared checks grant no filesystem or launch authority.
    pub(crate) fn validate_metadata_with_guest_paths(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("live inference version"));
        }
        self.model.identity.validate()?;
        self.runtime.identity.validate()?;
        self.worker.validate()?;
        self.clock
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        self.budget.validate()?;
        self.profile.admit(&self.budget)?;
        let p = &self.profile.preprocessing;
        if self.clock.epoch != self.identity.epoch
            || self.profile.model != self.model.identity
            || self.profile.runtime != self.runtime.identity
            || self.budget.wall_time_ms > 30_000
            || self.budget.cpu_threads != self.profile.cpu_threads
            || !matches!(
                self.profile.provider,
                InferenceProvider::Cpu | InferenceProvider::Cuda
            )
            || p.input.decoder != DetectorDecoder::TemporalScriptPosition
            || !matches!(p.resize, InferenceResize::NearestStretch)
            || !matches!(p.color, InferenceColor::RgbFramePair)
            || p.scale.iter().any(|v| *v != 1.0f32 / 255.0)
            || p.bias.iter().any(|v| *v != 0.0)
            || p.padding_byte != 0
        {
            return Err(ProtocolError::invalid(
                "live temporal profile, identity or lifetime differs",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveInferenceInput {
    pub sequence: u64,
    pub pair: RawFramePair,
    pub previous: ArtifactIdentity,
    pub current: ArtifactIdentity,
}
impl LiveInferenceInput {
    pub fn validate(&self, manifest: &LiveInferenceManifest) -> Result<(), ProtocolError> {
        self.previous.validate()?;
        self.current.validate()?;
        self.pair.elapsed()?;
        self.pair.presentation_mapping()?;
        if self.sequence == 0
            || self.pair.source_epoch != manifest.identity.epoch
            || self.pair.presentation_clock.as_ref() != Some(&manifest.clock)
            || [&self.previous, &self.current].iter().any(|v| {
                v.byte_len != LIVE_FRAME_BYTES as u64 || v.artifact_id.as_str() != v.sha256
            })
        {
            return Err(ProtocolError::invalid(
                "live input scope, pixels or clock differs",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LiveInferenceReply {
    Ready {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        profile_sha256: String,
    },
    Estimated {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        profile_sha256: String,
        input: LiveInferenceInput,
        position: NormalizedPosition,
        pair_analysis_elapsed_ns: u64,
    },
    /// Ready from the distinct captured CPU live role. Its manifest digest binds
    /// the image and complete declared dependencies; serialization is no proof.
    CapturedCpuReady {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        profile_sha256: String,
        manifest_sha256: String,
    },
    /// An estimate in the original captured CPU stream. The receiver must match
    /// its issued input and exact captured manifest, then recheck live authority.
    CapturedCpuEstimated {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        profile_sha256: String,
        manifest_sha256: String,
        input: LiveInferenceInput,
        position: NormalizedPosition,
        pair_analysis_elapsed_ns: u64,
    },
    /// A GPU stream estimate before consuming Finish and original trace
    /// verification. This variant never represents final CUDA evidence.
    CudaProvisional {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        profile_sha256: String,
        manifest_sha256: String,
        input: LiveInferenceInput,
        position: NormalizedPosition,
        pair_analysis_elapsed_ns: u64,
    },
    Error(ProtocolError),
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> LiveInferenceManifest {
        let identity = LiveIdentity {
            epoch: AttemptId::new("epoch").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(3),
            source: SourceVersionId::new("source").unwrap(),
        };
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 10,
        };
        let model = ArtifactReference {
            identity: artifact.clone(),
            path: crate::test_host_path("/input/model").into(),
        };
        let runtime = ArtifactReference {
            identity: artifact.clone(),
            path: crate::test_host_path("/input/runtime").into(),
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
            minimum_memory_bytes: 128 * 1024 * 1024,
            minimum_output_bytes: 4096,
            cpu_threads: 2,
            cuda_resources: None,
        };
        LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            clock: LivePresentationClock {
                epoch: identity.epoch.clone(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            },
            identity,
            model,
            runtime,
            worker: artifact,
            profile,
            budget: ResourceBudget {
                memory_bytes: 256 * 1024 * 1024,
                output_bytes: 4096,
                wall_time_ms: 30_000,
                cpu_threads: 2,
            },
        }
    }
    #[test]
    fn persistent_manifest_refuses_provider_preprocessing_and_scope_substitution() {
        let base = manifest();
        base.validate().unwrap();
        let mut changed = base.clone();
        changed.profile.provider = InferenceProvider::Cuda;
        assert!(changed.validate().is_err());
        changed = base.clone();
        changed.clock.epoch = AttemptId::new("other").unwrap();
        assert!(changed.validate().is_err());
        changed = base.clone();
        changed.model.identity.sha256 = "b".repeat(64);
        assert!(changed.validate().is_err());
        changed = base.clone();
        changed.profile.preprocessing.scale[0] = 0.5;
        assert!(changed.validate().is_err());
        changed = base.clone();
        changed.budget.wall_time_ms = 30_001;
        assert!(changed.validate().is_err());
    }
    #[test]
    fn issued_pair_contract_retains_clock_and_exact_pixel_hash_identity() {
        let manifest = manifest();
        let sha = "b".repeat(64);
        let pixels = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha.clone()).unwrap(),
            sha256: sha,
            byte_len: LIVE_FRAME_BYTES as u64,
        };
        let mut input = LiveInferenceInput {
            sequence: 1,
            pair: RawFramePair {
                source_epoch: manifest.identity.epoch.clone(),
                previous_frame: FrameId::new(1),
                current_frame: FrameId::new(2),
                previous_time: SourceTimestamp::new(1, 10).unwrap(),
                current_time: SourceTimestamp::new(2, 10).unwrap(),
                presentation_clock: Some(manifest.clock.clone()),
            },
            previous: pixels.clone(),
            current: pixels,
        };
        input.validate(&manifest).unwrap();
        input.current.byte_len += 1;
        assert!(input.validate(&manifest).is_err());
        input.current.byte_len -= 1;
        input.pair.presentation_clock = None;
        assert!(input.validate(&manifest).is_err());
    }
}
