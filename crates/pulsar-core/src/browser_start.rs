//! Conditional alignment of a declared media phase and one native reference instant.
use crate::*;
use serde::{Deserialize, Serialize};

const SCALE: i128 = 1_000_000;
const MAX_MS: u64 = (1u64 << 53) - 1;
fn invalid(message: &str) -> CoreError {
    CoreError::InvalidTransition(format!("browser start: {message}"))
}

/// Inputs must come from checked native observations and accepted external
/// assumptions. This pure value neither qualifies evidence nor grants playback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserStartPhaseRequest {
    pub bracket: BrowserClockBracket,
    pub browser_reference_us: u64,
    pub observed_project_phase: ExactSeconds,
    pub reference_boottime_ns: u64,
    pub clock: BrowserClockAssumptions,
    pub display_early_ns: u64,
    pub display_late_ns: u64,
    pub source_correspondence_error_ns: u64,
    pub discrete_media_phase_error_ns: u64,
    pub media_rate_error_ppm: u32,
    pub script_quantization_error_ns: u64,
    pub server_device_uncertainty_ns: u64,
    pub physical_execution_error_ns: u64,
    pub maximum_start_age_ns: u64,
    pub maximum_alignment_error_ns: u64,
    /// An already bounded external deadline (source, observation, controller and
    /// device clock). Calculation may shorten it, never extend it.
    pub valid_until_boottime_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserStartAlignment {
    pub reference_boottime_ns: u64,
    pub display_bounds: BrowserClockBounds,
    pub phase_earliest_ns: u64,
    pub phase_latest_ns: u64,
    pub start_time_ms: u64,
    pub phase_error_ns: u64,
    pub script_quantization_error_ns: u64,
    pub server_device_uncertainty_ns: u64,
    pub physical_execution_error_ns: u64,
    pub total_error_ns: u64,
    pub valid_until_boottime_ns: u64,
}
impl BrowserStartAlignment {
    pub fn validate(&self) -> Result<(), CoreError> {
        self.display_bounds.validate()?;
        if self.phase_earliest_ns > self.phase_latest_ns
            || self.start_time_ms > MAX_MS
            || self.reference_boottime_ns >= self.valid_until_boottime_ns
        {
            return Err(invalid("invalid phase interval or wire time"));
        }
        let wire = self
            .start_time_ms
            .checked_mul(1_000_000)
            .ok_or(CoreError::Overflow)?;
        if self.maximum_distance_ns(wire) != self.phase_error_ns
            || self.error_against(wire, self.server_device_uncertainty_ns)? != self.total_error_ns
        {
            return Err(invalid("alignment component accounting differs"));
        }
        Ok(())
    }
    pub fn maximum_distance_ns(&self, phase_ns: u64) -> u64 {
        phase_ns
            .abs_diff(self.phase_earliest_ns)
            .max(phase_ns.abs_diff(self.phase_latest_ns))
    }
    /// Compare against a separately observed device phase. Its uncertainty
    /// replaces, rather than duplicates, the original server/device component.
    pub fn error_against(
        &self,
        phase_ns: u64,
        device_uncertainty_ns: u64,
    ) -> Result<u64, CoreError> {
        self.maximum_distance_ns(phase_ns)
            .checked_add(self.script_quantization_error_ns)
            .and_then(|n| n.checked_add(device_uncertainty_ns))
            .and_then(|n| n.checked_add(self.physical_execution_error_ns))
            .ok_or(CoreError::Overflow)
    }
}

