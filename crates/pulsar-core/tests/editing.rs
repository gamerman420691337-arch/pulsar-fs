use pulsar_core::*;

fn time(value: i64) -> ProjectTime {
    ProjectTime::from_nanos(value)
}
fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(time(start), time(end)).unwrap()
}
fn track(axis: Axis, values: &[(i64, f64)]) -> MotionTrack {
    MotionTrack::new(
        axis,
        values
            .iter()
            .map(|(at, pos)| {
                MotionAction::new(
                    time(*at),
                    NormalizedPosition::new(*pos).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}
fn program(values: &[(i64, f64)]) -> MotionProgram {
    MotionProgram::new(vec![track(Axis::Stroke, values)]).unwrap()
}
#[test]
fn crossing_segment_and_neighbor_anchors_are_protected() {
    let old = program(&[(0, 0.0), (100, 1.0), (200, 0.0), (300, 1.0)]);
    let new = program(&[(0, 1.0), (50, 0.9), (100, 0.0), (200, 0.7), (300, 0.2)]);
    let protected = vec![ProtectedRegion {
        axis: Some(Axis::Stroke),
        range: range(20, 80),
    }];
    let merged =
        merge_candidate_preserving_protection(&old, &new, &[Axis::Stroke], None, &protected)
            .unwrap();
    assert_eq!(
        &merged.track(Axis::Stroke).unwrap().actions()[..2],
        &old.track(Axis::Stroke).unwrap().actions()[..2]
    );
    assert_eq!(
        merged.track(Axis::Stroke).unwrap().actions()[2]
            .position()
            .value(),
        0.7
    );
    assert!(ensure_protected_segments_unchanged(&old, &new, &protected).is_err());
}
#[test]
fn range_and_axis_selection_preserve_outside_segments() {
    let stroke = track(
        Axis::Stroke,
        &[(0, 0.0), (100, 0.2), (200, 0.4), (300, 0.6), (400, 0.8)],
    );
    let sway = track(Axis::Sway, &[(0, 0.9), (400, 0.1)]);
    let old = MotionProgram::new(vec![stroke.clone(), sway.clone()]).unwrap();
    let new = program(&[(0, 0.9), (100, 0.9), (200, 0.9), (300, 0.9), (400, 0.9)]);
    let merged = merge_candidate_preserving_protection(
        &old,
        &new,
        &[Axis::Stroke],
        Some(range(50, 350)),
        &[],
    )
    .unwrap();
    let actual = merged.track(Axis::Stroke).unwrap().actions();
    assert_eq!(actual[1], stroke.actions()[1]);
    assert_eq!(actual[2].position().value(), 0.9);
    assert_eq!(actual[3], stroke.actions()[3]);
    assert_eq!(merged.track(Axis::Sway), Some(&sway));
}
#[test]
fn protected_gaps_and_half_open_boundary_remain_exact() {
    let old = MotionProgram::new(vec![MotionTrack::with_gaps(
        Axis::Stroke,
        track(
            Axis::Stroke,
            &[(0, 0.0), (100, 1.0), (200, 0.2), (300, 0.8)],
        )
        .actions()
        .to_vec(),
        vec![range(20, 80)],
    )
    .unwrap()])
    .unwrap();
    let new = program(&[(0, 1.0), (50, 0.4), (100, 0.0), (200, 0.9), (300, 0.1)]);
    let protected = vec![ProtectedRegion {
        axis: None,
        range: range(20, 80),
    }];
    let merged =
        merge_candidate_preserving_protection(&old, &new, &[Axis::Stroke], None, &protected)
            .unwrap();
    assert_eq!(merged.track(Axis::Stroke).unwrap().gaps(), &[range(20, 80)]);
    ensure_protected_segments_unchanged(&old, &merged, &protected).unwrap();
}
#[test]
fn empty_protected_axis_cannot_gain_interpolated_motion() {
    let old = MotionProgram::default();
    let new = program(&[(0, 0.1), (100, 0.9)]);
    let protected = vec![ProtectedRegion {
        axis: None,
        range: range(40, 60),
    }];
    assert_eq!(
        merge_candidate_preserving_protection(&old, &new, &[Axis::Stroke], None, &protected)
            .unwrap(),
        old
    );
}
#[test]
fn malformed_selection_and_endpoint_overflow_reject() {
    let old = program(&[(0, 0.0), (100, 1.0)]);
    assert!(merge_candidate_preserving_protection(&old, &old, &[], None, &[]).is_err());
    assert!(merge_candidate_preserving_protection(
        &old,
        &old,
        &[Axis::Stroke, Axis::Stroke],
        None,
        &[]
    )
    .is_err());
    assert!(merge_candidate_preserving_protection(&old, &old, &[Axis::Yaw], None, &[]).is_err());
    let overflow = program(&[(i64::MAX, 0.5)]);
    assert!(
        merge_candidate_preserving_protection(&old, &overflow, &[Axis::Stroke], None, &[]).is_err()
    );
}

#[test]
fn exact_selection_anchors_allow_an_interior_candidate_action() {
    let old = program(&[(0, 0.1), (100, 0.2), (200, 0.3), (300, 0.4)]);
    let candidate = program(&[(0, 0.1), (100, 0.2), (150, 0.8), (200, 0.3), (300, 0.4)]);
    let merged = merge_candidate_preserving_protection(
        &old,
        &candidate,
        &[Axis::Stroke],
        Some(range(100, 200)),
        &[],
    )
    .unwrap();
    assert_eq!(merged, candidate);
}

#[test]
fn exact_protection_start_needs_no_predecessor_but_between_knots_does() {
    let old = program(&[(0, 0.1), (100, 0.2), (200, 0.3), (300, 0.4)]);
    let candidate = program(&[(0, 0.1), (50, 0.8), (100, 0.2), (200, 0.3), (300, 0.4)]);
    let protected = vec![ProtectedRegion {
        axis: Some(Axis::Stroke),
        range: range(100, 200),
    }];
    let merged =
        merge_candidate_preserving_protection(&old, &candidate, &[Axis::Stroke], None, &protected)
            .unwrap();
    assert_eq!(merged, candidate);
    let changed_predecessor = program(&[(0, 0.1), (100, 0.9), (200, 0.3), (300, 0.4)]);
    let between = vec![ProtectedRegion {
        axis: Some(Axis::Stroke),
        range: range(110, 190),
    }];
    assert!(ensure_protected_segments_unchanged(&old, &changed_predecessor, &between).is_err());
    assert_eq!(
        merge_candidate_preserving_protection(
            &old,
            &changed_predecessor,
            &[Axis::Stroke],
            None,
            &between,
        )
        .unwrap(),
        old
    );
}

#[test]
fn contribution_envelopes_exclude_protected_anchors_and_omitted_axes() {
    let action = |at, pos| {
        MotionAction::new(
            ProjectTime::from_nanos(at),
            NormalizedPosition::new(pos).unwrap(),
            EvidenceKind::Synthesized,
        )
        .unwrap()
    };
    let current = MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        vec![
            action(0, 0.1),
            action(100, 0.2),
            action(200, 0.3),
            action(300, 0.4),
        ],
    )
    .unwrap()])
    .unwrap();
    let candidate = MotionProgram::new(vec![
        MotionTrack::with_gaps(
            Axis::Stroke,
            vec![
                action(0, 0.8),
                action(100, 0.8),
                action(150, 0.8),
                action(200, 0.8),
                action(300, 0.8),
            ],
            vec![
                TimeRange::new(ProjectTime::from_nanos(175), ProjectTime::from_nanos(180)).unwrap(),
            ],
        )
        .unwrap(),
        MotionTrack::new(Axis::Sway, vec![action(0, 0.8), action(300, 0.8)]).unwrap(),
    ])
    .unwrap();
    let regions = candidate_merge_writable_regions(
        &current,
        &candidate,
        &[Axis::Stroke],
        Some(TimeRange::new(ProjectTime::from_nanos(100), ProjectTime::from_nanos(200)).unwrap()),
        &[],
    )
    .unwrap();
    assert_eq!(
        regions,
        vec![(
            Axis::Stroke,
            TimeRange::new(ProjectTime::from_nanos(101), ProjectTime::from_nanos(200)).unwrap()
        )]
    );
    let protected = [ProtectedRegion {
        axis: Some(Axis::Stroke),
        range: TimeRange::new(ProjectTime::from_nanos(120), ProjectTime::from_nanos(130)).unwrap(),
    }];
    assert!(candidate_merge_writable_regions(
        &current,
        &candidate,
        &[Axis::Stroke],
        Some(TimeRange::new(ProjectTime::from_nanos(100), ProjectTime::from_nanos(200)).unwrap()),
        &protected
    )
    .unwrap()
    .is_empty());
}

#[test]
fn singleton_candidate_envelopes_cover_interpolation_and_writable_gaps() {
    let action = |at, pos| {
        MotionAction::new(
            ProjectTime::from_nanos(at),
            NormalizedPosition::new(pos).unwrap(),
            EvidenceKind::Synthesized,
        )
        .unwrap()
    };
    let range = |start, end| {
        TimeRange::new(ProjectTime::from_nanos(start), ProjectTime::from_nanos(end)).unwrap()
    };
    let current = MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        vec![
            action(0, 0.1),
            action(100, 0.2),
            action(200, 0.3),
            action(300, 0.4),
        ],
    )
    .unwrap()])
    .unwrap();
    for gaps in [vec![], vec![range(170, 180)]] {
        let candidate = MotionProgram::new(vec![MotionTrack::with_gaps(
            Axis::Stroke,
            vec![action(150, 0.8)],
            gaps.clone(),
        )
        .unwrap()])
        .unwrap();
        let merged = merge_candidate_preserving_protection(
            &current,
            &candidate,
            &[Axis::Stroke],
            Some(range(100, 200)),
            &[],
        )
        .unwrap();
        assert!(merged
            .track(Axis::Stroke)
            .unwrap()
            .actions()
            .iter()
            .any(|action| action.time() == ProjectTime::from_nanos(150)));
        let regions = candidate_merge_writable_regions(
            &current,
            &candidate,
            &[Axis::Stroke],
            Some(range(100, 200)),
            &[],
        )
        .unwrap();
        assert_eq!(regions, vec![(Axis::Stroke, range(101, 200))]);
        assert!(regions[0].1.contains(ProjectTime::from_nanos(125)));
        assert!(regions[0].1.contains(ProjectTime::from_nanos(175)));
        assert!(!regions[0].1.contains(ProjectTime::from_nanos(100)));
        assert!(!regions[0].1.contains(ProjectTime::from_nanos(200)));
        assert_eq!(merged.track(Axis::Stroke).unwrap().gaps(), gaps.as_slice());
    }
}

