use pulsar_core::perception::{
    CanonicalDetection, TargetTracker, TargetTrackerCheckpoint, TrackerConfig,
};
use pulsar_core::{
    CoordinateBox, CoordinateSpace, EntityTrackId, FrameContext, FrameId, SourcePlacementId,
    SourceVersionId, TransformId,
};

fn namespace(value: &str) -> EntityTrackId {
    EntityTrackId::new(value).unwrap()
}
fn context(frame: u64, seek: u64) -> FrameContext {
    FrameContext {
        source_version: SourceVersionId::new("authored-source").unwrap(),
        source_placement: SourcePlacementId::new("authored-placement").unwrap(),
        frame: FrameId::new(frame),
        transform: TransformId::new("identity").unwrap(),
        seek_generation: seek,
        request_generation: frame,
    }
}
fn detection(x: f64, class: u32) -> CanonicalDetection {
    CanonicalDetection::new(
        CoordinateBox::new(CoordinateSpace::SourcePixels, x, 20.0, x + 10.0, 35.0).unwrap(),
        class,
        0.9012345678901234,
    )
    .unwrap()
}
fn restore(
    value: serde_json::Value,
    expected: Option<&FrameContext>,
) -> Result<TargetTracker, String> {
    let saved: TargetTrackerCheckpoint =
        serde_json::from_value(value).map_err(|e| e.to_string())?;
    TargetTracker::from_checkpoint(
        &saved,
        &namespace("original"),
        namespace("original"),
        expected,
        TrackerConfig::default(),
        Some([100, 80]),
    )
    .map_err(|e| e.to_string())
}

#[test]
fn serialized_resume_matches_uninterrupted_tracking_across_misses_ambiguity_cut_and_seek() {
    let config = TrackerConfig::default();
    let mut uninterrupted = TargetTracker::with_namespace(namespace("original"), config).unwrap();
    let mut resumed = TargetTracker::with_namespace(namespace("original"), config).unwrap();
    let steps = [
        (0, 0, 100, false, vec![detection(20.123456789012345, 0)]),
        (1, 0, 100, false, vec![detection(21.123456789012345, 0)]),
        (3, 0, 100, false, vec![]),
        (4, 0, 100, false, vec![detection(24.123456789012345, 0)]),
        (
            5,
            0,
            100,
            false,
            vec![detection(24.5, 0), detection(25.0, 0)],
        ),
        (6, 0, 100, false, vec![]),
        (7, 0, 100, true, vec![detection(35.0, 1)]),
        (8, 0, 120, false, vec![detection(36.0, 1)]),
        (0, 1, 120, false, vec![detection(10.0, 2)]),
        (1, 1, 120, false, vec![]),
        (6, 1, 120, false, vec![]),
        (7, 1, 120, false, vec![detection(11.0, 2)]),
    ];
    for (frame, seek, width, cut, detections) in steps {
        let ctx = context(frame, seek);
        let a = uninterrupted
            .update(ctx.clone(), width, 80, &detections, cut)
            .unwrap();
        let b = resumed
            .update(ctx.clone(), width, 80, &detections, cut)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&a.observations).unwrap(),
            serde_json::to_value(&b.observations).unwrap()
        );
        assert_eq!(a.reviews, b.reviews);
        for observation in &a.observations {
            let id = observation.track().unwrap();
            assert_eq!(a.class_for(id), b.class_for(id));
            assert_eq!(a.association_ambiguous(id), b.association_ambiguous(id));
        }
        let bytes = serde_json::to_vec(&resumed.checkpoint()).unwrap();
        let saved: TargetTrackerCheckpoint = serde_json::from_slice(&bytes).unwrap();
        resumed = TargetTracker::from_checkpoint(
            &saved,
            &namespace("original"),
            namespace("original"),
            Some(&ctx),
            config,
            Some([width, 80]),
        )
        .unwrap();
        // The complete snapshot uses exact float bits, not decimal tolerance.
        assert_eq!(
            serde_json::to_vec(&uninterrupted.checkpoint()).unwrap(),
            serde_json::to_vec(&resumed.checkpoint()).unwrap()
        );
    }
}

