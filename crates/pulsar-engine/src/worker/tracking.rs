//! Observation-bound stroke evidence adapter. No global image motion is promoted
//! into physical target motion, and no missing interval becomes a measured pause.

use super::{detector::RawDetection, tracking_plan::TrackingPlan};
use anyhow::{bail, Result};
use pulsar_core::perception::{
    seed_points_in_box, separate_target_motion_bounded, CanonicalDetection, TargetTracker,
    TrackerConfig, TrackingReview,
};
use pulsar_core::{
    AttemptId, CoordinateBox, CoordinateSpace, DetectionObservation, EntityTrackId, EvidenceKind,
    FrameContext, NormalizedPosition,
};
use pulsar_protocol::AnalysisTargetSeed;
use sha2::{Digest, Sha256};

#[path = "tracking_checkpoint.rs"]
mod checkpoint;
pub(super) use checkpoint::StrokeTrackerCheckpoint;

pub struct StrokeFrame {
    pub position: Option<NormalizedPosition>,
    pub evidence: EvidenceKind,
    /// Actual observations only, in source pixels, never predicted boxes.
    pub observed_boxes: Vec<DetectionObservation>,
    pub selected_class: Option<u32>,
    pub selected_track: Option<EntityTrackId>,
    pub reviews: Vec<String>,
    /// Checked translations in analysis-view pixels; no physical camera pose.
    pub motion_separation: Option<pulsar_core::perception::MotionSeparation>,
    pub flow_search: Option<[pulsar_core::perception::PyramidFlowSummary; 2]>,
    pub target_sampling_points: Option<usize>,
    pub background_sampling_points: Option<usize>,
}
struct Previous {
    gray: Vec<u8>,
    width: u32,
    height: u32,
    track: EntityTrackId,
    bounds: CoordinateBox,
}
pub struct StrokeTracker {
    plan: TrackingPlan,
    tracker: TargetTracker,
    previous: Option<Previous>,
    selected: Option<EntityTrackId>,
    position: NormalizedPosition,
    target_seed: Option<AnalysisTargetSeed>,
    seed_consumed: bool,
    last_context: Option<FrameContext>,
}
impl StrokeTracker {
    #[cfg(test)]
    pub fn new(attempt: &AttemptId) -> Result<Self> {
        Self::with_target_seed(attempt, None)
    }

    #[cfg(test)]
    pub fn with_target_seed(
        attempt: &AttemptId,
        target_seed: Option<AnalysisTargetSeed>,
    ) -> Result<Self> {
        Self::with_plan(attempt, target_seed, TrackingPlan::default())
    }

    pub fn with_plan(
        attempt: &AttemptId,
        target_seed: Option<AnalysisTargetSeed>,
        plan: TrackingPlan,
    ) -> Result<Self> {
        if let Some(seed) = &target_seed {
            seed.validate()?;
        }
        // A content-derived namespace supports every valid AttemptId length
        // without truncating caller identities or reusing another attempt's IDs.
        let namespace = EntityTrackId::new(format!(
            "attempt.{:x}",
            Sha256::digest(attempt.as_str().as_bytes())
        ))?;
        Ok(Self {
            plan,
            tracker: TargetTracker::with_namespace(
                namespace,
                TrackerConfig {
                    max_missed_frames: 30,
                    ..TrackerConfig::default()
                },
            )?,
            previous: None,
            selected: None,
            target_seed,
            seed_consumed: false,
            position: NormalizedPosition::new(0.5)?,
            last_context: None,
        })
    }

