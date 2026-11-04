use pulsar_core::*;

#[test]
fn streamed_triangle_matches_independent_samples_and_generated_track() {
    let pattern = parse_pattern_prompt("pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8").unwrap();
    let mut samples = pattern.sampled_actions().unwrap();
    assert_eq!(samples.len(), 17);
    let expected = [
        0.5, 0.55, 0.6, 0.55, 0.5, 0.45, 0.4, 0.45, 0.5, 0.55, 0.6, 0.55, 0.5, 0.45, 0.4, 0.45, 0.5,
    ];
    let generated = synthesize_pattern(&pattern).unwrap();
    for (index, expected) in expected.into_iter().enumerate() {
        let action = samples.next().unwrap().unwrap();
        assert_eq!(action.time().as_nanos(), index as i64 * 125_000_000);
        assert!((action.position().value() - expected).abs() < 1e-12);
        assert_eq!(action.evidence(), EvidenceKind::Synthesized);
        assert_eq!(&action, &generated.tracks()[0].actions()[index]);
        assert_eq!(samples.len(), 16 - index);
    }
    assert!(samples.next().is_none());
}

#[test]
fn exact_non_grid_endpoint_and_hold_are_retained() {
    let pattern = parse_pattern_prompt(
        "pattern=pulse axis=yaw duration=1.3s frequency=1hz amplitude=0.2 offset=0.5 sample_hz=8",
    )
    .unwrap();
    let samples = pattern
        .sampled_actions()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(samples.len(), 12);
    assert_eq!(samples[10].time().as_nanos(), 1_250_000_000);
    assert_eq!(samples[11].time().as_nanos(), 1_300_000_000);
    for (index, action) in samples.iter().enumerate() {
        let high = !(4..8).contains(&index);
        assert!((action.position().value() - if high { 0.7 } else { 0.3 }).abs() < 1e-12);
    }
    let hold = parse_pattern_prompt(
        "pattern=hold axis=pitch duration=86400s frequency=0hz amplitude=0 offset=0.4",
    )
    .unwrap();
    let samples = hold
        .sampled_actions()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].time(), ProjectTime::ZERO);
    assert_eq!(samples[1].time().as_nanos(), MAX_SYNTHESIS_DURATION_NS);
    assert!(samples
        .iter()
        .all(|a| a.position().value() == 0.4 && a.evidence() == EvidenceKind::Synthesized));
}
