//! Classify the existing native stamp refusal from its already sampled values.
//! This does not sample a clock, extend a lifetime or establish source authority.
use pulsar_protocol::{ErrorCode, ProtocolError};

#[derive(Clone, Copy)]
pub(super) struct Observation {
    pub now_latest_ns: u64,
    pub capture_earliest_ns: u64,
    pub maximum_age_ns: u64,
    pub stamped_generation: u64,
    pub current_generation: u64,
    pub sealed: bool,
    pub successor_scope: bool,
}
impl Observation {
    pub(super) fn refusal(self) -> Option<ProtocolError> {
        let generation_changed = self.stamped_generation != self.current_generation;
        let age_upper_ns = self.now_latest_ns.checked_sub(self.capture_earliest_ns);
        if !generation_changed
            && (self.sealed || age_upper_ns.is_some_and(|age| age <= self.maximum_age_ns))
        {
            return None;
        }
        let role = if self.successor_scope {
            "successor"
        } else {
            "ordinary_or_predecessor"
        };
        let age = age_upper_ns.map_or_else(|| "future".into(), |value| value.to_string());
        Some(ProtocolError::new(ErrorCode::Unavailable, format!(
            "original native capture stamp is stale or discontinuous: role={role}; sealed={}; generation_changed={generation_changed}; stamped_generation={}; current_generation={}; age_upper_ns={age}; limit_ns={}; now_latest_ns={}; capture_earliest_ns={}",
            self.sealed, self.stamped_generation, self.current_generation,
            self.maximum_age_ns, self.now_latest_ns, self.capture_earliest_ns,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_preserves_original_predicate_at_boundaries_and_clock_extremes() {
        for sealed in [false, true] {
            for successor_scope in [false, true] {
                for (stamped_generation, current_generation) in [(0, 0), (0, 1), (u64::MAX, 0)] {
                    for (now_latest_ns, capture_earliest_ns, maximum_age_ns) in [
                        (0, 0, 0),
                        (1, 2, 200),
                        (200, 0, 200),
                        (201, 0, 200),
                        (u64::MAX, 0, 200),
                        (u64::MAX, 0, u64::MAX),
                    ] {
                        let observation = Observation {
                            now_latest_ns,
                            capture_earliest_ns,
                            maximum_age_ns,
                            stamped_generation,
                            current_generation,
                            sealed,
                            successor_scope,
                        };
                        let originally_refused = stamped_generation != current_generation
                            || (!sealed
                                && now_latest_ns
                                    .checked_sub(capture_earliest_ns)
                                    .is_none_or(|age| age > maximum_age_ns));
                        let refusal = observation.refusal();
                        assert_eq!(refusal.is_some(), originally_refused);
                        if let Some(error) = refusal {
                            assert_eq!(error.code, ErrorCode::Unavailable);
                            assert!(!error.retryable);
                            assert!(error.message.len() <= 512);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn refusal_distinguishes_age_generation_and_future_capture_without_hiding_combined_failures() {
        let base = Observation {
            now_latest_ns: 201,
            capture_earliest_ns: 0,
            maximum_age_ns: 200,
            stamped_generation: 3,
            current_generation: 3,
            sealed: false,
            successor_scope: true,
        };
        let error = base.refusal().unwrap();
        assert!(error.message.contains("role=successor"));
        assert!(error.message.contains("generation_changed=false"));
        assert!(error.message.contains("age_upper_ns=201; limit_ns=200"));
        let combined = Observation {
            current_generation: 4,
            ..base
        }
        .refusal()
        .unwrap();
        assert!(combined.message.contains("generation_changed=true"));
        assert!(combined.message.contains("age_upper_ns=201"));
        let future = Observation {
            capture_earliest_ns: 202,
            ..base
        }
        .refusal()
        .unwrap();
        assert!(future.message.contains("age_upper_ns=future"));
        assert!(Observation {
            sealed: true,
            ..base
        }
        .refusal()
        .is_none());
        let sealed = Observation {
            sealed: true,
            current_generation: 4,
            ..base
        }
        .refusal()
        .unwrap();
        assert!(sealed
            .message
            .contains("sealed=true; generation_changed=true"));
    }
}
