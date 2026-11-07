//! Process-local measured IPC bracket. No deserialized fact can recreate the
//! observation handle or prove camera/player clock calibration.
use crate::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub struct LiveCaptureClockFacts {
    pub engine_epoch: AttemptId,
    pub identity: LiveIdentity,
    pub probe: LiveClockProbe,
    /// Equal-rate local monotonic clocks are the mapping assumption. This is
    /// neither a measured device rate nor physical capture provenance.
    pub basis: LiveCaptureClockBasis,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCaptureClockBasis {
    LocalMonotonicEqualRate,
}

#[derive(Debug)]
pub struct LiveCaptureClockObservation {
    origin: Instant,
    received: Instant,
    facts: LiveCaptureClockFacts,
}
#[derive(Clone, Debug, Serialize)]
pub struct LiveClockStampedFrame {
    pub header: LiveFrameHeader,
    pub clock: LiveCaptureClockFacts,
    pub bounds: LiveCaptureTickBounds,
}
#[derive(Clone, Debug, Serialize)]
pub struct LiveCapturedUpload {
    pub report: LiveCaptureReport,
    pub stamp: LiveClockStampedFrame,
}
fn invalid(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::invalid(error.to_string())
}
impl LiveCaptureClockObservation {
    pub(super) fn measured(
        report: &LiveCaptureReport,
        sent: Instant,
        received: Instant,
    ) -> Result<Self, ProtocolError> {
        report.validate()?;
        if !report.active {
            return Err(invalid("capture lease retired during clock probe"));
        }
        let elapsed = received
            .checked_duration_since(sent)
            .ok_or_else(|| invalid("local monotonic clock regressed"))?;
        let probe = LiveClockProbe {
            producer_send_ns: 0,
            producer_receive_ns: u64::try_from(elapsed.as_nanos()).map_err(invalid)?,
            server_sample: report
                .server_elapsed
                .ok_or_else(|| invalid("clock sample missing"))?,
            lease_end: LiveClockTick(u64::from(report.options.lease_ms) * 1_000_000),
            maximum_uncertainty_ns: 1_000_000_000,
        };
        probe.validate().map_err(invalid)?;
        Ok(Self {
            origin: sent,
            received,
            facts: LiveCaptureClockFacts {
                engine_epoch: report.engine_epoch.clone(),
                identity: report.identity.clone(),
                probe,
                basis: LiveCaptureClockBasis::LocalMonotonicEqualRate,
            },
        })
    }
    /// Archivable measured bracket facts; they confer no transport authority.
    pub fn facts(&self) -> &LiveCaptureClockFacts {
        &self.facts
    }
    /// Only a local monotonic acquisition event after this completed handshake
    /// can be mapped. Source PTS and decoder/hardware capture times are separate.
    pub fn bounds_at(&self, captured: Instant) -> Result<LiveCaptureTickBounds, ProtocolError> {
        if captured > Instant::now() {
            return Err(invalid("capture event is in the local future"));
        }
        if captured < self.received {
            return Err(invalid("capture precedes clock observation"));
        }
        let elapsed = captured
            .checked_duration_since(self.origin)
            .ok_or_else(|| invalid("capture precedes local clock origin"))?;
        self.facts
            .probe
            .bounds(u64::try_from(elapsed.as_nanos()).map_err(invalid)?)
            .map_err(invalid)
    }
    /// `captured` is the caller's acquisition event for these bytes, not upload
    /// time. This helper does not verify that assertion or erase decoder delay.
    pub fn stamp(
        &self,
        frame: FrameId,
        source_time: SourceTimestamp,
        pixels: &[u8],
        captured: Instant,
    ) -> Result<LiveClockStampedFrame, ProtocolError> {
        if pixels.len() != LIVE_FRAME_BYTES {
            return Err(invalid("capture must be packed 64x64 RGB"));
        }
        let bounds = self.bounds_at(captured)?;
        let header = LiveFrameHeader {
            frame,
            source_time,
            capture_tick: bounds.earliest,
            sha256: format!("{:x}", Sha256::digest(pixels)),
        };
        Ok(LiveClockStampedFrame {
            header,
            clock: self.facts.clone(),
            bounds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> LiveCaptureReport {
        let identity = LiveIdentity {
            epoch: AttemptId::new("capture").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(0),
            source: SourceVersionId::new("source").unwrap(),
        };
        let options = LiveCaptureOptions {
            inference_fallback: None,
            browser_analysis: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::Immediate,
            encoded_source: None,
            inference: None,
            policy: LivePolicy {
                maximum_analysis_age_ns: 200_000_000,
                added_buffer_ns: 0,
            },
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: 30_000,
        };
        LiveCaptureReport {
            input_close: None,
            continuation: None,
            age_basis: LiveCaptureAgeBasis::ProducerDeclared,
            encoded_source: None,
            engine_epoch: AttemptId::new("boot").unwrap(),
            clock: options.clock(identity.epoch.clone()),
            identity,
            options,
            active: true,
            retirement: None,
            server_elapsed: Some(LiveClockTick(1_000_000)),
            admitted_pixel_bytes: 61440,
            received_frames: 0,
            last_frame: None,
            inference: None,
            queue: None,
        }
    }
    #[test]
    fn excessive_probe_uncertainty_and_retired_or_invalid_reports_cannot_mint_handle() {
        let sent = Instant::now();
        assert!(LiveCaptureClockObservation::measured(
            &report(),
            sent,
            sent + std::time::Duration::from_nanos(1_000_000_001)
        )
        .is_err());
        let mut r = report();
        r.active = false;
        r.retirement = Some(LiveCaptureRetirement::Stopped);
        r.admitted_pixel_bytes = 0;
        assert!(LiveCaptureClockObservation::measured(&r, sent, sent).is_err());
        let mut r = report();
        r.server_elapsed = Some(LiveClockTick(30_000_000_000));
        assert!(LiveCaptureClockObservation::measured(&r, sent, sent).is_err());
    }
    #[test]
    fn acquisition_event_cannot_precede_probe_or_outlive_lease_and_bad_bytes_do_not_stamp() {
        let sent = Instant::now() - std::time::Duration::from_millis(20);
        let received = sent + std::time::Duration::from_millis(10);
        let o = LiveCaptureClockObservation::measured(&report(), sent, received).unwrap();
        assert!(o.bounds_at(sent).is_err());
        assert!(o
            .bounds_at(Instant::now() + std::time::Duration::from_secs(1))
            .is_err());
        assert!(o
            .bounds_at(sent + std::time::Duration::from_secs(30))
            .is_err());
        assert!(o
            .stamp(
                FrameId::new(1),
                SourceTimestamp::new(1, 3).unwrap(),
                &[],
                received
            )
            .is_err());
        let s = o
            .stamp(
                FrameId::new(1),
                SourceTimestamp::new(1, 3).unwrap(),
                &[0; LIVE_FRAME_BYTES],
                received,
            )
            .unwrap();
        assert_eq!(s.bounds.earliest, LiveClockTick(1_000_000));
        assert_eq!(s.bounds.latest, LiveClockTick(11_000_000));
        assert_eq!(s.header.capture_tick, s.bounds.earliest);
    }
}
