//! Explicit artifact-only project-to-source clock mapping. Never edits a master.
use crate::{
    CoreError, ExactSeconds, ExactTimeQuantization, MotionAction, MotionTrack,
    PlacedGenerationPlan, ProjectTime, SourcePlacementSpec, SourceVersionId,
};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeMap};

/// Incremental clock carrier, not an authority or receipt verifier. The engine
/// must supply verified, chronological retiming facts and their actual before
/// track. Edited positions retain their own evidence labels.
#[derive(Clone)]
pub struct SourceClockMapper {
    spec: SourcePlacementSpec,
    anchors: BTreeMap<ProjectTime, (ExactSeconds, Option<u64>)>,
    max_entries: usize,
}
impl SourceClockMapper {
    pub fn new(
        spec: &SourcePlacementSpec,
        generated: Option<&PlacedGenerationPlan>,
        max_entries: usize,
    ) -> Result<Self, CoreError> {
        spec.validate()?;
        if max_entries == 0 {
            return Err(invalid("clock carrier requires bounded admission"));
        }
        let anchors = sample_anchors(spec, generated, max_entries)?
            .into_iter()
            .map(|(time, (source, frame))| (time, (source, Some(frame))))
            .collect();
        Ok(Self {
            spec: spec.clone(),
            anchors,
            max_entries,
        })
    }
    pub fn spec(&self) -> &SourcePlacementSpec {
        &self.spec
    }
    pub(crate) fn anchor_count(&self) -> usize {
        self.anchors.len()
    }
    pub(crate) fn map_action(
        &self,
        action: &MotionAction,
    ) -> Result<(MotionAction, SourceClockActionMapping), CoreError> {
        let (source_time, frame) = match self.anchors.get(&action.time()).copied() {
            Some(clock) => clock,
            None => (self.spec.source_at_project(action.time())?, None),
        };
        let quantization = source_time.quantize_nanoseconds()?;
        Ok((
            MotionAction::new(quantization.time, action.position(), action.evidence())?,
            SourceClockActionMapping {
                project_time: action.time(),
                source_time,
                quantization,
                original_source_frame: frame,
            },
        ))
    }
    /// Apply only a selected-axis retime whose old mapping is this exact clock.
    /// Carrier replacement commits only after all entries validate, including
    /// quantization collisions. Anchors absent from the actual before track do
    /// not survive; new authored anchors use the declared old inverse mapping.
    pub fn apply_retime(
        &mut self,
        from: &SourcePlacementSpec,
        to: &SourcePlacementSpec,
        before: &MotionTrack,
    ) -> Result<(), CoreError> {
        from.validate()?;
        to.validate()?;
        if from != &self.spec
            || from.source_version != to.source_version
            || before.actions().len() > self.max_entries
        {
            return Err(invalid(
                "retiming clock does not match the current verified carrier",
            ));
        }
        let mut anchors = BTreeMap::new();
        for action in before.actions() {
            let declared = from.source_at_project(action.time())?;
            let mapped = to.project_at_source(declared)?.quantize_nanoseconds()?.time;
            let clock = self
                .anchors
                .get(&action.time())
                .copied()
                .unwrap_or((declared, None));
            to.project_at_source(clock.0)?;
            if anchors.insert(mapped, clock).is_some() {
                return Err(invalid("retimed source-clock anchors collide"));
            }
        }
        self.spec = to.clone();
        self.anchors = anchors;
        Ok(())
    }
    pub fn map_track(&self, track: &MotionTrack) -> Result<SourceClockMotion, CoreError> {
        if track.actions().is_empty()
            || track.actions().len() > self.max_entries
            || !track.gaps().is_empty()
        {
            return Err(invalid(
                "clock mapping requires bounded nonempty gap-resolved motion",
            ));
        }
        let mut mapped = Vec::with_capacity(track.actions().len());
        let mut reused = 0u64;
        for action in track.actions() {
            let value = self.map_action(action)?;
            if value.1.original_source_frame.is_some() {
                reused = reused.checked_add(1).ok_or(CoreError::Overflow)?;
            }
            mapped.push(value);
        }
        mapped.sort_by_key(|(action, _)| action.time());
        if mapped
            .windows(2)
            .any(|pair| pair[0].0.time() >= pair[1].0.time())
        {
            return Err(invalid("source-clock action quantization collides"));
        }
        let (actions, mappings) = mapped.into_iter().unzip();
        Ok(SourceClockMotion {
            source_version: self.spec.source_version.clone(),
            track: MotionTrack::with_name_and_gaps(track.name(), track.axis(), actions, vec![])?,
            mappings,
            reused_sample_clocks: reused,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClockActionMapping {
    pub project_time: ProjectTime,
    pub source_time: ExactSeconds,
    pub quantization: ExactTimeQuantization,
    /// A reused sample clock does not claim that an edited position is observed.
    pub original_source_frame: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClockMotion {
    pub source_version: SourceVersionId,
    pub track: MotionTrack,
    pub mappings: Vec<SourceClockActionMapping>,
    pub reused_sample_clocks: u64,
}
fn invalid(message: &str) -> CoreError {
    CoreError::InvalidMotion(message.into())
}

/// Map one explicitly selected, gap-resolved track to original elapsed-source
/// time. Generated samples preserve their exact original clock even when the
/// project's nanosecond timestamp was rounded. Other authored times use the
/// declared inverse transform. Position/evidence/name/axis are preserved.
/// The engine must independently verify the generation receipt and ownership;
/// this function validates arithmetic/associations, never evidence provenance.
pub fn map_track_to_source_clock(
    track: &MotionTrack,
    spec: &SourcePlacementSpec,
    generated: Option<&PlacedGenerationPlan>,
    max_entries: usize,
) -> Result<SourceClockMotion, CoreError> {
    spec.validate()?;
    if max_entries == 0
        || track.actions().is_empty()
        || track.actions().len() > max_entries
        || !track.gaps().is_empty()
    {
        return Err(invalid(
            "source-clock export requires bounded nonempty motion with explicitly resolved gaps",
        ));
    }
    let anchors = sample_anchors(spec, generated, max_entries)?;
    map_with_anchors(track, spec, &anchors, max_entries)
}

fn sample_anchors(
    spec: &SourcePlacementSpec,
    generated: Option<&PlacedGenerationPlan>,
    max_entries: usize,
) -> Result<BTreeMap<ProjectTime, (ExactSeconds, u64)>, CoreError> {
    let mut anchors = BTreeMap::new();
    if let Some(plan) = generated {
        if &plan.placement.spec != spec
            || plan.samples.is_empty()
            || plan.samples.len() > max_entries
        {
            return Err(invalid(
                "source-clock sample plan differs from mapping or admission",
            ));
        }
        let expected_order: Vec<_> = if spec.rate_numerator > 0 {
            (0..plan.samples.len()).collect()
        } else {
            (0..plan.samples.len()).rev().collect()
        };
        if plan.project_order != expected_order {
            return Err(invalid("source-clock sample project order is inconsistent"));
        }
        let mut previous: Option<(u64, ExactSeconds)> = None;
        for sample in &plan.samples {
            if let Some((frame, time)) = previous {
                if frame >= sample.source_frame_index
                    || time.checked_cmp(sample.elapsed_source_time)? != Ordering::Less
                {
                    return Err(invalid(
                        "source-clock sample ordinals/times are not strictly ordered",
                    ));
                }
            }
            let elapsed = ExactSeconds::from_source(sample.source_time)
                .checked_sub(ExactSeconds::from_source(plan.source_origin))?;
            let mapped = spec.project_at_source(sample.elapsed_source_time)?;
            if elapsed != sample.elapsed_source_time
                || mapped != sample.project_time
                || mapped.quantize_nanoseconds()? != sample.quantization
            {
                return Err(invalid(
                    "source-clock sample mapping or original timestamp is inconsistent",
                ));
            }
            if anchors
                .insert(
                    sample.quantization.time,
                    (sample.elapsed_source_time, sample.source_frame_index),
                )
                .is_some()
            {
                return Err(invalid(
                    "source-clock sample anchors collide at project precision",
                ));
            }
            previous = Some((sample.source_frame_index, sample.elapsed_source_time));
        }
        if expected_order
            .windows(2)
            .any(|p| plan.samples[p[0]].quantization.time >= plan.samples[p[1]].quantization.time)
        {
            return Err(invalid(
                "source-clock sample project timestamps are not ordered",
            ));
        }
    }
    Ok(anchors)
}

fn map_with_anchors(
    track: &MotionTrack,
    spec: &SourcePlacementSpec,
    anchors: &BTreeMap<ProjectTime, (ExactSeconds, u64)>,
    max_entries: usize,
) -> Result<SourceClockMotion, CoreError> {
    spec.validate()?;
    if max_entries == 0
        || track.actions().is_empty()
        || track.actions().len() > max_entries
        || !track.gaps().is_empty()
    {
        return Err(invalid(
            "source-clock export requires bounded nonempty gap-resolved motion",
        ));
    }
    let mut mapped = Vec::with_capacity(track.actions().len());
    let mut reused_sample_clocks = 0u64;
    for action in track.actions() {
        let (source_time, original_source_frame) =
            if let Some(&(source, frame)) = anchors.get(&action.time()) {
                reused_sample_clocks = reused_sample_clocks
                    .checked_add(1)
                    .ok_or(CoreError::Overflow)?;
                (source, Some(frame))
            } else {
                (spec.source_at_project(action.time())?, None)
            };
        let quantization = source_time.quantize_nanoseconds()?;
        let mapped_action =
            MotionAction::new(quantization.time, action.position(), action.evidence())?;
        mapped.push((
            mapped_action,
            SourceClockActionMapping {
                project_time: action.time(),
                source_time,
                quantization,
                original_source_frame,
            },
        ));
    }
    mapped.sort_by_key(|(a, _)| a.time());
    if mapped.windows(2).any(|p| p[0].0.time() >= p[1].0.time()) {
        return Err(invalid(
            "source-clock nanosecond quantization merges actions; explicit resampling required",
        ));
    }
    let (actions, mappings) = mapped.into_iter().unzip();
    let track = MotionTrack::with_name_and_gaps(track.name(), track.axis(), actions, vec![])?;
    Ok(SourceClockMotion {
        source_version: spec.source_version.clone(),
        track,
        mappings,
        reused_sample_clocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        plan_placed_generation, Axis, EvidenceKind, NormalizedPosition, RevisionId,
        SourcePlacement, SourcePlacementId, SourceTimestamp, TimeRange,
    };
    fn placement(rate: i32) -> SourcePlacement {
        SourcePlacement {
            placement_id: SourcePlacementId::new("placement.clock").unwrap(),
            created_revision: RevisionId::new(3),
            spec: SourcePlacementSpec {
                source_version: SourceVersionId::new("source.clock").unwrap(),
                source_start: SourceTimestamp::new(0, 1).unwrap(),
                source_end: SourceTimestamp::new(2, 1).unwrap(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::from_nanos(3_000_000_000),
                rate_numerator: rate,
                rate_denominator: 1,
            },
        }
    }
    fn track(plan: &PlacedGenerationPlan) -> MotionTrack {
        let mut actions: Vec<_> = plan
            .samples
            .iter()
            .enumerate()
            .map(|(i, s)| {
                MotionAction::new(
                    s.quantization.time,
                    NormalizedPosition::new(0.2 + 0.1 * i as f64).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap()
            })
            .collect();
        actions.sort_by_key(|a| a.time());
        MotionTrack::new(Axis::Stroke, actions).unwrap()
    }
    #[test]
    fn sample_clock_recovery_preserves_original_fraction_instead_of_inverting_rounded_time() {
        let p = placement(3);
        let plan = plan_placed_generation(&p, &[15, 16, 18], 1, 3, 1, 3).unwrap();
        let original = track(&plan);
        let before = serde_json::to_vec(&original).unwrap();
        let output = map_track_to_source_clock(&original, &p.spec, Some(&plan), 10).unwrap();
        assert_eq!(output.reused_sample_clocks, 3);
        assert_eq!(
            output.mappings[1].source_time,
            ExactSeconds::new(1, 3).unwrap()
        );
        assert_ne!(
            p.spec
                .source_at_project(original.actions()[1].time())
                .unwrap(),
            output.mappings[1].source_time
        );
        assert_eq!(output.mappings[1].quantization.remainder_numerator, "1");
        assert_eq!(output.track.actions()[1].time().as_nanos(), 333_333_333);
        assert_eq!(serde_json::to_vec(&original).unwrap(), before);
        for (a, b) in original.actions().iter().zip(output.track.actions()) {
            assert_eq!(a.position(), b.position());
            assert_eq!(a.evidence(), b.evidence());
        }
    }
    #[test]
    fn reverse_source_export_reorders_current_values_without_relabeling_authored_evidence() {
        let p = placement(-1);
        let plan = plan_placed_generation(&p, &[40, 41, 42], 1, 8, 1, 3).unwrap();
        let input = track(&plan);
        let output = map_track_to_source_clock(&input, &p.spec, Some(&plan), 10).unwrap();
        for (a, b) in input
            .actions()
            .iter()
            .zip(output.track.actions().iter().rev())
        {
            assert_eq!(a.position(), b.position());
            assert_eq!(b.evidence(), EvidenceKind::Synthesized);
        }
        assert_eq!(
            output
                .mappings
                .iter()
                .map(|m| m.original_source_frame.unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        let authored = MotionTrack::new(
            Axis::Stroke,
            vec![MotionAction::new(
                ProjectTime::from_nanos(2_750_000_000),
                NormalizedPosition::new(0.9).unwrap(),
                EvidenceKind::Synthesized,
            )
            .unwrap()],
        )
        .unwrap();
        let changed = map_track_to_source_clock(&authored, &p.spec, None, 10).unwrap();
        assert_eq!(changed.mappings[0].original_source_frame, None);
        assert_eq!(
            changed.mappings[0].source_time,
            ExactSeconds::new(1, 4).unwrap()
        );
    }
    #[test]
    fn retiming_chain_preserves_fractional_sample_clocks_and_edited_labels() {
        let p = placement(3);
        let plan = plan_placed_generation(&p, &[15, 16, 18], 1, 3, 1, 3).unwrap();
        let original = track(&plan);
        let expected = map_track_to_source_clock(&original, &p.spec, Some(&plan), 10).unwrap();
        let mut mapper = SourceClockMapper::new(&p.spec, Some(&plan), 10).unwrap();
        let mut to = p.spec.clone();
        to.rate_numerator = -2;
        to.project_anchor = ProjectTime::from_nanos(5_000_000_000);
        let program = crate::MotionProgram::new(vec![original.clone()]).unwrap();
        let retimed =
            crate::retime_motion_between_placements(&program, &p.spec, &to, &[crate::Axis::Stroke])
                .unwrap();
        mapper.apply_retime(&p.spec, &to, &original).unwrap();
        let current = retimed.track(crate::Axis::Stroke).unwrap();
        assert_eq!(mapper.map_track(current).unwrap().track, expected.track);
        let mut next = to.clone();
        next.rate_numerator = 5;
        next.project_anchor = ProjectTime::from_nanos(7_000_000_000);
        let twice =
            crate::retime_motion_between_placements(&retimed, &to, &next, &[crate::Axis::Stroke])
                .unwrap();
        mapper.apply_retime(&to, &next, current).unwrap();
        let mapped = mapper
            .map_track(twice.track(crate::Axis::Stroke).unwrap())
            .unwrap();
        assert_eq!(mapped.track, expected.track);
        assert_eq!(mapped.reused_sample_clocks, 3);
        assert_eq!(
            mapped.mappings[1].source_time,
            ExactSeconds::new(1, 3).unwrap()
        );
    }
    #[test]
    fn authored_clock_carriers_have_no_frame_claim_and_failed_updates_are_atomic() {
        let p = placement(1);
        let original = MotionTrack::new(
            crate::Axis::Stroke,
            vec![
                MotionAction::new(
                    ProjectTime::from_nanos(3_000_000_001),
                    NormalizedPosition::new(0.2).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(3_000_000_002),
                    NormalizedPosition::new(0.8).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let mut mapper = SourceClockMapper::new(&p.spec, None, 10).unwrap();
        let mut to = p.spec.clone();
        to.rate_numerator = 3;
        assert!(mapper.apply_retime(&p.spec, &to, &original).is_err());
        assert_eq!(mapper.spec(), &p.spec);
        to.rate_numerator = -1;
        to.project_anchor = ProjectTime::from_nanos(4_000_000_000);
        mapper.apply_retime(&p.spec, &to, &original).unwrap();
        let program = crate::MotionProgram::new(vec![original]).unwrap();
        let moved =
            crate::retime_motion_between_placements(&program, &p.spec, &to, &[crate::Axis::Stroke])
                .unwrap();
        let mapped = mapper
            .map_track(moved.track(crate::Axis::Stroke).unwrap())
            .unwrap();
        assert_eq!(mapped.reused_sample_clocks, 0);
        assert!(mapped
            .mappings
            .iter()
            .all(|m| m.original_source_frame.is_none()));
        assert_eq!(mapped.track.actions()[0].time().as_nanos(), 1);
    }
    #[test]
    fn corrupt_sample_plans_unknown_coverage_and_lossy_action_merges_refuse() {
        let p = placement(3);
        let plan = plan_placed_generation(&p, &[0, 1, 2], 1, 3, 1, 3).unwrap();
        let input = track(&plan);
        let mut bad = plan.clone();
        bad.samples[1].quantization.remainder_numerator = "999".into();
        assert!(map_track_to_source_clock(&input, &p.spec, Some(&bad), 10).is_err());
        let mut bad = plan.clone();
        bad.samples[1].source_frame_index = 0;
        assert!(map_track_to_source_clock(&input, &p.spec, Some(&bad), 10).is_err());
        let mut bad = plan.clone();
        bad.samples[1].source_time = SourceTimestamp::new(999, 1).unwrap();
        assert!(map_track_to_source_clock(&input, &p.spec, Some(&bad), 10).is_err());
        let mut bad = plan.clone();
        bad.project_order.reverse();
        assert!(map_track_to_source_clock(&input, &p.spec, Some(&bad), 10).is_err());
        assert!(map_track_to_source_clock(&input, &p.spec, Some(&plan), 1).is_err());
        let gap = MotionTrack::with_gaps(
            Axis::Stroke,
            vec![],
            vec![TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1)).unwrap()],
        )
        .unwrap();
        assert!(map_track_to_source_clock(&gap, &p.spec, None, 10).is_err());
        let outside = MotionTrack::new(
            Axis::Stroke,
            vec![MotionAction::new(
                ProjectTime::ZERO,
                NormalizedPosition::new(0.5).unwrap(),
                EvidenceKind::Synthesized,
            )
            .unwrap()],
        )
        .unwrap();
        assert!(map_track_to_source_clock(&outside, &p.spec, None, 10).is_err());
        let mut small = placement(1);
        small.spec.rate_denominator = 1_000_000_000;
        let tiny = MotionTrack::new(
            Axis::Stroke,
            vec![
                MotionAction::new(
                    ProjectTime::from_nanos(3_000_000_001),
                    NormalizedPosition::new(0.5).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(3_000_000_002),
                    NormalizedPosition::new(0.6).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        assert!(map_track_to_source_clock(&tiny, &small.spec, None, 10).is_err());
    }
}
