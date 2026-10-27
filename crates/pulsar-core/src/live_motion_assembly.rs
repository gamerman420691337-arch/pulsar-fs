//! Deterministic assembly only; the Engine supplies original accepted samples.
use crate::*;

pub struct LiveMotionPoint {
    pub time: ProjectTime,
    pub position: NormalizedPosition,
    /// Checked original frame-pair continuity, never inferred from proximity.
    pub continuous_from_previous: bool,
}

/// The explicit interpolation limit is policy, not measured model accuracy.
/// Missing edges and all coverage outside retained points remain unavailable.
pub fn assemble_live_motion(
    points: &[LiveMotionPoint],
    range: TimeRange,
    maximum_interpolation_ns: u64,
) -> Result<MotionProgram, CoreError> {
    if points.is_empty()
        || points.len() > 4096
        || range.start() < ProjectTime::ZERO
        || !(1..=1_000_000_000).contains(&maximum_interpolation_ns)
    {
        return Err(CoreError::InvalidMotion(
            "invalid live assembly bounds".into(),
        ));
    }
    if points.windows(2).any(|p| p[0].time >= p[1].time)
        || points.iter().any(|p| !range.contains(p.time))
    {
        return Err(CoreError::InvalidMotion(
            "live points must be ordered, distinct and inside the declared range".into(),
        ));
    }
    let mut gaps = Vec::new();
    let first = points[0].time;
    if range.start() < first {
        gaps.push(TimeRange::new(range.start(), first)?);
    }
    for pair in points.windows(2) {
        let delta = u64::try_from(pair[1].time.checked_sub(pair[0].time)?)
            .map_err(|_| CoreError::Overflow)?;
        if !pair[1].continuous_from_previous || delta > maximum_interpolation_ns {
            let start = pair[0].time.checked_add(1)?;
            if start < pair[1].time {
                gaps.push(TimeRange::new(start, pair[1].time)?);
            }
        }
    }
    let tail = points.last().expect("nonempty").time.checked_add(1)?;
    if tail < range.end() {
        gaps.push(TimeRange::new(tail, range.end())?);
    }
    let actions = points
        .iter()
        .map(|p| MotionAction::new(p.time, p.position, EvidenceKind::Inferred))
        .collect::<Result<Vec<_>, _>>()?;
    MotionProgram::new(vec![MotionTrack::with_gaps(Axis::Stroke, actions, gaps)?])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn point(time: i64, continuous: bool) -> LiveMotionPoint {
        LiveMotionPoint {
            time: ProjectTime::from_nanos(time),
            position: NormalizedPosition::new(0.4).unwrap(),
            continuous_from_previous: continuous,
        }
    }
    #[test]
    fn missing_edges_handoffs_and_tail_are_unavailable() {
        let range = TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(100)).unwrap();
        let p = assemble_live_motion(
            &[
                point(10, false),
                point(20, true),
                point(30, false),
                point(60, true),
            ],
            range,
            15,
        )
        .unwrap();
        let t = &p.tracks()[0];
        assert_eq!(
            t.gaps()
                .iter()
                .map(|g| (g.start().as_nanos(), g.end().as_nanos()))
                .collect::<Vec<_>>(),
            vec![(0, 10), (21, 30), (31, 60), (61, 100)]
        );
        assert!(t
            .actions()
            .iter()
            .all(|a| a.evidence() == EvidenceKind::Inferred));
    }
    #[test]
    fn no_sorting_clipping_or_implicit_policy_repairs() {
        let range = TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(100)).unwrap();
        for points in [
            vec![],
            vec![point(20, false), point(10, true)],
            vec![point(10, false), point(10, true)],
            vec![point(100, false)],
        ] {
            assert!(assemble_live_motion(&points, range, 10).is_err());
        }
        assert!(assemble_live_motion(&[point(10, false)], range, 0).is_err());
        assert!(assemble_live_motion(&[point(10, false)], range, 1_000_000_001).is_err());
    }
}
