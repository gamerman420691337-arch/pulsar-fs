//! Captured CPU live-stream identity and message correspondence.
//! Declared bytes do not verify a namespace, grant a lease, or qualify motion.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const LIVE_CPU_LIBRARY_DIRECTORY: &str = "/pulsar-cpu-libraries";
pub const LIVE_CPU_MODEL_PATH: &str = "/input/model";
pub const LIVE_CPU_RUNTIME_PATH: &str = "/pulsar-cpu-libraries/libonnxruntime.so.1";
pub const MAX_LIVE_CPU_DEPENDENCIES: usize = MAX_NATIVE_LIBRARIES + 5;

/// The exact captured execution declaration for one browser/camera RGB stream.
/// Absolute process-local deadlines belong in the private mount witness; this
/// serialized duration never creates or renews the original capture lifetime.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCpuInferenceManifest {
    pub schema_version: u16,
    pub stream: LiveInferenceManifest,
    pub image: CpuRuntimeImage,
    /// Exact unique closure, strictly ordered by artifact ID. Includes image,
    /// owner policy, libraries, model, worker and canonical profile identities.
    pub dependencies: Vec<ArtifactIdentity>,
}

impl LiveCpuInferenceManifest {
    pub fn new(
        stream: LiveInferenceManifest,
        image: CpuRuntimeImage,
    ) -> Result<Self, ProtocolError> {
        let dependencies = dependency_closure(&stream, &image)?;
        let manifest = Self {
            schema_version: 1,
            stream,
            image,
            dependencies,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1 || self.dependencies.len() > MAX_LIVE_CPU_DEPENDENCIES {
            return Err(invalid("envelope version or dependency bound differs"));
        }
        // Fixed POSIX guest coordinates are independent of the inspector's host
        // path syntax. Neither coordinate names a supplier file to open.
        if self.stream.model.path.to_str() != Some(LIVE_CPU_MODEL_PATH)
            || self.stream.runtime.path.to_str() != Some(LIVE_CPU_RUNTIME_PATH)
        {
            return Err(invalid("model or runtime guest coordinate differs"));
        }
        self.stream.validate_metadata_with_guest_paths()?;
        if self.stream.profile.provider != InferenceProvider::Cpu {
            return Err(invalid("stream requires an explicit CPU profile"));
        }
        self.image.validate()?;
        let runtime = self
            .image
            .libraries
            .iter()
            .find(|library| {
                library
                    .names
                    .iter()
                    .any(|name| name == "libonnxruntime.so.1")
            })
            .ok_or_else(|| invalid("captured runtime alias absent"))?;
        if runtime.identity != self.stream.runtime.identity
            || runtime.identity != self.stream.profile.runtime
        {
            return Err(invalid("stream runtime is not the captured image runtime"));
        }
        for identity in [
            &self.stream.model.identity,
            &self.stream.runtime.identity,
            &self.stream.worker,
        ] {
            if identity.byte_len == 0 {
                return Err(invalid("execution input identity is empty"));
            }
        }
        let expected = dependency_closure(&self.stream, &self.image)?;
        if self.dependencies != expected {
            return Err(invalid(
                "dependency closure is missing, extra, conflicting or noncanonical",
            ));
        }
        if encoded(self)?.len() > MAX_CONTROL_BYTES {
            return Err(invalid("envelope exceeds control-frame admission"));
        }
        Ok(())
    }

    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        identity_for(&encoded(self)?)
    }

    pub fn identity_sha256(&self) -> Result<String, ProtocolError> {
        Ok(self.identity()?.sha256)
    }

    pub fn reply_binding(&self) -> Result<LiveCpuReplyBinding, ProtocolError> {
        LiveCpuReplyBinding::new(self)
    }

    pub fn validate_ready(&self, reply: &LiveInferenceReply) -> Result<(), ProtocolError> {
        self.reply_binding()?.validate_ready(reply)
    }

