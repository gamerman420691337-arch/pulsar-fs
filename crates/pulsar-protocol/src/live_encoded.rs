//! Bounded encoded ingress and explicit delivery-age capture provenance.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCaptureAgeBasis {
    #[default]
    ProducerDeclared,
    DecodedStdoutLocalMonotonicEqualRate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEncodedSourceOptions {
    pub format: LiveEncodedFormat,
    pub maximum_input_bytes: u64,
    pub maximum_frames: u32,
}
impl LiveEncodedSourceOptions {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.maximum_input_bytes == 0
            || self.maximum_input_bytes > 512 * 1024 * 1024
            || self.maximum_frames == 0
            || self.maximum_frames > 9000
        {
            return Err(ProtocolError::invalid(
                "encoded source intake/frame bounds differ",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveEncodedSourceState {
    Pending,
    Starting,
    Running,
    Finished,
    Retired,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSourceCompletionFacts {
    pub attempt_id: AttemptId,
    pub identity: LiveIdentity,
    pub frames: u32,
    pub input_sha256: String,
    pub input_bytes: u64,
}
impl NativeSourceCompletionFacts {
    pub fn validate(&self, m: &LiveDecodeManifest) -> Result<(), ProtocolError> {
        LiveDecodeReply::End {
            attempt_id: self.attempt_id.clone(),
            identity: self.identity.clone(),
            frames: self.frames,
            input_sha256: self.input_sha256.clone(),
            input_bytes: self.input_bytes,
        }
        .validate(m)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEncodedSourceReport {
    pub state: LiveEncodedSourceState,
    pub manifest: Option<LiveDecodeManifest>,
    pub input_bytes: u64,
    pub latest_delivery: Option<NativeSourceDeliveryFacts>,
    pub completion: Option<NativeSourceCompletionFacts>,
    pub error: Option<ProtocolError>,
}
impl LiveEncodedSourceReport {
    pub fn validate(&self, c: &LiveCaptureReport) -> Result<(), ProtocolError> {
        let options = c
            .options
            .encoded_source
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("source report without encoded admission"))?;
        if self.state == LiveEncodedSourceState::Pending && self.manifest.is_some() {
            return Err(ProtocolError::invalid(
                "pending source cannot have an issued native attempt",
            ));
        }
        if self.state == LiveEncodedSourceState::Starting
            && (self.input_bytes != 0 || c.received_frames != 0)
        {
            return Err(ProtocolError::invalid(
                "starting source cannot publish input or frames",
            ));
        }
        if self.input_bytes > options.maximum_input_bytes
            || c.active
                != matches!(
                    self.state,
                    LiveEncodedSourceState::Pending
                        | LiveEncodedSourceState::Starting
                        | LiveEncodedSourceState::Running
                        | LiveEncodedSourceState::Finished
                )
            || (self.state == LiveEncodedSourceState::Failed) != self.error.is_some()
            || self.error.as_ref().is_some_and(|e| e.message.len() > 2048)
        {
            return Err(ProtocolError::invalid(
                "encoded source state/intake differs",
            ));
        }
        if let Some(m) = &self.manifest {
            m.validate()?;
            if m.identity != c.identity
                || m.format != options.format
                || m.maximum_input_bytes != options.maximum_input_bytes
                || m.maximum_frames != options.maximum_frames
                || m.budget.wall_time_ms != u64::from(c.options.lease_ms)
            {
                return Err(ProtocolError::invalid(
                    "encoded source manifest differs from capture",
                ));
            }
            if let Some(d) = &self.latest_delivery {
                d.validate(m)?;
                let header = LiveFrameHeader {
                    frame: d.frame,
                    source_time: d.source_time,
                    capture_tick: d.bounds.earliest,
                    sha256: d.pixels.sha256.clone(),
                };
                if c.last_frame.as_ref() != Some(&header)
                    || c.received_frames != d.frame.value() + 1
                    || d.probe.parent_end.0 != u64::from(c.options.lease_ms) * 1_000_000
                {
                    return Err(ProtocolError::invalid(
                        "encoded delivery differs from lease frame/clock",
                    ));
                }
            } else if c.received_frames != 0 {
                return Err(ProtocolError::invalid("encoded frames lack delivery facts"));
            }
            if let Some(end) = &self.completion {
                end.validate(m)?;
                if u64::from(end.frames) != c.received_frames || end.input_bytes != self.input_bytes
                {
                    return Err(ProtocolError::invalid("encoded completion counters differ"));
                }
            }
        } else if !matches!(
            self.state,
            LiveEncodedSourceState::Pending | LiveEncodedSourceState::Retired
        ) || self.input_bytes != 0
            || c.received_frames != 0
            || self.latest_delivery.is_some()
            || self.completion.is_some()
        {
            return Err(ProtocolError::invalid(
                "encoded source lacks admitted manifest",
            ));
        }
        if (self.state == LiveEncodedSourceState::Finished) != self.completion.is_some() && c.active
        {
            return Err(ProtocolError::invalid("encoded completion state differs"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum LiveEncodedRequest {
    Push {
        sequence: u64,
        byte_len: u32,
        sha256: String,
    },
    Finish {
        sequence: u64,
    },
}
impl LiveEncodedRequest {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Push { sequence, .. } | Self::Finish { sequence } => *sequence,
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Self::Push {
            byte_len, sha256, ..
        } = self
        {
            validate_sha256(sha256)?;
            if *byte_len == 0 || *byte_len > 65536 {
                return Err(ProtocolError::invalid("encoded chunk exceeds bounds"));
            }
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
pub enum LiveEncodedReply {
    Ready(LiveCaptureReport),
    SendBytes {
        sequence: u64,
    },
    Accepted {
        sequence: u64,
        input_bytes: u64,
        report: LiveCaptureReport,
    },
    Finished(LiveCaptureReport),
    Error(ProtocolError),
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending() -> LiveCaptureReport {
        let identity = LiveIdentity {
            epoch: AttemptId::new("epoch").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(0),
            source: SourceVersionId::new("source").unwrap(),
        };
        let options = LiveCaptureOptions {
            browser_analysis: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::Immediate,
            encoded_source: Some(LiveEncodedSourceOptions {
                format: LiveEncodedFormat::Yuv4mpegPipe,
                maximum_input_bytes: 1000,
                maximum_frames: 3,
            }),
            inference: None,
            policy: LivePolicy {
                maximum_analysis_age_ns: 1000000000,
                added_buffer_ns: 0,
            },
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: 30000,
        };
        LiveCaptureReport {
            input_close: None,
            continuation: None,
            engine_epoch: AttemptId::new("boot").unwrap(),
            clock: options.clock(identity.epoch.clone()),
            identity,
            options,
            active: true,
            retirement: None,
            server_elapsed: Some(LiveClockTick(0)),
            admitted_pixel_bytes: 5 * 64 * 64 * 3,
            received_frames: 0,
            last_frame: None,
            inference: None,
            queue: None,
            age_basis: LiveCaptureAgeBasis::DecodedStdoutLocalMonotonicEqualRate,
            encoded_source: Some(LiveEncodedSourceReport {
                state: LiveEncodedSourceState::Pending,
                manifest: None,
                input_bytes: 0,
                latest_delivery: None,
                completion: None,
                error: None,
            }),
        }
    }
    #[test]
    fn encoded_contract_refuses_unknown_format_unbounded_intake_and_bad_chunk_headers() {
        let mut options = pending().options.encoded_source.unwrap();
        options.maximum_input_bytes = 512 * 1024 * 1024 + 1;
        assert!(options.validate().is_err());
        options.maximum_input_bytes = 1000;
        options.maximum_frames = 0;
        assert!(options.validate().is_err());
        for n in [0, 65537] {
            assert!(LiveEncodedRequest::Push {
                sequence: 0,
                byte_len: n,
                sha256: "a".repeat(64)
            }
            .validate()
            .is_err());
        }
        assert!(LiveEncodedRequest::Push {
            sequence: 0,
            byte_len: 65536,
            sha256: "a".repeat(64)
        }
        .validate()
        .is_ok());
        assert!(LiveEncodedRequest::Push {
            sequence: 0,
            byte_len: 1,
            sha256: "wrong".into()
        }
        .validate()
        .is_err());
        assert!(serde_json::from_str::<LiveEncodedSourceOptions>(
            r#"{"format":"auto","maximum_input_bytes":1000,"maximum_frames":3}"#
        )
        .is_err());
    }
    #[test]
    fn source_admission_cannot_relabel_delivery_as_declared_exposure_or_publish_before_launch() {
        let mut r = pending();
        r.validate().unwrap();
        r.age_basis = LiveCaptureAgeBasis::ProducerDeclared;
        assert!(r.validate().is_err());
        r = pending();
        r.received_frames = 1;
        assert!(r.validate().is_err());
        r = pending();
        r.encoded_source.as_mut().unwrap().state = LiveEncodedSourceState::Running;
        assert!(r.validate().is_err());
        let mut wire = serde_json::to_value(pending()).unwrap();
        wire["age_basis"] = serde_json::json!("camera_exposure");
        assert!(serde_json::from_value::<LiveCaptureReport>(wire).is_err());
    }
    #[test]
    fn source_retirement_preserves_mode_but_cannot_remain_a_running_or_completed_lease() {
        let mut r = pending();
        r.active = false;
        r.retirement = Some(LiveCaptureRetirement::Stopped);
        r.admitted_pixel_bytes = 0;
        assert!(r.validate().is_err());
        r.encoded_source.as_mut().unwrap().state = LiveEncodedSourceState::Retired;
        r.validate().unwrap();
        let mut wire = serde_json::to_value(&r).unwrap();
        wire["encoded_source"]["qualified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LiveCaptureReport>(wire).is_err());
    }
}