#[test]
fn empty_incoming_candidate_envelope_covers_removed_interior_motion() {
    let action = |at, pos| {
        MotionAction::new(
            ProjectTime::from_nanos(at),
            NormalizedPosition::new(pos).unwrap(),
            EvidenceKind::Synthesized,
        )
        .unwrap()
    };
    let range = TimeRange::new(ProjectTime::from_nanos(100), ProjectTime::from_nanos(200)).unwrap();
    let current = MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        vec![
            action(0, 0.1),
            action(100, 0.2),
            action(150, 0.8),
            action(200, 0.3),
            action(300, 0.4),
        ],
    )
    .unwrap()])
    .unwrap();
    let candidate =
        MotionProgram::new(vec![MotionTrack::new(Axis::Stroke, vec![]).unwrap()]).unwrap();
    let regions =
        candidate_merge_writable_regions(&current, &candidate, &[Axis::Stroke], Some(range), &[])
            .unwrap();
    assert_eq!(
        regions,
        vec![(
            Axis::Stroke,
            TimeRange::new(ProjectTime::from_nanos(101), ProjectTime::from_nanos(200)).unwrap()
        )]
    );
}

fn check_origins(
    current: &MotionProgram,
    candidate: &MotionProgram,
    merged: &CandidateMergeOrigins,
) {
    assert_eq!(merged.action_origins.len(), merged.program.tracks().len());
    for output in merged.program.tracks() {
        let labels = &merged.action_origins[&output.axis()];
        assert_eq!(labels.len(), output.actions().len());
        for (action, label) in output.actions().iter().zip(labels) {
            assert_eq!(label.project_time, action.time());
            let input = match label.input {
                CandidateMergeInput::Current => current,
                CandidateMergeInput::Candidate => candidate,
            };
            assert_eq!(
                &input.track(output.axis()).unwrap().actions()[label.action_index],
                action
            );
        }
    }
}

