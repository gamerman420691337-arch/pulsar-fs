//! Conditional browser/native clock intervals, never display measurements or authority.
use crate::CoreError;
use serde::{Deserialize, Serialize};

const MAX_BROWSER_TICK_US: u64 = (1u64 << 53) - 1;
const RATE_SCALE: i128 = 1_000_000;

/// Native issue/complete-ingress bounds for one fresh browser sample execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserClockBracket {
    pub native_issue_ns: u64,
    pub native_receive_ns: u64,
    pub browser_sample_us: u64,
}

/// Externally justified assumptions, not inferred from a successful roundtrip.
/// `timestamp_error_ns` bounds each browser timestamp's error. The browser/native
/// clock rate ratio is in [1 - ppm/1_000_000, 1 + ppm/1_000_000].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserClockAssumptions {
    pub timestamp_error_ns: u64,
    pub relative_rate_error_ppm: u32,
    pub maximum_age_ns: u64,
    pub maximum_interval_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserClockBounds {
    pub earliest_ns: u64,
    pub latest_ns: u64,
}

fn invalid(message: &str) -> CoreError {
    CoreError::InvalidTransition(format!("browser clock: {message}"))
}

impl BrowserClockAssumptions {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.relative_rate_error_ppm >= RATE_SCALE as u32
            || self.maximum_age_ns == 0
            || self.maximum_interval_ns == 0
        {
            return Err(invalid("invalid rate, age or interval assumptions"));
        }
        Ok(())
    }
}

impl BrowserClockBounds {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.earliest_ns > self.latest_ns {
            return Err(invalid("reversed bounds"));
        }
        Ok(())
    }
}

