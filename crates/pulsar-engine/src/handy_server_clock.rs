//! Credential-free, bounded server-time acquisition and conservative native mapping.
use crate::playback_runtime::PlaybackClock;
use pulsar_protocol::*;
use std::{
    io::Read,
    time::{Duration, Instant},
};

const NS_MS: u128 = 1_000_000;
const MAX_MS: u64 = (1u64 << 53) - 1;
const BODY_LIMIT: u64 = 1024;
const ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
type Result<T> = std::result::Result<T, ProtocolError>;
fn invalid() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "Handy server-clock observation is invalid, stale or outside accepted bounds",
    )
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct QualifiedServerClock {
    pub(crate) contract: HandyServerClockQualification,
    pub(crate) max_age_ns: u64,
    pub(crate) max_uncertainty_ns: u64,
    pub(crate) drift_ppm: u32,
}
#[derive(Clone)]
pub(crate) struct NativeServerClock {
    pub(crate) clock: PlaybackClock,
    pub(crate) sent_ns: u64,
    qualified: QualifiedServerClock,
}
#[derive(Clone)]
pub(crate) struct Sample {
    pub(crate) sent_ns: u64,
    pub(crate) received_ns: u64,
    pub(crate) server_ms: u64,
}
fn drift(ns: u64, ppm: u32) -> u128 {
    (u128::from(ns) * u128::from(ppm)).div_ceil(NS_MS)
}
impl NativeServerClock {
    pub(crate) fn new(
        domain: ArtifactId,
        sample: Sample,
        qualified: QualifiedServerClock,
    ) -> Result<Self> {
        let rtt = sample
            .received_ns
            .checked_sub(sample.sent_ns)
            .ok_or_else(invalid)?;
        if sample.server_ms > MAX_MS || rtt > qualified.max_age_ns {
            return Err(invalid());
        }
        let server = u128::from(sample.server_ms) * NS_MS;
        let error = u128::from(qualified.contract.timestamp_error_ns);
        let lower = server.checked_sub(error).ok_or_else(invalid)?;
        let upper = server
            .checked_add(error)
            .and_then(|n| n.checked_add(u128::from(rtt)))
            .and_then(|n| n.checked_add(drift(rtt, qualified.drift_ppm)))
            .ok_or_else(invalid)?;
        // The entire request interval is covered; no symmetric network delay assumption.
        // Round to the nearest representable millisecond and include that rounding error.
        let estimate = lower
            .checked_add(upper)
            .and_then(|n| n.checked_add(NS_MS))
            .ok_or_else(invalid)?
            / (2 * NS_MS);
        let estimate_ns = estimate.checked_mul(NS_MS).ok_or_else(invalid)?;
        let uncertainty = estimate_ns.abs_diff(lower).max(upper.abs_diff(estimate_ns));
        let result = Self {
            clock: PlaybackClock {
                domain,
                monotonic_ns: sample.received_ns,
                // Saturation only shortens validity to the representable native domain.
                valid_until_ns: sample.sent_ns.saturating_add(qualified.max_age_ns),
                estimated_server_ms: estimate.try_into().map_err(|_| invalid())?,
                uncertainty_ns: uncertainty.try_into().map_err(|_| invalid())?,
            },
            sent_ns: sample.sent_ns,
            qualified,
        };
        result.at(sample.received_ns)?;
        Ok(result)
    }
    /// Freshness is measured from request start, never from a later status/replay.
    pub(crate) fn at(&self, now: u64) -> Result<PlaybackClock> {
        let full_age = now.checked_sub(self.sent_ns).ok_or_else(invalid)?;
        let age = now
            .checked_sub(self.clock.monotonic_ns)
            .ok_or_else(invalid)?;
        let uncertainty = u128::from(self.clock.uncertainty_ns)
            + drift(age, self.qualified.drift_ppm)
            + u128::from(age % NS_MS as u64);
        let estimate = self
            .clock
            .estimated_server_ms
            .checked_add(age / NS_MS as u64)
            .ok_or_else(invalid)?;
        if full_age > self.qualified.max_age_ns
            || uncertainty > u128::from(self.qualified.max_uncertainty_ns)
            || estimate > MAX_MS
        {
            return Err(invalid());
        }
        Ok(PlaybackClock {
            domain: self.clock.domain.clone(),
            monotonic_ns: now,
            valid_until_ns: self.clock.valid_until_ns,
            estimated_server_ms: estimate,
            uncertainty_ns: uncertainty as u64,
        })
    }
    pub(crate) fn agrees_with(&self, previous: &Self) -> Result<()> {
        // A stale predecessor supplies no accuracy claim. A still-valid predecessor
        // must overlap this independently acquired interval; averaging cannot narrow it.
        if let Ok(old) = previous.at(self.clock.monotonic_ns) {
            let a = u128::from(old.estimated_server_ms) * NS_MS;
            let b = u128::from(self.clock.estimated_server_ms) * NS_MS;
            if a.abs_diff(b)
                > u128::from(old.uncertainty_ns) + u128::from(self.clock.uncertainty_ns)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    /// Continued authority requires actual overlap while the predecessor is
    /// still valid. The ordinary comparison also serves independent startup
    /// observations and intentionally does not impose this continuity rule.
    pub(crate) fn continues(&self, previous: &Self, published_at_ns: u64) -> Result<()> {
        if self.clock.domain != previous.clock.domain
            || self.clock.monotonic_ns <= previous.clock.monotonic_ns
            || self.sent_ns < previous.clock.monotonic_ns
            || published_at_ns < self.clock.monotonic_ns
            || published_at_ns >= previous.clock.valid_until_ns
            || self.clock.monotonic_ns >= previous.clock.valid_until_ns
        {
            return Err(invalid());
        }
        previous.at(self.clock.monotonic_ns)?;
        previous.at(published_at_ns)?;
        self.at(published_at_ns)?;
        self.agrees_with(previous)
    }
}

fn decode(bytes: &[u8]) -> Result<u64> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        server_time: u64,
    }
    let reply: Reply = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if reply.server_time > MAX_MS {
        return Err(invalid());
    }
    Ok(reply.server_time)
}
pub(crate) fn sample<F: FnMut() -> Result<u64> + ?Sized>(clock: &mut F) -> Result<Sample> {
    sample_at(ORIGIN, clock, Duration::from_secs(10))
}
pub(crate) fn sample_until(
    deadline: Instant,
    clock: &mut dyn FnMut() -> Result<u64>,
) -> Result<Sample> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(invalid)?;
    if remaining.is_zero() {
        return Err(invalid());
    }
    sample_at(ORIGIN, clock, remaining.min(Duration::from_secs(10)))
}
fn sample_at<F: FnMut() -> Result<u64> + ?Sized>(
    origin: &str,
    clock: &mut F,
    timeout: Duration,
) -> Result<Sample> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let deadline = Instant::now().checked_add(timeout).ok_or_else(invalid)?;
    let sent_ns = clock()?;
    let mut response = agent
        .get(format!("{origin}/servertime"))
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache, no-store")
        .call()
        .map_err(|_| invalid())?;
    if response.status().as_u16() != 200 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(BODY_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    let received_ns = clock()?;
    if bytes.len() as u64 > BODY_LIMIT || Instant::now() >= deadline || received_ns < sent_ns {
        return Err(invalid());
    }
    Ok(Sample {
        sent_ns,
        received_ns,
        server_ms: decode(&bytes)?,
    })
}

