//! Quantization and a constant-space envelope for discrete HSP samples only.
//!
//! The caller supplies an explicitly mapped, stream-relative project time and
//! independently checked source continuity. Receipt time and analysis completion
//! are not time mappings. This module neither creates an initial point nor
//! establishes authority, interpolation, physical dynamics or qualification.
use crate::{HspPoint, NormalizedPosition, ProjectTime};

const NS_PER_MS: u64 = 1_000_000;

/// Accepted sample limits, applied to the integer points actually selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspEnvelope {
    pub minimum_position_percent: u8,
    pub maximum_position_percent: u8,
    pub maximum_step_percent: u8,
    pub minimum_segment_ms: u64,
}
impl HspEnvelope {
    pub fn validate(self) -> Result<(), HspEnvelopeError> {
        if self.minimum_position_percent >= self.maximum_position_percent
            || self.maximum_position_percent > 100
            || !(1..=100).contains(&self.maximum_step_percent)
            || !(1..=u64::from(u32::MAX)).contains(&self.minimum_segment_ms)
        {
            return Err(HspEnvelopeError::InvalidEnvelope);
        }
        Ok(())
    }
}

/// Displacement of this input sample caused solely by integer quantization.
/// Position error is in millionths of normalized full travel, rounded upward.
/// Neither bound estimates model, mapping, interpolation or physical error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspQuantizedPoint {
    pub point: HspPoint,
    pub time_error_ns: u64,
    pub position_error_ppm: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HspEnvelopeError {
    #[error("invalid discrete HSP sample envelope")]
    InvalidEnvelope,
    #[error("HSP stream-relative time must be nonnegative")]
    NegativeTime,
    #[error("rounded HSP time exceeds the unsigned 32-bit millisecond horizon")]
    TimeOverflow,
    #[error("HSP quantization error arithmetic overflow")]
    QuantizationOverflow,
    #[error("HSP integer sample times collide or regress")]
    NonIncreasingTime,
    #[error("HSP integer position exceeds the accepted sample range")]
    OutsidePositionRange,
    #[error("HSP adjacent integer position change exceeds the accepted sample limit")]
    StepTooLarge,
    #[error("HSP adjacent integer time interval is below the accepted sample limit")]
    SegmentTooShort,
    #[error("HSP sample lacks checked continuity from the previous accepted sample")]
    Discontinuous,
}

/// Select nearest integer milliseconds and percent, with halfway values rounded
/// upward. Percent selection follows `(position * 100.0).round()`; its reported
/// error also includes any floating-point multiplication rounding at a boundary.
///
/// Times that round past `u32::MAX` refuse. A time up to 499,999ns beyond the last
/// integer millisecond can round back to it with its explicit error reported.
/// No clipping, amplitude remapping, interpolation or implicit origin is used.
pub fn quantize_hsp_point(
    relative_time: ProjectTime,
    position: NormalizedPosition,
) -> Result<HspQuantizedPoint, HspEnvelopeError> {
    let ns = u64::try_from(relative_time.as_nanos()).map_err(|_| HspEnvelopeError::NegativeTime)?;
    let rounded_ms = ns
        .checked_add(NS_PER_MS / 2)
        .ok_or(HspEnvelopeError::TimeOverflow)?
        / NS_PER_MS;
    let time_ms = u32::try_from(rounded_ms).map_err(|_| HspEnvelopeError::TimeOverflow)?;
    let selected_ns = u64::from(time_ms)
        .checked_mul(NS_PER_MS)
        .ok_or(HspEnvelopeError::TimeOverflow)?;
    // NormalizedPosition admits only finite 0..=1 values, including through
    // deserialization. The rounded result is consequently an integer in 0..=100.
    let position_percent = (position.value() * 100.0).round() as u8;
    Ok(HspQuantizedPoint {
        point: HspPoint {
            time_ms,
            position_percent,
        },
        time_error_ns: ns.abs_diff(selected_ns),
        position_error_ppm: position_error_ppm(position, position_percent)?,
    })
}

