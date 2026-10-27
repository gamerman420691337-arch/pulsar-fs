//! Fixed values-only transformations. No observation, authority or protection override.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueEditOperation {
    Invert,
    HalveAmplitude,
}
/// Preserve gaps, other axes and every boundary-neighbor knot of protected spans.
/// The caller still reconciles provenance and proves protected segment equality.
pub fn recipe_edit_values(
    program: &MotionProgram,
    axis: Axis,
    operation: ValueEditOperation,
    protected: &[ProtectedRegion],
) -> Result<EditValuesProgram, CoreError> {
    let mut tracks = Vec::new();
    for track in program.tracks() {
        let mut blocked = vec![false; track.actions().len()];
        if track.axis() == axis {
            let mut deltas = vec![0_i64; blocked.len().checked_add(1).ok_or(CoreError::Overflow)?];
            for region in protected
                .iter()
                .filter(|r| r.axis.is_none_or(|a| a == axis))
            {
                let actions = track.actions();
                let left = actions.partition_point(|a| a.time() < region.range.start());
                let first = if actions
                    .get(left)
                    .is_some_and(|a| a.time() == region.range.start())
                {
                    left
                } else {
                    left.saturating_sub(1)
                };
                let right = actions.partition_point(|a| a.time() < region.range.end());
                let end = (right + usize::from(right < actions.len())).min(actions.len());
                if first < end {
                    deltas[first] = deltas[first].checked_add(1).ok_or(CoreError::Overflow)?;
                    deltas[end] = deltas[end].checked_sub(1).ok_or(CoreError::Overflow)?;
                }
            }
            let mut count = 0_i64;
            for (i, value) in blocked.iter_mut().enumerate() {
                count = count.checked_add(deltas[i]).ok_or(CoreError::Overflow)?;
                *value = count > 0;
            }
        }
        let actions = track
            .actions()
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let x = a.position().value();
                let position = if track.axis() != axis || blocked[i] {
                    x
                } else {
                    match operation {
                        ValueEditOperation::Invert => 1.0 - x,
                        ValueEditOperation::HalveAmplitude => 0.5 + (x - 0.5) * 0.5,
                    }
                };
                EditValueAction::new(a.time(), NormalizedPosition::new(position)?)
            })
            .collect::<Result<Vec<_>, _>>()?;
        tracks.push(EditValueTrack::new(
            track.axis(),
            actions,
            track.gaps().to_vec(),
        )?);
    }
    EditValuesProgram::new(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipes_preserve_protected_stencils_other_axes_gaps_and_reconcile_evidence() {
        let actions = [0.1, 0.2, 0.7, 0.8, 0.9]
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                MotionAction::new(
                    ProjectTime::from_nanos(i as i64 * 10),
                    NormalizedPosition::new(x).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap()
            })
            .collect();
        let stroke = MotionTrack::with_gaps(
            Axis::Stroke,
            actions,
            vec![TimeRange::new(ProjectTime::from_nanos(32), ProjectTime::from_nanos(35)).unwrap()],
        )
        .unwrap();
        let other = MotionTrack::new(
            Axis::Roll,
            vec![MotionAction::new(
                ProjectTime::ZERO,
                NormalizedPosition::new(0.2).unwrap(),
                EvidenceKind::Observed,
            )
            .unwrap()],
        )
        .unwrap();
        let program = MotionProgram::new(vec![stroke, other]).unwrap();
        let protected = vec![ProtectedRegion {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(ProjectTime::from_nanos(15), ProjectTime::from_nanos(25))
                .unwrap(),
        }];
        for operation in [
            ValueEditOperation::Invert,
            ValueEditOperation::HalveAmplitude,
        ] {
            let values = recipe_edit_values(&program, Axis::Stroke, operation, &protected).unwrap();
            let edited = reconcile_edit_values(
                &program,
                &values,
                &ProvenanceRef::new("edit:fixture").unwrap(),
            )
            .unwrap();
            ensure_protected_segments_unchanged(&program, &edited.program, &protected).unwrap();
            assert_eq!(edited.program.track(Axis::Roll), program.track(Axis::Roll));
            assert_eq!(
                edited.program.track(Axis::Stroke).unwrap().gaps(),
                program.track(Axis::Stroke).unwrap().gaps()
            );
            let old = program.track(Axis::Stroke).unwrap().actions();
            let new = edited.program.track(Axis::Stroke).unwrap().actions();
            assert_eq!(&new[1..4], &old[1..4]);
            assert_ne!(new[0].position(), old[0].position());
            assert_eq!(new[0].evidence(), EvidenceKind::Synthesized);
        }
    }
}
