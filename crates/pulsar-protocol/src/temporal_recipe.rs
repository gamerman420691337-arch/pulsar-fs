//! Shared annotated-view declarations and fixed model metadata, not qualification.
use crate::{ProtocolError, VideoView};
use serde::{Deserialize, Serialize};

pub const ANNOTATED_TEMPORAL_INPUT_NAME: &str = "target_frame_pair";
pub const ANNOTATED_TEMPORAL_RECIPE: &str = "annotated-temporal-view-v1";

/// Content-bound file view, independent of local paths and project source aliases.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotatedGenerationView {
    pub source_sha256: String,
    pub declaration: AnnotatedTemporalView,
}

impl AnnotatedGenerationView {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::validate_sha256(&self.source_sha256)?;
        self.declaration.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotatedTemporalView {
    pub schema_version: u16,
    pub source_dimensions: [u32; 2],
    pub analysis_dimensions: [u32; 2],
    pub video_view: VideoView,
    /// Integer x/y/width/height in the resolved native view before final resize.
    pub target_region: [u32; 4],
    /// Annotation identity only; no anatomical/physical claim is inferred.
    pub target_id: String,
}

impl AnnotatedTemporalView {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1
            || self.source_dimensions.iter().any(|d| *d == 0 || *d > 16384)
            || self.target_id.is_empty()
            || self.target_id.len() > 64
            || !self
                .target_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        {
            return Err(ProtocolError::invalid(
                "invalid annotated training view declaration",
            ));
        }
        self.video_view
            .eye_crop(self.source_dimensions[0], self.source_dimensions[1])
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        let [width, height] = self.analysis_dimensions;
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || u64::from(width) * u64::from(height) > 4 * 1024 * 1024
        {
            return Err(ProtocolError::invalid(
                "annotated analysis dimensions exceed admission",
            ));
        }
        // Flat views may resolve below these maxima; the native planner also
        // checks the region against its actual resolved dimensions.
        let [x, y, w, h] = self.target_region;
        if w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|right| right > width)
            || y.checked_add(h).is_none_or(|bottom| bottom > height)
        {
            return Err(ProtocolError::invalid(
                "target region exceeds annotated analysis bounds",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotatedTemporalModelContract {
    pub schema_version: u16,
    pub recipe: String,
    pub input_name: String,
    pub input_shape: [u32; 4],
    pub output_name: String,
    pub output_shape: [u32; 2],
    pub dtype: String,
    pub layout: String,
    pub normalization: String,
    pub output_range: [f64; 2],
    pub channel_order: String,
    pub orientation: String,
    pub view_policy: String,
}

impl AnnotatedTemporalModelContract {
    pub fn expected() -> Self {
        Self {
            schema_version: 1,
            recipe: ANNOTATED_TEMPORAL_RECIPE.into(),
            input_name: ANNOTATED_TEMPORAL_INPUT_NAME.into(),
            input_shape: [1, 6, 64, 64],
            output_name: "script_position".into(),
            output_shape: [1, 1],
            dtype: "float32".into(),
            layout: "nchw".into(),
            normalization: "byte / 255".into(),
            output_range: [0.0, 1.0],
            channel_order: "previous RGB then current RGB".into(),
            orientation: "encoded source; display rotation disabled".into(),
            view_policy:
                "declared native eye/projection, static integer target region, nearest 64x64".into(),
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self != &Self::expected() {
            return Err(ProtocolError::invalid(
                "annotated temporal model recipe differs",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_recipe_is_exact_and_unknown_fields_refuse() {
        let value = AnnotatedTemporalModelContract::expected();
        value.validate().unwrap();
        let mut bad = value.clone();
        bad.input_name = "frame_pair".into();
        assert!(bad.validate().is_err());
        let mut bad = value.clone();
        bad.normalization = "byte / 254".into();
        assert!(bad.validate().is_err());
        let mut json = serde_json::to_value(value).unwrap();
        json["extra"] = true.into();
        assert!(serde_json::from_value::<AnnotatedTemporalModelContract>(json).is_err());
    }
}