    pub fn update(
        &mut self,
        context: FrameContext,
        gray: &[u8],
        width: u32,
        height: u32,
        detections: &[RawDetection],
        cut: bool,
    ) -> Result<StrokeFrame> {
        let pixels = (width as usize).checked_mul(height as usize);
        if width == 0 || height == 0 || pixels != Some(gray.len()) || gray.len() > 1_048_576 {
            bail!("stroke tracker requires an exact bounded grayscale frame");
        }
        let canonical = detections
            .iter()
            .map(|detection| {
                if detection
                    .bbox
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                {
                    bail!("raw detection is outside its normalized adapter contract");
                }
                Ok(CanonicalDetection::new(
                    CoordinateBox::new(
                        CoordinateSpace::SourcePixels,
                        f64::from(detection.bbox[0]) * f64::from(width),
                        f64::from(detection.bbox[1]) * f64::from(height),
                        f64::from(detection.bbox[2]) * f64::from(width),
                        f64::from(detection.bbox[3]) * f64::from(height),
                    )?,
                    detection.class_id,
                    f64::from(detection.score),
                )?)
            })
            .collect::<Result<Vec<_>>>()?;
        let update = self
            .tracker
            .update(context.clone(), width, height, &canonical, cut)?;
        self.last_context = Some(context.clone());
        let observed_boxes: Vec<_> = update
            .observations
            .iter()
            .filter(|observation| observation.evidence() == EvidenceKind::Observed)
            .cloned()
            .collect();
        let mut reviews: Vec<String> = update
            .reviews
            .iter()
            .map(|review| {
                match review {
                    TrackingReview::SceneCut => "scene_cut",
                    TrackingReview::SourceContextChanged => "source_context_changed",
                    TrackingReview::MissingDetection => "missing_detection",
                    TrackingReview::AmbiguousAssociation => "ambiguous_tracking_association",
                    TrackingReview::TrackBudgetReached => "target_tracking_budget",
                }
                .to_string()
            })
            .collect();
        let selected = if let Some(seed) = &self.target_seed {
            if cut
                || update
                    .reviews
                    .contains(&TrackingReview::SourceContextChanged)
            {
                self.seed_consumed = true;
                self.selected = None;
                reviews.push("manual_target_identity_invalidated".into());
                None
            } else if !self.seed_consumed {
                if context.frame.value() != seed.source_frame_index {
                    reviews.push("manual_target_seed_pending".into());
                    None
                } else {
                    self.seed_consumed = true;
                    let x = seed.x * f64::from(width);
                    let y = seed.y * f64::from(height);
                    let matches: Vec<_> = observed_boxes
                        .iter()
                        .filter(|observation| {
                            let bounds = observation.bounds();
                            bounds.x_min() <= x
                                && x < bounds.x_max()
                                && bounds.y_min() <= y
                                && y < bounds.y_max()
                                && seed.class_id.is_none_or(|class| {
                                    observation
                                        .track()
                                        .is_some_and(|track| update.class_for(track) == Some(class))
                                })
                        })
                        .collect();
                    if matches.len() == 1 {
                        reviews.push("manual_target_seed_selected".into());
                        Some(matches[0].clone())
                    } else {
                        reviews.push(
                            if matches.is_empty() {
                                "manual_target_seed_missing"
                            } else {
                                "manual_target_seed_ambiguous"
                            }
                            .into(),
                        );
                        None
                    }
                }
            } else {
                let selected = self
                    .selected
                    .as_ref()
                    .and_then(|id| {
                        observed_boxes
                            .iter()
                            .find(|observation| observation.track() == Some(id))
                    })
                    .cloned();
                if selected.is_none() {
                    reviews.push("manual_target_identity_unavailable".into());
                }
                selected
            }
        } else {
            self.selected
                .as_ref()
                .and_then(|id| {
                    observed_boxes
                        .iter()
                        .find(|observation| observation.track() == Some(id))
                })
                .or_else(|| {
                    observed_boxes.iter().max_by(|a, b| {
                        a.confidence()
                            .value()
                            .total_cmp(&b.confidence().value())
                            .then_with(|| b.track().cmp(&a.track()))
                    })
                })
                .cloned()
        };
        let selected_track = selected
            .as_ref()
            .and_then(|observation| observation.track().cloned());
        let selected_class = selected_track
            .as_ref()
            .and_then(|track| update.class_for(track));
        if self.target_seed.is_none() && selected_track != self.selected && selected_track.is_some()
        {
            reviews.push("automatic_target_selection_unqualified".into());
        }
        if self.target_seed.is_none() || selected_track.is_some() {
            self.selected = selected_track.clone();
        }
        let mut result = StrokeFrame {
            position: None,
            evidence: EvidenceKind::Unavailable,
            observed_boxes,
            selected_class,
            selected_track: selected_track.clone(),
            reviews,
            motion_separation: None,
            flow_search: None,
            target_sampling_points: None,
            background_sampling_points: None,
        };
        if selected_track
            .as_ref()
            .is_some_and(|track| update.association_ambiguous(track))
        {
            result.reviews.push("ambiguous_target_association".into());
        }
        if cut
            || update
                .reviews
                .contains(&TrackingReview::SourceContextChanged)
            || selected_track
                .as_ref()
                .is_some_and(|track| update.association_ambiguous(track))
        {
            self.previous = None;
            return Ok(result);
        }
        let Some(observation) = selected else {
            self.previous = None;
            if !result
                .reviews
                .iter()
                .any(|reason| reason == "missing_detection")
            {
                result.reviews.push("missing_detection".into());
            }
            return Ok(result);
        };
        let track = selected_track
            .ok_or_else(|| anyhow::anyhow!("selected tracker observation lacks identity"))?;
        let next = Previous {
            gray: gray.to_vec(),
            width,
            height,
            track: track.clone(),
            bounds: observation.bounds().clone(),
        };
        let Some(previous) = self.previous.replace(next) else {
            result.position = Some(self.position);
            result.evidence = EvidenceKind::Synthesized;
            result.reviews.push("synthetic_motion_anchor".into());
            return Ok(result);
        };
        if previous.track != track || previous.width != width || previous.height != height {
            result.reviews.push("target_identity_discontinuity".into());
            return Ok(result);
        }
        let config = self.plan.config();
        let border = f64::from(config.patch_radius + 1);
        let roi = &previous.bounds;
        if roi.width() <= 2.0 * border || roi.height() <= 2.0 * border {
            result.reviews.push("target_flow_support_too_small".into());
            return Ok(result);
        }
        let interior = CoordinateBox::new(
            CoordinateSpace::SourcePixels,
            roi.x_min() + border,
            roi.y_min() + border,
            roi.x_max() - border,
            roi.y_max() - border,
        )?;
        let target_points = seed_points_in_box(&interior, width, height, self.plan.target_points)?;
        let full_frame = CoordinateBox::new(
            CoordinateSpace::SourcePixels,
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
        )?;
        let mut background_points =
            seed_points_in_box(&full_frame, width, height, self.plan.background_points)?;
        let current_bounds = observation.bounds();
        let background_allowed = |point: &[f64; 2]| {
            let in_expanded = |bounds: &CoordinateBox| {
                point[0] >= bounds.x_min() - border
                    && point[0] <= bounds.x_max() + border
                    && point[1] >= bounds.y_min() - border
                    && point[1] <= bounds.y_max() + border
            };
            !in_expanded(roi) && !in_expanded(current_bounds)
        };
        background_points.retain(background_allowed);
        let (mut target, mut target_search) =
            self.plan
                .track(&previous.gray, gray, width, height, &target_points)?;
        // Retain the original grid/evidence. If a larger view lacks even three
        // supported points, add bounded distinct candidates instead of lowering
        // the support requirement or accepting a coarse-only correspondence.
        if self.plan.sparse_recovery_points.is_some()
            && width.min(height) >= 256
            && target
                .iter()
                .filter(|flow| flow.displacement.is_some())
                .count()
                < 3
        {
            let extra: Vec<_> = seed_points_in_box(
                &interior,
                width,
                height,
                self.plan.sparse_recovery_points.unwrap_or(0),
            )?
            .into_iter()
            .filter(|point| !target_points.contains(point))
            .collect();
            let (flows, search) = self
                .plan
                .track(&previous.gray, gray, width, height, &extra)?;
            target.extend(flows);
            target_search.recovered_points += search.recovered_points;
        }
        result.target_sampling_points = Some(target.len());
        let (mut background, mut background_search) =
            self.plan
                .track(&previous.gray, gray, width, height, &background_points)?;
        if let Some(limit) = self.plan.background_recovery_points {
            let margin = f64::from(config.patch_radius + config.search_radius + 2);
            if background
                .iter()
                .filter(|flow| flow.displacement.is_some())
                .count()
                < 3
                && f64::from(width) > 2.0 * margin
                && f64::from(height) > 2.0 * margin
            {
                // Preserve the original grid; add distinct points only where the
                // checked full-resolution search fits, outside both target boxes.
                let supported = CoordinateBox::new(
                    CoordinateSpace::SourcePixels,
                    margin,
                    margin,
                    f64::from(width) - margin,
                    f64::from(height) - margin,
                )?;
                let extra: Vec<_> = seed_points_in_box(&supported, width, height, limit)?
                    .into_iter()
                    .filter(|point| background_allowed(point) && !background_points.contains(point))
                    .take(self.plan.background_points.saturating_sub(background.len()))
                    .collect();
                let (flows, search) =
                    self.plan
                        .track(&previous.gray, gray, width, height, &extra)?;
                background.extend(flows);
                background_search.recovered_points += search.recovered_points;
                result.background_sampling_points = Some(background.len());
            }
        }
        let bound = target_search
            .component_displacement_bound
            .max(background_search.component_displacement_bound);
        result.flow_search = Some([target_search, background_search]);
        let separation = separate_target_motion_bounded(&target, &background, bound)?;
        result.motion_separation = Some(separation.clone());
        let Some(relative) = separation.target_relative_to_background else {
            result.reviews.push("unsupported_relative_flow".into());
            return Ok(result);
        };
        // This is an explicitly inferred image-relative stroke proposal, not
        // observed anatomy, metric depth, calibrated camera pose, or six-DoF.
        self.position = pulsar_core::integrate_vertical_motion(self.position, relative[1], height)?;
        result.position = Some(self.position);
        result.evidence = EvidenceKind::Inferred;
        result.reviews.push("relative_image_motion_inferred".into());
        result
            .reviews
            .push("background_selection_unqualified".into());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::{FrameId, SourcePlacementId, SourceVersionId, TransformId};

    fn context(frame: u64) -> FrameContext {
        FrameContext {
            source_version: SourceVersionId::new("source").unwrap(),
            source_placement: SourcePlacementId::new("placement").unwrap(),
            frame: FrameId::new(frame),
            transform: TransformId::new("analysis").unwrap(),
            seek_generation: 0,
            request_generation: frame,
        }
    }

    #[test]
    fn checkpoint_roundtrip_continues_stroke_and_rejects_different_support() {
        let origin = AttemptId::new("origin-stroke-checkpoint").unwrap();
        let destination = AttemptId::new("destination-stroke-checkpoint").unwrap();
        let mut tracker = StrokeTracker::new(&origin).unwrap();
        let first = texture();
        tracker
            .update(context(0), &first, 128, 128, &[detection(0.35)], false)
            .unwrap();
        let snapshot = tracker.checkpoint().unwrap();
        let encoded = serde_json::to_vec(&snapshot).unwrap();
        let snapshot = serde_json::from_slice(&encoded).unwrap();
        let mut restored = StrokeTracker::from_checkpoint(
            &snapshot,
            &origin,
            &destination,
            Some(&context(0)),
            ([128, 128], Some(&first)),
            None,
            TrackingPlan::default(),
        )
        .unwrap();
        let mut wrong = first.clone();
        wrong[0] ^= 1;
        assert!(StrokeTracker::from_checkpoint(
            &snapshot,
            &origin,
            &destination,
            Some(&context(0)),
            ([128, 128], Some(&wrong)),
            None,
            TrackingPlan::default()
        )
        .is_err());
        assert!(StrokeTracker::from_checkpoint(
            &snapshot,
            &origin,
            &destination,
            Some(&context(1)),
            ([128, 128], Some(&first)),
            None,
            TrackingPlan::default()
        )
        .is_err());
        for frame in 1..4 {
            let image = translate(&first, frame as isize);
            let detections = [detection(0.35 + frame as f32 / 128.0)];
            let a = tracker
                .update(context(frame), &image, 128, 128, &detections, false)
                .unwrap();
            let b = restored
                .update(context(frame), &image, 128, 128, &detections, false)
                .unwrap();
            assert_eq!(a.position, b.position);
            assert_eq!(a.evidence, b.evidence);
            assert_eq!(a.reviews, b.reviews);
            assert_eq!(a.motion_separation, b.motion_separation);
            assert_eq!(
                a.selected_track
                    .as_ref()
                    .map(|id| snapshot.rebind(id, &destination).unwrap()),
                b.selected_track
            );
        }
    }

    #[test]
    fn unrelated_class_ambiguity_preserves_selected_target_motion() {
        let mut tracker = tracker();
        let image = texture();
        let target = detection(0.35);
        let decoy = |x| RawDetection {
            class_id: 3,
            score: 0.3,
            bbox: [x, 0.05, x + 0.1, 0.15],
        };
        let first = tracker
            .update(
                context(0),
                &image,
                128,
                128,
                &[target.clone(), decoy(0.05), decoy(0.15)],
                false,
            )
            .unwrap();
        let next = tracker
            .update(context(1), &image, 128, 128, &[target, decoy(0.10)], false)
            .unwrap();
        assert_eq!(first.selected_track, next.selected_track);
        assert_eq!(next.evidence, EvidenceKind::Inferred);
        assert!(next.motion_separation.is_some());
        assert!(next
            .reviews
            .iter()
            .any(|r| r == "ambiguous_tracking_association"));
        assert!(!next
            .reviews
            .iter()
            .any(|r| r == "ambiguous_target_association"));
    }

    #[test]
    fn selected_class_ambiguity_withholds_fresh_identity_motion() {
        let mut tracker = tracker();
        let image = texture();
        let a = detection(0.35);
        let b = RawDetection {
            class_id: 2,
            score: 0.8,
            bbox: [0.15, 0.35, 0.45, 0.65],
        };
        tracker
            .update(context(0), &image, 128, 128, &[a, b], false)
            .unwrap();
        let middle = RawDetection {
            class_id: 2,
            score: 0.9,
            bbox: [0.25, 0.35, 0.55, 0.65],
        };
        let next = tracker
            .update(context(1), &image, 128, 128, &[middle], false)
            .unwrap();
        assert_eq!(next.evidence, EvidenceKind::Unavailable);
        assert!(next.position.is_none());
        assert!(next.motion_separation.is_none());
        assert!(next
            .reviews
            .iter()
            .any(|r| r == "ambiguous_target_association"));
    }

    #[test]
    fn maximum_small_view_recovers_background_without_fabricating_flat_or_occluded_motion() {
        let textured: Vec<u8> = (0..64usize)
            .flat_map(|y| {
                (0..64usize).map(move |x| ((x * 31 + y * 17 + (x / 7) * (y / 5) * 13) % 256) as u8)
            })
            .collect();
        for case in 0..3 {
            let mut tracker = StrokeTracker::with_plan(
                &AttemptId::new("small-maximum").unwrap(),
                None,
                TrackingPlan::resolve("maximum").unwrap(),
            )
            .unwrap();
            let image = if case == 1 {
                vec![128; 4096]
            } else {
                textured.clone()
            };
            let detection = RawDetection {
                class_id: 0,
                score: 0.9,
                bbox: if case == 2 {
                    [0., 0., 1., 1.]
                } else {
                    [22. / 64., 22. / 64., 42. / 64., 42. / 64.]
                },
            };
            let anchor = tracker
                .update(
                    context(0),
                    &image,
                    64,
                    64,
                    std::slice::from_ref(&detection),
                    false,
                )
                .unwrap();
            let result = tracker
                .update(context(1), &image, 64, 64, &[detection], false)
                .unwrap();
            assert_eq!(result.target_sampling_points, Some(64));
            let separation = result.motion_separation.as_ref().unwrap();
            if case == 0 {
                assert!(separation.target_accepted_points >= 3);
                assert!(separation.background_accepted_points >= 3, "{separation:?}");
                assert_eq!(result.background_sampling_points, Some(64));
                assert_eq!(separation.target_relative_to_background, Some([0., 0.]));
                assert_eq!(result.evidence, EvidenceKind::Inferred);
                assert_eq!(result.position, anchor.position);
            } else {
                assert!(result.position.is_none());
                assert_eq!(result.evidence, EvidenceKind::Unavailable);
                assert!(separation.target_relative_to_background.is_none());
                assert!(result.reviews.contains(&"unsupported_relative_flow".into()));
            }
        }
    }

    #[test]
    fn tier_plans_execute_distinct_grids_without_inventing_static_motion() {
        let image = texture();
        for (preset, points) in [("fast", 8), ("default", 16), ("maximum", 64)] {
            let mut tracker = StrokeTracker::with_plan(
                &AttemptId::new("tier-grid").unwrap(),
                None,
                TrackingPlan::resolve(preset).unwrap(),
            )
            .unwrap();
            let anchor = tracker
                .update(context(0), &image, 128, 128, &[detection(0.35)], false)
                .unwrap();
            assert_eq!(anchor.evidence, EvidenceKind::Synthesized);
            let next = tracker
                .update(context(1), &image, 128, 128, &[detection(0.35)], false)
                .unwrap();
            assert_eq!(next.target_sampling_points, Some(points));
            assert_eq!(next.evidence, EvidenceKind::Inferred);
            assert_eq!(next.position, anchor.position);
            assert_eq!(
                next.motion_separation
                    .unwrap()
                    .target_relative_to_background,
                Some([0., 0.])
            );
        }
    }

    fn tracker() -> StrokeTracker {
        StrokeTracker::new(&AttemptId::new("attempt.test").unwrap()).unwrap()
    }
    fn detection(y: f32) -> RawDetection {
        RawDetection {
            class_id: 2,
            score: 0.9,
            bbox: [0.35, y, 0.65, y + 0.3],
        }
    }
    fn texture() -> Vec<u8> {
        (0..128)
            .flat_map(|y| (0..128).map(move |x| ((x * 37 + y * 61 + x * y * 7) % 251) as u8))
            .collect()
    }
    fn translate(image: &[u8], dy: isize) -> Vec<u8> {
        (0..128)
            .flat_map(|y| {
                (0..128).map(move |x| {
                    let old = y as isize - dy;
                    if (0..128).contains(&old) {
                        image[old as usize * 128 + x]
                    } else {
                        0
                    }
                })
            })
            .collect()
    }
    #[test]
    #[ignore = "requires a retained client pointer-widget seed fixture; run explicitly"]
    fn picked_viewer_seed_enters_tracker_without_decoy_substitution() {
        use std::io::Read;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ViewerSeed {
            context: FrameContext,
            seed: AnalysisTargetSeed,
        }
        let path = std::env::var_os("PULSAR_TEST_PICKED_SEED_INPUT")
            .expect("client widget seed fixture required");
        let file = std::fs::File::open(path).unwrap();
        assert!(file.metadata().unwrap().is_file());
        let mut bytes = Vec::new();
        file.take(4097).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 4096);
        let input: ViewerSeed = serde_json::from_slice(&bytes).unwrap();
        input.seed.validate().unwrap();
        assert_eq!(input.context.frame.value(), input.seed.source_frame_index);
        assert!(
            input.seed.class_id.is_none(),
            "preview must not invent detector class metadata"
        );
        let first_frame = input.seed.source_frame_index;
        assert_eq!(
            first_frame, 4,
            "fixture must retain the viewer source ordinal"
        );
        let frame_context = |frame| {
            let mut value = input.context.clone();
            value.frame = FrameId::new(frame);
            value
        };
        let mut tracker = StrokeTracker::with_target_seed(
            &AttemptId::new("viewer.picked").unwrap(),
            Some(input.seed),
        )
        .unwrap();
        let image = texture();
        let target = detection(0.35);
        let decoy = RawDetection {
            bbox: [0.75, 0.3, 0.95, 0.7],
            score: 0.99,
            class_id: 2,
        };
        let pending = tracker
            .update(
                frame_context(first_frame - 1),
                &image,
                128,
                128,
                &[target.clone(), decoy.clone()],
                false,
            )
            .unwrap();
        assert!(pending.selected_track.is_none());
        assert!(pending
            .reviews
            .iter()
            .any(|r| r == "manual_target_seed_pending"));
        let first = tracker
            .update(
                frame_context(first_frame),
                &image,
                128,
                128,
                &[target.clone(), decoy.clone()],
                false,
            )
            .unwrap();
        let chosen = first
            .selected_track
            .clone()
            .expect("unique clicked target must be selected");
        assert!(first
            .reviews
            .iter()
            .any(|r| r == "manual_target_seed_selected"));
        assert!(!first
            .reviews
            .iter()
            .any(|r| r == "automatic_target_selection_unqualified"));
        let missing = tracker
            .update(
                frame_context(first_frame + 1),
                &image,
                128,
                128,
                std::slice::from_ref(&decoy),
                false,
            )
            .unwrap();
        assert!(missing.selected_track.is_none());
        assert_eq!(missing.evidence, EvidenceKind::Unavailable);
        let restored = tracker
            .update(
                frame_context(first_frame + 2),
                &image,
                128,
                128,
                &[target.clone(), decoy.clone()],
                false,
            )
            .unwrap();
        assert_eq!(restored.selected_track, Some(chosen));
        assert!(tracker
            .update(
                frame_context(first_frame + 3),
                &image,
                128,
                128,
                std::slice::from_ref(&target),
                true
            )
            .unwrap()
            .selected_track
            .is_none());
        assert!(tracker
            .update(
                frame_context(first_frame + 4),
                &image,
                128,
                128,
                &[target],
                false
            )
            .unwrap()
            .selected_track
            .is_none());
    }