fn position_error_ppm(position: NormalizedPosition, selected: u8) -> Result<u32, HspEnvelopeError> {
    // The admitted f64 is exactly significand / 2^shift. Compare that rational
    // with selected/100 using integers: ceil(|100*significand-selected*2^shift|
    // *10_000 / 2^shift). Floating subtraction could lose a positive error (for
    // example 0.3 versus exact 30/100), or underflow a subnormal error to zero.
    let bits = position.value().to_bits();
    let fraction = bits & ((1u64 << 52) - 1);
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let (significand, shift) = if exponent == 0 {
        (fraction, 1074)
    } else {
        (fraction | (1u64 << 52), 1075 - exponent)
    };
    if shift >= u128::BITS {
        // All these admitted values are below one ppm, and round to zero.
        if selected != 0 {
            return Err(HspEnvelopeError::QuantizationOverflow);
        }
        return Ok(u32::from(significand != 0));
    }
    let denominator = 1u128
        .checked_shl(shift)
        .ok_or(HspEnvelopeError::QuantizationOverflow)?;
    let original = u128::from(significand)
        .checked_mul(100)
        .ok_or(HspEnvelopeError::QuantizationOverflow)?;
    let selected = u128::from(selected)
        .checked_mul(denominator)
        .ok_or(HspEnvelopeError::QuantizationOverflow)?;
    let scaled_error = original
        .abs_diff(selected)
        .checked_mul(10_000)
        .ok_or(HspEnvelopeError::QuantizationOverflow)?;
    let ceiling = (scaled_error / denominator)
        .checked_add(u128::from(scaled_error % denominator != 0))
        .ok_or(HspEnvelopeError::QuantizationOverflow)?;
    u32::try_from(ceiling).map_err(|_| HspEnvelopeError::QuantizationOverflow)
}

/// One previous accepted point, retained across every caller batch/chunk.
///
/// Every refusal leaves this state unchanged. It does not license skipping the
/// refused source edge: the caller must retire or independently establish valid
/// continuity. A clone can stage validation until the owning runtime accepts the
/// same point; cloning this pure state creates no device authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspEnvelopeState {
    envelope: HspEnvelope,
    last: Option<HspPoint>,
}
impl HspEnvelopeState {
    pub fn new(envelope: HspEnvelope) -> Result<Self, HspEnvelopeError> {
        envelope.validate()?;
        Ok(Self {
            envelope,
            last: None,
        })
    }

    pub fn envelope(&self) -> HspEnvelope {
        self.envelope
    }

    pub fn last_point(&self) -> Option<HspPoint> {
        self.last
    }

    /// Validate an already quantized point. The first point is retained exactly
    /// as supplied; its continuity flag has no preceding edge to describe.
    pub fn accept(
        &mut self,
        point: HspPoint,
        continuous_from_previous: bool,
    ) -> Result<(), HspEnvelopeError> {
        if point.position_percent < self.envelope.minimum_position_percent
            || point.position_percent > self.envelope.maximum_position_percent
        {
            return Err(HspEnvelopeError::OutsidePositionRange);
        }
        if let Some(previous) = self.last {
            if !continuous_from_previous {
                return Err(HspEnvelopeError::Discontinuous);
            }
            let segment = point
                .time_ms
                .checked_sub(previous.time_ms)
                .filter(|delta| *delta != 0)
                .ok_or(HspEnvelopeError::NonIncreasingTime)?;
            if u64::from(segment) < self.envelope.minimum_segment_ms {
                return Err(HspEnvelopeError::SegmentTooShort);
            }
            if point.position_percent.abs_diff(previous.position_percent)
                > self.envelope.maximum_step_percent
            {
                return Err(HspEnvelopeError::StepTooLarge);
            }
        }
        self.last = Some(point);
        Ok(())
    }

