//! Explicit existing-motion clock changes. Positions/evidence never change.
use crate::*;
use std::{cmp::Ordering, collections::BTreeSet};

fn source_boundary(
    spec: &SourcePlacementSpec,
    time: ProjectTime,
) -> Result<ExactSeconds, CoreError> {
    let source = ExactSeconds::from_source(spec.source_anchor).checked_add(
        ExactSeconds::from_project(time)
            .checked_sub(ExactSeconds::from_project(spec.project_anchor))?
            .checked_ratio(i64::from(spec.rate_numerator), spec.rate_denominator)?,
    )?;
    spec.project_at_source_boundary(source)?;
    Ok(source)
}
fn mapped_boundary(
    from: &SourcePlacementSpec,
    to: &SourcePlacementSpec,
    time: ProjectTime,
) -> Result<ExactSeconds, CoreError> {
    to.project_at_source_boundary(source_boundary(from, time)?)
}
/// Map selected existing tracks from one declared source placement to another.
/// Every action and gap must fit both trims. This never clips, invents anchors,
/// shifts other tracks or resolves unknown motion. Action quantization floors
/// to nanoseconds; merged/reordered-equal timestamps and negative output refuse.
/// Gap bounds round outward, including the reversed formerly closed endpoint.
/// If conservative gap rounding overlaps an action, the whole edit refuses.
pub fn retime_motion_between_placements(
    program: &MotionProgram,
    from: &SourcePlacementSpec,
    to: &SourcePlacementSpec,
    axes: &[Axis],
) -> Result<MotionProgram, CoreError> {
    from.validate()?;
    to.validate()?;
    if from.source_version != to.source_version
        || axes.is_empty()
        || axes.len() > 6
        || axes.iter().collect::<BTreeSet<_>>().len() != axes.len()
    {
        return Err(CoreError::InvalidMotion(
            "retiming requires one source and unique explicit axes".into(),
        ));
    }
    let mut output = program.clone();
    for &axis in axes {
        let track = program
            .track(axis)
            .ok_or_else(|| CoreError::InvalidMotion("selected retiming axis is absent".into()))?;
        let mut actions = track
            .actions()
            .iter()
            .map(|a| {
                let source = from.source_at_project(a.time())?;
                let mapped = to.project_at_source(source)?.quantize_nanoseconds()?;
                MotionAction::new(mapped.time, a.position(), a.evidence())
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        actions.sort_by_key(|a| a.time());
        let reverse = (from.rate_numerator < 0) != (to.rate_numerator < 0);
        let mut gaps = Vec::with_capacity(track.gaps().len());
        for gap in track.gaps() {
            let a = mapped_boundary(from, to, gap.start())?;
            let b = mapped_boundary(from, to, gap.end())?;
            let (start, end) = if a.checked_cmp(b)? == Ordering::Greater {
                (b, a)
            } else {
                (a, b)
            };
            let start = start.quantize_nanoseconds()?;
            let end = end.quantize_nanoseconds()?;
            let needs_extra = end.remainder_numerator != "0" || reverse;
            let end = ProjectTime::from_nanos(
                end.time
                    .as_nanos()
                    .checked_add(i64::from(needs_extra))
                    .ok_or(CoreError::Overflow)?,
            );
            gaps.push(TimeRange::new(start.time, end)?);
        }
        gaps.sort_by_key(|g| g.start());
        output = output.replaced_track(MotionTrack::with_name_and_gaps(
            track.name(),
            axis,
            actions,
            gaps,
        )?)?;
    }
    Ok(output)
}
