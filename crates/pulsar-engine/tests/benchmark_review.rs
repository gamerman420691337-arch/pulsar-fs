use pulsar_engine::benchmark::benchmark_review_flags;
use pulsar_protocol::*;

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(ProjectTime::from_nanos(start), ProjectTime::from_nanos(end)).unwrap()
}
fn flag(axis: Option<Axis>, start: i64, end: i64) -> ReviewFlag {
    ReviewFlag {
        axis,
        range: range(start, end),
        reason: ReviewReason::new("uncertain-motion").unwrap(),
        evidence: EvidenceKind::Inferred,
        confidence: None,
    }
}

#[test]
fn projects_only_global_and_evaluated_axis_without_losing_exact_times() {
    let flags = benchmark_review_flags(
        &[
            flag(None, 0, 10),
            flag(Some(Axis::Stroke), 10, 20),
            flag(Some(Axis::Yaw), 20, 30),
        ],
        Axis::Stroke,
        range(0, 30),
    )
    .unwrap();
    assert_eq!(flags.as_array().unwrap().len(), 2);
    assert_eq!(flags[1]["start_ns"], "10");
    assert_eq!(flags[1]["reasons"][0], "uncertain-motion");
}

#[test]
fn rejects_out_of_window_warnings_instead_of_silently_clipping() {
    assert!(benchmark_review_flags(&[flag(None, 0, 31)], Axis::Stroke, range(0, 30)).is_err());
}
#[test]
fn explicitly_windowed_review_preserves_reasons_and_does_not_modify_original_evidence() {
    use pulsar_engine::benchmark::{benchmark_review_flags_for_window, EvaluationWindowPolicy};
    let review = vec![
        flag(None, 0, 50),
        flag(Some(Axis::Stroke), 60, 70),
        flag(Some(Axis::Yaw), 0, 50),
    ];
    let before = serde_json::to_vec(&review).unwrap();
    let selected = benchmark_review_flags_for_window(
        &review,
        Axis::Stroke,
        range(10, 30),
        EvaluationWindowPolicy::RecordedActions,
    )
    .unwrap();
    assert_eq!(selected.as_array().unwrap().len(), 1);
    assert_eq!(selected[0]["start_ns"], "10");
    assert_eq!(selected[0]["end_ns"], "30");
    assert_eq!(selected[0]["reasons"][0], "uncertain-motion");
    assert_eq!(serde_json::to_vec(&review).unwrap(), before);
}