    #[test]
    fn manual_seed_selects_lower_confidence_target_and_never_substitutes_a_decoy() {
        let seed = AnalysisTargetSeed {
            source_frame_index: 0,
            x: 0.5,
            y: 0.5,
            class_id: Some(2),
        };
        let mut tracker =
            StrokeTracker::with_target_seed(&AttemptId::new("manual.test").unwrap(), Some(seed))
                .unwrap();
        let image = texture();
        let target = detection(0.35);
        let decoy = RawDetection {
            bbox: [0.75, 0.3, 0.95, 0.7],
            score: 0.99,
            class_id: 2,
        };
        let first = tracker
            .update(
                context(0),
                &image,
                128,
                128,
                &[target.clone(), decoy.clone()],
                false,
            )
            .unwrap();
        let chosen = first.selected_track.clone().unwrap();
        assert_eq!(first.selected_class, Some(2));
        assert!(first
            .reviews
            .iter()
            .any(|r| r == "manual_target_seed_selected"));
        assert!(!first
            .reviews
            .iter()
            .any(|r| r == "automatic_target_selection_unqualified"));
        let missing = tracker
            .update(
                context(1),
                &image,
                128,
                128,
                std::slice::from_ref(&decoy),
                false,
            )
            .unwrap();
        assert_eq!(missing.evidence, EvidenceKind::Unavailable);
        assert!(missing.selected_track.is_none());
        let reacquired = tracker
            .update(
                context(2),
                &image,
                128,
                128,
                &[target.clone(), decoy.clone()],
                false,
            )
            .unwrap();
        assert_eq!(reacquired.selected_track, Some(chosen));
        assert_eq!(reacquired.evidence, EvidenceKind::Synthesized);
        for frame in 3..36 {
            assert!(tracker
                .update(
                    context(frame),
                    &image,
                    128,
                    128,
                    std::slice::from_ref(&decoy),
                    false
                )
                .unwrap()
                .selected_track
                .is_none());
        }
        let expired = tracker
            .update(context(36), &image, 128, 128, &[target, decoy], false)
            .unwrap();
        assert!(expired.selected_track.is_none());
        assert_eq!(expired.evidence, EvidenceKind::Unavailable);
    }
    #[test]
    fn manual_seed_is_exact_one_shot_and_invalidates_at_a_cut() {
        let image = texture();
        for (class, expected) in [
            (None, "manual_target_seed_ambiguous"),
            (Some(9), "manual_target_seed_missing"),
        ] {
            let mut tracker = StrokeTracker::with_target_seed(
                &AttemptId::new("manual.ambiguous").unwrap(),
                Some(AnalysisTargetSeed {
                    source_frame_index: 2,
                    x: 0.5,
                    y: 0.5,
                    class_id: class,
                }),
            )
            .unwrap();
            let mut other = detection(0.35);
            other.class_id = 3;
            assert!(tracker
                .update(context(0), &image, 128, 128, &[detection(0.35)], false)
                .unwrap()
                .position
                .is_none());
            let result = tracker
                .update(
                    context(2),
                    &image,
                    128,
                    128,
                    &[detection(0.35), other],
                    false,
                )
                .unwrap();
            assert!(result.selected_track.is_none());
            assert!(result.reviews.iter().any(|r| r == expected));
            assert!(tracker
                .update(context(3), &image, 128, 128, &[detection(0.35)], false)
                .unwrap()
                .selected_track
                .is_none());
        }
        let mut tracker = StrokeTracker::with_target_seed(
            &AttemptId::new("manual.cut").unwrap(),
            Some(AnalysisTargetSeed {
                source_frame_index: 0,
                x: 0.5,
                y: 0.5,
                class_id: Some(2),
            }),
        )
        .unwrap();
        assert!(tracker
            .update(context(0), &image, 128, 128, &[detection(0.35)], false)
            .unwrap()
            .selected_track
            .is_some());
        assert!(tracker
            .update(context(1), &image, 128, 128, &[detection(0.35)], true)
            .unwrap()
            .position
            .is_none());
        assert!(tracker
            .update(context(2), &image, 128, 128, &[detection(0.35)], false)
            .unwrap()
            .selected_track
            .is_none());
    }

