//! Explicit per-action clock correspondence, without provenance or authority.
use crate::{CoreError, MotionTrack, ProjectTime, SourceClockMapper, SourceClockMotion};

/// Select a verified carrier for one exact current project action. Interval
/// membership alone cannot select a merge boundary's originating clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceClockSelection {
    pub project_time: ProjectTime,
    pub carrier: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        plan_placed_generation, Axis, EvidenceKind, ExactSeconds, MotionAction, NormalizedPosition,
        RevisionId, SourcePlacement, SourcePlacementId, SourcePlacementSpec, SourceTimestamp,
        SourceVersionId, TimeRange,
    };

    fn placement(rate: i32, anchor: i64, project: i64) -> SourcePlacement {
        SourcePlacement {
            placement_id: SourcePlacementId::new("piece.clock").unwrap(),
            created_revision: RevisionId::new(1),
            spec: SourcePlacementSpec {
                source_version: SourceVersionId::new("source.clock").unwrap(),
                source_start: SourceTimestamp::new(0, 1).unwrap(),
                source_end: SourceTimestamp::new(2, 1).unwrap(),
                source_anchor: SourceTimestamp::new(anchor, 1).unwrap(),
                project_anchor: ProjectTime::from_nanos(project),
                rate_numerator: rate,
                rate_denominator: 1,
            },
        }
    }
    fn carrier(p: &SourcePlacement) -> SourceClockMapper {
        let pts: Vec<i64> = (0..16).collect();
        let plan = plan_placed_generation(p, &pts, 1, 8, 1, 16).unwrap();
        SourceClockMapper::new(&p.spec, Some(&plan), 16).unwrap()
    }
    fn track(times: &[i64]) -> MotionTrack {
        MotionTrack::with_name_and_gaps(
            "authored mixed clocks",
            Axis::Stroke,
            times
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    MotionAction::new(
                        ProjectTime::from_nanos(*t),
                        NormalizedPosition::new(0.1 + i as f64 * 0.1).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap()
                })
                .collect(),
            vec![],
        )
        .unwrap()
    }
    fn selections(track: &MotionTrack, clocks: &[usize]) -> Vec<SourceClockSelection> {
        track
            .actions()
            .iter()
            .zip(clocks)
            .map(|(a, i)| SourceClockSelection {
                project_time: a.time(),
                carrier: *i,
            })
            .collect()
    }
    #[test]
    fn different_clocks_preserve_selected_frames_values_and_master() {
        let input = track(&[0, 250_000_000, 500_000_000, 750_000_000]);
        let before = serde_json::to_vec(&input).unwrap();
        let clocks = [
            carrier(&placement(1, 0, 0)),
            carrier(&placement(2, 1, 500_000_000)),
        ];
        let out = map_track_to_selected_source_clocks(
            &input,
            &clocks,
            &selections(&input, &[0, 0, 1, 1]),
            32,
            2,
        )
        .unwrap();
        assert_eq!(out.carriers, vec![0, 0, 1, 1]);
        assert_eq!(out.motion.reused_sample_clocks, 4);
        assert_eq!(
            out.motion
                .mappings
                .iter()
                .map(|m| m.source_time)
                .collect::<Vec<_>>(),
            vec![
                ExactSeconds::new(0, 1).unwrap(),
                ExactSeconds::new(1, 4).unwrap(),
                ExactSeconds::new(1, 1).unwrap(),
                ExactSeconds::new(3, 2).unwrap()
            ]
        );
        assert_eq!(
            out.motion
                .mappings
                .iter()
                .map(|m| m.original_source_frame)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(2), Some(8), Some(12)]
        );
        assert_eq!(out.motion.track.name(), input.name());
        assert_eq!(out.motion.track.axis(), input.axis());
        for (a, b) in input.actions().iter().zip(out.motion.track.actions()) {
            assert_eq!(a.position(), b.position());
            assert_eq!(a.evidence(), b.evidence());
        }
        assert_eq!(serde_json::to_vec(&input).unwrap(), before);
    }
    #[test]
    fn merge_boundary_is_explicit_and_correct_interior_can_collide_with_parent() {
        let clocks = [carrier(&placement(1, 0, 0)), carrier(&placement(2, 0, 0))];
        let input = track(&[125_000_000, 250_000_000]);
        let out = map_track_to_selected_source_clocks(
            &input,
            &clocks,
            &selections(&input, &[0, 1]),
            32,
            2,
        )
        .unwrap();
        assert_eq!(
            out.motion.mappings[0].source_time,
            ExactSeconds::new(1, 8).unwrap()
        );
        assert_eq!(out.motion.mappings[0].original_source_frame, Some(1));
        assert_eq!(
            out.motion.mappings[1].source_time,
            ExactSeconds::new(1, 2).unwrap()
        );
        assert_eq!(out.motion.mappings[1].original_source_frame, Some(4));
        let collision = track(&[125_000_000, 250_000_000, 500_000_000]);
        assert!(map_track_to_selected_source_clocks(
            &collision,
            &clocks,
            &selections(&collision, &[0, 1, 0]),
            32,
            2
        )
        .is_err());
    }
    #[test]
    fn reverse_clock_keeps_carrier_labels_parallel_to_source_order() {
        let clocks = [
            carrier(&placement(-1, 0, 2_000_000_000)),
            carrier(&placement(1, 0, 0)),
        ];
        let input = track(&[125_000_000, 250_000_000, 500_000_000]);
        let out = map_track_to_selected_source_clocks(
            &input,
            &clocks,
            &selections(&input, &[0, 0, 1]),
            32,
            2,
        )
        .unwrap();
        assert_eq!(out.carriers, vec![1, 0, 0]);
        assert_eq!(
            out.motion
                .mappings
                .iter()
                .map(|m| m.project_time.as_nanos())
                .collect::<Vec<_>>(),
            vec![500_000_000, 250_000_000, 125_000_000]
        );
        for (action, mapping) in out.motion.track.actions().iter().zip(&out.motion.mappings) {
            let original = input
                .actions()
                .iter()
                .find(|a| a.time() == mapping.project_time)
                .unwrap();
            assert_eq!(action.position(), original.position());
            assert_eq!(action.evidence(), original.evidence());
        }
    }
    #[test]
    fn single_selected_clock_matches_existing_fractional_and_reverse_mapping() {
        for rate in [-3, -1, 1, 3] {
            let p = placement(rate, 0, 3_000_000_000);
            let plan = plan_placed_generation(&p, &[15, 16, 18], 1, 3, 1, 3).unwrap();
            let mut times: Vec<_> = plan
                .samples
                .iter()
                .map(|s| s.quantization.time.as_nanos())
                .collect();
            times.sort();
            let input = track(&times);
            let clocks = [SourceClockMapper::new(&p.spec, Some(&plan), 3).unwrap()];
            let expected = clocks[0].map_track(&input).unwrap();
            let out = map_track_to_selected_source_clocks(
                &input,
                &clocks,
                &selections(&input, &[0, 0, 0]),
                3,
                1,
            )
            .unwrap();
            assert_eq!(
                serde_json::to_vec(&out.motion).unwrap(),
                serde_json::to_vec(&expected).unwrap()
            );
            assert_eq!(out.carriers, vec![0, 0, 0]);
            assert!(out
                .motion
                .mappings
                .iter()
                .any(|m| m.source_time == ExactSeconds::new(1, 3).unwrap()));
        }
    }
    #[test]
    fn authored_actions_keep_no_frame_claim_and_quantization_overflow_refuses() {
        let mut p = placement(1, 0, 0);
        let input = track(&[1, 125_000_001]);
        let clocks = [SourceClockMapper::new(&p.spec, None, 2).unwrap()];
        let out = map_track_to_selected_source_clocks(
            &input,
            &clocks,
            &selections(&input, &[0, 0]),
            2,
            1,
        )
        .unwrap();
        assert_eq!(out.motion.reused_sample_clocks, 0);
        assert!(out
            .motion
            .mappings
            .iter()
            .all(|m| m.original_source_frame.is_none()));
        p.spec.source_end = SourceTimestamp::new(i64::MAX, 1).unwrap();
        p.spec.source_anchor = SourceTimestamp::new(i64::MAX - 1, 1).unwrap();
        let clocks = [SourceClockMapper::new(&p.spec, None, 2).unwrap()];
        assert!(map_track_to_selected_source_clocks(
            &input,
            &clocks,
            &selections(&input, &[0, 0]),
            2,
            1
        )
        .is_err());
    }
    #[test]
    fn partial_stale_foreign_outside_trim_and_over_admission_selections_refuse() {
        let input = track(&[0, 250_000_000]);
        let clocks = [carrier(&placement(1, 0, 0)), carrier(&placement(2, 0, 0))];
        let selected = selections(&input, &[0, 1]);
        for (entries, count) in [(0, 2), (32, 0), (1, 2), (31, 2), (32, 1)] {
            assert!(map_track_to_selected_source_clocks(
                &input, &clocks, &selected, entries, count
            )
            .is_err());
        }
        assert!(
            map_track_to_selected_source_clocks(&input, &clocks, &selected[..1], 32, 2).is_err()
        );
        let mut stale = selected.clone();
        stale[1].project_time = ProjectTime::from_nanos(1);
        assert!(map_track_to_selected_source_clocks(&input, &clocks, &stale, 32, 2).is_err());
        stale = selected.clone();
        stale[1].carrier = 2;
        assert!(map_track_to_selected_source_clocks(&input, &clocks, &stale, 32, 2).is_err());
        let mut foreign = placement(1, 0, 0);
        foreign.spec.source_version = SourceVersionId::new("foreign").unwrap();
        assert!(map_track_to_selected_source_clocks(
            &input,
            &[carrier(&placement(1, 0, 0)), carrier(&foreign)],
            &selected,
            32,
            2
        )
        .is_err());
        let outside = track(&[2_000_000_000]);
        assert!(map_track_to_selected_source_clocks(
            &outside,
            &clocks,
            &selections(&outside, &[0]),
            32,
            2
        )
        .is_err());
        let gap = TimeRange::new(ProjectTime::from_nanos(1), ProjectTime::from_nanos(2)).unwrap();
        let unresolved = MotionTrack::with_name_and_gaps(
            input.name(),
            input.axis(),
            input.actions().to_vec(),
            vec![gap],
        )
        .unwrap();
        assert!(
            map_track_to_selected_source_clocks(&unresolved, &clocks, &selected, 32, 2).is_err()
        );
    }
}

