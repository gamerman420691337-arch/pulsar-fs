//! Checked data snapshots. Source/job/model custody remains the caller's duty.
use super::*;
use serde::{Deserialize, Serialize};

/// Inert state for continuing the same tracking computation. Deserialization
/// grants no authority and callers must bind it to their exact source/config.
/// Float bits preserve state without relying on a JSON decimal round trip.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetTrackerCheckpoint {
    schema_version: u16,
    namespace: EntityTrackId,
    config: Config,
    #[serde(deserialize_with = "bounded_tracks")]
    tracks: Vec<SavedTrack>,
    last: Option<FrameContext>,
    dimensions: Option<[u32; 2]>,
    next_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    max_missed_frames: u32,
    max_tracks: u16,
    max_center_distance_bits: u64,
    ambiguity_margin_bits: u64,
}

impl From<TrackerConfig> for Config {
    fn from(config: TrackerConfig) -> Self {
        Self {
            max_missed_frames: config.max_missed_frames,
            max_tracks: config.max_tracks.min(u16::MAX as usize) as u16,
            max_center_distance_bits: config.max_center_distance.to_bits(),
            ambiguity_margin_bits: config.ambiguity_margin.to_bits(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedTrack {
    id: EntityTrackId,
    class_id: u32,
    bounds_bits: [u64; 4],
    velocity_bits: [u64; 2],
    score_bits: u64,
    missed: u64,
    last_observed_frame: u64,
    last_observed_center_bits: [u64; 2],
}

fn bounded_tracks<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<SavedTrack>, D::Error> {
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Vec<SavedTrack>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "at most {MAX_DETECTIONS} tracker entries")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() == MAX_DETECTIONS {
                    return Err(serde::de::Error::custom("tracker checkpoint entry budget"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    d.deserialize_seq(Visitor)
}

impl TargetTrackerCheckpoint {
    /// Rebind an allocated identity, including one that has expired from the
    /// active tracker. This preserves its ordinal without merging attempts.
    pub fn rebind_identity(
        &self,
        identity: &EntityTrackId,
        new_namespace: &EntityTrackId,
    ) -> Result<EntityTrackId, CoreError> {
        if new_namespace.as_str().len() > 96 {
            return Err(invalid("checkpoint destination namespace exceeds budget"));
        }
        let prefix = format!("{}.", self.namespace);
        let suffix = identity
            .as_str()
            .strip_prefix(&prefix)
            .ok_or_else(|| invalid("checkpoint identity belongs to another namespace"))?;
        let ordinal = suffix
            .parse::<u64>()
            .map_err(|_| invalid("checkpoint identity has no canonical ordinal"))?;
        if ordinal >= self.next_id || suffix != ordinal.to_string() {
            return Err(invalid("checkpoint identity was not canonically allocated"));
        }
        EntityTrackId::new(format!("{new_namespace}.{ordinal}"))
    }
}

impl TargetTracker {
    pub fn checkpoint(&self) -> TargetTrackerCheckpoint {
        TargetTrackerCheckpoint {
            schema_version: 1,
            namespace: self.namespace.clone(),
            config: self.config.into(),
            tracks: self
                .tracks
                .iter()
                .map(|t| SavedTrack {
                    id: t.id.clone(),
                    class_id: t.class_id,
                    bounds_bits: [
                        t.bounds.x_min(),
                        t.bounds.y_min(),
                        t.bounds.x_max(),
                        t.bounds.y_max(),
                    ]
                    .map(f64::to_bits),
                    velocity_bits: t.velocity.map(f64::to_bits),
                    score_bits: t.score.to_bits(),
                    missed: t.missed,
                    last_observed_frame: t.last_observed_frame,
                    last_observed_center_bits: t.last_observed_center.map(f64::to_bits),
                })
                .collect(),
            last: self.last.clone(),
            dimensions: self.dimensions.map(|(w, h)| [w, h]),
            next_id: self.next_id,
        }
    }

    /// Restore only under an independently expected origin namespace. A fresh
    /// attempt can explicitly rebind every allocated identity to its namespace.
    /// This pure check does not validate external artifact hashes or job grants.
    pub fn from_checkpoint(
        saved: &TargetTrackerCheckpoint,
        expected_origin: &EntityTrackId,
        destination: EntityTrackId,
        expected_context: Option<&FrameContext>,
        expected_config: TrackerConfig,
        expected_dimensions: Option<[u32; 2]>,
    ) -> Result<Self, CoreError> {
        if saved.schema_version != 1
            || &saved.namespace != expected_origin
            || saved.last.as_ref() != expected_context
            || saved.dimensions != expected_dimensions
            || saved.config != Config::from(expected_config)
            || expected_config.max_tracks > MAX_DETECTIONS
        {
            return Err(invalid(
                "tracker checkpoint schema/origin/context/configuration mismatch",
            ));
        }
        let mut restored = Self::with_namespace(
            destination.clone(),
            TrackerConfig {
                max_missed_frames: saved.config.max_missed_frames,
                max_tracks: usize::from(saved.config.max_tracks),
                max_center_distance: f64::from_bits(saved.config.max_center_distance_bits),
                ambiguity_margin: f64::from_bits(saved.config.ambiguity_margin_bits),
            },
        )?;
        if saved.tracks.len() > restored.config.max_tracks
            || saved.namespace.as_str().len() > 96
            || saved.last.is_some() != saved.dimensions.is_some()
            || (saved.last.is_none() && (!saved.tracks.is_empty() || saved.next_id != 0))
        {
            return Err(invalid("tracker checkpoint topology/budget mismatch"));
        }
        if let Some([width, height]) = saved.dimensions {
            if width == 0 || height == 0 || width > 65_536 || height > 65_536 {
                return Err(invalid(
                    "tracker checkpoint dimensions exceed geometry budget",
                ));
            }
            let frame = saved
                .last
                .as_ref()
                .ok_or_else(|| invalid("checkpoint context missing"))?
                .frame
                .value();
            let mut ids = BTreeSet::new();
            for t in &saved.tracks {
                let id = saved.rebind_identity(&t.id, &destination)?;
                let [x0, y0, x1, y1] = t.bounds_bits.map(f64::from_bits);
                let bounds = CoordinateBox::new(CoordinateSpace::SourcePixels, x0, y0, x1, y1)?;
                let velocity = t.velocity_bits.map(f64::from_bits);
                let center = t.last_observed_center_bits.map(f64::from_bits);
                let score = f64::from_bits(t.score_bits);
                if !ids.insert(id.clone())
                    || t.class_id >= 256
                    || x0 < 0.0
                    || y0 < 0.0
                    || x1 > f64::from(width)
                    || y1 > f64::from(height)
                    || !fraction(score)
                    || velocity.iter().any(|v| !v.is_finite())
                    || velocity[0].abs() > f64::from(width)
                    || velocity[1].abs() > f64::from(height)
                    || center.iter().any(|v| !v.is_finite())
                    || !(0.0..=f64::from(width)).contains(&center[0])
                    || !(0.0..=f64::from(height)).contains(&center[1])
                    || t.last_observed_frame > frame
                    || t.missed > u64::from(restored.config.max_missed_frames)
                    || frame - t.last_observed_frame != t.missed
                    || (t.missed == 0 && center != [bounds.center().0, bounds.center().1])
                {
                    return Err(invalid(
                        "tracker checkpoint contains inconsistent track state",
                    ));
                }
                restored.tracks.push(Track {
                    id,
                    class_id: t.class_id,
                    bounds,
                    velocity,
                    score,
                    missed: t.missed,
                    last_observed_frame: t.last_observed_frame,
                    last_observed_center: center,
                });
            }
        }
        restored.last = saved.last.clone();
        restored.dimensions = saved.dimensions.map(|[w, h]| (w, h));
        restored.next_id = saved.next_id;
        Ok(restored)
    }
}