impl BrowserClockBracket {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.native_receive_ns < self.native_issue_ns
            || self.browser_sample_us > MAX_BROWSER_TICK_US
        {
            return Err(invalid("reversed bracket or invalid browser sample"));
        }
        Ok(())
    }

    /// The retained bounds for the known sample execution itself. This is raw
    /// historical metadata: no expiry, current authority or display claim follows.
    /// Equal reported browser ticks do not identify this same execution event.
    pub fn sample_bounds(&self) -> Result<BrowserClockBounds, CoreError> {
        self.validate()?;
        Ok(BrowserClockBounds {
            earliest_ns: self.native_issue_ns,
            latest_ns: self.native_receive_ns,
        })
    }

    /// Map a browser timestamp under explicit error/rate assumptions. Both stamp
    /// errors contribute even when its reported tick equals the sample: coarse
    /// timers can assign identical ticks to distinct events. Signed deltas
    /// support earlier callback/composition timestamps. Neither this result nor
    /// `sample_bounds` measures actual display time; expected-display predictions
    /// need their own externally justified error envelope.
    ///
    /// The bracket expires at issue + maximum_age (half-open). All mapped bounds
    /// must also precede that horizon, and their earliest possible instant must
    /// be less than maximum_age old at `now_ns`. Future targets cannot extend the
    /// original horizon. No saturating arithmetic silently clips uncertainty.
    pub fn bounds_at(
        &self,
        browser_tick_us: u64,
        now_ns: u64,
        assumptions: &BrowserClockAssumptions,
    ) -> Result<BrowserClockBounds, CoreError> {
        self.validate()?;
        assumptions.validate()?;
        let horizon = self
            .native_issue_ns
            .checked_add(assumptions.maximum_age_ns)
            .ok_or(CoreError::Overflow)?;
        if browser_tick_us > MAX_BROWSER_TICK_US
            || now_ns < self.native_receive_ns
            || now_ns >= horizon
        {
            return Err(invalid(
                "invalid target, native reversal or expired bracket",
            ));
        }
        let delta = i128::from(browser_tick_us)
            .checked_sub(i128::from(self.browser_sample_us))
            .and_then(|value| value.checked_mul(1000))
            .ok_or(CoreError::Overflow)?;
        let error = i128::from(assumptions.timestamp_error_ns)
            .checked_mul(2)
            .ok_or(CoreError::Overflow)?;
        let low = delta.checked_sub(error).ok_or(CoreError::Overflow)?;
        let high = delta.checked_add(error).ok_or(CoreError::Overflow)?;
        let rate = i128::from(assumptions.relative_rate_error_ppm);
        let slow = RATE_SCALE.checked_sub(rate).ok_or(CoreError::Overflow)?;
        let fast = RATE_SCALE.checked_add(rate).ok_or(CoreError::Overflow)?;
        // For negative deltas the slower positive rate makes the result
        // more negative. Cross-zero intervals use the slow denominator at
        // both outer endpoints. Reciprocal division is essential near ppm=1e6.
        let lower_denominator = if low < 0 { slow } else { fast };
        let upper_denominator = if high < 0 { fast } else { slow };
        let lower = low
            .checked_mul(RATE_SCALE)
            .ok_or(CoreError::Overflow)?
            .div_euclid(lower_denominator);
        let upper_numerator = high.checked_mul(RATE_SCALE).ok_or(CoreError::Overflow)?;
        let upper = upper_numerator
            .div_euclid(upper_denominator)
            .checked_add(i128::from(
                upper_numerator.rem_euclid(upper_denominator) != 0,
            ))
            .ok_or(CoreError::Overflow)?;
        let earliest = i128::from(self.native_issue_ns)
            .checked_add(lower)
            .ok_or(CoreError::Overflow)?;
        let latest = i128::from(self.native_receive_ns)
            .checked_add(upper)
            .ok_or(CoreError::Overflow)?;
        let bounds = BrowserClockBounds {
            earliest_ns: u64::try_from(earliest).map_err(|_| CoreError::Overflow)?,
            latest_ns: u64::try_from(latest).map_err(|_| CoreError::Overflow)?,
        };
        bounds.validate()?;
        let width = bounds
            .latest_ns
            .checked_sub(bounds.earliest_ns)
            .ok_or(CoreError::Overflow)?;
        if width > assumptions.maximum_interval_ns
            || bounds.latest_ns >= horizon
            || (bounds.earliest_ns <= now_ns
                && now_ns - bounds.earliest_ns >= assumptions.maximum_age_ns)
        {
            return Err(invalid(
                "mapped interval exceeds uncertainty or age horizon",
            ));
        }
        Ok(bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bracket() -> BrowserClockBracket {
        BrowserClockBracket {
            native_issue_ns: 1_000_000,
            native_receive_ns: 1_002_000,
            browser_sample_us: 1000,
        }
    }
    fn assumptions() -> BrowserClockAssumptions {
        BrowserClockAssumptions {
            timestamp_error_ns: 0,
            relative_rate_error_ppm: 0,
            maximum_age_ns: 1_000_000,
            maximum_interval_ns: 100_000,
        }
    }

    #[test]
    fn anchor_retains_whole_bracket_without_invented_precision() {
        let b = bracket();
        assert_eq!(
            b.sample_bounds().unwrap(),
            BrowserClockBounds {
                earliest_ns: 1_000_000,
                latest_ns: 1_002_000,
            }
        );
        let mut delayed = b;
        delayed.native_receive_ns += 9000;
        let mapped = delayed.sample_bounds().unwrap();
        assert_eq!(mapped.latest_ns - mapped.earliest_ns, 11_000);
    }

    #[test]
    fn equal_coarse_ticks_do_not_identify_the_anchor_event() {
        let b = bracket();
        let mut a = assumptions();
        a.timestamp_error_ns = 500;
        a.relative_rate_error_ppm = 500_000;
        assert_eq!(
            b.bounds_at(b.browser_sample_us, b.native_receive_ns, &a)
                .unwrap(),
            BrowserClockBounds {
                earliest_ns: 998_000,
                latest_ns: 1_004_000,
            }
        );
        assert_eq!(
            b.sample_bounds().unwrap(),
            BrowserClockBounds {
                earliest_ns: 1_000_000,
                latest_ns: 1_002_000,
            }
        );
        // Quantization permits two distinct events, separated by 2000 native
        // nanoseconds at the slowest admitted rate, to report this same tick.
        a.timestamp_error_ns = 0;
        assert_eq!(
            b.bounds_at(b.browser_sample_us, b.native_receive_ns, &a)
                .unwrap(),
            b.sample_bounds().unwrap()
        );
    }

    #[test]
    fn reciprocal_rate_bounds_round_outward_for_both_directions() {
        let mut b = bracket();
        b.native_receive_ns = b.native_issue_ns;
        let mut a = assumptions();
        a.relative_rate_error_ppm = 500_000;
        assert_eq!(
            b.bounds_at(1001, b.native_receive_ns, &a).unwrap(),
            BrowserClockBounds {
                earliest_ns: 1_000_666,
                latest_ns: 1_002_000,
            }
        );
        assert_eq!(
            b.bounds_at(999, b.native_receive_ns, &a).unwrap(),
            BrowserClockBounds {
                earliest_ns: 998_000,
                latest_ns: 999_334,
            }
        );
        // A first-order 1+ppm bound would incorrectly cap the future at 1500ns.
    }

    #[test]
    fn both_timestamp_errors_expand_delta_and_can_cross_zero() {
        let mut a = assumptions();
        a.timestamp_error_ns = 750;
        a.relative_rate_error_ppm = 500_000;
        let got = bracket().bounds_at(1001, 1_002_000, &a).unwrap();
        assert_eq!(
            got,
            BrowserClockBounds {
                earliest_ns: 999_000,
                latest_ns: 1_007_000
            }
        );
    }

    #[test]
    fn interval_contains_rational_oracle_for_stamp_errors_rates_and_sample_instants() {
        let b = bracket();
        let mut a = assumptions();
        a.timestamp_error_ns = 500;
        a.relative_rate_error_ppm = 500_000;
        for target in [998, 999, 1000, 1001, 1002] {
            let got = b.bounds_at(target, b.native_receive_ns, &a).unwrap();
            for actual_sample in [b.native_issue_ns, 1_001_000, b.native_receive_ns] {
                for anchor_error in [-500i128, 0, 500] {
                    for target_error in [-500i128, 0, 500] {
                        let true_browser_delta =
                            (i128::from(target) - 1000) * 1000 + anchor_error - target_error;
                        for rate in [500_000i128, 750_000, 1_000_000, 1_250_000, 1_500_000] {
                            let actual_numerator =
                                i128::from(actual_sample) * rate + true_browser_delta * 1_000_000;
                            assert!(i128::from(got.earliest_ns) * rate <= actual_numerator);
                            assert!(i128::from(got.latest_ns) * rate >= actual_numerator);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn expired_reversed_old_future_uncertain_and_unrepresentable_refuse() {
        let b = bracket();
        let a = assumptions();
        assert!(b.bounds_at(1000, 1_001_999, &a).is_err());
        assert!(b.bounds_at(1000, 2_000_000, &a).is_err());
        assert!(b.bounds_at(2000, 1_002_000, &a).is_err());
        assert!(b.bounds_at(0, 1_002_000, &a).is_err());
        assert!(b.bounds_at(MAX_BROWSER_TICK_US + 1, 1_002_000, &a).is_err());
        let mut bad = b;
        bad.native_receive_ns = bad.native_issue_ns - 1;
        assert!(bad.validate().is_err());
        bad = b;
        bad.browser_sample_us = MAX_BROWSER_TICK_US + 1;
        assert!(bad.validate().is_err());
        let mut limited = a;
        limited.maximum_interval_ns = 1999;
        assert!(b.bounds_at(1000, 1_002_000, &limited).is_err());
        limited = a;
        limited.timestamp_error_ns = u64::MAX;
        assert!(b.bounds_at(999, 1_002_000, &limited).is_err());
        bad = b;
        bad.native_issue_ns = u64::MAX - 10;
        bad.native_receive_ns = u64::MAX;
        assert!(bad.bounds_at(1000, u64::MAX, &a).is_err());
    }

    #[test]
    fn assumptions_and_wire_values_revalidate_without_defaults_or_authority() {
        for field in [
            "relative_rate_error_ppm",
            "maximum_age_ns",
            "maximum_interval_ns",
        ] {
            let mut value = serde_json::to_value(assumptions()).unwrap();
            value[field] = serde_json::json!(if field == "relative_rate_error_ppm" {
                1_000_000
            } else {
                0
            });
            let bad: BrowserClockAssumptions = serde_json::from_value(value).unwrap();
            assert!(bad.validate().is_err());
        }
        let mut value = serde_json::to_value(bracket()).unwrap();
        value["qualified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<BrowserClockBracket>(value).is_err());
        assert!(serde_json::from_str::<BrowserClockAssumptions>("{}").is_err());
        assert!(BrowserClockBounds {
            earliest_ns: 2,
            latest_ns: 1
        }
        .validate()
        .is_err());
    }
}