#[test]
fn merge_origins_distinguish_equal_values_at_boundary_and_interior() {
    let current = program(&[
        (0, 0.1),
        (125_000_000, 0.2),
        (250_000_000, 0.3),
        (375_000_000, 0.4),
        (500_000_000, 0.5),
    ]);
    let candidate = current.clone();
    let merged = merge_candidate_with_action_origins(
        &current,
        &candidate,
        &[Axis::Stroke],
        Some(range(125_000_000, 375_000_001)),
        &[],
    )
    .unwrap();
    assert_eq!(merged.program, current);
    let labels = &merged.action_origins[&Axis::Stroke];
    assert_eq!(
        labels.iter().map(|l| l.input).collect::<Vec<_>>(),
        vec![
            CandidateMergeInput::Current,
            CandidateMergeInput::Current,
            CandidateMergeInput::Candidate,
            CandidateMergeInput::Current,
            CandidateMergeInput::Current
        ]
    );
    assert_eq!(
        labels.iter().map(|l| l.action_index).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
    check_origins(&current, &candidate, &merged);
}

#[test]
fn merge_origins_keep_original_indices_after_sparse_insertion_and_deletion() {
    let current = program(&[(0, 0.1), (100, 0.2), (125, 0.8), (200, 0.3), (300, 0.4)]);
    let candidate = program(&[
        (0, 0.8),
        (50, 0.8),
        (100, 0.8),
        (150, 0.8),
        (175, 0.8),
        (200, 0.8),
        (300, 0.8),
    ]);
    let merged = merge_candidate_with_action_origins(
        &current,
        &candidate,
        &[Axis::Stroke],
        Some(range(100, 200)),
        &[],
    )
    .unwrap();
    assert_eq!(
        merged.action_origins[&Axis::Stroke]
            .iter()
            .map(|l| (l.project_time.as_nanos(), l.input, l.action_index))
            .collect::<Vec<_>>(),
        vec![
            (0, CandidateMergeInput::Current, 0),
            (100, CandidateMergeInput::Current, 1),
            (150, CandidateMergeInput::Candidate, 3),
            (175, CandidateMergeInput::Candidate, 4),
            (200, CandidateMergeInput::Current, 3),
            (300, CandidateMergeInput::Current, 4)
        ]
    );
    check_origins(&current, &candidate, &merged);
    assert_eq!(
        merged.program,
        merge_candidate_preserving_protection(
            &current,
            &candidate,
            &[Axis::Stroke],
            Some(range(100, 200)),
            &[]
        )
        .unwrap()
    );
}

#[test]
fn merge_origins_cover_omitted_axes_protection_and_empty_outputs() {
    let current = MotionProgram::new(vec![
        track(
            Axis::Stroke,
            &[(0, 0.1), (100, 0.2), (200, 0.3), (300, 0.4)],
        ),
        track(Axis::Sway, &[(0, 0.9), (300, 0.1)]),
    ])
    .unwrap();
    let candidate = program(&[
        (0, 0.8),
        (50, 0.8),
        (100, 0.8),
        (150, 0.8),
        (200, 0.8),
        (250, 0.8),
        (300, 0.8),
    ]);
    let protected = [ProtectedRegion {
        axis: Some(Axis::Stroke),
        range: range(110, 190),
    }];
    let merged = merge_candidate_with_action_origins(
        &current,
        &candidate,
        &[Axis::Stroke],
        None,
        &protected,
    )
    .unwrap();
    check_origins(&current, &candidate, &merged);
    assert!(merged.action_origins[&Axis::Sway]
        .iter()
        .all(|l| l.input == CandidateMergeInput::Current));
    for at in [100, 200] {
        assert_eq!(
            merged.action_origins[&Axis::Stroke]
                .iter()
                .find(|l| l.project_time == time(at))
                .unwrap()
                .input,
            CandidateMergeInput::Current
        );
    }
    let empty = MotionProgram::new(vec![track(Axis::Stroke, &[])]).unwrap();
    let removed =
        merge_candidate_with_action_origins(&current, &empty, &[Axis::Stroke], None, &[]).unwrap();
    check_origins(&current, &empty, &removed);
    assert!(removed.action_origins[&Axis::Stroke].is_empty());
    let absent = merge_candidate_with_action_origins(
        &MotionProgram::default(),
        &candidate,
        &[Axis::Stroke],
        None,
        &protected,
    )
    .unwrap();
    assert!(absent.program.tracks().is_empty());
    assert!(absent.action_origins.is_empty());
}

#[test]
fn merge_origins_do_not_promote_gap_envelopes_into_actions() {
    let current = program(&[(0, 0.1), (100, 0.2), (125, 0.8), (200, 0.3), (300, 0.4)]);
    let candidate = MotionProgram::new(vec![MotionTrack::with_gaps(
        Axis::Stroke,
        vec![],
        vec![range(140, 180)],
    )
    .unwrap()])
    .unwrap();
    let merged = merge_candidate_with_action_origins(
        &current,
        &candidate,
        &[Axis::Stroke],
        Some(range(100, 200)),
        &[],
    )
    .unwrap();
    assert_eq!(
        merged.program.track(Axis::Stroke).unwrap().gaps(),
        &[range(140, 180)]
    );
    assert!(merged.action_origins[&Axis::Stroke]
        .iter()
        .all(|l| l.input == CandidateMergeInput::Current));
    assert!(!merged.action_origins[&Axis::Stroke]
        .iter()
        .any(|l| l.project_time == time(125)));
    check_origins(&current, &candidate, &merged);
}

#[test]
fn merge_origins_share_production_refusals() {
    let current = program(&[(0, 0.1), (100, 0.2)]);
    for axes in [vec![], vec![Axis::Stroke, Axis::Stroke], vec![Axis::Yaw]] {
        assert!(merge_candidate_with_action_origins(&current, &current, &axes, None, &[]).is_err());
    }
    assert!(merge_candidate_with_action_origins(
        &current,
        &program(&[(i64::MAX, 0.5)]),
        &[Axis::Stroke],
        None,
        &[]
    )
    .is_err());
}

#[test]
fn exact_merge_inputs_select_clock_frames_and_preserve_collision_refusal() {
    let clocks: Vec<_> = [1, 2]
        .into_iter()
        .map(|rate| {
            let placement = SourcePlacement {
                placement_id: SourcePlacementId::new(format!("merge-clock-{rate}")).unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: SourceVersionId::new("merge-source").unwrap(),
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(1, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: time(0),
                    rate_numerator: rate,
                    rate_denominator: 1,
                },
            };
            let plan =
                plan_placed_generation(&placement, &[0, 1, 2, 3, 4, 5, 6, 7], 1, 8, 1, 8).unwrap();
            SourceClockMapper::new(&placement.spec, Some(&plan), 8).unwrap()
        })
        .collect();
    for collision in [false, true] {
        let mut values = vec![
            (0, 0.1),
            (125_000_000, 0.2),
            (250_000_000, 0.3),
            (375_000_000, 0.4),
        ];
        if collision {
            values.push((500_000_000, 0.5));
        }
        let current = program(&values);
        let candidate = current.clone();
        let merged = merge_candidate_with_action_origins(
            &current,
            &candidate,
            &[Axis::Stroke],
            Some(range(125_000_000, 375_000_001)),
            &[],
        )
        .unwrap();
        let selected: Vec<_> = merged.action_origins[&Axis::Stroke]
            .iter()
            .map(|label| SourceClockSelection {
                project_time: label.project_time,
                carrier: match label.input {
                    CandidateMergeInput::Current => 0,
                    CandidateMergeInput::Candidate => 1,
                },
            })
            .collect();
        let mapped = map_track_to_selected_source_clocks(
            merged.program.track(Axis::Stroke).unwrap(),
            &clocks,
            &selected,
            32,
            2,
        );
        if collision {
            assert!(mapped.unwrap_err().to_string().contains("collides"));
            continue;
        }
        let mapped = mapped.unwrap();
        assert_eq!(mapped.carriers, vec![0, 0, 0, 1]);
        let boundary = &mapped.motion.mappings[1];
        assert_eq!(boundary.project_time, time(125_000_000));
        assert_eq!(boundary.source_time, ExactSeconds::new(1, 8).unwrap());
        assert_eq!(boundary.original_source_frame, Some(1));
        let interior = &mapped.motion.mappings[3];
        assert_eq!(interior.project_time, time(250_000_000));
        assert_eq!(interior.source_time, ExactSeconds::new(1, 2).unwrap());
        assert_eq!(interior.original_source_frame, Some(4));
    }
}
