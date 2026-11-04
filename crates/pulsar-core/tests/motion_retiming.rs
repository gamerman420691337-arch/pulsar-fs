use pulsar_core::*;
fn spec(rate: i32) -> SourcePlacementSpec {
    SourcePlacementSpec {
        source_version: SourceVersionId::new("source").unwrap(),
        source_start: SourceTimestamp::new(0, 1).unwrap(),
        source_end: SourceTimestamp::new(10, 1).unwrap(),
        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
        project_anchor: ProjectTime::ZERO,
        rate_numerator: rate,
        rate_denominator: 1,
    }
}
fn program(times: &[i64]) -> MotionProgram {
    MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        times
            .iter()
            .enumerate()
            .map(|(i, &time)| {
                MotionAction::new(
                    ProjectTime::from_nanos(time),
                    NormalizedPosition::new(i as f64 / 10.0).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()])
    .unwrap()
}
#[test]
fn forward_reverse_and_selected_axes_preserve_positions_evidence_and_input() {
    let input = program(&[0, 1_000_000_000, 2_000_000_000]);
    let before = input.clone();
    let mut to = spec(2);
    to.project_anchor = ProjectTime::from_nanos(3_000_000_000);
    let output = retime_motion_between_placements(&input, &spec(1), &to, &[Axis::Stroke]).unwrap();
    assert_eq!(
        output
            .track(Axis::Stroke)
            .unwrap()
            .actions()
            .iter()
            .map(|a| a.time().as_nanos())
            .collect::<Vec<_>>(),
        [3_000_000_000, 3_500_000_000, 4_000_000_000]
    );
    to.rate_numerator = -1;
    let output = retime_motion_between_placements(&input, &spec(1), &to, &[Axis::Stroke]).unwrap();
    for (a, b) in input
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .zip(output.track(Axis::Stroke).unwrap().actions().iter().rev())
    {
        assert_eq!(a.position(), b.position());
        assert_eq!(a.evidence(), b.evidence());
    }
    assert_eq!(input, before);
    let multi = input
        .replaced_track(
            MotionTrack::new(
                Axis::Yaw,
                vec![MotionAction::new(
                    ProjectTime::ZERO,
                    NormalizedPosition::new(0.5).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap()],
            )
            .unwrap(),
        )
        .unwrap();
    let out = retime_motion_between_placements(&multi, &spec(1), &to, &[Axis::Stroke]).unwrap();
    assert_eq!(out.track(Axis::Yaw), multi.track(Axis::Yaw));
}
#[test]
fn fractional_floor_is_explicit_and_collisions_or_trim_loss_refuse() {
    let input = program(&[0, 1_000_000_000]);
    let out =
        retime_motion_between_placements(&input, &spec(1), &spec(3), &[Axis::Stroke]).unwrap();
    assert_eq!(
        out.track(Axis::Stroke).unwrap().actions()[1]
            .time()
            .as_nanos(),
        333_333_333
    );
    assert!(retime_motion_between_placements(
        &program(&[0, 1]),
        &spec(1),
        &spec(3),
        &[Axis::Stroke]
    )
    .is_err());
    let mut to = spec(1);
    to.source_end = SourceTimestamp::new(1, 1).unwrap();
    assert!(retime_motion_between_placements(&input, &spec(1), &to, &[Axis::Stroke]).is_err());
    assert!(
        retime_motion_between_placements(&input, &spec(1), &spec(-1), &[Axis::Stroke]).is_err()
    );
}
#[test]
fn unknown_intervals_remain_unknown_with_outward_bounds() {
    let track = MotionTrack::with_gaps(
        Axis::Stroke,
        vec![],
        vec![TimeRange::new(
            ProjectTime::from_nanos(1_000_000_000),
            ProjectTime::from_nanos(2_000_000_000),
        )
        .unwrap()],
    )
    .unwrap();
    let input = MotionProgram::new(vec![track]).unwrap();
    let output =
        retime_motion_between_placements(&input, &spec(1), &spec(3), &[Axis::Stroke]).unwrap();
    let gap = output.track(Axis::Stroke).unwrap().gaps()[0];
    assert_eq!(
        (gap.start().as_nanos(), gap.end().as_nanos()),
        (333_333_333, 666_666_667)
    );
    let mut to = spec(-1);
    to.project_anchor = ProjectTime::from_nanos(4_000_000_000);
    let output = retime_motion_between_placements(&input, &spec(1), &to, &[Axis::Stroke]).unwrap();
    let gap = output.track(Axis::Stroke).unwrap().gaps()[0];
    assert_eq!(
        (gap.start().as_nanos(), gap.end().as_nanos()),
        (2_000_000_000, 3_000_000_001)
    );
    assert!(output.has_unresolved_gaps());
}
#[test]
fn unrelated_sources_absent_axes_and_protected_segments_refuse() {
    let input = program(&[0, 1_000_000_000]);
    let mut to = spec(2);
    to.source_version = SourceVersionId::new("foreign").unwrap();
    assert!(retime_motion_between_placements(&input, &spec(1), &to, &[Axis::Stroke]).is_err());
    assert!(retime_motion_between_placements(&input, &spec(1), &spec(2), &[Axis::Yaw]).is_err());
    assert!(retime_motion_between_placements(
        &input,
        &spec(1),
        &spec(2),
        &[Axis::Stroke, Axis::Stroke]
    )
    .is_err());
    let output =
        retime_motion_between_placements(&input, &spec(1), &spec(2), &[Axis::Stroke]).unwrap();
    assert!(ensure_protected_segments_unchanged(
        &input,
        &output,
        &[ProtectedRegion {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(ProjectTime::from_nanos(100), ProjectTime::from_nanos(200))
                .unwrap()
        }]
    )
    .is_err());
}