    #[test]
    fn absent_detection_cannot_generate_motion_or_decorative_box() {
        let mut tracker = tracker();
        let result = tracker
            .update(context(0), &texture(), 128, 128, &[], false)
            .unwrap();
        assert!(result.position.is_none());
        assert!(result.observed_boxes.is_empty());
        assert_eq!(result.evidence, EvidenceKind::Unavailable);
    }
    #[test]
    fn neutral_anchor_is_synthesized_not_observed() {
        let mut tracker = tracker();
        let result = tracker
            .update(context(0), &texture(), 128, 128, &[detection(0.35)], false)
            .unwrap();
        assert_eq!(result.position.unwrap().value(), 0.5);
        assert_eq!(result.evidence, EvidenceKind::Synthesized);
        assert_eq!(result.observed_boxes[0].evidence(), EvidenceKind::Observed);
    }
    #[test]
    fn camera_translation_does_not_invent_relative_stroke() {
        let mut tracker = tracker();
        let first = texture();
        let second = translate(&first, 2);
        tracker
            .update(context(0), &first, 128, 128, &[detection(0.35)], false)
            .unwrap();
        let result = tracker
            .update(
                context(1),
                &second,
                128,
                128,
                &[detection(0.35 + 2.0 / 128.0)],
                false,
            )
            .unwrap();
        assert_eq!(
            result.evidence,
            EvidenceKind::Inferred,
            "{:?}",
            result.reviews
        );
        assert!((result.position.unwrap().value() - 0.5).abs() < 0.001);
        let separation = result.motion_separation.as_ref().unwrap();
        assert!(separation.target_accepted_points >= 3);
        assert!(separation.background_accepted_points >= 3);
        assert!((separation.background_image_translation.unwrap()[1] - 2.0).abs() < 0.01);
        assert!(separation.target_relative_to_background.unwrap()[1].abs() < 0.01);
        assert!(result
            .reviews
            .contains(&"background_selection_unqualified".into()));
    }
    #[test]
    fn cut_and_absence_never_reuse_stale_observations_as_motion() {
        let mut tracker = tracker();
        let image = texture();
        let first = tracker
            .update(context(0), &image, 128, 128, &[detection(0.35)], false)
            .unwrap();
        let cut = tracker
            .update(context(1), &image, 128, 128, &[detection(0.35)], true)
            .unwrap();
        assert!(cut.position.is_none());
        assert_ne!(first.selected_track, cut.selected_track);
        let absent = tracker
            .update(context(2), &image, 128, 128, &[], false)
            .unwrap();
        assert!(absent.observed_boxes.is_empty());
        assert!(absent.position.is_none());
    }
    #[test]
    fn low_texture_requires_review_not_measured_pause() {
        let mut tracker = tracker();
        tracker
            .update(
                context(0),
                &[128; 16384],
                128,
                128,
                &[detection(0.35)],
                false,
            )
            .unwrap();
        let result = tracker
            .update(
                context(1),
                &[128; 16384],
                128,
                128,
                &[detection(0.35)],
                false,
            )
            .unwrap();
        assert!(result.position.is_none());
        assert!(result.reviews.contains(&"unsupported_relative_flow".into()));
    }
    #[test]
    fn malformed_gray_or_box_is_rejected_before_state_advance() {
        let mut tracker = tracker();
        assert!(tracker
            .update(context(0), &[0; 2], 128, 128, &[], false)
            .is_err());
        let invalid = RawDetection {
            class_id: 2,
            score: 0.9,
            bbox: [0.8, 0.1, 0.2, 0.4],
        };
        assert!(tracker
            .update(context(0), &texture(), 128, 128, &[invalid], false)
            .is_err());
        assert!(tracker
            .update(context(0), &texture(), 128, 128, &[detection(0.35)], false)
            .is_ok());
    }
    #[test]
    fn target_only_motion_is_small_relative_displacement_and_reverses() {
        let mut tracker = tracker();
        let first = texture();
        let mut second = first.clone();
        for y in 46..88 {
            for x in 40..90 {
                second[y * 128 + x] = first[(y - 2) * 128 + x];
            }
        }
        tracker
            .update(context(0), &first, 128, 128, &[detection(0.35)], false)
            .unwrap();
        let forward = tracker
            .update(
                context(1),
                &second,
                128,
                128,
                &[detection(0.35 + 2.0 / 128.0)],
                false,
            )
            .unwrap();
        assert_eq!(
            forward.evidence,
            EvidenceKind::Inferred,
            "{:?}",
            forward.reviews
        );
        assert!((forward.position.unwrap().value() - (0.5 + 2.0 / 128.0)).abs() < 0.002);
        let reverse = tracker
            .update(context(2), &first, 128, 128, &[detection(0.35)], false)
            .unwrap();
        assert_eq!(
            reverse.evidence,
            EvidenceKind::Inferred,
            "{:?}",
            reverse.reviews
        );
        assert!((reverse.position.unwrap().value() - 0.5).abs() < 0.002);
    }

    #[test]
    fn intentional_stride_does_not_expire_a_present_target() {
        let mut tracker = tracker();
        let image = texture();
        let first = tracker
            .update(context(0), &image, 128, 128, &[detection(0.35)], false)
            .unwrap();
        let next = tracker
            .update(context(32), &image, 128, 128, &[detection(0.35)], false)
            .unwrap();
        assert_eq!(first.selected_track, next.selected_track);
        assert_eq!(next.evidence, EvidenceKind::Inferred);
        assert_eq!(next.position.unwrap().value(), 0.5);
    }
    #[test]
    fn full_length_attempt_ids_have_valid_namespaces() {
        assert!(StrokeTracker::new(&AttemptId::new("x".repeat(128)).unwrap()).is_ok());
    }
}
