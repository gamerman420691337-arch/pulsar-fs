//! Anatomical stroke tracking and hybrid neural-optical fusion.

use crate::neural::yolo::Detection;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StrokeObservation {
    pub timestamp_ms: i64,
    /// Estimated position in [0.0, 100.0]
    pub position: f32,
    /// Whether this observation was derived from neural YOLO detection
    pub is_neural: bool,
    /// Probe organ center
    pub probe_center: Option<(f32, f32)>,
    /// Target organ center
    pub target_center: Option<(f32, f32)>,
}

#[derive(Debug, Clone)]
pub struct AnatomicalTracker {
    /// Rolling minimum distance observed (full penetration)
    min_dist: f32,
    /// Rolling maximum distance observed (full retraction)
    max_dist: f32,
    /// Last known estimated stroke position
    last_position: f32,
}

impl Default for AnatomicalTracker {
    fn default() -> Self {
        Self {
            min_dist: 0.05,
            max_dist: 0.35,
            last_position: 50.0,
        }
    }
}

impl AnatomicalTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Estimate stroke position from YOLO detections.
    /// Returns Some(position) in [0.0, 100.0] if both probe and target are detected, else None.
    pub fn update_from_detections(
        &mut self,
        detections: &[Detection],
    ) -> Option<(f32, (f32, f32), (f32, f32))> {
        // 1. Find probe organ: glans (1) takes priority, followed by penis (0)
        let probe = detections
            .iter()
            .filter(|d| d.class_id == 1)
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
            .or_else(|| {
                detections
                    .iter()
                    .filter(|d| d.class_id == 0)
                    .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
            })?;

        // 2. Find target organ: pussy (2) > anus (4) > butt (3) > hand (7)
        let target = detections
            .iter()
            .filter(|d| d.class_id == 2)
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
            .or_else(|| {
                detections
                    .iter()
                    .filter(|d| d.class_id == 4)
                    .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
            })
            .or_else(|| {
                detections
                    .iter()
                    .filter(|d| d.class_id == 3)
                    .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
            })
            .or_else(|| {
                detections
                    .iter()
                    .filter(|d| d.class_id == 7)
                    .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
            })?;

        // 3. Compute Euclidean & vertical distance between probe and target
        let dx = probe.center.0 - target.center.0;
        let dy = probe.center.1 - target.center.1;
        let dist = (dx * dx + dy * dy).sqrt();

        // Update adaptive min and max distance bounds
        if dist < self.min_dist {
            self.min_dist = dist * 0.95 + self.min_dist * 0.05;
        }
        if dist > self.max_dist {
            self.max_dist = dist * 0.95 + self.max_dist * 0.05;
        }

        let span = (self.max_dist - self.min_dist).max(0.01);
        // Normalized: 100 = closest (deepest insertion), 0 = farthest (withdrawn)
        let raw_pos = ((self.max_dist - dist) / span * 100.0).clamp(0.0, 100.0);

        // Smooth with EMA
        self.last_position = self.last_position * 0.3 + raw_pos * 0.7;

        Some((self.last_position, probe.center, target.center))
    }

    /// Hybrid update: when neural detection fails, update via integrated optical velocity
    pub fn update_hybrid_fallback(&mut self, optical_velocity: f32) -> f32 {
        // Optical expansion/contraction shifts the last known position
        let delta = optical_velocity * 2.0;
        self.last_position = (self.last_position + delta).clamp(0.0, 100.0);
        self.last_position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anatomical_tracker_distance_mapping() {
        let mut tracker = AnatomicalTracker::new();

        // Target (pussy) at center (0.5, 0.5)
        let target = Detection {
            class_id: 2,
            class_name: "pussy",
            confidence: 0.9,
            bbox: [0.4, 0.4, 0.6, 0.6],
            center: (0.5, 0.5),
        };

        // Deep insertion (probe right at target)
        let probe_close = Detection {
            class_id: 1,
            class_name: "glans",
            confidence: 0.9,
            bbox: [0.48, 0.48, 0.52, 0.52],
            center: (0.5, 0.52),
        };

        let (pos_deep, _, _) = tracker.update_from_detections(&[target.clone(), probe_close]).unwrap();

        // Full retraction (probe far away)
        let probe_far = Detection {
            class_id: 1,
            class_name: "glans",
            confidence: 0.9,
            bbox: [0.45, 0.8, 0.55, 0.9],
            center: (0.5, 0.85),
        };

        let (pos_far, _, _) = tracker.update_from_detections(&[target, probe_far]).unwrap();

        assert!(pos_deep > pos_far, "Deep insertion position should be higher than retracted");
    }
}
