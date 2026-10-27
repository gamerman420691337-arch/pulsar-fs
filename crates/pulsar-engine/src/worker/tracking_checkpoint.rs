use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::worker) struct StrokeTrackerCheckpoint {
    core: pulsar_core::perception::TargetTrackerCheckpoint,
    previous: Option<SavedPrevious>,
    selected: Option<EntityTrackId>,
    position_bits: u64,
    target_seed: Option<AnalysisTargetSeed>,
    seed_consumed: bool,
    plan_sha256: String,
    last_context: Option<FrameContext>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPrevious {
    gray: Vec<u8>,
    width: u32,
    height: u32,
    track: EntityTrackId,
    bounds_bits: [u64; 4],
}
fn namespace(attempt: &AttemptId) -> Result<EntityTrackId> {
    Ok(EntityTrackId::new(format!(
        "attempt.{:x}",
        Sha256::digest(attempt.as_str().as_bytes())
    ))?)
}
impl StrokeTrackerCheckpoint {
    pub(in crate::worker) fn rebind(
        &self,
        id: &EntityTrackId,
        attempt: &AttemptId,
    ) -> Result<EntityTrackId> {
        Ok(self.core.rebind_identity(id, &namespace(attempt)?)?)
    }
}
impl StrokeTracker {
    pub(in crate::worker) fn checkpoint(&self) -> Result<StrokeTrackerCheckpoint> {
        Ok(StrokeTrackerCheckpoint {
            core: self.tracker.checkpoint(),
            previous: self.previous.as_ref().map(|p| SavedPrevious {
                gray: p.gray.clone(),
                width: p.width,
                height: p.height,
                track: p.track.clone(),
                bounds_bits: [
                    p.bounds.x_min(),
                    p.bounds.y_min(),
                    p.bounds.x_max(),
                    p.bounds.y_max(),
                ]
                .map(f64::to_bits),
            }),
            selected: self.selected.clone(),
            position_bits: self.position.value().to_bits(),
            target_seed: self.target_seed.clone(),
            seed_consumed: self.seed_consumed,
            plan_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&self.plan)?)),
            last_context: self.last_context.clone(),
        })
    }
    pub(in crate::worker) fn from_checkpoint(
        saved: &StrokeTrackerCheckpoint,
        origin: &AttemptId,
        destination: &AttemptId,
        expected: Option<&FrameContext>,
        support: ([u32; 2], Option<&[u8]>),
        seed: Option<AnalysisTargetSeed>,
        plan: TrackingPlan,
    ) -> Result<Self> {
        let (dimensions, pixels) = support;
        anyhow::ensure!(
            saved.last_context.as_ref() == expected
                && serde_json::to_vec(&saved.target_seed)? == serde_json::to_vec(&seed)?
                && saved.plan_sha256 == format!("{:x}", Sha256::digest(serde_json::to_vec(&plan)?))
                && !(saved.seed_consumed && seed.is_none()),
            "stroke checkpoint context/seed/plan mismatch"
        );
        let mut restored = Self::with_plan(destination, seed, plan)?;
        restored.tracker = TargetTracker::from_checkpoint(
            &saved.core,
            &namespace(origin)?,
            namespace(destination)?,
            expected,
            TrackerConfig {
                max_missed_frames: 30,
                ..TrackerConfig::default()
            },
            expected.map(|_| dimensions),
        )?;
        restored.selected = saved
            .selected
            .as_ref()
            .map(|id| saved.rebind(id, destination))
            .transpose()?;
        restored.position = NormalizedPosition::new(f64::from_bits(saved.position_bits))?;
        restored.seed_consumed = saved.seed_consumed;
        restored.last_context = expected.cloned();
        restored.previous = saved
            .previous
            .as_ref()
            .map(|p| -> Result<Previous> {
                anyhow::ensure!(
                    [p.width, p.height] == dimensions
                        && p.gray.len() <= 1_048_576
                        && (p.width as usize).checked_mul(p.height as usize) == Some(p.gray.len())
                        && pixels == Some(p.gray.as_slice())
                        && saved.selected.as_ref() == Some(&p.track),
                    "stroke checkpoint pixel/identity mismatch"
                );
                let [x0, y0, x1, y1] = p.bounds_bits.map(f64::from_bits);
                let bounds = CoordinateBox::new(CoordinateSpace::SourcePixels, x0, y0, x1, y1)?;
                anyhow::ensure!(
                    x0 >= 0.0 && y0 >= 0.0 && x1 <= f64::from(p.width) && y1 <= f64::from(p.height),
                    "stroke checkpoint previous bounds outside view"
                );
                Ok(Previous {
                    gray: p.gray.clone(),
                    width: p.width,
                    height: p.height,
                    track: saved.rebind(&p.track, destination)?,
                    bounds,
                })
            })
            .transpose()?;
        Ok(restored)
    }
}