#[cfg(test)]
#[path = "handy_server_clock_tests.rs"]
mod tests;

#[cfg(test)]
mod continuity_tests {
    use super::*;
    #[test]
    fn continuing_clock_requires_original_unexpired_overlap_at_completion_and_publication() {
        let q = QualifiedServerClock {
            contract: HandyServerClockQualification {
                model: HandyServerClockModel::RequestBracketMonotonic,
                timestamp_error_ns: 1_000_000,
            },
            max_age_ns: 1_000_000_000,
            max_uncertainty_ns: 10_000_000,
            drift_ppm: 0,
        };
        let first = NativeServerClock::new(
            ArtifactId::new("domain").unwrap(),
            Sample {
                sent_ns: 0,
                received_ns: 1,
                server_ms: 1000,
            },
            q.clone(),
        )
        .unwrap();
        let next = NativeServerClock::new(
            first.clock.domain.clone(),
            Sample {
                sent_ns: 100_000_000,
                received_ns: 100_000_001,
                server_ms: 1100,
            },
            q.clone(),
        )
        .unwrap();
        next.continues(&first, 100_000_002).unwrap();
        assert!(next.continues(&first, 1_000_000_000).is_err());
        let late = NativeServerClock::new(
            first.clock.domain.clone(),
            Sample {
                sent_ns: 1_100_000_000,
                received_ns: 1_100_000_001,
                server_ms: 2100,
            },
            q.clone(),
        )
        .unwrap();
        late.agrees_with(&first).unwrap();
        assert!(late.continues(&first, 1_100_000_002).is_err());
        let foreign = NativeServerClock::new(
            ArtifactId::new("other").unwrap(),
            Sample {
                sent_ns: 100_000_000,
                received_ns: 100_000_001,
                server_ms: 1100,
            },
            q,
        )
        .unwrap();
        assert!(foreign.continues(&first, 100_000_002).is_err());
        assert!(first.continues(&next, 100_000_002).is_err());
    }
}