    pub fn validate_estimated(
        &self,
        issued: &LiveInferenceInput,
        reply: &LiveInferenceReply,
    ) -> Result<(), ProtocolError> {
        self.reply_binding()?.validate_estimated(issued, reply)
    }
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(format!("captured CPU live {message}"))
}
fn encoded<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    serde_json::to_vec(value).map_err(|_| invalid("metadata encoding failed"))
}
fn identity_for(bytes: &[u8]) -> Result<ArtifactIdentity, ProtocolError> {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())
            .map_err(|e| ProtocolError::invalid(e.to_string()))?,
        sha256,
        byte_len: bytes.len() as u64,
    })
}
fn dependency_closure(
    stream: &LiveInferenceManifest,
    image: &CpuRuntimeImage,
) -> Result<Vec<ArtifactIdentity>, ProtocolError> {
    image.validate()?;
    stream.profile.validate()?;
    let image_identity = image.identity()?;
    let profile_identity = identity_for(&encoded(&stream.profile)?)?;
    let mut identities = BTreeMap::<String, ArtifactIdentity>::new();
    for identity in [
        &image_identity,
        &image.owner_policy,
        &stream.model.identity,
        &stream.runtime.identity,
        &stream.worker,
        &profile_identity,
    ]
    .into_iter()
    .chain(image.libraries.iter().map(|library| &library.identity))
    {
        identity.validate()?;
        match identities.get(identity.artifact_id.as_str()) {
            Some(previous) if previous != identity => {
                return Err(invalid("one dependency ID names different bytes"));
            }
            Some(_) => {}
            None => {
                identities.insert(identity.artifact_id.to_string(), identity.clone());
            }
        }
    }
    if identities.len() > MAX_LIVE_CPU_DEPENDENCIES {
        return Err(invalid("dependency closure exceeds admission"));
    }
    Ok(identities.into_values().collect())
}

