//! Interval correspondence for an equal-rate local monotonic clock probe.
//! Supplied facts do not attest camera timestamps, clock rates or authority.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveClockProbe {
    pub producer_send_ns: u64,
    pub producer_receive_ns: u64,
    pub server_sample: LiveClockTick,
    pub lease_end: LiveClockTick,
    pub maximum_uncertainty_ns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureTickBounds {
    pub earliest: LiveClockTick,
    pub latest: LiveClockTick,
    pub uncertainty_ns: u64,
}
fn invalid(message: &str) -> CoreError {
    CoreError::InvalidTransition(format!("live clock probe: {message}"))
}
impl LiveClockProbe {
    pub fn validate(&self) -> Result<(), CoreError> {
        let uncertainty = self
            .producer_receive_ns
            .checked_sub(self.producer_send_ns)
            .ok_or_else(|| invalid("receive precedes send"))?;
        if self.maximum_uncertainty_ns == 0
            || self.maximum_uncertainty_ns > 1_000_000_000
            || uncertainty > self.maximum_uncertainty_ns
            || self.lease_end.0 == 0
            || self.lease_end.0 > 30_000_000_000
            || self.server_sample >= self.lease_end
        {
            return Err(invalid("uncertainty or lease is outside admission"));
        }
        Ok(())
    }
    /// Bounds assume equal-rate clocks and a server sample taken during the
    /// supplied request interval. Only captures after receipt are admitted.
    /// Use `earliest` for conservative age; never the midpoint or latest tick.
    pub fn bounds(&self, producer_capture_ns: u64) -> Result<LiveCaptureTickBounds, CoreError> {
        self.validate()?;
        let after_receive = producer_capture_ns
            .checked_sub(self.producer_receive_ns)
            .ok_or_else(|| invalid("capture precedes completed probe"))?;
        let after_send = producer_capture_ns
            .checked_sub(self.producer_send_ns)
            .ok_or(CoreError::Overflow)?;
        let earliest = self
            .server_sample
            .0
            .checked_add(after_receive)
            .ok_or(CoreError::Overflow)?;
        let latest = self
            .server_sample
            .0
            .checked_add(after_send)
            .ok_or(CoreError::Overflow)?;
        if latest >= self.lease_end.0 {
            return Err(invalid("capture interval reaches lease expiry"));
        }
        Ok(LiveCaptureTickBounds {
            earliest: LiveClockTick(earliest),
            latest: LiveClockTick(latest),
            uncertainty_ns: latest - earliest,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn probe() -> LiveClockProbe {
        LiveClockProbe {
            producer_send_ns: 10,
            producer_receive_ns: 110,
            server_sample: LiveClockTick(200),
            lease_end: LiveClockTick(1000),
            maximum_uncertainty_ns: 100,
        }
    }
    #[test]
    fn all_possible_sample_instants_bound_capture_without_rejuvenation() {
        let p = probe();
        for capture in 110..900 {
            if let Ok(bounds) = p.bounds(capture) {
                assert_eq!(bounds.uncertainty_ns, 100);
                for sample_in_producer in 10..=110 {
                    let actual = 200 + capture - sample_in_producer;
                    assert!(bounds.earliest.0 <= actual && actual <= bounds.latest.0);
                    let analysis = bounds.latest.0 + 300;
                    assert!(analysis - bounds.earliest.0 >= analysis - actual);
                }
            }
        }
    }
    #[test]
    fn delayed_probe_changes_uncertainty_and_conservative_age_not_source_time() {
        let mut p = probe();
        p.producer_receive_ns = 60;
        let a = p.bounds(150).unwrap();
        p.producer_receive_ns = 110;
        let b = p.bounds(150).unwrap();
        assert_eq!(a.latest, b.latest);
        assert_eq!(a.earliest.0 - b.earliest.0, 50);
        assert_eq!(b.uncertainty_ns - a.uncertainty_ns, 50);
    }
    #[test]
    fn preprobe_expired_reversed_and_excess_uncertainty_refuse() {
        let mut p = probe();
        assert!(p.bounds(109).is_err());
        assert!(p.bounds(810).is_err()); // Latest equals lease end.
        p.maximum_uncertainty_ns = 99;
        assert!(p.validate().is_err());
        p.maximum_uncertainty_ns = 100;
        p.producer_receive_ns = 9;
        assert!(p.validate().is_err());
        p = probe();
        p.server_sample = p.lease_end;
        assert!(p.validate().is_err());
        p = probe();
        p.lease_end = LiveClockTick(30_000_000_001);
        assert!(p.validate().is_err());
        p = probe();
        assert!(p.bounds(u64::MAX).is_err());
    }
    #[test]
    fn zero_roundtrip_is_exact_and_wire_facts_revalidate() {
        let mut p = probe();
        p.producer_receive_ns = p.producer_send_ns;
        let b = p.bounds(20).unwrap();
        assert_eq!(b.earliest, b.latest);
        assert_eq!(b.uncertainty_ns, 0);
        let mut value = serde_json::to_value(&p).unwrap();
        value["producer_receive_ns"] = serde_json::json!(9);
        let untrusted: LiveClockProbe = serde_json::from_value(value.clone()).unwrap();
        assert!(untrusted.bounds(20).is_err());
        value["assumed_authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LiveClockProbe>(value).is_err());
    }
}
