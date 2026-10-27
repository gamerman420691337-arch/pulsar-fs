use pulsar_core::*;

fn profile() -> DynamicsProfile {
    DynamicsProfile {
        axes: vec![AxisDynamics {
            axis: Axis::Stroke,
            unit: "mm".into(),
            minimum: 10.0,
            maximum: 90.0,
            max_velocity: 1000.0,
            max_acceleration: 10000.0,
            max_jerk: 100000.0,
        }],
        coupled: vec![],
        minimum_gain: 0.001,
    }
}
fn program(axis: Axis, values: &[(i64, f64)]) -> MotionProgram {
    MotionProgram::new(vec![MotionTrack::new(
        axis,
        values
            .iter()
            .map(|(time, pos)| {
                MotionAction::new(
                    ProjectTime::from_nanos(*time),
                    NormalizedPosition::new(*pos).unwrap(),
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
fn specification_shallow_mapping_preserves_master_timestamps_and_evidence() {
    let master = program(Axis::Stroke, &[(0, 0.2), (1_000_000_000, 0.4)]);
    let original = master.clone();
    let adapted = adapt_device_motion(&master, &profile()).unwrap();
    assert_eq!(master, original);
    assert_eq!(adapted.gain, 1.0);
    assert_eq!(adapted.tracks[0].actions[0].position, 26.0);
    assert_eq!(adapted.tracks[0].actions[1].position, 42.0);
    assert_eq!(
        adapted.tracks[0].actions[1].time,
        ProjectTime::from_nanos(1_000_000_000)
    );
    assert_eq!(
        adapted.tracks[0].actions[1].evidence,
        EvidenceKind::Observed
    );
    assert!(adapted.requires_stopped_entry);
}

#[test]
fn analytic_limits_compress_globally_and_never_retime_or_expand() {
    let master = program(
        Axis::Stroke,
        &[(0, 0.1), (1_000_000_000, 0.9), (2_000_000_000, 0.6)],
    );
    for (v, a, j) in [
        (60.0, 10000.0, 100000.0),
        (1000.0, 120.0, 100000.0),
        (1000.0, 10000.0, 960.0),
    ] {
        let mut p = profile();
        p.axes[0].max_velocity = v;
        p.axes[0].max_acceleration = a;
        p.axes[0].max_jerk = j;
        let result = adapt_device_motion(&master, &p).unwrap();
        assert!(result.amplitude_reduced);
        let track = &result.tracks[0];
        assert!(
            track.bounds.velocity <= v && track.bounds.acceleration <= a && track.bounds.jerk <= j
        );
        let ratio = (track.actions[2].position - track.actions[1].position)
            / (track.actions[1].position - track.actions[0].position);
        assert!((ratio - (-0.3 / 0.8)).abs() < 1e-12);
        for (input, output) in master.tracks()[0].actions().iter().zip(&track.actions) {
            assert_eq!(input.time(), output.time);
            assert!((10.0..=90.0).contains(&output.position));
        }
        // Independent derivative oracle, over the complete segment.
        let d = track.actions[1].position - track.actions[0].position;
        for step in 0..=10000 {
            let u = step as f64 / 10000.0;
            let velocity = d * 30.0 * u * u * (1.0 - u) * (1.0 - u);
            let acceleration = d * (60.0 * u - 180.0 * u * u + 120.0 * u * u * u);
            let jerk = d * (60.0 - 360.0 * u + 360.0 * u * u);
            assert!(velocity.abs() <= v && acceleration.abs() <= a && jerk.abs() <= j);
        }
    }
}

#[test]
fn coupled_constraints_bound_asynchronous_axes_between_actions() {
    let stroke = program(Axis::Stroke, &[(0, 0.0), (1_000_000_000, 1.0)]);
    let sway = program(Axis::Sway, &[(500_000_000, 1.0), (2_000_000_000, 0.0)]);
    let master =
        MotionProgram::new(vec![stroke.tracks()[0].clone(), sway.tracks()[0].clone()]).unwrap();
    let mut p = profile();
    let mut other = p.axes[0].clone();
    other.axis = Axis::Sway;
    p.axes.push(other);
    p.coupled.push(CoupledDynamics {
        coefficients: vec![(Axis::Stroke, 1.0), (Axis::Sway, -2.0)],
        max_displacement: 30.0,
        max_velocity: 100.0,
        max_acceleration: 1000.0,
        max_jerk: 10000.0,
    });
    let output = adapt_device_motion(&master, &p).unwrap();
    assert!(output.gain < 0.25);
    let bounds = output.coupled_bounds[0];
    assert!(
        bounds.displacement <= 30.0
            && bounds.velocity <= 100.0
            && bounds.acceleration <= 1000.0
            && bounds.jerk <= 10000.0
    );
    for step in 0..=2000 {
        let t = step as f64 / 1000.0;
        let s = &output.tracks[0].actions;
        let w = &output.tracks[1].actions;
        let x = quintic_position(s[0].position, s[1].position, t.min(1.0)).unwrap();
        let y = quintic_position(
            w[0].position,
            w[1].position,
            ((t - 0.5) / 1.5).clamp(0.0, 1.0),
        )
        .unwrap();
        assert!((x - 50.0).abs() + 2.0 * (y - 50.0).abs() <= 30.0);
    }
}

#[test]
fn unavailable_axes_gaps_and_infeasible_compression_fail_explicitly() {
    let mut p = profile();
    p.minimum_gain = 1.0;
    p.axes[0].max_jerk = 1.0;
    assert!(adapt_device_motion(
        &program(Axis::Stroke, &[(0, 0.0), (1_000_000_000, 1.0)]),
        &p
    )
    .is_err());
    assert!(adapt_device_motion(&program(Axis::Yaw, &[(0, 0.5)]), &profile()).is_err());
    let master = MotionProgram::new(vec![MotionTrack::with_gaps(
        Axis::Stroke,
        vec![],
        vec![TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1)).unwrap()],
    )
    .unwrap()])
    .unwrap();
    assert!(adapt_device_motion(&master, &profile()).is_err());
    let mut p = profile();
    p.axes.push(AxisDynamics {
        axis: Axis::Sway,
        ..p.axes[0].clone()
    });
    p.coupled.push(CoupledDynamics {
        coefficients: vec![(Axis::Sway, 1.0)],
        max_displacement: 1.0,
        max_velocity: 1.0,
        max_acceleration: 1.0,
        max_jerk: 1.0,
    });
    assert!(adapt_device_motion(&program(Axis::Stroke, &[(0, 0.5)]), &p).is_err());
}

#[test]
fn invalid_wire_profiles_and_extreme_arithmetic_do_not_bypass_validation() {
    let input = program(Axis::Stroke, &[(0, 0.0), (1, 1.0)]);
    let mut p = profile();
    p.minimum_gain = 0.0;
    p.axes[0].minimum = -f64::MAX;
    p.axes[0].maximum = f64::MAX;
    assert!(adapt_device_motion(&input, &p).is_err());
    let mut p = profile();
    p.axes[0].max_velocity = f64::NAN;
    assert!(p.validate().is_err());
    let mut p = profile();
    p.axes.push(p.axes[0].clone());
    assert!(p.validate().is_err());
    assert!(quintic_position(0.0, 1.0, f64::NAN).is_err());
    assert_eq!(quintic_position(10.0, 90.0, 0.5).unwrap(), 50.0);
    assert_eq!(quintic_position(10.0, 90.0, 0.0).unwrap(), 10.0);
    assert_eq!(quintic_position(10.0, 90.0, 1.0).unwrap(), 90.0);
}
#[test]
fn feasible_analytic_gain_must_also_retain_representable_physical_motion() {
    let input = program(Axis::Stroke, &[(0, 0.2), (1_000_000_000, 0.4)]);
    let mut p = profile();
    p.minimum_gain = 0.0;
    p.axes[0].minimum = 1e15;
    p.axes[0].maximum = 1e15 + 100.0;
    p.axes[0].max_jerk = 1e-9;
    assert!(adapt_device_motion(&input, &p)
        .unwrap_err()
        .to_string()
        .contains("coordinate precision"));
}
