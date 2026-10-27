//! Explicit authored interpolation; never inferred missing observations.
use crate::*;

pub struct GapResolution {
    pub program: MotionProgram,
    pub filled: Vec<TimeRange>,
}

/// Linear interior bridges and nearest-value holds for leading/trailing gaps.
/// Added interior knots use a millisecond grid when possible. Original knots are
/// unchanged; the ordinary edit reconciler determines all authored span ancestry.
pub fn resolve_selected_gaps(
    base: &MotionProgram,
    axis: Axis,
    selected: &[TimeRange],
    protected: &[ProtectedRegion],
) -> Result<GapResolution, CoreError> {
    EditValuesProgram::from_program_values(base)?;
    if selected.is_empty()
        || selected.len() > 4096
        || selected.windows(2).any(|p| p[0].end() > p[1].start())
    {
        return Err(CoreError::InvalidMotion(
            "select 1..4096 ordered, distinct complete gaps".into(),
        ));
    }
    let track = base
        .track(axis)
        .ok_or_else(|| CoreError::InvalidMotion("selected axis absent".into()))?;
    let original = track.actions();
    if original.is_empty() {
        return Err(CoreError::InvalidMotion(
            "gap filling needs at least one known position; no implicit zero".into(),
        ));
    }
    let mut actions = original.to_vec();
    let mut selected_indices = Vec::with_capacity(selected.len());
    for gap in selected {
        let index = track
            .gaps()
            .binary_search_by_key(&gap.start(), |g| g.start())
            .map_err(|_| {
                CoreError::InvalidMotion("selected interval is not an exact existing gap".into())
            })?;
        if track.gaps()[index] != *gap {
            return Err(CoreError::InvalidMotion(
                "selected interval is not an exact existing gap".into(),
            ));
        }
        selected_indices.push(index);
        let before = original.partition_point(|a| a.time() < gap.start());
        let left = before.checked_sub(1).and_then(|i| original.get(i));
        let right = original.get(original.partition_point(|a| a.time() < gap.end()));
        let affected = TimeRange::new(
            left.map_or(gap.start(), MotionAction::time),
            right.map_or(Ok(gap.end()), |a| a.time().checked_add(1))?,
        )?;
        if protected.iter().any(|p| {
            p.axis.is_none_or(|a| a == axis)
                && p.range.start() < affected.end()
                && affected.start() < p.range.end()
        }) {
            return Err(CoreError::InvalidMotion(
                "gap fill would change a protected interpolation stencil".into(),
            ));
        }
        let point = match (left, right) {
            (None, Some(r)) => Some((gap.start(), r.position())),
            (Some(l), None) => Some((gap.end().checked_add(-1)?, l.position())),
            (Some(l), Some(r)) => {
                let mid = gap.start().as_nanos() + gap.end().checked_sub(gap.start())? / 2;
                let grid = ProjectTime::from_nanos(mid / 1_000_000 * 1_000_000);
                // A tiny interior interval can be filled by the original endpoint
                // stencil without inserting a colliding export knot.
                if gap.contains(grid) && grid > l.time() && grid < r.time() {
                    let ratio =
                        grid.checked_sub(l.time())? as f64 / r.time().checked_sub(l.time())? as f64;
                    Some((
                        grid,
                        NormalizedPosition::new(
                            l.position().value()
                                + (r.position().value() - l.position().value()) * ratio,
                        )?,
                    ))
                } else {
                    None
                }
            }
            (None, None) => return Err(CoreError::InvalidMotion("no neighboring position".into())),
        };
        if let Some((time, position)) = point {
            actions.push(MotionAction::new(
                time,
                position,
                EvidenceKind::Synthesized,
            )?);
        }
    }
    actions.sort_by_key(MotionAction::time);
    let mut cursor = 0;
    let gaps = track
        .gaps()
        .iter()
        .enumerate()
        .filter_map(|(i, g)| {
            if selected_indices.get(cursor) == Some(&i) {
                cursor += 1;
                None
            } else {
                Some(*g)
            }
        })
        .collect();
    let replacement = MotionTrack::with_name_and_gaps(track.name(), axis, actions, gaps)?;
    let tracks = base
        .tracks()
        .iter()
        .map(|t| {
            if t.axis() == axis {
                replacement.clone()
            } else {
                t.clone()
            }
        })
        .collect();
    Ok(GapResolution {
        program: MotionProgram::new(tracks)?,
        filled: selected.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(ms: i64) -> ProjectTime {
        ProjectTime::from_nanos(ms * 1_000_000)
    }
    fn range(a: i64, b: i64) -> TimeRange {
        TimeRange::new(t(a), t(b)).unwrap()
    }
    fn fixture() -> MotionProgram {
        let actions = [(100, 0.2), (300, 0.8), (500, 0.5)]
            .into_iter()
            .map(|(ms, p)| {
                MotionAction::new(
                    t(ms),
                    NormalizedPosition::new(p).unwrap(),
                    EvidenceKind::Inferred,
                )
                .unwrap()
            })
            .collect();
        MotionProgram::new(vec![
            MotionTrack::with_gaps(
                Axis::Stroke,
                actions,
                vec![range(0, 100), range(150, 250), range(501, 600)],
            )
            .unwrap(),
            MotionTrack::new(
                Axis::Yaw,
                vec![MotionAction::new(
                    t(0),
                    NormalizedPosition::new(0.7).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap()],
            )
            .unwrap(),
        ])
        .unwrap()
    }
    #[test]
    fn explicit_fill_preserves_original_knots_other_axes_and_marks_added_values_authored() {
        let base = fixture();
        let gaps = base.track(Axis::Stroke).unwrap().gaps();
        let output = resolve_selected_gaps(&base, Axis::Stroke, gaps, &[]).unwrap();
        let track = output.program.track(Axis::Stroke).unwrap();
        assert!(track.gaps().is_empty());
        assert_eq!(track.actions().len(), 6);
        assert_eq!(output.program.track(Axis::Yaw), base.track(Axis::Yaw));
        for a in base.track(Axis::Stroke).unwrap().actions() {
            assert!(track.actions().contains(a));
        }
        assert_eq!(
            track
                .actions()
                .iter()
                .filter(|a| a.evidence() == EvidenceKind::Synthesized)
                .count(),
            3
        );
        assert!(
            (track
                .actions()
                .iter()
                .find(|a| a.time() == t(200))
                .unwrap()
                .position()
                .value()
                - 0.5)
                .abs()
                < 1e-12
        );
        let reconciled = reconcile_edit_values(
            &base,
            &EditValuesProgram::from_program_values(&output.program).unwrap(),
            &ProvenanceRef::new("gap-fill-fixture").unwrap(),
        )
        .unwrap();
        assert!(!reconciled.authored_ranges.is_empty());
        assert_eq!(reconciled.program, output.program);
    }
    #[test]
    fn selection_and_protection_are_explicit_and_no_axis_is_invented() {
        let base = fixture();
        let gap = range(150, 250);
        let output = resolve_selected_gaps(&base, Axis::Stroke, &[gap], &[]).unwrap();
        assert_eq!(
            output.program.track(Axis::Stroke).unwrap().gaps(),
            &[range(0, 100), range(501, 600)]
        );
        assert!(resolve_selected_gaps(&base, Axis::Stroke, &[], &[]).is_err());
        assert!(resolve_selected_gaps(&base, Axis::Stroke, &[gap, gap], &[]).is_err());
        assert!(resolve_selected_gaps(&base, Axis::Stroke, &[range(151, 250)], &[]).is_err());
        assert!(resolve_selected_gaps(&base, Axis::Surge, &[gap], &[]).is_err());
        let protected = ProtectedRegion {
            axis: Some(Axis::Stroke),
            range: range(100, 101),
        };
        assert!(resolve_selected_gaps(&base, Axis::Stroke, &[gap], &[protected]).is_err());
        let empty = MotionProgram::new(vec![MotionTrack::with_gaps(
            Axis::Stroke,
            vec![],
            vec![gap],
        )
        .unwrap()])
        .unwrap();
        assert!(resolve_selected_gaps(&empty, Axis::Stroke, &[gap], &[]).is_err());
    }
}
