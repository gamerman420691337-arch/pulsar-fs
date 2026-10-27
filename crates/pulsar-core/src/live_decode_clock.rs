//! Equal-rate process-local correspondence for native stdout delivery.
//! These facts cannot attest camera exposure, source buffering or authority.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveDecodeClockProbe {
    pub parent_send: LiveClockTick,
    pub parent_receive: LiveClockTick,
    pub worker_ready_elapsed_ns: u64,
    pub parent_end: LiveClockTick,
    pub maximum_uncertainty_ns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveDeliveryTickBounds {
    pub earliest: LiveClockTick,
    pub latest: LiveClockTick,
    pub uncertainty_ns: u64,
}
fn invalid(message: &str) -> CoreError {
    CoreError::InvalidTransition(format!("native delivery clock: {message}"))
}
impl LiveDecodeClockProbe {
    pub fn validate(&self) -> Result<(), CoreError> {
        let uncertainty = self
            .parent_receive
            .0
            .checked_sub(self.parent_send.0)
            .ok_or_else(|| invalid("receipt precedes send"))?;
        if self.maximum_uncertainty_ns == 0
            || self.maximum_uncertainty_ns > 1_000_000_000
            || uncertainty > self.maximum_uncertainty_ns
            || self.parent_end.0 == 0
            || self.parent_end.0 > 30_000_000_000
            || self.parent_receive >= self.parent_end
            || self.worker_ready_elapsed_ns >= 30_000_000_000
        {
            return Err(invalid(
                "probe exceeds clock/lifetime/uncertainty admission",
            ));
        }
        Ok(())
    }
    /// The worker sampled Ready between parent send and receipt. Equal-rate
    /// local monotonic clocks map a later delivery D to [A+D-S, B+D-S].
    /// Preserve the complete uncertainty and use earliest for conservative age.
    pub fn bounds(
        &self,
        delivered_elapsed_ns: u64,
        parent_receipt: LiveClockTick,
    ) -> Result<LiveDeliveryTickBounds, CoreError> {
        self.validate()?;
        let delta = delivered_elapsed_ns
            .checked_sub(self.worker_ready_elapsed_ns)
            .ok_or_else(|| invalid("delivery precedes readiness"))?;
        if delivered_elapsed_ns >= 30_000_000_000
            || parent_receipt < self.parent_receive
            || parent_receipt >= self.parent_end
        {
            return Err(invalid("delivery receipt exceeds probe/lifetime"));
        }
        let earliest = self
            .parent_send
            .0
            .checked_add(delta)
            .ok_or(CoreError::Overflow)?;
        let latest = self
            .parent_receive
            .0
            .checked_add(delta)
            .ok_or(CoreError::Overflow)?;
        if latest >= self.parent_end.0 || earliest > parent_receipt.0 {
            return Err(invalid(
                "delivery interval reaches expiry or contradicts receipt",
            ));
        }
        Ok(LiveDeliveryTickBounds {
            earliest: LiveClockTick(earliest),
            latest: LiveClockTick(latest),
            uncertainty_ns: latest - earliest,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn probe() -> LiveDecodeClockProbe {
        LiveDecodeClockProbe {
            parent_send: LiveClockTick(100),
            parent_receive: LiveClockTick(200),
            worker_ready_elapsed_ns: 50,
            parent_end: LiveClockTick(1000),
            maximum_uncertainty_ns: 100,
        }
    }
    #[test]
    fn every_ready_sample_offset_bounds_delivery_and_conservative_age() {
        let p = probe();
        for delta in 0..800 {
            let b = p.bounds(50 + delta, LiveClockTick(200 + delta)).unwrap();
            for sample in 100..=200 {
                let actual = sample + delta;
                assert!(b.earliest.0 <= actual && actual <= b.latest.0);
                let now = 200 + delta;
                assert!(now - b.earliest.0 >= now - actual);
            }
            assert_eq!(b.uncertainty_ns, 100);
        }
    }
    #[test]
    fn delayed_readiness_or_consumer_does_not_rejuvenate_delivery() {
        let a = probe().bounds(100, LiveClockTick(300)).unwrap();
        let mut p = probe();
        p.parent_receive = LiveClockTick(250);
        p.maximum_uncertainty_ns = 150;
        let b = p.bounds(100, LiveClockTick(900)).unwrap();
        assert_eq!(a.earliest, b.earliest);
        assert_eq!(b.uncertainty_ns, 150);
        let c = probe().bounds(100, LiveClockTick(950)).unwrap();
        assert_eq!(a, c);
    }
    #[test]
    fn reversed_preprobe_expired_future_and_excess_uncertainty_refuse() {
        let mut p = probe();
        assert!(p.bounds(49, LiveClockTick(300)).is_err());
        assert!(p.bounds(850, LiveClockTick(999)).is_err());
        assert!(p.bounds(500, LiveClockTick(300)).is_err());
        assert!(p.bounds(50, LiveClockTick(199)).is_err());
        assert!(p.bounds(50, LiveClockTick(1000)).is_err());
        assert!(p.bounds(u64::MAX, LiveClockTick(300)).is_err());
        p.maximum_uncertainty_ns = 99;
        assert!(p.validate().is_err());
        p = probe();
        p.parent_receive = LiveClockTick(99);
        assert!(p.validate().is_err());
        p = probe();
        p.parent_end = LiveClockTick(30_000_000_001);
        assert!(p.validate().is_err());
    }
    #[test]
    fn zero_roundtrip_is_exact_and_wire_facts_do_not_skip_validation() {
        let mut p = probe();
        p.parent_receive = p.parent_send;
        let b = p.bounds(100, LiveClockTick(200)).unwrap();
        assert_eq!(b.earliest, b.latest);
        assert_eq!(b.uncertainty_ns, 0);
        let mut v = serde_json::to_value(probe()).unwrap();
        v["parent_receive"] = serde_json::json!(99);
        let untrusted: LiveDecodeClockProbe = serde_json::from_value(v.clone()).unwrap();
        assert!(untrusted.validate().is_err());
        v["authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LiveDecodeClockProbe>(v).is_err());
    }
}
