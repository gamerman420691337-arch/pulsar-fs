use pulsar_core::*;

fn envelope(times: &[i64], level: f64) -> Vec<AudioEnvelopeSample> {
    times
        .iter()
        .map(|t| AudioEnvelopeSample::new(ProjectTime::from_nanos(*t), level).unwrap())
        .collect()
}
fn mapping() -> AudioMapping {
    AudioMapping::new(Axis::Yaw, 0.5, NormalizedPosition::new(0.25).unwrap()).unwrap()
}

#[test]
fn quarter_cycles_preserve_times_axis_range_endpoint_and_synthesized_evidence() {
    let times = [0, 31_250_000, 62_500_000, 93_750_000, 125_000_000];
    let program = synthesize_audio_oscillation(&envelope(&times, 1.0), &mapping()).unwrap();
    let actions = program.track(Axis::Yaw).unwrap().actions();
    for ((action, time), expected) in actions.iter().zip(times).zip([0.5, 0.75, 0.5, 0.25, 0.5]) {
        assert_eq!(action.time().as_nanos(), time);
        assert!((action.position().value() - expected).abs() < 1e-15);
        assert_eq!(action.evidence(), EvidenceKind::Synthesized);
    }
    assert_eq!(actions.len(), times.len());
}

#[test]
fn silent_quiet_and_zero_span_are_not_stretched() {
    let times = [0, 31_250_000, 62_500_000, 93_750_000, 125_000_000];
    for (level, expected) in [
        (0.0, [0.5, 0.5, 0.5, 0.5, 0.5]),
        (0.2, [0.5, 0.55, 0.5, 0.45, 0.5]),
    ] {
        let program = synthesize_audio_oscillation(&envelope(&times, level), &mapping()).unwrap();
        for (action, expected) in program
            .track(Axis::Yaw)
            .unwrap()
            .actions()
            .iter()
            .zip(expected)
        {
            assert!((action.position().value() - expected).abs() < 1e-15);
        }
    }
    let zero = AudioMapping::new(Axis::Stroke, 0.0, NormalizedPosition::new(0.3).unwrap()).unwrap();
    assert!(synthesize_audio_oscillation(&envelope(&times, 1.0), &zero)
        .unwrap()
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .all(|a| a.position().value() == 0.3));
}

#[test]
fn long_time_phase_uses_original_clock_and_retains_leading_gap() {
    let base = 86_399_000_000_000;
    let times = [
        base,
        base + 31_250_000,
        base + 62_500_000,
        base + 93_750_000,
        base + 125_000_000,
    ];
    let program = synthesize_audio_oscillation(&envelope(&times, 1.0), &mapping()).unwrap();
    let track = program.track(Axis::Yaw).unwrap();
    assert_eq!(
        track.gaps(),
        &[TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(base)).unwrap()]
    );
    for (action, expected) in track.actions().iter().zip([0.5, 0.75, 0.5, 0.25, 0.5]) {
        assert!((action.position().value() - expected).abs() < 1e-15);
    }
}

#[test]
fn invalid_or_sparse_sequences_refuse_instead_of_interpolating() {
    for times in [vec![], vec![0, 0], vec![1, 0], vec![0, 31_250_001]] {
        assert!(synthesize_audio_oscillation(&envelope(&times, 0.5), &mapping()).is_err());
    }
    assert!(AudioEnvelopeSample::new(ProjectTime::ZERO, f64::NAN).is_err());
    assert!(AudioMapping::new(Axis::Stroke, 1.0, NormalizedPosition::new(0.1).unwrap()).is_err());
}