    /// Quantize first, then check every discrete envelope constraint before
    /// changing the previous accepted point. Never repairs a rejected sample.
    pub fn quantize_and_accept(
        &mut self,
        relative_time: ProjectTime,
        position: NormalizedPosition,
        continuous_from_previous: bool,
    ) -> Result<HspQuantizedPoint, HspEnvelopeError> {
        let selected = quantize_hsp_point(relative_time, position)?;
        self.accept(selected.point, continuous_from_previous)?;
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(value: f64) -> NormalizedPosition {
        NormalizedPosition::new(value).unwrap()
    }
    fn point(time_ms: u32, position_percent: u8) -> HspPoint {
        HspPoint {
            time_ms,
            position_percent,
        }
    }
    fn envelope() -> HspEnvelope {
        HspEnvelope {
            minimum_position_percent: 20,
            maximum_position_percent: 80,
            maximum_step_percent: 10,
            minimum_segment_ms: 10,
        }
    }

    #[test]
    fn time_quantization_preserves_nearest_half_up_and_exact_error() {
        for (ns, ms, error) in [
            (0, 0, 0),
            (499_999, 0, 499_999),
            (500_000, 1, 500_000),
            (999_999, 1, 1),
            (1_000_000, 1, 0),
            (1_500_000, 2, 500_000),
            (4_400_000, 4, 400_000),
        ] {
            let value = quantize_hsp_point(ProjectTime::from_nanos(ns), position(0.25)).unwrap();
            assert_eq!(value.point, point(ms, 25));
            assert_eq!(value.time_error_ns, error);
            assert_eq!(value.position_error_ppm, 0);
        }
        for ns in [-1, -499_999, i64::MIN] {
            assert_eq!(
                quantize_hsp_point(ProjectTime::from_nanos(ns), position(0.5)),
                Err(HspEnvelopeError::NegativeTime)
            );
        }
    }

    #[test]
    fn time_quantization_refuses_rounded_u32_overflow_without_clipping() {
        let last_ns = i64::from(u32::MAX) * NS_PER_MS as i64;
        for excess in [0, 1, 499_999] {
            let value =
                quantize_hsp_point(ProjectTime::from_nanos(last_ns + excess), position(1.0))
                    .unwrap();
            assert_eq!(value.point, point(u32::MAX, 100));
            assert_eq!(value.time_error_ns, excess as u64);
        }
        for ns in [last_ns + 500_000, last_ns + 1_000_000, i64::MAX] {
            assert_eq!(
                quantize_hsp_point(ProjectTime::from_nanos(ns), position(1.0)),
                Err(HspEnvelopeError::TimeOverflow)
            );
        }
    }

    #[test]
    fn percent_quantization_uses_canonical_scale_and_half_up_boundaries() {
        for (p, selected, ppm) in [
            (0.0, 0, 0),
            // Multiplication rounds this binary value to the 1.5% tie;
            // its exact displacement from 2% is slightly above 5,000ppm.
            (0.015, 2, 5_001),
            (0.125, 13, 5_000),
            (0.25, 25, 0),
            (0.375, 38, 5_000),
            (0.5, 50, 0),
            (0.625, 63, 5_000),
            (0.75, 75, 0),
            (0.875, 88, 5_000),
            (1.0, 100, 0),
        ] {
            let value = quantize_hsp_point(ProjectTime::ZERO, position(p)).unwrap();
            assert_eq!(value.point.position_percent, selected);
            assert_eq!(value.position_error_ppm, ppm);
        }
        for (p, selected) in [(0.125f64.next_down(), 12), (0.125f64.next_up(), 13)] {
            let value = quantize_hsp_point(ProjectTime::ZERO, position(p)).unwrap();
            assert_eq!(value.point.position_percent, selected);
            assert!(value.position_error_ppm <= 5_001);
        }
    }

    #[test]
    fn conservative_percent_error_cannot_disappear_through_float_rounding() {
        for selected in 0u8..=100 {
            let p = f64::from(selected) / 100.0;
            let value = quantize_hsp_point(ProjectTime::ZERO, position(p)).unwrap();
            assert_eq!(value.point.position_percent, selected);
            // Only quarter-travel multiples are exactly representable binary
            // fractions among integer percent positions; the rest have a tiny
            // positive difference from the exact encoded integer percentage.
            assert_eq!(value.position_error_ppm, u32::from(selected % 25 != 0));
        }
        for p in [f64::from_bits(1), f64::MIN_POSITIVE, 0.25f64.next_down()] {
            let value = quantize_hsp_point(ProjectTime::ZERO, position(p)).unwrap();
            assert_eq!(value.position_error_ppm, 1);
        }
    }

    #[test]
    fn configuration_refuses_empty_or_unusable_limits() {
        let good = envelope();
        for bad in [
            HspEnvelope {
                maximum_position_percent: 20,
                ..good
            },
            HspEnvelope {
                minimum_position_percent: 81,
                ..good
            },
            HspEnvelope {
                maximum_position_percent: 101,
                ..good
            },
            HspEnvelope {
                maximum_step_percent: 0,
                ..good
            },
            HspEnvelope {
                maximum_step_percent: 101,
                ..good
            },
            HspEnvelope {
                minimum_segment_ms: 0,
                ..good
            },
            HspEnvelope {
                minimum_segment_ms: u64::from(u32::MAX) + 1,
                ..good
            },
            HspEnvelope {
                minimum_segment_ms: u64::MAX,
                ..good
            },
        ] {
            assert_eq!(
                HspEnvelopeState::new(bad),
                Err(HspEnvelopeError::InvalidEnvelope)
            );
        }
        let mut maximum = HspEnvelopeState::new(HspEnvelope {
            minimum_position_percent: 0,
            maximum_position_percent: 100,
            maximum_step_percent: 100,
            minimum_segment_ms: u64::from(u32::MAX),
        })
        .unwrap();
        maximum.accept(point(0, 0), false).unwrap();
        maximum.accept(point(u32::MAX, 100), true).unwrap();
    }

    #[test]
    fn first_point_is_supplied_without_origin_or_amplitude_remapping() {
        let mut state = HspEnvelopeState::new(envelope()).unwrap();
        assert_eq!(state.last_point(), None);
        let selected = state
            .quantize_and_accept(ProjectTime::from_nanos(123_000_000), position(0.3), false)
            .unwrap();
        assert_eq!(selected.point, point(123, 30));
        assert_eq!(state.last_point(), Some(point(123, 30)));
        assert_eq!(state.envelope(), envelope());
        let mut outside = HspEnvelopeState::new(envelope()).unwrap();
        assert_eq!(
            outside.quantize_and_accept(ProjectTime::ZERO, position(0.0), false),
            Err(HspEnvelopeError::OutsidePositionRange)
        );
        assert_eq!(outside.last_point(), None);
    }

    #[test]
    fn envelope_is_checked_after_rounding_and_does_not_claim_continuous_dynamics() {
        let mut state = HspEnvelopeState::new(envelope()).unwrap();
        // Original samples are only 9.1ms apart, but the selected integer points
        // are 10ms apart. This deliberately checks the discrete envelope only.
        let first = state
            .quantize_and_accept(ProjectTime::from_nanos(400_000), position(0.195), false)
            .unwrap();
        let second = state
            .quantize_and_accept(ProjectTime::from_nanos(9_500_000), position(0.3049), true)
            .unwrap();
        assert_eq!(first.point, point(0, 20));
        assert_eq!(second.point, point(10, 30));
        let before = state.clone();
        assert_eq!(
            state.quantize_and_accept(ProjectTime::from_nanos(20_000_000), position(0.4051), true),
            Err(HspEnvelopeError::StepTooLarge)
        );
        assert_eq!(state, before);
        for p in [0.194, 0.806] {
            assert_eq!(
                state.quantize_and_accept(ProjectTime::from_nanos(20_000_000), position(p), true),
                Err(HspEnvelopeError::OutsidePositionRange)
            );
            assert_eq!(state, before);
        }
    }

    #[test]
    fn colliding_or_regressing_quantized_times_leave_original_state_intact() {
        let mut state = HspEnvelopeState::new(HspEnvelope {
            minimum_segment_ms: 1,
            ..envelope()
        })
        .unwrap();
        state
            .quantize_and_accept(ProjectTime::from_nanos(1_100_000), position(0.3), false)
            .unwrap();
        let before = state.clone();
        for ns in [1_400_000, 1_100_000, 900_000, 400_000] {
            assert_eq!(
                state.quantize_and_accept(ProjectTime::from_nanos(ns), position(0.3), true),
                Err(HspEnvelopeError::NonIncreasingTime)
            );
            assert_eq!(state, before);
        }
        state
            .quantize_and_accept(ProjectTime::from_nanos(2_100_000), position(0.3), true)
            .unwrap();
        assert_eq!(state.last_point(), Some(point(2, 30)));
    }

    #[test]
    fn previous_point_crosses_batch_boundaries_and_failed_sample_is_atomic() {
        let mut state = HspEnvelopeState::new(envelope()).unwrap();
        let first_batch = [point(7, 20), point(17, 30)];
        for value in first_batch {
            state.accept(value, true).unwrap();
        }
        let before_second_batch = state.clone();
        for (value, continuous, error) in [
            (point(26, 40), true, HspEnvelopeError::SegmentTooShort),
            (point(27, 41), true, HspEnvelopeError::StepTooLarge),
            (point(27, 40), false, HspEnvelopeError::Discontinuous),
            (point(27, 255), true, HspEnvelopeError::OutsidePositionRange),
        ] {
            assert_eq!(state.accept(value, continuous), Err(error));
            assert_eq!(state, before_second_batch);
        }
        let second_batch = [point(27, 40), point(37, 50)];
        for value in second_batch {
            state.accept(value, true).unwrap();
        }
        assert_eq!(state.last_point(), Some(point(37, 50)));
    }

    #[test]
    fn quantizer_refusal_does_not_change_envelope_and_clones_stage_acceptance() {
        let mut state = HspEnvelopeState::new(envelope()).unwrap();
        state.accept(point(10, 30), false).unwrap();
        let original = state.clone();
        for (ns, error) in [
            (-1, HspEnvelopeError::NegativeTime),
            (i64::MAX, HspEnvelopeError::TimeOverflow),
        ] {
            assert_eq!(
                state.quantize_and_accept(ProjectTime::from_nanos(ns), position(0.4), true),
                Err(error)
            );
            assert_eq!(state, original);
        }
        let mut staged = state.clone();
        staged
            .quantize_and_accept(ProjectTime::from_nanos(20_000_000), position(0.4), true)
            .unwrap();
        assert_eq!(
            state, original,
            "validation in a candidate cannot commit another owner's point"
        );
        state = staged;
        assert_eq!(state.last_point(), Some(point(20, 40)));
    }
}
