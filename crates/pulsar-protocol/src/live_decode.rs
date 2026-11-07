//! Private native source-worker contract. Encoded intake and stdout delivery
//! are not camera exposure provenance, model qualification or device authority.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveEncodedFormat {
    Yuv4mpegPipe,
    Nut,
    Matroska,
}
impl LiveEncodedFormat {
    pub fn demuxer(self) -> &'static str {
        match self {
            Self::Yuv4mpegPipe => "yuv4mpegpipe",
            Self::Nut => "nut",
            Self::Matroska => "matroska",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveDecodeManifest {
    pub version: u16,
    pub attempt_id: AttemptId,
    pub identity: LiveIdentity,
    pub ffmpeg: ArtifactReference,
    pub worker: ArtifactIdentity,
    pub format: LiveEncodedFormat,
    pub maximum_input_bytes: u64,
    pub maximum_frames: u32,
    pub budget: ResourceBudget,
}
impl LiveDecodeManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("live decode version"));
        }
        self.ffmpeg.validate()?;
        self.worker.validate()?;
        self.budget.validate()?;
        if self.maximum_input_bytes == 0
            || self.maximum_input_bytes > 512 * 1024 * 1024
            || self.maximum_frames == 0
            || self.maximum_frames > 9000
            || self.budget.wall_time_ms > 30_000
            || self.budget.cpu_threads > 2
        {
            return Err(ProtocolError::invalid(
                "native source intake/lifetime exceeds bounds",
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
pub enum LiveDecodeReply {
    Ready {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        format: LiveEncodedFormat,
        worker_elapsed_ns: u64,
    },
    Frame {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        frame: FrameId,
        source_time: SourceTimestamp,
        delivered_elapsed_ns: u64,
        pixels: ArtifactIdentity,
    },
    End {
        attempt_id: AttemptId,
        identity: LiveIdentity,
        frames: u32,
        input_sha256: String,
        input_bytes: u64,
    },
    Error(ProtocolError),
}
impl LiveDecodeReply {
    pub fn validate(&self, manifest: &LiveDecodeManifest) -> Result<(), ProtocolError> {
        manifest.validate()?;
        let (attempt, identity) = match self {
            Self::Ready {
                attempt_id,
                identity,
                format,
                worker_elapsed_ns,
            } => {
                if *format != manifest.format
                    || *worker_elapsed_ns >= manifest.budget.wall_time_ms * 1_000_000
                {
                    return Err(ProtocolError::invalid("native source readiness differs"));
                }
                (attempt_id, identity)
            }
            Self::Frame {
                attempt_id,
                identity,
                frame,
                delivered_elapsed_ns,
                pixels,
                ..
            } => {
                pixels.validate()?;
                if frame.value() >= u64::from(manifest.maximum_frames)
                    || *delivered_elapsed_ns >= manifest.budget.wall_time_ms * 1_000_000
                    || pixels.byte_len != LIVE_FRAME_BYTES as u64
                    || pixels.artifact_id.as_str() != pixels.sha256
                {
                    return Err(ProtocolError::invalid("native decoded frame exceeds scope"));
                }
                (attempt_id, identity)
            }
            Self::End {
                attempt_id,
                identity,
                frames,
                input_sha256,
                input_bytes,
            } => {
                validate_sha256(input_sha256)?;
                if *frames == 0
                    || *frames > manifest.maximum_frames
                    || *input_bytes == 0
                    || *input_bytes > manifest.maximum_input_bytes
                {
                    return Err(ProtocolError::invalid(
                        "native source completion exceeds scope",
                    ));
                }
                (attempt_id, identity)
            }
            Self::Error(error) => {
                if error.message.len() > 2048 {
                    return Err(ProtocolError::invalid(
                        "native source diagnostic exceeds bound",
                    ));
                }
                return Ok(());
            }
        };
        if *attempt != manifest.attempt_id || *identity != manifest.identity {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "native source result identity differs",
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> LiveDecodeManifest {
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 100,
        };
        LiveDecodeManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            identity: LiveIdentity {
                epoch: AttemptId::new("epoch").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(0),
                source: SourceVersionId::new("source").unwrap(),
            },
            ffmpeg: ArtifactReference {
                path: crate::test_host_path("/input/ffmpeg").into(),
                identity: artifact.clone(),
            },
            worker: artifact,
            format: LiveEncodedFormat::Yuv4mpegPipe,
            maximum_input_bytes: 100000,
            maximum_frames: 100,
            budget: ResourceBudget {
                memory_bytes: 1 << 30,
                output_bytes: 1 << 20,
                wall_time_ms: 30000,
                cpu_threads: 2,
            },
        }
    }
    #[test]
    fn explicit_native_format_and_intake_are_strict_bounded_facts() {
        let mut m = manifest();
        m.validate().unwrap();
        m.maximum_input_bytes = 512 * 1024 * 1024 + 1;
        assert!(m.validate().is_err());
        m = manifest();
        m.maximum_frames = 0;
        assert!(m.validate().is_err());
        m = manifest();
        m.budget.wall_time_ms = 30001;
        assert!(m.validate().is_err());
        let mut v = serde_json::to_value(manifest()).unwrap();
        v["format"] = serde_json::json!("auto");
        assert!(serde_json::from_value::<LiveDecodeManifest>(v).is_err());
    }
    #[test]
    fn native_delivery_is_fixed_rgb_bound_to_attempt_epoch_and_lifetime() {
        let m = manifest();
        let mut p = m.worker.clone();
        p.byte_len = LIVE_FRAME_BYTES as u64;
        let frame = LiveDecodeReply::Frame {
            attempt_id: m.attempt_id.clone(),
            identity: m.identity.clone(),
            frame: FrameId::new(0),
            source_time: SourceTimestamp::new(-1, 3).unwrap(),
            delivered_elapsed_ns: 1,
            pixels: p,
        };
        frame.validate(&m).unwrap();
        let mut wrong = m.clone();
        wrong.identity.epoch = AttemptId::new("other").unwrap();
        assert!(frame.validate(&wrong).is_err());
        wrong = m.clone();
        wrong.attempt_id = AttemptId::new("other").unwrap();
        assert!(frame.validate(&wrong).is_err());
        let mut v = serde_json::to_value(frame).unwrap();
        v["payload"]["delivered_elapsed_ns"] = serde_json::json!(30000000000u64);
        assert!(serde_json::from_value::<LiveDecodeReply>(v)
            .unwrap()
            .validate(&m)
            .is_err());
    }
}