/// Cached expected message fields for a previously validated declaration.
/// This is intentionally not serializable. It verifies correspondence only:
/// it is freely constructible metadata, not native verification or a live permit.
pub struct LiveCpuReplyBinding {
    stream: LiveInferenceManifest,
    manifest_sha256: String,
    profile_sha256: String,
}
impl LiveCpuReplyBinding {
    pub fn new(manifest: &LiveCpuInferenceManifest) -> Result<Self, ProtocolError> {
        let manifest_sha256 = manifest.identity_sha256()?;
        let profile_sha256 = identity_for(&encoded(&manifest.stream.profile)?)?.sha256;
        Ok(Self {
            stream: manifest.stream.clone(),
            manifest_sha256,
            profile_sha256,
        })
    }
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }
    pub fn profile_sha256(&self) -> &str {
        &self.profile_sha256
    }
    fn matches(
        &self,
        attempt: &AttemptId,
        identity: &LiveIdentity,
        profile: &str,
        manifest: &str,
    ) -> bool {
        attempt == &self.stream.attempt_id
            && identity == &self.stream.identity
            && profile == self.profile_sha256
            && manifest == self.manifest_sha256
    }
    pub fn validate_ready(&self, reply: &LiveInferenceReply) -> Result<(), ProtocolError> {
        match reply {
            LiveInferenceReply::CapturedCpuReady {
                attempt_id,
                identity,
                profile_sha256,
                manifest_sha256,
            } if self.matches(attempt_id, identity, profile_sha256, manifest_sha256) => Ok(()),
            _ => Err(invalid(
                "Ready does not bind the original captured execution",
            )),
        }
    }
    /// The caller still owns ordering, pixel-byte verification, current lease
    /// and freshness. Exact equality prevents replacing the caller's issued work.
    pub fn validate_estimated(
        &self,
        issued: &LiveInferenceInput,
        reply: &LiveInferenceReply,
    ) -> Result<(), ProtocolError> {
        issued.validate(&self.stream)?;
        match reply {
            LiveInferenceReply::CapturedCpuEstimated {
                attempt_id,
                identity,
                profile_sha256,
                manifest_sha256,
                input,
                pair_analysis_elapsed_ns,
                ..
            } if self.matches(attempt_id, identity, profile_sha256, manifest_sha256)
                && input == issued
                && *pair_analysis_elapsed_ns > 0
                && *pair_analysis_elapsed_ns <= self.stream.budget.wall_time_ms * 1_000_000 =>
            {
                Ok(())
            }
            _ => Err(invalid(
                "estimate differs from the captured execution or issued input",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 10,
        }
    }
    fn fixture() -> LiveCpuInferenceManifest {
        let image = CpuRuntimeImage {
            schema_version: 1,
            owner_policy: artifact(3),
            libraries: CPU_RUNTIME_REQUIRED_ALIASES
                .iter()
                .enumerate()
                .map(|(i, name)| CapturedNativeLibrary {
                    identity: artifact(i as u8 + 10),
                    names: vec![name.to_string()],
                })
                .collect(),
        };
        let model = artifact(1);
        let runtime = artifact(12);
        let identity = LiveIdentity {
            epoch: AttemptId::new("epoch").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(2),
            source: SourceVersionId::new("source").unwrap(),
        };
        let stream = LiveInferenceManifest {
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
            model: ArtifactReference {
                path: LIVE_CPU_MODEL_PATH.into(),
                identity: model.clone(),
            },
            runtime: ArtifactReference {
                path: LIVE_CPU_RUNTIME_PATH.into(),
                identity: runtime.clone(),
            },
            worker: artifact(2),
            profile: DeclaredInferenceProfile {
                schema_version: 1,
                model,
                runtime,
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
                minimum_memory_bytes: 128 << 20,
                minimum_output_bytes: 4096,
                cpu_threads: 2,
                cuda_resources: None,
            },
            budget: ResourceBudget {
                memory_bytes: 256 << 20,
                output_bytes: 4096,
                wall_time_ms: 30_000,
                cpu_threads: 2,
            },
        };
        LiveCpuInferenceManifest::new(stream, image).unwrap()
    }
    fn input(manifest: &LiveCpuInferenceManifest) -> LiveInferenceInput {
        let mut pixels = artifact(15);
        pixels.byte_len = LIVE_FRAME_BYTES as u64;
        LiveInferenceInput {
            sequence: 1,
            pair: RawFramePair {
                source_epoch: manifest.stream.identity.epoch.clone(),
                previous_frame: FrameId::new(0),
                current_frame: FrameId::new(1),
                previous_time: SourceTimestamp::new(0, 1).unwrap(),
                current_time: SourceTimestamp::new(1, 10).unwrap(),
                presentation_clock: Some(manifest.stream.clock.clone()),
            },
            previous: pixels.clone(),
            current: pixels,
        }
    }
    fn ready(manifest: &LiveCpuInferenceManifest) -> LiveInferenceReply {
        let binding = manifest.reply_binding().unwrap();
        LiveInferenceReply::CapturedCpuReady {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: binding.profile_sha256().into(),
            manifest_sha256: binding.manifest_sha256().into(),
        }
    }
    fn estimated(
        manifest: &LiveCpuInferenceManifest,
        input: &LiveInferenceInput,
    ) -> LiveInferenceReply {
        let binding = manifest.reply_binding().unwrap();
        LiveInferenceReply::CapturedCpuEstimated {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: binding.profile_sha256().into(),
            manifest_sha256: binding.manifest_sha256().into(),
            input: input.clone(),
            position: NormalizedPosition::new(0.5).unwrap(),
            pair_analysis_elapsed_ns: 1,
        }
    }

    #[test]
    fn canonical_identity_binds_stream_image_policy_and_exact_dependency_closure() {
        let manifest = fixture();
        let bytes = encoded(&manifest).unwrap();
        let decoded: LiveCpuInferenceManifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.identity().unwrap(), manifest.identity().unwrap());
        assert_eq!(manifest.identity().unwrap().byte_len, bytes.len() as u64);
        assert!(manifest
            .dependencies
            .windows(2)
            .all(|pair| pair[0].artifact_id.as_str() < pair[1].artifact_id.as_str()));
        assert!(manifest
            .dependencies
            .contains(&manifest.image.identity().unwrap()));
        for mutate in [
            |m: &mut LiveCpuInferenceManifest| m.image.owner_policy = artifact(4),
            |m: &mut LiveCpuInferenceManifest| m.stream.worker = artifact(5),
            |m: &mut LiveCpuInferenceManifest| {
                m.stream.identity.source = SourceVersionId::new("other-source").unwrap()
            },
            |m: &mut LiveCpuInferenceManifest| m.stream.clock.added_buffer_ns = 1,
            |m: &mut LiveCpuInferenceManifest| m.stream.budget.wall_time_ms = 29_000,
            |m: &mut LiveCpuInferenceManifest| {
                m.image.libraries[0].names.push("loader-extra.so".into())
            },
        ] {
            let mut changed = manifest.clone();
            mutate(&mut changed);
            let changed = LiveCpuInferenceManifest::new(changed.stream, changed.image).unwrap();
            assert_ne!(
                changed.identity_sha256().unwrap(),
                manifest.identity_sha256().unwrap()
            );
        }
    }

    #[test]
    fn missing_extra_duplicate_conflicting_or_reordered_dependencies_are_refused() {
        let base = fixture();
        for mutate in [
            |m: &mut LiveCpuInferenceManifest| {
                m.dependencies.pop();
            },
            |m: &mut LiveCpuInferenceManifest| m.dependencies.push(artifact(20)),
            |m: &mut LiveCpuInferenceManifest| m.dependencies.push(m.dependencies[0].clone()),
            |m: &mut LiveCpuInferenceManifest| m.dependencies.swap(0, 1),
            |m: &mut LiveCpuInferenceManifest| m.dependencies[0].byte_len += 1,
        ] {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert!(changed.validate().is_err());
        }
        let mut stream = base.stream.clone();
        stream.worker.artifact_id = stream.model.identity.artifact_id.clone();
        assert!(LiveCpuInferenceManifest::new(stream, base.image).is_err());
    }

    #[test]
    fn host_runtime_provider_path_and_stream_scope_substitution_are_refused() {
        let base = fixture();
        for mutate in [
            |m: &mut LiveCpuInferenceManifest| m.stream.runtime.path = "/input/runtime".into(),
            |m: &mut LiveCpuInferenceManifest| m.stream.model.path = "/input/./model".into(),
            |m: &mut LiveCpuInferenceManifest| m.stream.runtime.identity = artifact(25),
            |m: &mut LiveCpuInferenceManifest| m.stream.profile.runtime = artifact(25),
            |m: &mut LiveCpuInferenceManifest| m.stream.profile.provider = InferenceProvider::Cuda,
            |m: &mut LiveCpuInferenceManifest| {
                m.stream.clock.epoch = AttemptId::new("other-epoch").unwrap()
            },
            |m: &mut LiveCpuInferenceManifest| m.stream.profile.preprocessing.scale[0] = 0.5,
            |m: &mut LiveCpuInferenceManifest| m.stream.budget.wall_time_ms = 30_001,
            |m: &mut LiveCpuInferenceManifest| m.stream.budget.wall_time_ms = 0,
            |m: &mut LiveCpuInferenceManifest| m.stream.worker.byte_len = 0,
        ] {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert!(changed.validate().is_err());
        }
        let mut changed = base.clone();
        changed.stream.runtime.identity = artifact(25);
        changed.stream.profile.runtime = artifact(25);
        assert!(LiveCpuInferenceManifest::new(changed.stream, changed.image).is_err());
    }

    #[test]
    fn captured_ready_cannot_be_replaced_by_host_or_another_captured_image() {
        let manifest = fixture();
        let reply = ready(&manifest);
        manifest.validate_ready(&reply).unwrap();
        let binding = manifest.reply_binding().unwrap();
        let host = LiveInferenceReply::Ready {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: binding.profile_sha256().into(),
        };
        assert!(binding.validate_ready(&host).is_err());
        let mut changed = manifest.clone();
        changed.image.owner_policy = artifact(4);
        let changed = LiveCpuInferenceManifest::new(changed.stream, changed.image).unwrap();
        assert_eq!(
            changed.reply_binding().unwrap().profile_sha256(),
            binding.profile_sha256()
        );
        assert!(changed.validate_ready(&reply).is_err());
        let mut changed_reply = ready(&manifest);
        if let LiveInferenceReply::CapturedCpuReady { attempt_id, .. } = &mut changed_reply {
            *attempt_id = AttemptId::new("other-attempt").unwrap();
        }
        assert!(binding.validate_ready(&changed_reply).is_err());
    }

    #[test]
    fn captured_estimates_bind_issued_work_clock_pixels_and_analysis_duration() {
        let manifest = fixture();
        let issued = input(&manifest);
        let reply = estimated(&manifest, &issued);
        let binding = manifest.reply_binding().unwrap();
        binding.validate_estimated(&issued, &reply).unwrap();
        for mutate in [
            |input: &mut LiveInferenceInput| input.sequence += 1,
            |input: &mut LiveInferenceInput| {
                input.current = ArtifactIdentity {
                    byte_len: LIVE_FRAME_BYTES as u64,
                    ..artifact(16)
                }
            },
            |input: &mut LiveInferenceInput| {
                input.pair.source_epoch = AttemptId::new("other-epoch").unwrap()
            },
            |input: &mut LiveInferenceInput| {
                input
                    .pair
                    .presentation_clock
                    .as_mut()
                    .unwrap()
                    .added_buffer_ns = 1
            },
        ] {
            let mut changed = issued.clone();
            mutate(&mut changed);
            assert!(binding.validate_estimated(&changed, &reply).is_err());
        }
        for duration in [0, 30_000_000_001] {
            let mut changed = estimated(&manifest, &issued);
            if let LiveInferenceReply::CapturedCpuEstimated {
                pair_analysis_elapsed_ns,
                ..
            } = &mut changed
            {
                *pair_analysis_elapsed_ns = duration;
            }
            assert!(binding.validate_estimated(&issued, &changed).is_err());
        }
        let host = LiveInferenceReply::Estimated {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: binding.profile_sha256().into(),
            input: issued.clone(),
            position: NormalizedPosition::new(0.5).unwrap(),
            pair_analysis_elapsed_ns: 1,
        };
        assert!(binding.validate_estimated(&issued, &host).is_err());
        assert!(binding
            .validate_estimated(&issued, &ready(&manifest))
            .is_err());
    }

    #[test]
    fn strict_serialization_does_not_add_authority_or_change_legacy_reply_bytes() {
        let manifest = fixture();
        let mut value = serde_json::to_value(&manifest).unwrap();
        for key in ["qualified", "deadline_ns", "approved", "authority"] {
            value[key] = true.into();
            assert!(serde_json::from_value::<LiveCpuInferenceManifest>(value.clone()).is_err());
            value.as_object_mut().unwrap().remove(key);
        }
        let old = LiveInferenceReply::Ready {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: "a".repeat(64),
        };
        let encoded = serde_json::to_value(old).unwrap();
        assert_eq!(encoded["status"], "ready");
        assert_eq!(encoded["payload"].as_object().unwrap().len(), 3);
        let captured = ready(&manifest);
        let mut encoded = serde_json::to_value(captured).unwrap();
        assert_eq!(encoded["status"], "captured_cpu_ready");
        encoded["payload"]["qualified"] = true.into();
        assert!(serde_json::from_value::<LiveInferenceReply>(encoded).is_err());
    }
}