#[derive(Clone, Debug)]
pub struct SelectedSourceClockMotion {
    pub motion: SourceClockMotion,
    /// Parallel to the output source-ordered actions and mappings, not to the
    /// project-ordered input selections. Indices identify the supplied carriers.
    pub carriers: Vec<usize>,
}

/// Map every current action through one explicitly selected source clock.
/// Input selections must exactly match the action times in project order.
/// Values, evidence, track name and axis are preserved; source ordering may
/// change. Duplicate quantized source times refuse rather than discard a value.
/// Admission bounds both the input track and aggregate retained sample anchors.
///
/// The Engine must independently prove each selection from immutable generation,
/// retiming and edit/merge ancestry. This pure arithmetic operation does not
/// establish that a caller's selections are true, approve playback/publication,
/// or resolve interpolation/resampling across overlapping source intervals.
pub fn map_track_to_selected_source_clocks(
    track: &MotionTrack,
    carriers: &[SourceClockMapper],
    selections: &[SourceClockSelection],
    max_entries: usize,
    max_carriers: usize,
) -> Result<SelectedSourceClockMotion, CoreError> {
    let invalid = |s: &str| CoreError::InvalidMotion(s.into());
    if max_entries == 0
        || max_carriers == 0
        || track.actions().is_empty()
        || track.actions().len() > max_entries
        || !track.gaps().is_empty()
        || selections.len() != track.actions().len()
        || carriers.is_empty()
        || carriers.len() > max_carriers
    {
        return Err(invalid(
            "selected clocks require bounded complete gap-resolved motion",
        ));
    }
    let source = &carriers[0].spec().source_version;
    let mut anchors = 0usize;
    for carrier in carriers {
        if &carrier.spec().source_version != source {
            return Err(invalid("selected clocks must belong to one source version"));
        }
        anchors = anchors
            .checked_add(carrier.anchor_count())
            .ok_or(CoreError::Overflow)?;
        if anchors > max_entries {
            return Err(invalid("aggregate selected sample clocks exceed admission"));
        }
    }
    let mut mapped = Vec::with_capacity(track.actions().len());
    let mut reused = 0u64;
    for (action, selection) in track.actions().iter().zip(selections) {
        if action.time() != selection.project_time {
            return Err(invalid(
                "clock selection differs from the exact current action time",
            ));
        }
        let carrier = carriers
            .get(selection.carrier)
            .ok_or_else(|| invalid("selected source clock carrier is absent"))?;
        let (action, mapping) = carrier.map_action(action)?;
        if mapping.original_source_frame.is_some() {
            reused = reused.checked_add(1).ok_or(CoreError::Overflow)?;
        }
        mapped.push((action, mapping, selection.carrier));
    }
    mapped.sort_by_key(|(action, _, _)| action.time());
    if mapped.windows(2).any(|p| p[0].0.time() >= p[1].0.time()) {
        return Err(invalid(
            "selected source-clock action quantization collides",
        ));
    }
    let mut actions = Vec::with_capacity(mapped.len());
    let mut mappings = Vec::with_capacity(mapped.len());
    let mut selected = Vec::with_capacity(mapped.len());
    for (action, mapping, carrier) in mapped {
        actions.push(action);
        mappings.push(mapping);
        selected.push(carrier);
    }
    Ok(SelectedSourceClockMotion {
        motion: SourceClockMotion {
            source_version: source.clone(),
            track: MotionTrack::with_name_and_gaps(track.name(), track.axis(), actions, vec![])?,
            mappings,
            reused_sample_clocks: reused,
        },
        carriers: selected,
    })
}
