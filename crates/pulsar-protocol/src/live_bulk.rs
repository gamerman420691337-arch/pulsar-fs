//! Bounded live RGB ingress. Frame headers precede authorization; only then may
//! one fixed-size pixel body follow. This channel grants no inference or playback.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, Write},
    path::Path,
    time::{Duration, Instant},
};

pub const LIVE_FRAME_BYTES: usize = 64 * 64 * 3;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveBulkChannel {
    LiveCapture,
    LiveEncodedSource,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveBulkHandshake {
    pub version: u16,
    pub channel: LiveBulkChannel,
    pub session: SessionId,
    pub auth_token: String,
    pub engine_epoch: AttemptId,
    pub identity: LiveIdentity,
}
impl fmt::Debug for LiveBulkHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveBulkHandshake")
            .field("session", &self.session)
            .field("auth_token", &"[REDACTED]")
            .field("engine_epoch", &self.engine_epoch)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
impl LiveBulkHandshake {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("live bulk protocol"));
        }
        crate::project_package_operations::bounded_text(&self.auth_token, 4096, true)?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveFrameHeader {
    pub frame: FrameId,
    pub source_time: SourceTimestamp,
    /// Producer-declared lease-relative nanoseconds, never a measured clock calibration.
    pub capture_tick: LiveClockTick,
    pub sha256: String,
}
impl LiveFrameHeader {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.sha256)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LiveBulkReply {
    Ready(LiveCaptureReport),
    SendPixels {
        frame: FrameId,
    },
    Accepted {
        frame: FrameId,
        replayed: bool,
        report: LiveCaptureReport,
    },
    Error(ProtocolError),
}
/// Any ambiguous remote/I/O response poisons the connection. Reconnect and query
/// capture status before retrying; only the latest exact frame is replayable.
pub struct LiveBulkClient {
    stream: Option<LocalStream>,
    timeout: Duration,
    operation_deadline: Option<Instant>,
    scope: Option<LiveBulkHandshake>,
    clock_observation: Option<LiveCaptureClockObservation>,
}
impl LiveBulkClient {
    pub fn connect(path: impl AsRef<Path>, timeout: Duration) -> Result<Self, BulkError> {
        if timeout.is_zero() {
            return Err(ProtocolError::invalid("zero live bulk timeout").into());
        }
        Ok(Self {
            stream: Some(crate::transport::connect_local(path.as_ref(), timeout)?),
            timeout,
            operation_deadline: None,
            scope: None,
            clock_observation: None,
        })
    }
    /// Replace the absolute bound for subsequent operations on this retained
    /// connection. Each operation captures the earlier of this bound and its
    /// ordinary timeout once; partial I/O cannot extend it. Clear with `None`.
    /// This does not change the handshake clock observation or restore a
    /// connection poisoned by an ambiguous exchange. An already-expired bound
    /// refuses locally before taking the stream, allowing an explicit replacement.
    pub fn set_operation_deadline(&mut self, deadline: Option<Instant>) {
        self.operation_deadline = deadline;
    }
    fn deadline(&self) -> Result<Instant, BulkError> {
        let now = Instant::now();
        let ordinary = now
            .checked_add(self.timeout)
            .ok_or_else(|| ProtocolError::invalid("live timeout overflow"))?;
        let end = self
            .operation_deadline
            .map_or(ordinary, |absolute| absolute.min(ordinary));
        if end <= now {
            return Err(
                io::Error::new(io::ErrorKind::TimedOut, "live operation deadline expired").into(),
            );
        }
        Ok(end)
    }
    fn check_report(
        scope: &LiveBulkHandshake,
        report: &LiveCaptureReport,
    ) -> Result<(), BulkError> {
        report.validate()?;
        if !report.active
            || report.identity != scope.identity
            || report.engine_epoch != scope.engine_epoch
        {
            return Err(TransportError::ResponseMismatch);
        }
        Ok(())
    }
    pub fn handshake(&mut self, scope: &LiveBulkHandshake) -> Result<LiveCaptureReport, BulkError> {
        if scope.channel != LiveBulkChannel::LiveCapture {
            return Err(ProtocolError::invalid("RGB channel requires live_capture marker").into());
        }
        scope.validate()?;
        if self.scope.is_some() {
            return Err(ProtocolError::invalid("live connection already authenticated").into());
        }
        let end = self.deadline()?;
        let mut stream = self
            .stream
            .take()
            .ok_or(TransportError::ConnectionPoisoned)?;
        let mut io = DeadlineStream::new(&mut stream, end);
        let sent = Instant::now();
        write_bulk_header(&mut io, scope)?;
        let report = match read_bulk_header::<_, LiveBulkReply>(&mut io)? {
            LiveBulkReply::Ready(report) => report,
            LiveBulkReply::Error(error) => return Err(error.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        let received = Instant::now();
        Self::check_report(scope, &report)?;
        // A slow probe leaves the legacy declared-tick channel available, but
        // never creates a usable observation or silently narrows uncertainty.
        self.clock_observation =
            LiveCaptureClockObservation::measured(&report, sent, received).ok();
        self.scope = Some(scope.clone());
        self.stream = Some(stream);
        Ok(report)
    }
    /// The observation belongs to this exact authenticated connection/epoch.
    /// Failed remote/I/O exchanges poison its use with the pixel channel.
    pub fn clock_observation(&self) -> Result<&LiveCaptureClockObservation, BulkError> {
        if self.stream.is_none() {
            return Err(TransportError::ConnectionPoisoned);
        }
        self.clock_observation.as_ref().ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                "no admitted local clock observation; handshake or uncertainty bound differs",
            )
            .into()
        })
    }
    /// Upload bytes acquired at a process-local monotonic event. The returned
    /// stamp retains the IPC uncertainty; the engine still treats producer
    /// capture provenance as declared and independently checks age/authority.
    pub fn upload_captured(
        &mut self,
        frame: FrameId,
        source_time: SourceTimestamp,
        captured: Instant,
        pixels: &[u8],
    ) -> Result<LiveCapturedUpload, BulkError> {
        let end = self.deadline()?;
        let stamp = self
            .clock_observation()?
            .stamp(frame, source_time, pixels, captured)?;
        let report = self.upload_before(&stamp.header, pixels, end)?;
        Ok(LiveCapturedUpload { report, stamp })
    }
    pub fn upload(
        &mut self,
        header: &LiveFrameHeader,
        pixels: &[u8],
    ) -> Result<LiveCaptureReport, BulkError> {
        let end = self.deadline()?;
        self.upload_before(header, pixels, end)
    }
    fn upload_before(
        &mut self,
        header: &LiveFrameHeader,
        pixels: &[u8],
        end: Instant,
    ) -> Result<LiveCaptureReport, BulkError> {
        header.validate()?;
        if pixels.len() != LIVE_FRAME_BYTES {
            return Err(
                ProtocolError::invalid("live body must be one packed 64x64 RGB frame").into(),
            );
        }
        if format!("{:x}", Sha256::digest(pixels)) != header.sha256 {
            return Err(ProtocolError::invalid("live body differs from declared hash").into());
        }
        let scope = self
            .scope
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("live handshake required"))?;
        let mut stream = self
            .stream
            .take()
            .ok_or(TransportError::ConnectionPoisoned)?;
        let mut io = DeadlineStream::new(&mut stream, end);
        write_bulk_header(&mut io, header)?;
        match read_bulk_header::<_, LiveBulkReply>(&mut io)? {
            LiveBulkReply::SendPixels { frame } if frame == header.frame => {}
            LiveBulkReply::Error(error) => return Err(error.into()),
            _ => return Err(TransportError::ResponseMismatch),
        }
        io.write_all(pixels)?;
        let report = match read_bulk_header::<_, LiveBulkReply>(&mut io)? {
            LiveBulkReply::Accepted { frame, report, .. } if frame == header.frame => report,
            LiveBulkReply::Error(error) => return Err(error.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        Self::check_report(scope, &report)?;
        if report.last_frame.as_ref() != Some(header) {
            return Err(TransportError::ResponseMismatch);
        }
        self.stream = Some(stream);
        Ok(report)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_channel_is_distinct_strict_and_redacts_bearer() {
        let handshake = LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveCapture,
            session: SessionId::new("session").unwrap(),
            auth_token: "fixture-secret".into(),
            engine_epoch: AttemptId::new("boot").unwrap(),
            identity: LiveIdentity {
                epoch: AttemptId::new("epoch").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(0),
                source: SourceVersionId::new("source").unwrap(),
            },
        };
        handshake.validate().unwrap();
        assert!(!format!("{handshake:?}").contains("fixture-secret"));
        let mut value = serde_json::to_value(&handshake).unwrap();
        assert!(matches!(
            serde_json::from_value::<BulkHandshakeEnvelope>(value.clone()).unwrap(),
            BulkHandshakeEnvelope::LiveCapture(_)
        ));
        value.as_object_mut().unwrap().remove("channel");
        assert!(serde_json::from_value::<BulkHandshakeEnvelope>(value).is_err());
        let mut value = serde_json::to_value(&handshake).unwrap();
        value["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<BulkHandshakeEnvelope>(value).is_err());
        assert!(serde_json::from_str::<LiveFrameHeader>(r#"{"frame":1,"source_time":{"numerator":0,"denominator":1},"capture_tick":0,"sha256":"wrong"}"#).map(|v|v.validate().is_err()).unwrap_or(true));
    }
}
