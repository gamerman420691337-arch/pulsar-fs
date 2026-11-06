use pulsar_core::*;
fn program(points: &[(i64, f64)]) -> MotionProgram {
    MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        points
            .iter()
            .map(|(t, p)| {
                MotionAction::new(
                    ProjectTime::from_nanos(t * 1_000_000),
                    NormalizedPosition::new(p / 100.0).unwrap(),
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
fn independent_discrete_oracle_and_endpoint_times() {
    let input = program(&[(0, 0.0), (10, 1.0), (20, 100.0), (40, 100.0), (100, 50.0)]);
    let result =
        repair_standard_script(&input, ScriptRepairOptions::new(450.0, 20, 2).unwrap()).unwrap();
    assert_eq!(result.removed_jitters, 1);
    assert_eq!(result.clamped_positions, 3);
    let actions = result.program.track(Axis::Stroke).unwrap().actions();
    for (action, (time, position)) in
        actions
            .iter()
            .zip([(0, 0.0), (20, 9.0), (40, 18.0), (100, 45.0)])
    {
        assert_eq!(action.time().as_nanos(), time * 1_000_000);
        assert_eq!(action.position().value(), position / 100.0);
    }
    assert_eq!(actions.len(), 4);
    assert!(scan_segment_speeds(&result.program, 450.0, 1024)
        .unwrap()
        .violations
        .is_empty());
}
#[test]
fn no_op_and_fractional_speed_floor_preserve_neutral_grid() {
    let input = program(&[(0, 50.0), (1, 100.0), (2, 0.0)]);
    let result =
        repair_standard_script(&input, ScriptRepairOptions::new(450.0, 0, 0).unwrap()).unwrap();
    assert_eq!(result.clamped_positions, 2);
    assert!(result
        .program
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .all(|a| a.position().value() == 0.5));
    let input = program(&[(0, 0.0), (20, 9.0), (40, 18.0)]);
    let result =
        repair_standard_script(&input, ScriptRepairOptions::new(450.0, 0, 0).unwrap()).unwrap();
    assert_eq!(result.program, input);
    assert_eq!(result.clamped_positions, 0);
    assert_eq!(
        repair_standard_script(
            &program(&[(100, 30.0)]),
            ScriptRepairOptions::new(450.0, 20, 2).unwrap()
        )
        .unwrap()
        .program,
        program(&[(100, 30.0)])
    );
}
#[test]
fn jitter_uses_last_retained_point_and_keeps_exact_window_boundary() {
    let input = program(&[(0, 0.0), (5, 1.0), (10, 2.0), (20, 2.0), (21, 2.0)]);
    let result =
        repair_standard_script(&input, ScriptRepairOptions::new(1000.0, 20, 2).unwrap()).unwrap();
    assert_eq!(result.removed_jitters, 2);
    assert_eq!(
        result
            .program
            .track(Axis::Stroke)
            .unwrap()
            .actions()
            .iter()
            .map(|a| a.time().as_nanos() / 1_000_000)
            .collect::<Vec<_>>(),
        vec![0, 20, 21]
    );
}
#[test]
fn invalid_policies_and_nonstandard_input_refuse() {
    for speed in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e12] {
        assert!(ScriptRepairOptions::new(speed, 20, 2).is_err());
    }
    for (window, threshold) in [(-1, 2), (i64::MAX, 2), (20, -1), (20, 101)] {
        assert!(ScriptRepairOptions::new(450.0, window, threshold).is_err());
    }
    let options = ScriptRepairOptions::new(450.0, 20, 2).unwrap();
    assert!(repair_standard_script(&program(&[(0, 0.1), (10, 1.0)]), options).is_err());
    let action = MotionAction::new(
        ProjectTime::from_nanos(1),
        NormalizedPosition::new(0.5).unwrap(),
        EvidenceKind::Synthesized,
    )
    .unwrap();
    assert!(repair_standard_script(
        &MotionProgram::new(vec![MotionTrack::new(Axis::Stroke, vec![action]).unwrap()]).unwrap(),
        options
    )
    .is_err());
}