fn ceil_div(n: i128, d: i128) -> Result<i128, CoreError> {
    n.div_euclid(d)
        .checked_add(i128::from(n.rem_euclid(d) != 0))
        .ok_or(CoreError::Overflow)
}
impl BrowserStartPhaseRequest {
    pub fn calculate(&self) -> Result<BrowserStartAlignment, CoreError> {
        self.clock.validate()?;
        self.bracket.validate()?;
        if self.media_rate_error_ppm >= 1_000_000
            || self.maximum_start_age_ns == 0
            || self.maximum_alignment_error_ns == 0
        {
            return Err(invalid("invalid phase rate or admission budget"));
        }
        let deadline = self
            .bracket
            .native_issue_ns
            .checked_add(self.maximum_start_age_ns)
            .ok_or(CoreError::Overflow)?
            .min(
                self.bracket
                    .native_issue_ns
                    .checked_add(self.clock.maximum_age_ns)
                    .ok_or(CoreError::Overflow)?,
            )
            .min(self.valid_until_boottime_ns);
        if self.reference_boottime_ns >= deadline {
            return Err(invalid("original start evidence expired"));
        }
        let mapped = self.bracket.bounds_at(
            self.browser_reference_us,
            self.reference_boottime_ns,
            &self.clock,
        )?;
        let display_bounds = BrowserClockBounds {
            earliest_ns: mapped
                .earliest_ns
                .checked_sub(self.display_early_ns)
                .ok_or(CoreError::Overflow)?,
            latest_ns: mapped
                .latest_ns
                .checked_add(self.display_late_ns)
                .ok_or(CoreError::Overflow)?,
        };
        let display_horizon = self
            .bracket
            .native_issue_ns
            .checked_add(self.maximum_start_age_ns)
            .ok_or(CoreError::Overflow)?;
        if display_bounds.latest_ns >= display_horizon
            || (display_bounds.earliest_ns <= self.reference_boottime_ns
                && self.reference_boottime_ns - display_bounds.earliest_ns
                    >= self.maximum_start_age_ns)
        {
            return Err(invalid("display residual escapes the admitted age horizon"));
        }
        let lower_delta =
            i128::from(self.reference_boottime_ns) - i128::from(display_bounds.latest_ns);
        let upper_delta =
            i128::from(self.reference_boottime_ns) - i128::from(display_bounds.earliest_ns);
        let slow = SCALE - i128::from(self.media_rate_error_ppm);
        let fast = SCALE + i128::from(self.media_rate_error_ppm);
        let low_progress = lower_delta
            .checked_mul(if lower_delta < 0 { fast } else { slow })
            .ok_or(CoreError::Overflow)?
            .div_euclid(SCALE);
        let high_progress = ceil_div(
            upper_delta
                .checked_mul(if upper_delta < 0 { slow } else { fast })
                .ok_or(CoreError::Overflow)?,
            SCALE,
        )?;
        let phase_num = self
            .observed_project_phase
            .numerator()
            .checked_mul(1_000_000_000)
            .ok_or(CoreError::Overflow)?;
        let phase_den = i128::from(self.observed_project_phase.denominator());
        let phase_low = phase_num.div_euclid(phase_den);
        let phase_high = ceil_div(phase_num, phase_den)?;
        let residual = i128::from(self.source_correspondence_error_ns)
            .checked_add(i128::from(self.discrete_media_phase_error_ns))
            .ok_or(CoreError::Overflow)?;
        let low = phase_low
            .checked_add(low_progress)
            .and_then(|n| n.checked_sub(residual))
            .ok_or(CoreError::Overflow)?;
        let high = phase_high
            .checked_add(high_progress)
            .and_then(|n| n.checked_add(residual))
            .ok_or(CoreError::Overflow)?;
        let phase_earliest_ns =
            u64::try_from(low).map_err(|_| invalid("negative or unrepresentable phase"))?;
        let phase_latest_ns =
            u64::try_from(high).map_err(|_| invalid("negative or unrepresentable phase"))?;
        // A representative only: all endpoint distance, including millisecond
        // rounding, is retained. This makes no network symmetry assumption.
        let start_time_ms = u64::try_from(
            (u128::from(phase_earliest_ns) + u128::from(phase_latest_ns) + 1_000_000) / 2_000_000,
        )
        .map_err(|_| CoreError::Overflow)?;
        let wire = start_time_ms
            .checked_mul(1_000_000)
            .ok_or(CoreError::Overflow)?;
        let phase_error_ns = wire
            .abs_diff(phase_earliest_ns)
            .max(wire.abs_diff(phase_latest_ns));
        let total_error_ns = phase_error_ns
            .checked_add(self.script_quantization_error_ns)
            .and_then(|n| n.checked_add(self.server_device_uncertainty_ns))
            .and_then(|n| n.checked_add(self.physical_execution_error_ns))
            .ok_or(CoreError::Overflow)?;
        if total_error_ns > self.maximum_alignment_error_ns {
            return Err(invalid("combined alignment error exceeds accepted budget"));
        }
        let value = BrowserStartAlignment {
            reference_boottime_ns: self.reference_boottime_ns,
            display_bounds,
            phase_earliest_ns,
            phase_latest_ns,
            start_time_ms,
            phase_error_ns,
            script_quantization_error_ns: self.script_quantization_error_ns,
            server_device_uncertainty_ns: self.server_device_uncertainty_ns,
            physical_execution_error_ns: self.physical_execution_error_ns,
            total_error_ns,
            valid_until_boottime_ns: deadline,
        };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> BrowserStartPhaseRequest {
        BrowserStartPhaseRequest {
            bracket: BrowserClockBracket {
                native_issue_ns: 1_000_000_000,
                native_receive_ns: 1_002_000_000,
                browser_sample_us: 1_000_000,
            },
            browser_reference_us: 1_000_000,
            observed_project_phase: ExactSeconds::new(10, 1).unwrap(),
            reference_boottime_ns: 1_010_000_000,
            clock: BrowserClockAssumptions {
                timestamp_error_ns: 0,
                relative_rate_error_ppm: 0,
                maximum_age_ns: 1_000_000_000,
                maximum_interval_ns: 10_000_000,
            },
            display_early_ns: 0,
            display_late_ns: 0,
            source_correspondence_error_ns: 0,
            discrete_media_phase_error_ns: 0,
            media_rate_error_ppm: 0,
            script_quantization_error_ns: 500_000,
            server_device_uncertainty_ns: 100_000,
            physical_execution_error_ns: 200_000,
            maximum_start_age_ns: 200_000_000,
            maximum_alignment_error_ns: 20_000_000,
            valid_until_boottime_ns: 1_150_000_000,
        }
    }
    #[test]
    fn whole_bracket_and_each_error_component_survive_wire_rounding() {
        let a = request().calculate().unwrap();
        assert_eq!(
            (a.phase_earliest_ns, a.phase_latest_ns),
            (10_008_000_000, 10_010_000_000)
        );
        assert_eq!(a.start_time_ms, 10_009);
        assert_eq!(a.phase_error_ns, 1_000_000);
        assert_eq!(a.total_error_ns, 1_800_000);
        assert_eq!(a.valid_until_boottime_ns, 1_150_000_000);
        // An independently observed device phase replaces the old server/device
        // component; the full browser interval and script/physical terms remain.
        assert_eq!(a.error_against(10_009_000_000, 300_000).unwrap(), 2_000_000);
    }
    #[test]
    fn reference_advances_phase_without_rejuvenating_original_expiry() {
        let r = request();
        let first = r.calculate().unwrap();
        let mut later = r.clone();
        later.reference_boottime_ns += 10_000_000;
        let after = later.calculate().unwrap();
        assert_eq!(after.start_time_ms, first.start_time_ms + 10);
        assert_eq!(
            after.phase_earliest_ns,
            first.phase_earliest_ns + 10_000_000
        );
        assert_eq!(after.valid_until_boottime_ns, first.valid_until_boottime_ns);
        later.reference_boottime_ns = first.valid_until_boottime_ns;
        assert!(later.calculate().is_err());
    }
    #[test]
    fn signed_display_residual_and_rate_progression_contain_rational_oracle() {
        for future in [false, true] {
            let mut r = request();
            r.display_early_ns = 2_000_000;
            r.display_late_ns = 3_000_000;
            r.media_rate_error_ppm = 100_000;
            r.source_correspondence_error_ns = 100;
            r.discrete_media_phase_error_ns = 200;
            if future {
                r.browser_reference_us += 20_000;
            }
            let a = r.calculate().unwrap();
            for display in [a.display_bounds.earliest_ns, a.display_bounds.latest_ns] {
                for rate in [900_000i128, 1_000_000, 1_100_000] {
                    for residual in [-300i128, 0, 300] {
                        let exact = (10_000_000_000i128 + residual) * SCALE
                            + (i128::from(r.reference_boottime_ns) - i128::from(display)) * rate;
                        assert!(i128::from(a.phase_earliest_ns) * SCALE <= exact);
                        assert!(i128::from(a.phase_latest_ns) * SCALE >= exact);
                    }
                }
            }
        }
    }
    #[test]
    fn fractional_phase_rounds_outward_and_keeps_millisecond_distance() {
        let mut r = request();
        r.bracket.native_receive_ns = r.bracket.native_issue_ns;
        r.reference_boottime_ns = r.bracket.native_issue_ns;
        r.observed_project_phase = ExactSeconds::new(1, 3).unwrap();
        let a = r.calculate().unwrap();
        assert_eq!(
            (a.phase_earliest_ns, a.phase_latest_ns),
            (333_333_333, 333_333_334)
        );
        assert_eq!(a.start_time_ms, 333);
        assert_eq!(a.phase_error_ns, 333_334);
    }
    #[test]
    fn negative_overflow_expired_and_excess_budget_refuse() {
        for case in 0..7 {
            let mut r = request();
            match case {
                0 => {
                    r.observed_project_phase = ExactSeconds::new(0, 1).unwrap();
                    r.display_late_ns = 20_000_000;
                }
                1 => r.reference_boottime_ns = r.valid_until_boottime_ns,
                2 => r.maximum_alignment_error_ns = 1_799_999,
                3 => r.media_rate_error_ppm = 1_000_000,
                4 => r.physical_execution_error_ns = u64::MAX,
                5 => r.display_early_ns = u64::MAX,
                _ => r.observed_project_phase = ExactSeconds::new(i128::MAX, 1).unwrap(),
            }
            assert!(r.calculate().is_err(), "case {case}");
        }
        let mut a = request().calculate().unwrap();
        a.total_error_ns -= 1;
        assert!(a.validate().is_err());
    }
}
