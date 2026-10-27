//! Archivable native delivery facts. Deserialization does not recreate a
//! measured runtime handle, immutable pixels, capture or device authority.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeDeliveryAgeBasis {
    DecodedStdoutLocalMonotonicEqualRate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSourceDeliveryFacts {
    pub attempt_id: AttemptId,
    pub identity: LiveIdentity,
    pub worker: ArtifactIdentity,
    pub ffmpeg: ArtifactIdentity,
    pub frame: FrameId,
    pub source_time: SourceTimestamp,
    pub worker_delivered_elapsed_ns: u64,
    pub parent_receipt: LiveClockTick,
    pub pixels: ArtifactIdentity,
    pub probe: LiveDecodeClockProbe,
    pub bounds: LiveDeliveryTickBounds,
    pub age_basis: NativeDeliveryAgeBasis,
}
impl NativeSourceDeliveryFacts {
    pub fn validate(&self, manifest: &LiveDecodeManifest) -> Result<(), ProtocolError> {
        LiveDecodeReply::Frame {
            attempt_id: self.attempt_id.clone(),
            identity: self.identity.clone(),
            frame: self.frame,
            source_time: self.source_time,
            delivered_elapsed_ns: self.worker_delivered_elapsed_ns,
            pixels: self.pixels.clone(),
        }
        .validate(manifest)?;
        if self.worker != manifest.worker
            || self.ffmpeg != manifest.ffmpeg.identity
            || self.bounds
                != self
                    .probe
                    .bounds(self.worker_delivered_elapsed_ns, self.parent_receipt)
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "native delivery facts differ from pinned bytes or measured bounds",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (LiveDecodeManifest, NativeSourceDeliveryFacts) {
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 100,
        };
        let identity = LiveIdentity {
            epoch: AttemptId::new("epoch").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(0),
            source: SourceVersionId::new("source").unwrap(),
        };
        let manifest = LiveDecodeManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            identity: identity.clone(),
            ffmpeg: ArtifactReference {
                path: crate::test_host_path("/input/ffmpeg").into(),
                identity: artifact.clone(),
            },
            worker: artifact.clone(),
            format: LiveEncodedFormat::Nut,
            maximum_input_bytes: 1000,
            maximum_frames: 10,
            budget: ResourceBudget {
                memory_bytes: 1 << 30,
                output_bytes: 1 << 20,
                wall_time_ms: 30000,
                cpu_threads: 1,
            },
        };
        let probe = LiveDecodeClockProbe {
            parent_send: LiveClockTick(100),
            parent_receive: LiveClockTick(200),
            worker_ready_elapsed_ns: 10,
            parent_end: LiveClockTick(1000000000),
            maximum_uncertainty_ns: 100,
        };
        let facts = NativeSourceDeliveryFacts {
            attempt_id: manifest.attempt_id.clone(),
            identity,
            worker: artifact.clone(),
            ffmpeg: artifact,
            frame: FrameId::new(0),
            source_time: SourceTimestamp::new(-1, 3).unwrap(),
            worker_delivered_elapsed_ns: 20,
            parent_receipt: LiveClockTick(300),
            pixels: ArtifactIdentity {
                artifact_id: ArtifactId::new("b".repeat(64)).unwrap(),
                sha256: "b".repeat(64),
                byte_len: LIVE_FRAME_BYTES as u64,
            },
            bounds: probe.bounds(20, LiveClockTick(300)).unwrap(),
            probe,
            age_basis: NativeDeliveryAgeBasis::DecodedStdoutLocalMonotonicEqualRate,
        };
        (manifest, facts)
    }
    #[test]
    fn archived_delivery_preserves_signed_source_time_and_revalidates_scope_pins_bounds() {
        let (manifest, facts) = fixture();
        facts.validate(&manifest).unwrap();
        let roundtrip: NativeSourceDeliveryFacts =
            serde_json::from_value(serde_json::to_value(&facts).unwrap()).unwrap();
        roundtrip.validate(&manifest).unwrap();
        assert_eq!(roundtrip.source_time, SourceTimestamp::new(-1, 3).unwrap());
        let mut wrong = facts.clone();
        wrong.attempt_id = AttemptId::new("other").unwrap();
        assert!(wrong.validate(&manifest).is_err());
        let mut wrong = facts.clone();
        wrong.worker.sha256 = "c".repeat(64);
        assert!(wrong.validate(&manifest).is_err());
        let mut wrong = facts.clone();
        wrong.ffmpeg.byte_len += 1;
        assert!(wrong.validate(&manifest).is_err());
        let mut wrong = facts.clone();
        wrong.bounds.earliest.0 += 1;
        assert!(wrong.validate(&manifest).is_err());
        let mut wrong = facts;
        wrong.bounds.uncertainty_ns = 0;
        assert!(wrong.validate(&manifest).is_err());
    }
    #[test]
    fn delivery_facts_cannot_claim_hardware_exposure_or_qualified_authority() {
        let (_, facts) = fixture();
        let mut v = serde_json::to_value(&facts).unwrap();
        v["age_basis"] = serde_json::json!("camera_exposure");
        assert!(serde_json::from_value::<NativeSourceDeliveryFacts>(v).is_err());
        let mut v = serde_json::to_value(facts).unwrap();
        v["qualified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<NativeSourceDeliveryFacts>(v).is_err());
    }
}
