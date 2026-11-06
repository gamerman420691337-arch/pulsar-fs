//! Engine-resolved placements are immutable preview inputs, not client authority.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedPreviewRequest {
    pub placement: SourcePlacement,
    pub project_time: ProjectTime,
}
impl PlacedPreviewRequest {
    pub fn validate(&self, context: &FrameContext) -> Result<(), ProtocolError> {
        self.placement
            .spec
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if context.source_version != self.placement.spec.source_version
            || context.source_placement != self.placement.placement_id
        {
            return Err(ProtocolError::invalid(
                "preview context differs from resolved source placement",
            ));
        }
        self.placement
            .spec
            .source_at_project(self.project_time)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedPreviewReceipt {
    pub request: PlacedPreviewRequest,
    pub actual_project_time: ExactSeconds,
    pub quantization: ExactTimeQuantization,
}