#[test]
fn fresh_attempt_rebinds_allocator_without_reusing_origin_identities() {
    let config = TrackerConfig::default();
    let origin = namespace("original");
    let mut tracker = TargetTracker::with_namespace(origin.clone(), config).unwrap();
    let first = tracker
        .update(context(0, 0), 100, 80, &[detection(20.0, 0)], false)
        .unwrap();
    let old = first.observations[0].track().unwrap().clone();
    tracker
        .update(context(1, 0), 100, 80, &[detection(21.0, 0)], false)
        .unwrap();
    let saved = tracker.checkpoint();
    let destination = namespace("fresh");
    let rebound = saved.rebind_identity(&old, &destination).unwrap();
    assert_eq!(rebound.as_str(), "fresh.0");
    let mut resumed = TargetTracker::from_checkpoint(
        &saved,
        &origin,
        destination.clone(),
        Some(&context(1, 0)),
        config,
        Some([100, 80]),
    )
    .unwrap();
    let continued = resumed
        .update(context(2, 0), 100, 80, &[detection(22.0, 0)], false)
        .unwrap();
    assert_eq!(continued.observations[0].track(), Some(&rebound));
    let fresh = resumed
        .update(
            context(3, 0),
            100,
            80,
            &[detection(22.0, 0), detection(60.0, 1)],
            false,
        )
        .unwrap();
    assert!(fresh
        .observations
        .iter()
        .any(|o| o.track().unwrap().as_str() == "fresh.1"));
    for bad in ["elsewhere.0", "original.00", "original.2"] {
        assert!(saved
            .rebind_identity(&namespace(bad), &destination)
            .is_err());
    }
    assert!(TargetTracker::from_checkpoint(
        &saved,
        &destination,
        origin,
        Some(&context(1, 0)),
        config,
        Some([100, 80])
    )
    .is_err());
}

#[test]
fn malformed_or_mismatched_state_never_becomes_a_tracker() {
    let mut tracker =
        TargetTracker::with_namespace(namespace("original"), TrackerConfig::default()).unwrap();
    tracker
        .update(context(0, 0), 100, 80, &[detection(20.0, 0)], false)
        .unwrap();
    tracker.update(context(2, 0), 100, 80, &[], false).unwrap();
    let valid = serde_json::to_value(tracker.checkpoint()).unwrap();
    assert!(restore(valid.clone(), Some(&context(2, 0))).is_ok());
    assert!(restore(valid.clone(), Some(&context(1, 0))).is_err());
    let mut other = context(2, 0);
    other.transform = TransformId::new("other").unwrap();
    assert!(restore(valid.clone(), Some(&other)).is_err());
    for mutation in [
        "schema",
        "unknown",
        "config",
        "class",
        "nan",
        "outside",
        "missed",
        "future",
        "allocator",
        "duplicate",
        "budget",
        "topology",
        "dimensions",
    ] {
        let mut value = valid.clone();
        match mutation {
            "schema" => value["schema_version"] = 2.into(),
            "unknown" => value["authority"] = true.into(),
            "config" => value["config"]["max_missed_frames"] = 4.into(),
            "class" => value["tracks"][0]["class_id"] = 256.into(),
            "nan" => value["tracks"][0]["velocity_bits"][0] = f64::NAN.to_bits().into(),
            "outside" => value["tracks"][0]["bounds_bits"][2] = 101.0f64.to_bits().into(),
            "missed" => value["tracks"][0]["missed"] = 1.into(),
            "future" => value["tracks"][0]["last_observed_frame"] = 3.into(),
            "allocator" => value["next_id"] = 0.into(),
            "duplicate" => {
                let t = value["tracks"][0].clone();
                value["tracks"].as_array_mut().unwrap().push(t);
            }
            "budget" => {
                value["tracks"] =
                    serde_json::json!(vec![value["tracks"][0].clone(); MAX_TRACKS_PLUS_ONE])
            }
            "topology" => value["dimensions"] = serde_json::Value::Null,
            "dimensions" => value["dimensions"] = serde_json::json!([120, 80]),
            _ => unreachable!(),
        }
        assert!(restore(value, Some(&context(2, 0))).is_err(), "{mutation}");
    }
}
const MAX_TRACKS_PLUS_ONE: usize = pulsar_core::perception::MAX_DETECTIONS + 1;
