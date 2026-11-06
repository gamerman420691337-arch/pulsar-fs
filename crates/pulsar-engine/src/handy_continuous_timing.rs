//! Native, independently observed playback phase. Original play references never change.
use crate::{
    handy_online::PlayingTimingRead,
    handy_server_clock::{NativeServerClock, QualifiedServerClock, Sample},
};
use pulsar_protocol::*;

type Result<T> = std::result::Result<T, ProtocolError>;
const NS_MS: u128 = 1_000_000;
const MAX_MS: u64 = (1_u64 << 53) - 1;

fn invalid() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "continuous playback timing is stale, discontinuous or outside accepted bounds",
    )
}
fn drift(ns: u64, ppm: u32) -> u128 {
    (u128::from(ns) * u128::from(ppm)).div_ceil(NS_MS)
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct QualifiedContinuousTiming {
    pub(crate) contract: HandyContinuousTimingQualification,
    pub(crate) maximum_total_error_ns: u64,
    pub(crate) server: QualifiedServerClock,
}
#[derive(Clone)]
pub(crate) struct ContinuousTimingSamples {
    pub(crate) device: PlayingTimingRead,
    pub(crate) server_before: Sample,
    pub(crate) server_after: Sample,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContinuousTimingEstimate {
    pub(crate) observed_phase_ms: u64,
    pub(crate) phase_uncertainty_ns: u64,
    pub(crate) estimated_server_ms: u64,
    pub(crate) server_uncertainty_ns: u64,
    pub(crate) alignment_error_ns: u64,
    pub(crate) valid_until_boottime_ns: u64,
}

#[derive(Clone)]
struct Phase {
    lower_ns: u128,
    upper_ns: u128,
    anchor_ns: u64,
    drift_ppm: u32,
}
impl Phase {
    fn new(
        value_ms: u64,
        start: u64,
        end: u64,
        contract: &HandyContinuousTimingQualification,
    ) -> Result<Self> {
        let span = end.checked_sub(start).ok_or_else(invalid)?;
        if value_ms > MAX_MS || span > contract.maximum_sample_span_ns {
            return Err(invalid());
        }
        let value = u128::from(value_ms) * NS_MS;
        let error = u128::from(contract.timestamp_error_ns);
        // A timestamp may originate anywhere inside the complete HTTP bracket.
        // No symmetric network delay or atomic multi-field reply is assumed.
        Ok(Self {
            lower_ns: value.saturating_sub(error),
            upper_ns: value + error + u128::from(span) + drift(span, contract.drift_ppm),
            anchor_ns: end,
            drift_ppm: contract.drift_ppm,
        })
    }
    fn interval(&self, now: u64) -> Result<(u128, u128)> {
        let elapsed = now.checked_sub(self.anchor_ns).ok_or_else(invalid)?;
        let drift = drift(elapsed, self.drift_ppm);
        // The qualified phase is nondecreasing. A very broad drift declaration
        // cannot imply a negative rate or wrap unsigned arithmetic.
        Ok((
            self.lower_ns + u128::from(elapsed).saturating_sub(drift),
            self.upper_ns + u128::from(elapsed) + drift,
        ))
    }
    fn estimate(&self, now: u64) -> Result<(u64, u64)> {
        let (low, high) = self.interval(now)?;
        let value = (low + high + NS_MS) / (2 * NS_MS);
        if value > u128::from(MAX_MS) {
            return Err(invalid());
        }
        let center = value * NS_MS;
        let error = center.abs_diff(low).max(high.abs_diff(center));
        Ok((value as u64, error.try_into().map_err(|_| invalid())?))
    }
    fn overlaps(&self, other: &Self, now: u64) -> Result<()> {
        let (a, b) = self.interval(now)?;
        let (c, d) = other.interval(now)?;
        if a > d || c > b {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Constructed only from a current accepted physical model and native sampling.
/// This type has no deserialization or historical-receipt constructor.
#[derive(Clone)]
pub(crate) struct NativeContinuousTiming {
    reference: HandyContinuousTimeReference,
    samples: ContinuousTimingSamples,
    qualified: QualifiedContinuousTiming,
    phase: Phase,
    server: NativeServerClock,
    valid_until_ns: u64,
}
impl NativeContinuousTiming {
    pub(crate) fn new(
        domain: ArtifactId,
        reference: HandyContinuousTimeReference,
        samples: ContinuousTimingSamples,
        qualified: QualifiedContinuousTiming,
    ) -> Result<Self> {
        reference.validate()?;
        qualified.contract.validate()?;
        let q = &qualified.contract;
        let read = &samples.device;
        let d = &read.device;
        validate_handy_playing_context(&d.before, &d.after)?;
        d.first.validate()?;
        d.second.validate()?;
        let server_sample = |s: &Sample| {
            if s.server_ms > MAX_MS
                || s.received_ns
                    .checked_sub(s.sent_ns)
                    .is_none_or(|span| span > q.maximum_sample_span_ns)
            {
                Err(invalid())
            } else {
                Ok(())
            }
        };
        server_sample(&samples.server_before)?;
        server_sample(&samples.server_after)?;
        if qualified.maximum_total_error_ns == 0
            || reference.uncertainty_ns > qualified.maximum_total_error_ns
            || samples.server_before.received_ns > read.started_boottime_ns
            || read.started_boottime_ns > d.first.read_started_boottime_ns
            || d.first.read_finished_boottime_ns > d.second.read_started_boottime_ns
            || d.second.read_finished_boottime_ns > read.finished_boottime_ns
            || read.finished_boottime_ns > samples.server_after.sent_ns
            || read
                .finished_boottime_ns
                .checked_sub(read.started_boottime_ns)
                .is_none_or(|span| span > q.maximum_sample_span_ns)
            || d.second.info.device_uptime_ms < d.first.info.device_uptime_ms
            || d.first.info.reported_clock_offset_ms != d.second.info.reported_clock_offset_ms
            || d.first.info.last_sync_round_trip_ms != d.second.info.last_sync_round_trip_ms
            || d.first.user_offset != d.second.user_offset
        {
            return Err(invalid());
        }
        let before = Phase::new(
            d.before
                .remote_hssp
                .as_ref()
                .ok_or_else(invalid)?
                .current_time_ms,
            read.started_boottime_ns,
            d.first.read_started_boottime_ns,
            q,
        )?;
        let phase = Phase::new(
            d.after
                .remote_hssp
                .as_ref()
                .ok_or_else(invalid)?
                .current_time_ms,
            d.second.read_finished_boottime_ns,
            read.finished_boottime_ns,
            q,
        )?;
        let now = samples.server_after.received_ns;
        before.overlaps(&phase, now)?;
        let server_before = NativeServerClock::new(
            domain.clone(),
            samples.server_before.clone(),
            qualified.server.clone(),
        )?;
        let server = NativeServerClock::new(
            domain,
            samples.server_after.clone(),
            qualified.server.clone(),
        )?;
        // A valid overlap must cover the whole acquisition; a stale predecessor
        // cannot serve as continuity evidence merely because its DTO exists.
        server_before.at(now)?;
        server.agrees_with(&server_before)?;
        let valid_until_ns = read
            .started_boottime_ns
            .checked_add(q.maximum_observation_age_ns)
            .ok_or_else(invalid)?
            .min(server.clock.valid_until_ns);
        if valid_until_ns <= now {
            return Err(invalid());
        }
        let value = Self {
            reference,
            samples,
            qualified,
            phase,
            server,
            valid_until_ns,
        };
        value.at(now)?;
        Ok(value)
    }
    pub(crate) fn observed_at(&self) -> u64 {
        self.samples.server_after.received_ns
    }
    pub(crate) fn samples(&self) -> &ContinuousTimingSamples {
        &self.samples
    }
    pub(crate) fn at(&self, now: u64) -> Result<ContinuousTimingEstimate> {
        if now < self.observed_at() || now >= self.valid_until_ns {
            return Err(invalid());
        }
        let server = self.server.at(now)?;
        let (phase, error) = self.phase.estimate(now)?;
        let alignment = handy_continuous_alignment_error_ns(
            &self.reference,
            phase,
            error,
            server.estimated_server_ms,
            server.uncertainty_ns,
        )?;
        if alignment > self.qualified.maximum_total_error_ns {
            return Err(invalid());
        }
        Ok(ContinuousTimingEstimate {
            observed_phase_ms: phase,
            phase_uncertainty_ns: error,
            estimated_server_ms: server.estimated_server_ms,
            server_uncertainty_ns: server.uncertainty_ns,
            alignment_error_ns: alignment,
            valid_until_boottime_ns: self.valid_until_ns,
        })
    }
    pub(crate) fn agrees_with(&self, previous: &Self) -> Result<()> {
        if self.reference != previous.reference || self.qualified != previous.qualified {
            return Err(invalid());
        }
        let now = self.observed_at();
        self.at(now)?;
        previous.at(now)?;
        if now <= previous.observed_at() {
            return Err(invalid());
        }
        self.phase.overlaps(&previous.phase, now)?;
        self.server.agrees_with(&previous.server)
    }
}

#[cfg(test)]
#[path = "handy_continuous_timing_tests.rs"]
mod tests;
