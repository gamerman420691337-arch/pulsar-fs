//! Supplied reference-curve sampling at declared source/script times.
//!
//! This preserves raw integer-percent reference knots. It neither infers
//! alignment nor establishes target identity, permissions or qualification.
use crate::{CoreError, ExactSeconds};
use std::cmp::Ordering;

pub const REFERENCE_CURVE_MAX_POINTS: usize = 250_000;
pub const REFERENCE_CURVE_MAX_TIME_MS: u32 = 86_400_000;
pub const REFERENCE_CURVE_MAX_OFFSET_MS: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferencePoint {
    pub at_ms: u32,
    pub pos: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceCurveError {
    #[error("reference requires 2..=250000 points; received {actual}")]
    PointCount { actual: usize },
    #[error("reference point {index} exceeds the 86400000 ms time bound: {at_ms}")]
    TimeOutOfRange { index: usize, at_ms: u32 },
    #[error("reference point {index} exceeds the 0..=100 position range: {pos}")]
    PositionOutOfRange { index: usize, pos: u8 },
    #[error("reference point {index} does not strictly follow its predecessor")]
    NonIncreasingTime { index: usize },
    #[error("reference offset exceeds the signed 86400000 ms bound: {offset_ms}")]
    OffsetOutOfRange { offset_ms: i64 },
    #[error("reference curve arithmetic failed: {0}")]
    Arithmetic(#[from] CoreError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceCurve {
    points: Vec<ReferencePoint>,
}

/// Exact normalized position in [0, 1] at the retained declared script time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceSample {
    pub script_time: ExactSeconds,
    pub position_numerator: i128,
    pub position_denominator: u64,
}

impl ReferenceSample {
    /// Presentation conversion only; retain the rational fields for calculations.
    pub fn display_position(&self) -> f64 {
        self.position_numerator as f64 / self.position_denominator as f64
    }
}

impl ReferenceCurve {
    pub fn new(points: Vec<ReferencePoint>) -> Result<Self, ReferenceCurveError> {
        if !(2..=REFERENCE_CURVE_MAX_POINTS).contains(&points.len()) {
            return Err(ReferenceCurveError::PointCount {
                actual: points.len(),
            });
        }
        for (index, point) in points.iter().enumerate() {
            if point.at_ms > REFERENCE_CURVE_MAX_TIME_MS {
                return Err(ReferenceCurveError::TimeOutOfRange {
                    index,
                    at_ms: point.at_ms,
                });
            }
            if point.pos > 100 {
                return Err(ReferenceCurveError::PositionOutOfRange {
                    index,
                    pos: point.pos,
                });
            }
            if index > 0 && point.at_ms <= points[index - 1].at_ms {
                return Err(ReferenceCurveError::NonIncreasingTime { index });
            }
        }
        Ok(Self { points })
    }

    pub fn points(&self) -> &[ReferencePoint] {
        &self.points
    }

    /// Script time equals source elapsed time plus the declared signed offset.
    /// Both support endpoints are included. Unsupported times return `None`;
    /// no endpoint holds, extrapolation, inversion or retiming are applied.
    pub fn sample_at(
        &self,
        source_elapsed: ExactSeconds,
        offset_ms: i64,
    ) -> Result<Option<ReferenceSample>, ReferenceCurveError> {
        if !(-REFERENCE_CURVE_MAX_OFFSET_MS..=REFERENCE_CURVE_MAX_OFFSET_MS).contains(&offset_ms) {
            return Err(ReferenceCurveError::OffsetOutOfRange { offset_ms });
        }
        let script_time =
            source_elapsed.checked_add(ExactSeconds::new(i128::from(offset_ms), 1000)?)?;
        let first = point_time(self.points[0])?;
        let last = point_time(self.points[self.points.len() - 1])?;
        if script_time.checked_cmp(first)? == Ordering::Less
            || script_time.checked_cmp(last)? == Ordering::Greater
        {
            return Ok(None);
        }

        // Find the first strictly later knot. The checked support test above
        // guarantees that its predecessor exists, including at the first knot.
        let mut low = 0;
        let mut high = self.points.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if script_time.checked_cmp(point_time(self.points[middle])?)? == Ordering::Less {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        let left = self.points[low - 1];
        let left_time = point_time(left)?;
        let base = ExactSeconds::new(i128::from(left.pos), 100)?;
        // Reuse checked rational arithmetic for the unitless position. An exact
        // knot or plateau needs no interpolation and preserves its exact value.
        let position = if low == self.points.len()
            || script_time.checked_cmp(left_time)? == Ordering::Equal
            || left.pos == self.points[low].pos
        {
            base
        } else {
            let right = self.points[low];
            let phase = script_time
                .checked_sub(left_time)?
                .checked_ratio(1000, right.at_ms - left.at_ms)?;
            base.checked_add(phase.checked_ratio(i64::from(right.pos) - i64::from(left.pos), 100)?)?
        };
        Ok(Some(ReferenceSample {
            script_time,
            position_numerator: position.numerator(),
            position_denominator: position.denominator(),
        }))
    }
}

fn point_time(point: ReferencePoint) -> Result<ExactSeconds, CoreError> {
    ExactSeconds::new(i128::from(point.at_ms), 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(values: &[(u32, u8)]) -> Vec<ReferencePoint> {
        values
            .iter()
            .map(|&(at_ms, pos)| ReferencePoint { at_ms, pos })
            .collect()
    }

    fn curve(values: &[(u32, u8)]) -> ReferenceCurve {
        ReferenceCurve::new(points(values)).unwrap()
    }

    fn seconds(numerator: i128, denominator: u64) -> ExactSeconds {
        ExactSeconds::new(numerator, denominator).unwrap()
    }

    fn sample(curve: &ReferenceCurve, at: ExactSeconds, offset: i64) -> ReferenceSample {
        curve.sample_at(at, offset).unwrap().unwrap()
    }

    fn assert_position(sample: ReferenceSample, numerator: i128, denominator: u64) {
        assert_eq!(sample.position_numerator, numerator);
        assert_eq!(sample.position_denominator, denominator);
    }

    #[test]
    fn fractional_source_pts_preserve_exact_time_and_linear_position() {
        let reference = curve(&[(0, 20), (100, 80)]);
        let result = sample(&reference, seconds(1001, 30_000), 0);
        assert_eq!(result.script_time, seconds(1001, 30_000));
        assert_position(result, 2001, 5000);
        assert!((result.display_position() - 0.4002).abs() < f64::EPSILON);
        assert_eq!(reference.points(), points(&[(0, 20), (100, 80)]));
    }

    #[test]
    fn signed_offsets_add_to_source_elapsed_time_without_rebasing() {
        let reference = curve(&[(0, 0), (1000, 100)]);
        for (at, offset) in [(seconds(1, 4), 250), (seconds(3, 4), -250)] {
            let result = sample(&reference, at, offset);
            assert_eq!(result.script_time, seconds(1, 2));
            assert_position(result, 1, 2);
        }
        let result = sample(&reference, seconds(-1, 4), 500);
        assert_eq!(result.script_time, seconds(1, 4));
        assert_position(result, 1, 4);
    }

    #[test]
    fn support_endpoints_are_inclusive_without_extrapolation_or_edge_holds() {
        let reference = curve(&[(100, 20), (200, 80)]);
        assert_position(sample(&reference, seconds(1, 10), 0), 1, 5);
        assert_position(sample(&reference, seconds(1, 5), 0), 4, 5);
        assert_eq!(reference.sample_at(seconds(99_999, 1_000_000), 0), Ok(None));
        assert_eq!(
            reference.sample_at(seconds(200_001, 1_000_000), 0),
            Ok(None)
        );
        assert_eq!(reference.sample_at(seconds(-1, 1), 0), Ok(None));
        assert_eq!(reference.sample_at(seconds(1, 10), -1), Ok(None));
        assert_eq!(reference.sample_at(seconds(1, 5), 1), Ok(None));
    }

    #[test]
    fn plateaus_and_reversals_preserve_supplied_travel() {
        let reference = curve(&[(0, 40), (1000, 40), (2000, 80), (3000, 20)]);
        for (at, numerator, denominator) in [
            (seconds(1, 2), 2, 5),
            (seconds(1, 1), 2, 5),
            (seconds(3, 2), 3, 5),
            (seconds(2, 1), 4, 5),
            (seconds(5, 2), 1, 2),
            (seconds(3, 1), 1, 5),
        ] {
            assert_position(sample(&reference, at, 0), numerator, denominator);
        }
    }

    #[test]
    fn count_bounds_include_the_maximum_and_refuse_empty_single_or_oversized_curves() {
        for count in [0, 1, REFERENCE_CURVE_MAX_POINTS + 1] {
            let input = (0..count)
                .map(|index| ReferencePoint {
                    at_ms: u32::try_from(index).unwrap(),
                    pos: 50,
                })
                .collect();
            assert_eq!(
                ReferenceCurve::new(input),
                Err(ReferenceCurveError::PointCount { actual: count })
            );
        }
        let input: Vec<_> = (0..REFERENCE_CURVE_MAX_POINTS)
            .map(|index| ReferencePoint {
                at_ms: u32::try_from(index).unwrap(),
                pos: 50,
            })
            .collect();
        let last = input.last().copied().unwrap();
        let reference = ReferenceCurve::new(input).unwrap();
        assert_position(sample(&reference, point_time(last).unwrap(), 0), 1, 2);
    }

    #[test]
    fn invalid_order_time_and_position_refuse_without_repair() {
        for input in [&[(0, 10), (0, 20)][..], &[(2, 10), (1, 20)][..]] {
            assert_eq!(
                ReferenceCurve::new(points(input)),
                Err(ReferenceCurveError::NonIncreasingTime { index: 1 })
            );
        }
        assert_eq!(
            ReferenceCurve::new(points(&[(0, 0), (REFERENCE_CURVE_MAX_TIME_MS + 1, 100)])),
            Err(ReferenceCurveError::TimeOutOfRange {
                index: 1,
                at_ms: REFERENCE_CURVE_MAX_TIME_MS + 1,
            })
        );
        for pos in [101, u8::MAX] {
            assert_eq!(
                ReferenceCurve::new(points(&[(0, pos), (1, 100)])),
                Err(ReferenceCurveError::PositionOutOfRange { index: 0, pos })
            );
        }
    }

    #[test]
    fn signed_offset_and_timestamp_bounds_are_checked_before_arithmetic() {
        let reference = curve(&[(0, 0), (REFERENCE_CURVE_MAX_TIME_MS, 100)]);
        for (at, offset) in [
            (seconds(-86_400, 1), REFERENCE_CURVE_MAX_OFFSET_MS),
            (seconds(86_400, 1), -REFERENCE_CURVE_MAX_OFFSET_MS),
        ] {
            assert_position(sample(&reference, at, offset), 0, 1);
        }
        assert_position(sample(&reference, seconds(86_400, 1), 0), 1, 1);
        for offset_ms in [
            i64::MIN,
            -REFERENCE_CURVE_MAX_OFFSET_MS - 1,
            REFERENCE_CURVE_MAX_OFFSET_MS + 1,
            i64::MAX,
        ] {
            assert_eq!(
                reference.sample_at(seconds(i128::MAX, 1), offset_ms),
                Err(ReferenceCurveError::OffsetOutOfRange { offset_ms })
            );
        }
    }

    #[test]
    fn arithmetic_overflow_is_an_error_not_missing_support_or_a_clamped_position() {
        let reference = curve(&[(0, 0), (7, 100)]);
        assert_eq!(
            reference.sample_at(seconds(i128::MAX, 1), 1),
            Err(ReferenceCurveError::Arithmetic(CoreError::Overflow))
        );
        assert_eq!(
            reference.sample_at(seconds(1, u64::MAX), 0),
            Err(ReferenceCurveError::Arithmetic(CoreError::Overflow))
        );
        // A plateau does not need the unrepresentable interpolation phase.
        let plateau = curve(&[(0, 40), (7, 40)]);
        assert_position(sample(&plateau, seconds(1, u64::MAX), 0), 2, 5);
    }
}
