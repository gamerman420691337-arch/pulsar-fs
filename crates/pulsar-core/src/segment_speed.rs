//! Average normalized segment slopes, not physical or peak velocity admission.
use crate::{Axis, CoreError, MotionProgram, TimeRange};
use serde::{Deserialize, Serialize};

pub fn validate_segment_speed_limit(value: f64) -> Result<(), CoreError> {
    if !value.is_finite() {
        return Err(CoreError::NonFinite);
    }
    if value <= 0.0 || value > 100_000_000_000.0 {
        return Err(CoreError::OutOfRange);
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentSpeedViolation {
    pub axis: Axis,
    pub range: TimeRange,
    pub average_percent_per_second: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentSpeedScan {
    pub maximum_percent_per_second: f64,
    /// Counts cover the visited prefix if the violation page was truncated.
    pub scanned_segments: u64,
    pub unavailable_segments: u64,
    pub truncated: bool,
    pub violations: Vec<SegmentSpeedViolation>,
}
impl SegmentSpeedScan {
    pub fn validate(&self) -> Result<(), CoreError> {
        validate_segment_speed_limit(self.maximum_percent_per_second)?;
        if self.violations.len() > 1024
            || self.unavailable_segments > self.scanned_segments
            || self.violations.len() as u64 > self.scanned_segments - self.unavailable_segments
            || self.violations.iter().any(|v| {
                !v.average_percent_per_second.is_finite()
                    || v.average_percent_per_second <= self.maximum_percent_per_second
                    || v.average_percent_per_second > 100_000_000_000.0
            })
        {
            return Err(CoreError::InvalidMotion(
                "invalid bounded segment-speed scan".into(),
            ));
        }
        Ok(())
    }
}

/// Return at most `limit` violating supported edges. No interpolation is added
/// across gaps; a page overflow stops the scan and explicitly reports a prefix.
pub fn scan_segment_speeds(
    program: &MotionProgram,
    maximum: f64,
    limit: usize,
) -> Result<SegmentSpeedScan, CoreError> {
    validate_segment_speed_limit(maximum)?;
    if limit > 1024 {
        return Err(CoreError::OutOfRange);
    }
    let mut result = SegmentSpeedScan {
        maximum_percent_per_second: maximum,
        scanned_segments: 0,
        unavailable_segments: 0,
        truncated: false,
        violations: Vec::with_capacity(limit),
    };
    'tracks: for track in program.tracks() {
        let mut gap = 0;
        for edge in track.actions().windows(2) {
            result.scanned_segments += 1;
            while gap < track.gaps().len() && track.gaps()[gap].end() <= edge[0].time() {
                gap += 1;
            }
            if gap < track.gaps().len() && track.gaps()[gap].start() < edge[1].time() {
                result.unavailable_segments += 1;
                continue;
            }
            let elapsed = edge[1]
                .time()
                .as_nanos()
                .checked_sub(edge[0].time().as_nanos())
                .ok_or(CoreError::Overflow)?;
            if elapsed <= 0 {
                return Err(CoreError::InvalidMotion("unordered speed segment".into()));
            }
            let speed = (edge[1].position().value() - edge[0].position().value()).abs()
                * 100_000_000_000.0
                / elapsed as f64;
            if speed > maximum {
                if result.violations.len() == limit {
                    result.truncated = true;
                    break 'tracks;
                }
                result.violations.push(SegmentSpeedViolation {
                    axis: track.axis(),
                    range: TimeRange::new(edge[0].time(), edge[1].time())?,
                    average_percent_per_second: speed,
                });
            }
        }
    }
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvidenceKind, MotionAction, MotionTrack, NormalizedPosition, ProjectTime};
    fn program(times: &[i64], positions: &[f64], gaps: Vec<TimeRange>) -> MotionProgram {
        MotionProgram::new(vec![MotionTrack::with_gaps(
            Axis::Stroke,
            times
                .iter()
                .zip(positions)
                .map(|(t, p)| {
                    MotionAction::new(
                        ProjectTime::from_nanos(*t),
                        NormalizedPosition::new(*p).unwrap(),
                        EvidenceKind::Observed,
                    )
                    .unwrap()
                })
                .collect(),
            gaps,
        )
        .unwrap()])
        .unwrap()
    }
    #[test]
    fn exact_limit_holds_nanosecond_edges_and_extreme_times_are_bounded() {
        let p = program(&[0, 1_000_000_000], &[0.0, 1.0], vec![]);
        assert!(scan_segment_speeds(&p, 100.0, 10)
            .unwrap()
            .violations
            .is_empty());
        assert_eq!(
            scan_segment_speeds(&p, 99.0, 10).unwrap().violations[0].average_percent_per_second,
            100.0
        );
        let p = program(&[0, 1, i64::MAX], &[0.0, 1.0, 0.0], vec![]);
        let s = scan_segment_speeds(&p, 450.0, 10).unwrap();
        assert_eq!(s.violations.len(), 1);
        assert_eq!(
            s.violations[0].average_percent_per_second,
            100_000_000_000.0
        );
        for v in [0.0, -1.0, f64::NAN, f64::INFINITY, 100_000_000_001.0] {
            assert!(scan_segment_speeds(&p, v, 10).is_err());
        }
    }
    #[test]
    fn gaps_do_not_become_motion_and_page_overflow_is_explicit() {
        let gap = TimeRange::new(
            ProjectTime::from_nanos(1),
            ProjectTime::from_nanos(1_000_000_000),
        )
        .unwrap();
        let p = program(
            &[0, 1_000_000_000, 1_000_000_001, 1_000_000_002],
            &[0.0, 1.0, 0.0, 1.0],
            vec![gap],
        );
        let s = scan_segment_speeds(&p, 450.0, 1).unwrap();
        assert_eq!(s.unavailable_segments, 1);
        assert_eq!(s.violations.len(), 1);
        assert!(s.truncated);
        assert!(scan_segment_speeds(&p, 450.0, 0).unwrap().truncated);
        assert!(
            !scan_segment_speeds(&program(&[0, 1], &[0.2, 0.2], vec![]), 450.0, 0)
                .unwrap()
                .truncated
        );
    }
}
