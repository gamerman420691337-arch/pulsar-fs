use pulsar_core::*;
fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(ProjectTime::from_nanos(start), ProjectTime::from_nanos(end)).unwrap()
}
#[test]
fn interval_preserves_irregular_origin_stride_and_unavailable_edges() {
    let p = plan_generation_interval(
        &[10, 11, 13, 16, 20],
        1,
        2,
        2,
        None,
        range(1_000_000_000, 5_000_000_000),
        4,
    )
    .unwrap();
    assert_eq!(
        p.samples,
        vec![GenerationIntervalSample {
            source_frame_index: 2,
            project_time: ProjectTime::from_nanos(1_500_000_000)
        }]
    );
    assert_eq!(
        p.uncovered_boundaries,
        vec![
            range(1_000_000_000, 1_500_000_000),
            range(1_500_000_001, 5_000_000_000)
        ]
    );
    assert_eq!(p.project_order, vec![0]);
}
#[test]
fn reverse_placement_keeps_decode_order_and_selects_project_half_open_coverage() {
    let placement = SourcePlacement {
        placement_id: SourcePlacementId::new("reverse").unwrap(),
        created_revision: RevisionId::new(0),
        spec: SourcePlacementSpec {
            source_version: SourceVersionId::new("source").unwrap(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(2, 1).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(4_000_000_000),
            rate_numerator: -1,
            rate_denominator: 1,
        },
    };
    let pts = [100, 101, 102, 103, 104];
    let placed = plan_placed_generation(&placement, &pts, 1, 2, 1, 5).unwrap();
    let p = plan_generation_interval(
        &pts,
        1,
        2,
        1,
        Some(&placed),
        range(2_700_000_000, 3_600_000_000),
        2,
    )
    .unwrap();
    assert_eq!(
        p.samples
            .iter()
            .map(|s| s.source_frame_index)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        p.samples
            .iter()
            .map(|s| s.project_time.as_nanos())
            .collect::<Vec<_>>(),
        vec![3_500_000_000, 3_000_000_000]
    );
    assert_eq!(p.project_order, vec![1, 0]);
    assert_eq!(
        p.uncovered_boundaries,
        vec![
            range(2_700_000_000, 3_000_000_000),
            range(3_500_000_001, 3_600_000_000)
        ]
    );
}
#[test]
fn interval_refuses_empty_lattice_overflow_invalid_source_and_admission() {
    assert!(plan_generation_interval(&[0, 1], 1, 1, 2, None, range(1, 2), 2).is_err());
    assert!(
        plan_generation_interval(&[0, 1, 2], 1, 1, 1, None, range(0, 3_000_000_000), 2).is_err()
    );
    assert!(plan_generation_interval(&[0, 0], 1, 1, 1, None, range(0, 1), 2).is_err());
    assert!(
        plan_generation_interval(&[0, i64::MAX], u32::MAX, 1, 1, None, range(0, i64::MAX), 2)
            .is_err()
    );
    assert!(plan_generation_interval(&[0, 1], 1, 1, 1, None, range(-1, 1), 2).is_err());
}
