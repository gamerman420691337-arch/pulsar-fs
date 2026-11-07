//! Private CUDA live-stream metadata and ordered execution transcript.
//! Neither declaration nor transcript grants launch, freshness or motion authority.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, PathBuf};

pub const MAX_LIVE_CUDA_RUNS: u64 = 16_384;
/// Indexed identities of retained original bytes. These declarations alone do
/// not authenticate execution, current authority or motion quality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCudaEvidenceRecord {
    pub schema_version: u16,
    pub qualified: bool,
    pub manifest: ArtifactIdentity,
    pub summary: LiveCudaStreamSummary,
    pub receipt: ArtifactIdentity,
    pub trace: ArtifactIdentity,
}
impl LiveCudaEvidenceRecord {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        for (identity, limit) in [
            (&self.manifest, 1024 * 1024),
            (&self.receipt, 65536),
            (&self.trace, 8 * 1024 * 1024),
        ] {
            identity.validate()?;
            if identity.artifact_id.as_str() != identity.sha256
                || identity.byte_len == 0
                || identity.byte_len > limit
            {
                return Err(ProtocolError::invalid(
                    "live CUDA retained artifact identity or byte bound",
                ));
            }
        }
        validate_sha256(&self.summary.transcript_sha256)?;
        if self.schema_version != 1
            || self.qualified
            || self.summary.schema_version != 1
            || self.summary.manifest_sha256 != self.manifest.sha256
            || !(1..=MAX_LIVE_CUDA_RUNS).contains(&self.summary.successful_runs)
            || self.summary.last_sequence != self.summary.successful_runs
            || self.summary.last_frame.value() < self.summary.successful_runs
        {
            return Err(ProtocolError::invalid(
                "live CUDA retained summary correspondence",
            ));
        }
        Ok(())
    }
}
/// Selects original retained bytes, never a filesystem locator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCudaEvidencePart {
    Origin,
    Manifest,
    Summary,
    Receipt,
    Trace,
}
impl LiveCudaEvidencePart {
    pub fn maximum_bytes(self) -> u64 {
        match self {
            Self::Origin | Self::Manifest => 1048576,
            Self::Summary => 4096,
            Self::Receipt => 65536,
            Self::Trace => 8388608,
        }
    }
}
/// Historical descriptors, available independently of an active capture lease.
/// Current project Read authority is still required; no original paths are opened.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedLiveCudaEvidence {
    pub schema_version: u16,
    pub engine_epoch: AttemptId,
    pub identity: LiveIdentity,
    pub owner: SessionId,
    pub start_request: RequestId,
    pub evidence: LiveCudaEvidenceRecord,
}
impl RetainedLiveCudaEvidence {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1 {
            return Err(ProtocolError::invalid(
                "retained live CUDA inspection version",
            ));
        }
        self.evidence.validate()
    }
}
/// A logical retained-row removal receipt, not a secure-erasure certificate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCudaEvidenceRemoval {
    pub schema_version: u16,
    pub project_id: ProjectId,
    pub epoch: AttemptId,
    pub manifest: ArtifactIdentity,
    pub removed_original_bytes: u64,
    pub physical_erasure: bool,
}
impl LiveCudaEvidenceRemoval {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.manifest.validate()?;
        if self.schema_version != 1
            || self.physical_erasure
            || self.manifest.artifact_id.as_str() != self.manifest.sha256
            || self.manifest.byte_len == 0
            || self.manifest.byte_len > 1048576
            || self.removed_original_bytes < self.manifest.byte_len
            || self.removed_original_bytes > 2 * 1048576 + 4096 + 65536 + 8388608
        {
            return Err(ProtocolError::invalid(
                "retained CUDA removal receipt bound or erasure claim",
            ));
        }
        Ok(())
    }
}
/// Bounded database index, not validation of original execution evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCudaEvidencePage {
    pub project: ProjectId,
    pub after: Option<AttemptId>,
    pub limit: u16,
    pub entries: Vec<LiveIdentity>,
    pub next_after: Option<AttemptId>,
}
impl LiveCudaEvidencePage {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !(1..=16).contains(&self.limit) || self.entries.len() > usize::from(self.limit) {
            return Err(ProtocolError::invalid("retained CUDA listing page bound"));
        }
        let mut previous = self.after.as_ref().map(|id| id.as_str());
        for entry in &self.entries {
            if entry.project != self.project
                || previous.is_some_and(|id| entry.epoch.as_str() <= id)
            {
                return Err(ProtocolError::invalid(
                    "retained CUDA listing scope or order",
                ));
            }
            previous = Some(entry.epoch.as_str());
        }
        if let Some(next) = &self.next_after {
            if self.entries.len() != usize::from(self.limit)
                || self.entries.last().map(|e| &e.epoch) != Some(next)
            {
                return Err(ProtocolError::invalid("retained CUDA listing continuation"));
            }
        }
        Ok(())
    }
}
const TRANSCRIPT_DOMAIN: &[u8] = b"PULSAR-LIVE-CUDA-TRANSCRIPT-V1\0";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCudaInferenceManifest {
    pub schema_version: u16,
    pub stream: LiveInferenceManifest,
    pub execution: WorkerCudaExecution,
    pub dependencies: Vec<ArtifactIdentity>,
    pub output_dir: PathBuf,
}
impl LiveCudaInferenceManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1 || self.dependencies.len() > 260 {
            return Err(ProtocolError::invalid(
                "CUDA live envelope version or dependency admission",
            ));
        }
        self.stream.validate_metadata()?;
        if self.stream.profile.provider != InferenceProvider::Cuda {
            return Err(ProtocolError::invalid(
                "CUDA live envelope requires a CUDA profile",
            ));
        }
        if !self.output_dir.is_absolute()
            || self.output_dir.parent().is_none()
            || self.output_dir.to_string_lossy().len() > 4096
            || self
                .output_dir
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(ProtocolError::invalid(
                "CUDA live evidence directory admission",
            ));
        }
        for identity in &self.dependencies {
            identity.validate()?;
        }
        for identity in [
            &self.stream.model.identity,
            &self.stream.runtime.identity,
            &self.stream.worker,
        ] {
            if !self.dependencies.contains(identity) {
                return Err(ProtocolError::invalid(
                    "CUDA live input identity lacks dependency membership",
                ));
            }
        }
        self.execution.validate(
            &self.stream.profile,
            &self.stream.budget,
            &self.dependencies,
        )?;
        if self.stream.runtime.path != self.execution.library_directory.join("libonnxruntime.so.1")
            || self
                .output_dir
                .starts_with(&self.execution.library_directory)
            || self
                .execution
                .library_directory
                .starts_with(&self.output_dir)
            || self.stream.model.path.starts_with(&self.output_dir)
        {
            return Err(ProtocolError::invalid(
                "CUDA live capture/runtime/evidence paths differ",
            ));
        }
        if !self
            .execution
            .libraries
            .iter()
            .any(|lib| lib.names.iter().any(|name| name == "libnvidia-ml.so.1"))
        {
            return Err(ProtocolError::invalid(
                "CUDA live envelope requires captured memory observer dependency",
            ));
        }
        let bytes = serde_json::to_vec(self).map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(ProtocolError::invalid(
                "CUDA live envelope exceeds control bound",
            ));
        }
        Ok(())
    }
    pub fn identity_sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(self).map_err(|e| ProtocolError::invalid(e.to_string()))?
            )
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCudaStreamSummary {
    pub schema_version: u16,
    pub manifest_sha256: String,
    pub successful_runs: u64,
    pub last_sequence: u64,
    pub last_frame: FrameId,
    pub last_source_time: SourceTimestamp,
    pub transcript_sha256: String,
}
impl LiveCudaStreamSummary {
    pub fn validate(&self, manifest: &LiveCudaInferenceManifest) -> Result<(), ProtocolError> {
        if self.schema_version != 1
            || !(1..=MAX_LIVE_CUDA_RUNS).contains(&self.successful_runs)
            || self.last_sequence != self.successful_runs
            || self.last_frame.value() < self.successful_runs
            || self.manifest_sha256 != manifest.identity_sha256()?
            || self.transcript_sha256.len() != 64
            || !self
                .transcript_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ProtocolError::invalid(
                "CUDA live stream summary correspondence",
            ));
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
pub enum LiveCudaStreamCommand {
    FramePair { input: Box<LiveInferenceInput> },
    Finish { expected: LiveCudaStreamSummary },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LiveCudaStreamReply {
    /// A negative provider-list observation before Ready. This is inert
    /// metadata, not proof of original transport, worker retirement or retry
    /// authority. The parent must bind it to its exact admitted manifest.
    ProviderUnavailable {
        manifest_sha256: String,
        observation: InferenceProviderUnavailable,
    },
    Ready {
        manifest_sha256: String,
    },
    Estimated {
        manifest_sha256: String,
        input: LiveInferenceInput,
        position: NormalizedPosition,
        pair_analysis_elapsed_ns: u64,
    },
    Finished {
        summary: LiveCudaStreamSummary,
        artifacts: WorkerCudaArtifacts,
    },
    Error(ProtocolError),
}

impl LiveCudaStreamReply {
    /// Validate only the declared correspondence of an initialization failure.
    /// No other reply or error message can substitute for this typed variant.
    pub fn validate_provider_unavailable(
        &self,
        manifest: &LiveCudaInferenceManifest,
    ) -> Result<&InferenceProviderUnavailable, ProtocolError> {
        let Self::ProviderUnavailable {
            manifest_sha256,
            observation,
        } = self
        else {
            return Err(ProtocolError::invalid(
                "live CUDA provider observation absent",
            ));
        };
        if *manifest_sha256 != manifest.identity_sha256()? {
            return Err(ProtocolError::invalid(
                "live CUDA provider observation belongs to another manifest",
            ));
        }
        let profile_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&manifest.stream.profile).map_err(|_| {
                ProtocolError::invalid("live CUDA profile serialization failed")
            })?)
        );
        observation.validate_for(&manifest.stream.profile, &profile_sha256)?;
        Ok(observation)
    }
}

