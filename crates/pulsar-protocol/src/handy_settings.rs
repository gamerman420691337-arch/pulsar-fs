//! Explicit remote configuration; declarations and observations are not motion proof.
use crate::*;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
// Firmware reports settings beyond the pinned schema. Unknown fields are
// ignored while declared fields remain required and validated. Retain declared
// firmware extensions below; do not alias x_limit_end to x_limit_stop because
// choosing the wrong range can affect a device profile.
pub struct HandySliderSettings {
    pub x_limit_start: f64,
    pub x_limit_stop: f64,
    pub x_end_buffer: f64,
    pub x_end_zone_size: f64,
    pub x_end_zone_speed: f64,
    pub x_min_speed: f64,
    pub x_max_speed: f64,
    pub x_min_speed_theoretical: f64,
    pub x_max_speed_theoretical: f64,
    pub turn_time: f64,
    pub regulator_timeout: f64,
    pub temp_high_trigger: f64,
    pub temp_offset: f64,
    pub x_stroke_min: f64,
    pub x_stroke_max: f64,
    pub temp_hysteresis: f64,
    pub overclock_enabled: bool,
    pub x_inverse_motor: bool,
    pub x_inverse_hall: bool,
    /// Reported by Handy 2 Pro firmware 4.3.0 and later; absent from the pinned
    /// v3 schema. Declared so observed values are retained rather than dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_cw_is_forward: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_pitch: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks_per_rot: Option<f64>,
}
impl HandySliderSettings {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let nonnegative = [
            self.x_end_buffer,
            self.x_end_zone_size,
            self.x_end_zone_speed,
            self.x_min_speed,
            self.x_max_speed,
            self.x_min_speed_theoretical,
            self.x_max_speed_theoretical,
            self.turn_time,
            self.regulator_timeout,
            self.x_stroke_min,
            self.x_stroke_max,
            self.temp_hysteresis,
        ];
        if nonnegative.iter().any(|v| !v.is_finite() || *v < 0.0)
            || [
                self.x_limit_start,
                self.x_limit_stop,
                self.temp_high_trigger,
                self.temp_offset,
            ]
            .iter()
            .any(|v| !v.is_finite())
            || self.x_limit_start >= self.x_limit_stop
            || self.x_min_speed > self.x_max_speed
            || self.x_min_speed_theoretical > self.x_max_speed_theoretical
            || self.x_stroke_min > self.x_stroke_max
            || [self.lead_pitch, self.ticks_per_rot]
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(ProtocolError::invalid("invalid Handy slider settings"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyStrokeSettings {
    pub min: f64,
    pub max: f64,
    pub min_absolute: f64,
    pub max_absolute: f64,
}
impl HandyStrokeSettings {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if [self.min, self.max, self.min_absolute, self.max_absolute]
            .iter()
            .any(|v| !v.is_finite())
            || !(0.0..=1.0).contains(&self.min)
            || !(0.0..=1.0).contains(&self.max)
            || self.min >= self.max
            || self.min_absolute >= self.max_absolute
        {
            return Err(ProtocolError::invalid("invalid Handy stroke settings"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyOperativeSettings {
    pub slider: HandySliderSettings,
    pub stroke: HandyStrokeSettings,
}
impl HandyOperativeSettings {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.slider.validate()?;
        self.stroke.validate()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_handy2pro_slider_settings_with_firmware_extras_are_accepted() {
        // Verbatim Handy 2 Pro firmware 4.3.0 response. lead_pitch, ticks_per_rot
        // and x_cw_is_forward are absent from the pinned v3 schema but are real.
        let bytes = r#"{"result":{
            "x_limit_start":0,"x_limit_stop":124.326,"x_end_buffer":5,
            "x_end_zone_size":10,"x_end_zone_speed":200,"x_min_speed":32,
            "x_max_speed":450,"x_min_speed_theoretical":45,
            "x_max_speed_theoretical":1600,"turn_time":0,"regulator_timeout":10,
            "temp_high_trigger":80,"temp_offset":0.06,"x_stroke_min":0,
            "x_stroke_max":0.91,"x_cw_is_forward":false,"temp_hysteresis":10,
            "overclock_enabled":false,"x_inverse_motor":false,"x_inverse_hall":false,
            "lead_pitch":8,"ticks_per_rot":24,
            "an_unseen_future_field":true}}"#;
        let mut value: serde_json::Value = serde_json::from_str(bytes).unwrap();
        let body = value["result"].clone();
        let slider: HandySliderSettings = serde_json::from_value(body.clone()).unwrap();
        slider.validate().unwrap();
        assert_eq!(slider.lead_pitch, Some(8.0));
        assert_eq!(slider.ticks_per_rot, Some(24.0));
        assert_eq!(slider.x_cw_is_forward, Some(false));
        assert_eq!(slider.x_max_speed_theoretical, 1600.0);
        // Unknown fields are ignored rather than fatal; declared fields stay required.
        value["result"]
            .as_object_mut()
            .unwrap()
            .remove("x_max_speed_theoretical");
        assert!(serde_json::from_value::<HandySliderSettings>(value["result"].clone()).is_err());
    }
    #[test]
    fn handy_operative_settings_are_complete_finite_and_ordered_without_defaults() {
        let bytes = include_str!("../tests/fixtures/handy-operative-settings.json");
        let settings: HandyOperativeSettings = serde_json::from_str(bytes).unwrap();
        settings.validate().unwrap();
        for field in [
            "x_limit_stop",
            "overclock_enabled",
            "temp_offset",
            "regulator_timeout",
        ] {
            let mut value: serde_json::Value = serde_json::from_str(bytes).unwrap();
            value["slider"].as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<HandyOperativeSettings>(value).is_err());
        }
        let mut invalid = settings.clone();
        invalid.slider.x_max_speed = f64::INFINITY;
        assert!(invalid.validate().is_err());
        let mut invalid = settings.clone();
        invalid.stroke.max = 1.1;
        assert!(invalid.validate().is_err());
        let mut invalid = settings.clone();
        invalid.stroke.min_absolute = 101.0;
        assert!(invalid.validate().is_err());
        let mut invalid = settings.clone();
        invalid.slider.x_limit_stop = 0.0;
        assert!(invalid.validate().is_err());
        let mut changed = settings.clone();
        changed.slider.overclock_enabled = true;
        assert_ne!(changed, settings);
    }
    #[test]
    fn handy_qualification_v1_receipt_retains_its_canonical_identity() {
        let receipt: HandyQualificationReceipt = serde_json::from_str(include_str!(
            "../tests/fixtures/handy-qualification-v1.json"
        ))
        .unwrap();
        receipt.validate().unwrap();
        assert_eq!(receipt.report.manifest.subject.schema_version, 1);
        assert!(receipt.report.manifest.subject.operative_settings.is_none());
        let raw = serde_json::to_string(&receipt).unwrap();
        assert!(!raw.contains("operative_settings"));
        let mut subject = receipt.report.manifest.subject;
        subject.schema_version = 2;
        assert!(subject.validate().is_err());
        subject.operative_settings = Some(
            serde_json::from_str(include_str!(
                "../tests/fixtures/handy-operative-settings.json"
            ))
            .unwrap(),
        );
        subject.validate().unwrap();
        subject.schema_version = 1;
        assert!(subject.validate().is_err());
    }
}
