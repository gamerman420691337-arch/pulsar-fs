//! Explicit remote configuration; declarations and observations are not motion proof.
use crate::*;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