/// Process-local state for every successful exchange, even if its output later
/// becomes stale. Not serializable, resumable or evidence of native execution.
pub struct LiveCudaTranscript {
    manifest: LiveCudaInferenceManifest,
    manifest_sha256: String,
    sequence: u64,
    last_frame: Option<FrameId>,
    last_source_time: Option<SourceTimestamp>,
    last_pixels: Option<ArtifactIdentity>,
    digest: [u8; 32],
    poisoned: bool,
}
impl LiveCudaTranscript {
    pub fn new(manifest: &LiveCudaInferenceManifest) -> Result<Self, ProtocolError> {
        let manifest_sha256 = manifest.identity_sha256()?;
        let mut hash = Sha256::new();
        hash.update(TRANSCRIPT_DOMAIN);
        hash.update(manifest_sha256.as_bytes());
        Ok(Self {
            manifest: manifest.clone(),
            manifest_sha256,
            sequence: 0,
            last_frame: None,
            last_source_time: None,
            last_pixels: None,
            digest: hash.finalize().into(),
            poisoned: false,
        })
    }
    /// Invalid exchange poisons the transcript permanently; callers must retire
    /// the worker rather than omit a failed exchange and resume the proof chain.
    pub fn append(
        &mut self,
        input: &LiveInferenceInput,
        position: NormalizedPosition,
        elapsed_ns: u64,
    ) -> Result<(), ProtocolError> {
        let result = self.append_inner(input, position, elapsed_ns);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn append_inner(
        &mut self,
        input: &LiveInferenceInput,
        position: NormalizedPosition,
        elapsed_ns: u64,
    ) -> Result<(), ProtocolError> {
        self.validate_input(input)?;
        if elapsed_ns == 0 || elapsed_ns > self.manifest.stream.budget.wall_time_ms * 1_000_000 {
            return Err(ProtocolError::invalid(
                "CUDA live exchange duration differs",
            ));
        }
        let bytes = serde_json::to_vec(&(input, position, elapsed_ns))
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        let mut hash = Sha256::new();
        hash.update(TRANSCRIPT_DOMAIN);
        hash.update(self.digest);
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(&bytes);
        self.digest = hash.finalize().into();
        self.sequence = input.sequence;
        self.last_frame = Some(input.pair.current_frame);
        self.last_source_time = Some(input.pair.current_time);
        self.last_pixels = Some(input.current.clone());
        Ok(())
    }
    /// Check the next issued pair before native execution. Failure permanently
    /// retires the transcript; successful preflight does not advance its chain.
    pub fn validate_input(&mut self, input: &LiveInferenceInput) -> Result<(), ProtocolError> {
        let result = self.validate_input_inner(input);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn validate_input_inner(&self, input: &LiveInferenceInput) -> Result<(), ProtocolError> {
        if self.poisoned || self.sequence >= MAX_LIVE_CUDA_RUNS {
            return Err(ProtocolError::invalid(
                "CUDA live transcript poisoned or exhausted",
            ));
        }
        input.validate(&self.manifest.stream)?;
        if let (Some(last), Some(time), Some(pixels)) =
            (self.last_frame, self.last_source_time, &self.last_pixels)
        {
            let previous = input.pair.previous_time;
            let clock_order = (i128::from(previous.numerator()) * i128::from(time.denominator()))
                .cmp(&(i128::from(time.numerator()) * i128::from(previous.denominator())));
            if input.pair.previous_frame < last
                || (input.pair.previous_frame == last
                    && (clock_order != std::cmp::Ordering::Equal || input.previous != *pixels))
                || (input.pair.previous_frame > last && clock_order != std::cmp::Ordering::Greater)
            {
                return Err(ProtocolError::invalid(
                    "CUDA live previous frame identity or clock contradicts prior exchange",
                ));
            }
        }
        if input.sequence != self.sequence + 1
            || self
                .last_frame
                .is_some_and(|frame| input.pair.current_frame <= frame)
            || self.last_source_time.is_some_and(|last| {
                let current = input.pair.current_time;
                i128::from(last.numerator()) * i128::from(current.denominator())
                    >= i128::from(current.numerator()) * i128::from(last.denominator())
            })
        {
            return Err(ProtocolError::invalid(
                "CUDA live exchange order or duration differs",
            ));
        }
        Ok(())
    }
    pub fn finish(self) -> Result<LiveCudaStreamSummary, ProtocolError> {
        if self.poisoned || self.sequence == 0 {
            return Err(ProtocolError::invalid(
                "empty or poisoned CUDA live transcript cannot finish",
            ));
        }
        Ok(LiveCudaStreamSummary {
            schema_version: 1,
            manifest_sha256: self.manifest_sha256,
            successful_runs: self.sequence,
            last_sequence: self.sequence,
            last_frame: self.last_frame.expect("nonempty transcript"),
            last_source_time: self.last_source_time.expect("nonempty transcript"),
            transcript_sha256: self
                .digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
    }
}

#[cfg(test)]
pub(crate) fn authored_live_cuda_manifest() -> LiveCudaInferenceManifest {
    tests::fixture()
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> LiveCudaInferenceManifest {
        let mut stream: LiveInferenceManifest =
            serde_json::from_str(include_str!("../tests/fixtures/live-cpu-manifest.json")).unwrap();
        stream.model.path = crate::test_host_path("/input/model").into();
        let names = [
            "ld-linux-x86-64.so.2",
            "libcuda.so.1",
            "libonnxruntime.so.1",
            "libonnxruntime_providers_cuda.so",
            "libonnxruntime_providers_shared.so",
            "libcudnn.so.9",
            "libnvidia-ml.so.1",
        ];
        let libraries: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let sha = format!("{:064x}", i + 1);
                CapturedNativeLibrary {
                    identity: ArtifactIdentity {
                        artifact_id: ArtifactId::new(sha.clone()).unwrap(),
                        sha256: sha,
                        byte_len: 10,
                    },
                    names: vec![(*name).into()],
                }
            })
            .collect();
        stream.profile.provider = InferenceProvider::Cuda;
        stream.profile.schema_version = 2;
        stream.profile.runtime = libraries[2].identity.clone();
        stream.runtime.identity = libraries[2].identity.clone();
        stream.runtime.path =
            crate::test_host_path("/captured/libraries/libonnxruntime.so.1").into();
        stream.budget.output_bytes = 16 << 20;
        let uuid = "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351";
        stream.profile.cuda_resources = Some(CudaResourceLimits {
            device_uuid: uuid.into(),
            device_ordinal: 0,
            address_space_bytes: 16 << 30,
            device_reservation_bytes: 64 << 20,
            arena_limit_bytes: 16 << 20,
        });
        let owner = ArtifactIdentity {
            artifact_id: ArtifactId::new("f".repeat(64)).unwrap(),
            sha256: "f".repeat(64),
            byte_len: 10,
        };
        let mut dependencies = vec![
            owner.clone(),
            stream.model.identity.clone(),
            stream.worker.clone(),
        ];
        dependencies.extend(libraries.iter().map(|lib| lib.identity.clone()));
        LiveCudaInferenceManifest {
            schema_version: 1,
            stream,
            execution: WorkerCudaExecution {
                schema_version: 1,
                owner_policy: owner,
                device_uuid: uuid.into(),
                pci_bus_address: "0000:01:00.0".into(),
                device_minor: 0,
                device_ordinal: 0,
                library_directory: crate::test_host_path("/captured/libraries").into(),
                libraries,
            },
            dependencies,
            output_dir: crate::test_host_path("/work").into(),
        }
    }
    fn input(manifest: &LiveCudaInferenceManifest, sequence: u64) -> LiveInferenceInput {
        let sha = "b".repeat(64);
        let pixels = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha.clone()).unwrap(),
            sha256: sha,
            byte_len: LIVE_FRAME_BYTES as u64,
        };
        LiveInferenceInput {
            sequence,
            pair: RawFramePair {
                source_epoch: manifest.stream.identity.epoch.clone(),
                previous_frame: FrameId::new(sequence - 1),
                current_frame: FrameId::new(sequence),
                previous_time: SourceTimestamp::new((sequence - 1) as i64, 1000).unwrap(),
                current_time: SourceTimestamp::new(sequence as i64, 1000).unwrap(),
                presentation_clock: Some(manifest.stream.clock.clone()),
            },
            previous: pixels.clone(),
            current: pixels,
        }
    }
    #[test]
    fn provider_absence_requires_the_exact_typed_initial_manifest() {
        let manifest = fixture();
        let observation = InferenceProviderUnavailable {
            schema_version: 1,
            provider: InferenceProvider::Cuda,
            profile_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&manifest.stream.profile).unwrap())
            ),
            check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
        };
        let reply = LiveCudaStreamReply::ProviderUnavailable {
            manifest_sha256: manifest.identity_sha256().unwrap(),
            observation,
        };
        reply.validate_provider_unavailable(&manifest).unwrap();
        let bytes = serde_json::to_vec(&reply).unwrap();
        serde_json::from_slice::<LiveCudaStreamReply>(&bytes)
            .unwrap()
            .validate_provider_unavailable(&manifest)
            .unwrap();
        for case in [
            "attempt",
            "epoch",
            "clock",
            "budget",
            "worker",
            "dependency",
        ] {
            let mut changed = manifest.clone();
            match case {
                "attempt" => changed.stream.attempt_id = AttemptId::new("other-attempt").unwrap(),
                "epoch" => {
                    changed.stream.identity.epoch = AttemptId::new("other-epoch").unwrap();
                    changed.stream.clock.epoch = changed.stream.identity.epoch.clone();
                }
                "clock" => changed.stream.clock.presentation_anchor = LiveClockTick(1),
                "budget" => changed.stream.budget.wall_time_ms -= 1,
                "worker" => changed.stream.worker.byte_len += 1,
                _ => changed.dependencies.pop().map(|_| ()).unwrap(),
            }
            assert!(
                reply.validate_provider_unavailable(&changed).is_err(),
                "{case}"
            );
        }
        for (field, value) in [
            ("provider", serde_json::json!("cpu")),
            ("profile_sha256", serde_json::json!("0".repeat(64))),
            ("schema_version", serde_json::json!(2)),
        ] {
            let mut value_reply = serde_json::to_value(&reply).unwrap();
            value_reply["payload"]["observation"][field] = value;
            let changed: LiveCudaStreamReply = serde_json::from_value(value_reply).unwrap();
            assert!(
                changed.validate_provider_unavailable(&manifest).is_err(),
                "{field}"
            );
        }
        let mut unknown = serde_json::to_value(&reply).unwrap();
        unknown["payload"]["observation"]["check"] = serde_json::json!("initialization_failed");
        assert!(serde_json::from_value::<LiveCudaStreamReply>(unknown).is_err());
        for generic in [
            LiveCudaStreamReply::Error(ProtocolError::new(
                ErrorCode::Unavailable,
                "selected CUDA provider is not listed by the initialized runtime",
            )),
            LiveCudaStreamReply::Ready {
                manifest_sha256: manifest.identity_sha256().unwrap(),
            },
        ] {
            assert!(generic.validate_provider_unavailable(&manifest).is_err());
        }
    }

    #[test]
    fn consecutive_pairs_cannot_rebind_previous_frame_pixels_or_clock() {
        let manifest = fixture();
        for case in [
            "pixels",
            "clock",
            "backwards-frame",
            "ahead-frame-old-clock",
        ] {
            let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
            transcript
                .append(
                    &input(&manifest, 1),
                    NormalizedPosition::new(0.5).unwrap(),
                    1,
                )
                .unwrap();
            let mut next = input(&manifest, 2);
            match case {
                "pixels" => {
                    next.previous.sha256 = "0".repeat(64);
                    next.previous.artifact_id = ArtifactId::new("0".repeat(64)).unwrap();
                }
                "clock" => next.pair.previous_time = SourceTimestamp::new(3, 2000).unwrap(),
                "backwards-frame" => next.pair.previous_frame = FrameId::new(0),
                "ahead-frame-old-clock" => {
                    next.pair.previous_frame = FrameId::new(2);
                    next.pair.current_frame = FrameId::new(3);
                }
                _ => unreachable!(),
            }
            assert!(transcript.validate_input(&next).is_err(), "{case}");
            assert!(transcript.finish().is_err());
        }
        let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
        transcript
            .append(
                &input(&manifest, 1),
                NormalizedPosition::new(0.5).unwrap(),
                1,
            )
            .unwrap();
        let mut equivalent = input(&manifest, 2);
        equivalent.pair.previous_time = SourceTimestamp::new(2, 2000).unwrap();
        transcript
            .append(&equivalent, NormalizedPosition::new(0.5).unwrap(), 1)
            .unwrap();
        let skipped = input(&manifest, 5);
        transcript
            .append(&skipped, NormalizedPosition::new(0.5).unwrap(), 1)
            .unwrap_err();
        let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
        transcript
            .append(
                &input(&manifest, 1),
                NormalizedPosition::new(0.5).unwrap(),
                1,
            )
            .unwrap();
        let mut skipped = input(&manifest, 5);
        skipped.sequence = 2;
        transcript
            .append(&skipped, NormalizedPosition::new(0.5).unwrap(), 1)
            .unwrap();
        assert_eq!(transcript.finish().unwrap().last_frame, FrameId::new(5));
    }
    #[test]
    fn native_float32_positions_roundtrip_without_changing_transcript() {
        let manifest = fixture();
        let issued = input(&manifest, 1);
        for sample in 0..=255 {
            let position =
                NormalizedPosition::new(f64::from(0.25f32 + 0.25f32 * (sample as f32 / 255.0)))
                    .unwrap();
            let reply = LiveCudaStreamReply::Estimated {
                manifest_sha256: manifest.identity_sha256().unwrap(),
                input: issued.clone(),
                position,
                pair_analysis_elapsed_ns: 12345,
            };
            let bytes = serde_json::to_vec(&reply).unwrap();
            let decoded: LiveCudaStreamReply = serde_json::from_slice(&bytes).unwrap();
            let LiveCudaStreamReply::Estimated {
                input: returned,
                position: decoded,
                pair_analysis_elapsed_ns,
                ..
            } = decoded
            else {
                unreachable!()
            };
            assert_eq!(
                position.value().to_bits(),
                decoded.value().to_bits(),
                "native float sample {sample}"
            );
            let mut worker = LiveCudaTranscript::new(&manifest).unwrap();
            let mut parent = LiveCudaTranscript::new(&manifest).unwrap();
            worker.append(&issued, position, 12345).unwrap();
            parent
                .append(&returned, decoded, pair_analysis_elapsed_ns)
                .unwrap();
            assert_eq!(worker.finish().unwrap(), parent.finish().unwrap());
        }
    }
    #[test]
    fn input_preflight_refuses_replay_before_run_and_permanently_poisoned_finish() {
        let manifest = fixture();
        let first = input(&manifest, 1);
        let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
        transcript.validate_input(&first).unwrap();
        // Preflight is not a successful native run or advancement.
        assert!(transcript.finish().is_err());
        let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
        transcript.validate_input(&first).unwrap();
        transcript
            .append(&first, NormalizedPosition::new(0.25).unwrap(), 1)
            .unwrap();
        assert!(transcript.validate_input(&first).is_err());
        assert!(transcript.validate_input(&input(&manifest, 2)).is_err());
        assert!(transcript.finish().is_err());
    }
    #[test]
    fn envelope_requires_captured_dependencies_and_never_enters_ordinary_cpu_worker() {
        let base = fixture();
        base.validate().unwrap();
        assert!(base.stream.validate().is_err());
        let bytes = serde_json::to_vec(&base).unwrap();
        assert!(serde_json::from_slice::<LiveInferenceManifest>(&bytes).is_err());
        let decoded: LiveCudaInferenceManifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            decoded.identity_sha256().unwrap(),
            base.identity_sha256().unwrap()
        );
        let mut bad = base.clone();
        bad.dependencies
            .retain(|x| x != &base.stream.model.identity);
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.execution.device_ordinal = 1;
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.execution.libraries.pop();
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.output_dir = crate::test_host_path("/work/../outside").into();
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.output_dir = "/".into();
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.stream.runtime.path = crate::test_host_path("/supplier/runtime").into();
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.output_dir = crate::test_host_path("/captured").into();
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.stream.clock.epoch = AttemptId::new("other").unwrap();
        assert!(bad.validate().is_err());
        let mut bad = base;
        bad.stream.profile.provider = InferenceProvider::Cpu;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn transcript_matches_independent_length_delimited_chain_and_counts_all_successes() {
        let manifest = fixture();
        let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
        let mut seed = Sha256::new();
        seed.update(b"PULSAR-LIVE-CUDA-TRANSCRIPT-V1\0");
        seed.update(manifest.identity_sha256().unwrap().as_bytes());
        let mut digest: [u8; 32] = seed.finalize().into();
        for sequence in 1..=3 {
            let issued = input(&manifest, sequence);
            let position = NormalizedPosition::new(sequence as f64 / 10.0).unwrap();
            transcript.append(&issued, position, 7).unwrap();
            let record = serde_json::to_vec(&(issued, position, 7u64)).unwrap();
            let mut preimage = b"PULSAR-LIVE-CUDA-TRANSCRIPT-V1\0".to_vec();
            preimage.extend_from_slice(&digest);
            preimage.extend_from_slice(&(record.len() as u64).to_be_bytes());
            preimage.extend_from_slice(&record);
            digest = Sha256::digest(&preimage).into();
        }
        let summary = transcript.finish().unwrap();
        summary.validate(&manifest).unwrap();
        assert_eq!(summary.successful_runs, 3);
        assert_eq!(summary.last_sequence, 3);
        assert_eq!(summary.last_frame, FrameId::new(3));
        assert_eq!(
            summary.transcript_sha256,
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let bytes = serde_json::to_vec(&LiveCudaStreamCommand::Finish {
            expected: summary.clone(),
        })
        .unwrap();
        let LiveCudaStreamCommand::Finish { expected } = serde_json::from_slice(&bytes).unwrap()
        else {
            panic!("finish")
        };
        assert_eq!(expected, summary);
        assert!(serde_json::from_slice::<LiveInferenceInput>(&bytes).is_err());
    }
    #[test]
    fn failed_or_replayed_exchange_permanently_poisoned_and_empty_finish_refused() {
        let manifest = fixture();
        assert!(LiveCudaTranscript::new(&manifest)
            .unwrap()
            .finish()
            .is_err());
        for kind in [
            "sequence",
            "clock",
            "pixels",
            "duration",
            "duration-zero",
            "frame",
            "source-time",
        ] {
            let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
            transcript
                .append(
                    &input(&manifest, 1),
                    NormalizedPosition::new(0.5).unwrap(),
                    1,
                )
                .unwrap();
            let mut changed = input(&manifest, 2);
            let mut duration = 1;
            match kind {
                "sequence" => changed.sequence = 1,
                "clock" => changed.pair.presentation_clock = None,
                "pixels" => changed.current.byte_len = 1,
                "duration" => duration = 30_000_000_001,
                "duration-zero" => duration = 0,
                "source-time" => {
                    changed.pair.previous_time = SourceTimestamp::new(0, 1000).unwrap();
                    changed.pair.current_time = SourceTimestamp::new(1, 1000).unwrap();
                }
                "frame" => {
                    changed.pair.previous_frame = FrameId::new(0);
                    changed.pair.current_frame = FrameId::new(1);
                }
                _ => unreachable!(),
            }
            assert!(
                transcript
                    .append(&changed, NormalizedPosition::new(0.5).unwrap(), duration)
                    .is_err(),
                "{kind}"
            );
            assert!(transcript
                .append(
                    &input(&manifest, 2),
                    NormalizedPosition::new(0.5).unwrap(),
                    1
                )
                .is_err());
            assert!(transcript.finish().is_err());
        }
    }
    #[test]
    fn manifest_result_and_timing_changes_cannot_reuse_transcript_digest() {
        let base = fixture();
        let summary = |manifest: &LiveCudaInferenceManifest, position, duration| {
            let mut t = LiveCudaTranscript::new(manifest).unwrap();
            t.append(
                &input(manifest, 1),
                NormalizedPosition::new(position).unwrap(),
                duration,
            )
            .unwrap();
            t.finish().unwrap()
        };
        let original = summary(&base, 0.5, 1);
        assert_ne!(
            original.transcript_sha256,
            summary(&base, 0.6, 1).transcript_sha256
        );
        assert_ne!(
            original.transcript_sha256,
            summary(&base, 0.5, 2).transcript_sha256
        );
        let mut changed = base.clone();
        changed.stream.attempt_id = AttemptId::new("fresh-attempt").unwrap();
        assert_ne!(
            original.transcript_sha256,
            summary(&changed, 0.5, 1).transcript_sha256
        );
        assert!(original.validate(&changed).is_err());
        for kind in ["count", "sequence", "digest", "version"] {
            let mut bad = original.clone();
            match kind {
                "count" => bad.successful_runs = 0,
                "sequence" => bad.last_sequence += 1,
                "digest" => bad.transcript_sha256 = "A".repeat(64),
                "version" => bad.schema_version = 2,
                _ => unreachable!(),
            }
            assert!(bad.validate(&base).is_err());
        }
    }
    #[test]
    fn bounded_run_exhaustion_prevents_any_later_success_summary() {
        let manifest = fixture();
        let mut t = LiveCudaTranscript::new(&manifest).unwrap();
        for n in 1..=MAX_LIVE_CUDA_RUNS {
            t.append(
                &input(&manifest, n),
                NormalizedPosition::new(0.5).unwrap(),
                1,
            )
            .unwrap();
        }
        assert!(t
            .append(
                &input(&manifest, MAX_LIVE_CUDA_RUNS + 1),
                NormalizedPosition::new(0.5).unwrap(),
                1
            )
            .is_err());
        assert!(t.finish().is_err());
    }
    #[test]
    fn ordinary_cpu_fixture_roundtrips_without_cuda_fields() {
        let fixture = include_bytes!("../tests/fixtures/live-cpu-manifest.json");
        let mut manifest: LiveInferenceManifest = serde_json::from_slice(fixture).unwrap();
        manifest.model.path = crate::test_host_path("/input/model").into();
        manifest.runtime.path = crate::test_host_path("/input/runtime").into();
        manifest.validate().unwrap();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        assert!(!String::from_utf8(bytes.clone()).unwrap().contains("cuda"));
        let decoded: LiveInferenceManifest = serde_json::from_slice(&bytes).unwrap();
        decoded.validate().unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());
    }
}
