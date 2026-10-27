//! Effective bounded image-flow work. Model selection stays explicit.
use anyhow::Result;
use pulsar_core::perception::{
    track_points_fb, track_points_fb_multiscale, FlowConfig, PointFlow, PyramidFlowSummary,
};
use pulsar_protocol::ProtocolError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Search {
    SingleScale,
    CheckedPyramid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TrackingPlan {
    pub id: &'static str,
    pub target_points: usize,
    pub background_points: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_recovery_points: Option<usize>,
    pub search_radius: u32,
    pub search: Search,
    pub sparse_recovery_points: Option<usize>,
}
impl TrackingPlan {
    pub fn resolve(preset: &str) -> Result<Self> {
        Ok(match preset {
            "fast" | "economy" => Self {
                id: "visual-flow-fast-v1",
                target_points: 8,
                background_points: 32,
                background_recovery_points: None,
                search_radius: 6,
                search: Search::SingleScale,
                sparse_recovery_points: None,
            },
            "default" => Self::default(),
            "maximum" | "quality" => Self {
                id: "visual-flow-maximum-v2",
                target_points: 64,
                background_points: 64,
                background_recovery_points: Some(64),
                search_radius: 8,
                search: Search::CheckedPyramid,
                sparse_recovery_points: None,
            },
            _ => {
                return Err(
                    ProtocolError::unsupported(format!("generation preset {preset}")).into(),
                )
            }
        })
    }
    pub fn config(self) -> FlowConfig {
        FlowConfig {
            search_radius: self.search_radius,
            ..FlowConfig::default()
        }
    }
    pub fn track(
        self,
        previous: &[u8],
        current: &[u8],
        width: u32,
        height: u32,
        points: &[[f64; 2]],
    ) -> Result<(Vec<PointFlow>, PyramidFlowSummary)> {
        match self.search {
            Search::CheckedPyramid => Ok(track_points_fb_multiscale(
                previous,
                current,
                width,
                height,
                points,
                self.config(),
            )?),
            Search::SingleScale => Ok((
                track_points_fb(previous, current, width, height, points, self.config())?,
                PyramidFlowSummary {
                    maximum_level: 0,
                    recovered_points: 0,
                    component_displacement_bound: 8.5,
                },
            )),
        }
    }
}
impl Default for TrackingPlan {
    fn default() -> Self {
        Self {
            id: "visual-flow-default-v4",
            target_points: 16,
            background_points: 64,
            background_recovery_points: None,
            search_radius: 6,
            search: Search::CheckedPyramid,
            sparse_recovery_points: Some(48),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_preserve_default_and_make_aliases_explicit() {
        let fast = TrackingPlan::resolve("fast").unwrap();
        let default = TrackingPlan::resolve("default").unwrap();
        let maximum = TrackingPlan::resolve("maximum").unwrap();
        assert_eq!(default, TrackingPlan::default());
        assert_eq!(fast, TrackingPlan::resolve("economy").unwrap());
        assert_eq!(maximum, TrackingPlan::resolve("quality").unwrap());
        assert!(
            fast.target_points < default.target_points
                && default.target_points < maximum.target_points
        );
        assert!(TrackingPlan::resolve("unknown").is_err());
    }
    #[test]
    fn actual_search_modes_preserve_cycle_checks_and_refuse_flat_evidence() {
        let size = 256usize;
        let image: Vec<_> = (0..size * size)
            .map(|i| {
                let x = i % size;
                let y = i / size;
                ((x * 37 + y * 61 + x * y * 7) % 251) as u8
            })
            .collect();
        let flat = vec![128; size * size];
        for preset in ["fast", "default", "maximum"] {
            let plan = TrackingPlan::resolve(preset).unwrap();
            let (flows, summary) = plan
                .track(
                    &image,
                    &image,
                    256,
                    256,
                    &[[100., 100.], [140., 120.], [110., 150.]],
                )
                .unwrap();
            assert!(flows
                .iter()
                .all(|f| f.displacement == Some([0., 0.]) && f.forward_backward_error == Some(0.)));
            assert_eq!(summary.maximum_level, if preset == "fast" { 0 } else { 2 });
            let (flows, _) = plan
                .track(
                    &flat,
                    &flat,
                    256,
                    256,
                    &[[100., 100.], [140., 120.], [110., 150.]],
                )
                .unwrap();
            assert!(flows.iter().all(|f| f.displacement.is_none()));
            assert!(plan
                .track(&image[..3], &image, 256, 256, &[[100., 100.]])
                .is_err());
        }
    }
}
