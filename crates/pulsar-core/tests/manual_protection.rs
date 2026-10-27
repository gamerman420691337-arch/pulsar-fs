use pulsar_core::*;
fn motion(positions: &[f64]) -> MotionProgram {
    MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        positions
            .iter()
            .enumerate()
            .map(|(i, p)| {
                MotionAction::new(
                    ProjectTime::from_nanos(i as i64 * 1_000_000_000),
                    NormalizedPosition::new(*p).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()])
    .unwrap()
}
#[test]
fn authored_stencil_protection_preserves_neighbors_and_allows_distant_regeneration() {
    let before = motion(&[0.4; 5]);
    let manual = motion(&[0.4, 0.4, 0.9, 0.4, 0.4]);
    let changed = changed_motion_regions(&before, &manual).unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].0, Axis::Stroke);
    assert!(changed[0]
        .1
        .contains(ProjectTime::from_nanos(2_000_000_000)));
    let protected = protect_changed_motion(&[], &changed, 1024).unwrap();
    let incoming = motion(&[0.1; 5]);
    let merged = merge_candidate_preserving_protection(
        &manual,
        &incoming,
        &[Axis::Stroke],
        None,
        &protected,
    )
    .unwrap();
    let values: Vec<_> = merged
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .map(|a| a.position().value())
        .collect();
    assert_eq!(values, vec![0.1, 0.4, 0.9, 0.4, 0.1]);
    ensure_protected_segments_unchanged(&manual, &merged, &protected).unwrap();
    assert!(ensure_protected_segments_unchanged(&manual, &incoming, &protected).is_err());
    assert!(changed_motion_regions(&manual, &manual).unwrap().is_empty());
}
#[test]
fn protection_union_retains_global_policy_and_refuses_overflow_without_dropping_ranges() {
    let range = |start, end| {
        TimeRange::new(ProjectTime::from_nanos(start), ProjectTime::from_nanos(end)).unwrap()
    };
    let existing = vec![
        ProtectedRegion {
            axis: None,
            range: range(0, 1),
        },
        ProtectedRegion {
            axis: Some(Axis::Stroke),
            range: range(0, 2),
        },
    ];
    let changed = vec![(Axis::Stroke, range(2, 3)), (Axis::Stroke, range(1, 2))];
    let result = protect_changed_motion(&existing, &changed, 2).unwrap();
    assert_eq!(result[0], existing[0]);
    assert_eq!(result[1].range, range(0, 3));
    assert!(protect_changed_motion(&existing, &changed, 1).is_err());
    let fragmented: Vec<_> = (0..1025)
        .map(|i| (Axis::Stroke, range(i * 2, i * 2 + 1)))
        .collect();
    assert!(protect_changed_motion(&[], &fragmented, 1024).is_err());
}
#[test]
fn deleted_support_is_an_authored_footprint_and_overflowing_endpoints_refuse() {
    let before = motion(&[0.4; 5]);
    let after = MotionProgram::new(vec![]).unwrap();
    let changed = changed_motion_regions(&before, &after).unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].1.start(), ProjectTime::ZERO);
    assert_eq!(changed[0].1.end(), ProjectTime::from_nanos(4_000_000_001));
    let extreme = MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        vec![MotionAction::new(
            ProjectTime::from_nanos(i64::MAX),
            NormalizedPosition::new(0.5).unwrap(),
            EvidenceKind::Synthesized,
        )
        .unwrap()],
    )
    .unwrap()])
    .unwrap();
    assert!(changed_motion_regions(&before, &extreme).is_err());
}
