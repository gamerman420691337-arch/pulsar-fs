use pulsar_engine::benchmark::motion_candidate;
use pulsar_protocol::*;
use sha2::{Digest, Sha256};

fn candidate(points: &[(i64, f64)], gaps: Vec<TimeRange>) -> serde_json::Value {
    let actions = points
        .iter()
        .map(|(t, p)| {
            MotionAction::new(
                ProjectTime::from_nanos(*t),
                NormalizedPosition::new(*p).unwrap(),
                EvidenceKind::Inferred,
            )
            .unwrap()
        })
        .collect();
    let program = MotionProgram::new(vec![
        MotionTrack::with_gaps(Axis::Stroke, actions, gaps).unwrap()
    ])
    .unwrap();
    let bytes = serde_json::to_vec(&program).unwrap();
    motion_candidate(
        &bytes,
        &format!("{:x}", Sha256::digest(&bytes)),
        "clip",
        "target",
        TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(i64::MAX)).unwrap(),
    )
    .unwrap()
}

#[test]
fn preserves_exact_nanoseconds_plateaus_and_shallow_travel() {
    let result = candidate(
        &[
            (0, 0.4),
            (101, 0.42),
            (203, 0.44),
            (307, 0.44),
            (409, 0.46),
            (511, 0.43),
        ],
        vec![],
    );
    let strokes = result["candidate"]["strokes"].as_array().unwrap();
    assert_eq!(strokes.len(), 3);
    assert_eq!(strokes[0]["end_ns"], "203");
    assert_eq!(strokes[1]["start_ns"], "307");
    assert_eq!(strokes[0]["end_position_ppm"], 440000);
}

#[test]
fn missing_intervals_are_not_interpolated() {
    let gap = TimeRange::new(ProjectTime::from_nanos(100), ProjectTime::from_nanos(200)).unwrap();
    let result = candidate(&[(0, 0.4), (200, 0.5), (300, 0.6)], vec![gap]);
    assert_eq!(result["receipt"]["skipped_gap_edges"], 1);
    assert_eq!(result["candidate"]["strokes"][0]["start_ns"], "200");
}

#[test]
fn retains_times_above_javascript_integer_precision() {
    let result = candidate(&[(9007199254740993, 0.4), (9007199254741003, 0.5)], vec![]);
    assert_eq!(
        result["candidate"]["strokes"][0]["start_ns"],
        "9007199254740993"
    );
}
#[test]
fn explicitly_windowed_actions_keep_original_hash_times_and_gap_boundaries() {
    use pulsar_engine::benchmark::{motion_candidate_for_window, EvaluationWindowPolicy};
    let actions = [(0, 0.1), (100, 0.3), (300, 0.6), (400, 0.7), (600, 0.4)]
        .into_iter()
        .map(|(t, p)| {
            MotionAction::new(
                ProjectTime::from_nanos(t),
                NormalizedPosition::new(p).unwrap(),
                EvidenceKind::Observed,
            )
            .unwrap()
        })
        .collect();
    let gap = TimeRange::new(ProjectTime::from_nanos(150), ProjectTime::from_nanos(250)).unwrap();
    let program = MotionProgram::new(vec![MotionTrack::with_gaps(
        Axis::Stroke,
        actions,
        vec![gap],
    )
    .unwrap()])
    .unwrap();
    let bytes = serde_json::to_vec(&program).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let window = TimeRange::new(ProjectTime::from_nanos(50), ProjectTime::from_nanos(500)).unwrap();
    assert!(motion_candidate(&bytes, &hash, "clip", "target", window).is_err());
    let result = motion_candidate_for_window(
        &bytes,
        &hash,
        "clip",
        "target",
        window,
        EvaluationWindowPolicy::RecordedActions,
    )
    .unwrap();
    assert_eq!(result["receipt"]["raw_sha256"], hash);
    assert_eq!(result["receipt"]["excluded_actions"], 2);
    assert_eq!(result["receipt"]["skipped_gap_edges"], 1);
    assert_eq!(result["candidate"]["strokes"][0]["start_ns"], "300");
    assert_eq!(result["candidate"]["strokes"][0]["end_ns"], "400");
    assert_eq!(result["candidate"]["strokes"].as_array().unwrap().len(), 1);
}
