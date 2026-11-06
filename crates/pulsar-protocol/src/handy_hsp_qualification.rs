//! Explicit HSP streaming declarations. These are evidence subjects, not native
//! owner capabilities or a substitute for physical qualification and review.
use crate::*;

pub const HANDY_HSP_QUALIFICATION_TRANSPORT: &str =
    "handy-rest-v3-hsp.9ad90e385ff1bf5f454a9af5f02862ca6cb064654b30df873d7d7389198cb684";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyQualificationTransport {
    #[default]
    Hssp,
    Hsp,
}
impl HandyQualificationTransport {
    pub fn transport_id(self) -> &'static str {
        match self {
            Self::Hssp => HANDY_QUALIFICATION_TRANSPORT,
            Self::Hsp => HANDY_HSP_QUALIFICATION_TRANSPORT,
        }
    }
    pub fn is_hssp(&self) -> bool {
        *self == Self::Hssp
    }
    pub fn from_transport_id(value: &str) -> Result<Self, ProtocolError> {
        match value {
            HANDY_QUALIFICATION_TRANSPORT => Ok(Self::Hssp),
            HANDY_HSP_QUALIFICATION_TRANSPORT => Ok(Self::Hsp),
            _ => Err(ProtocolError::invalid(
                "unknown Handy qualification transport",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspPolicy {
    pub max_buffered_points: u32,
    pub device_capacity_points: u32,
    pub max_chunk_points: u32,
    pub min_lead_ms: u32,
    pub max_future_ms: u32,
    pub max_sample_age_ns: u64,
    pub max_gap_ms: u32,
}
impl HandyHspPolicy {
    /// A validated scheduling policy, never an acceptance or execution grant.
    pub fn to_policy(&self) -> Result<HspPolicy, ProtocolError> {
        let value = HspPolicy {
            max_buffered_points: usize::try_from(self.max_buffered_points)
                .map_err(|_| ProtocolError::invalid("HSP point bound exceeds platform"))?,
            device_capacity_points: usize::try_from(self.device_capacity_points)
                .map_err(|_| ProtocolError::invalid("HSP capacity exceeds platform"))?,
            max_chunk_points: usize::try_from(self.max_chunk_points)
                .map_err(|_| ProtocolError::invalid("HSP chunk bound exceeds platform"))?,
            min_lead_ms: self.min_lead_ms,
            max_future_ms: self.max_future_ms,
            max_sample_age_ns: self.max_sample_age_ns,
            max_gap_ms: self.max_gap_ms,
        };
        value
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.to_policy().map(|_| ())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspFiniteDrainQualification {
    pub schema_version: u8,
    pub model: HandyHspFiniteDrainModel,
    pub maximum_drain_duration_ns: u64,
    pub maximum_terminal_overrun_ns: u64,
    /// D runs from native tail passage to the final native Stop send gate,
    /// immediately before the first write. Queue insertion is not that gate.
    pub maximum_stop_dispatch_delay_ns: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspFiniteDrainModel {
    /// Physical evidence must establish terminal behavior and applicability of
    /// the existing stopping bound S from the actual final native send gate.
    /// The controller projects to now + D + S; passage is not physical completion.
    EarliestPhaseTailThenOwnedStop,
}
impl HandyHspFiniteDrainQualification {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1
            || self.maximum_drain_duration_ns == 0
            || self.maximum_terminal_overrun_ns == 0
            || self.maximum_stop_dispatch_delay_ns == 0
        {
            return Err(ProtocolError::invalid(
                "invalid HSP finite drain declaration",
            ));
        }
        Ok(())
    }
    /// Checked D + S using this subject's unchanged qualified stopping bound.
    /// This arithmetic does not establish acceptance or native send authority.
    pub fn stop_budget_ns(&self, stopping_bound_ns: Option<u64>) -> Result<u64, ProtocolError> {
        self.validate()?;
        let stopping_bound_ns = stopping_bound_ns.filter(|s| *s > 0).ok_or_else(|| {
            ProtocolError::invalid("HSP finite drain requires the exact qualified stopping bound")
        })?;
        self.maximum_stop_dispatch_delay_ns
            .checked_add(stopping_bound_ns)
            .filter(|budget| *budget <= self.maximum_terminal_overrun_ns)
            .ok_or_else(|| ProtocolError::invalid("HSP finite drain Stop budget exceeds overrun"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspQualificationProfile {
    pub policy: HandyHspPolicy,
    pub playback_rate: u8,
    pub looping: bool,
    pub pause_on_starving: bool,
    pub append_flush: bool,
    /// Physical evidence must establish this operating assumption. The vendor
    /// protocol has no atomic expected-stream lease that enforces it remotely.
    pub sole_controller: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finite_drain: Option<HandyHspFiniteDrainQualification>,
}
impl HandyHspQualificationProfile {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.policy.validate()?;
        if let Some(finite) = &self.finite_drain {
            finite.validate()?;
        }
        if self.playback_rate != 1
            || self.looping
            || self.pause_on_starving
            || self.append_flush
            || !self.sole_controller
        {
            return Err(ProtocolError::invalid(
                "invalid HSP streaming options or controller assumption",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> HandyHspQualificationProfile {
        HandyHspQualificationProfile {
            policy: HandyHspPolicy {
                max_buffered_points: 100,
                device_capacity_points: 100,
                max_chunk_points: 50,
                min_lead_ms: 100,
                max_future_ms: 1000,
                max_sample_age_ns: 10_000_000,
                max_gap_ms: 100,
            },
            playback_rate: 1,
            looping: false,
            pause_on_starving: false,
            append_flush: false,
            sole_controller: true,
            finite_drain: None,
        }
    }
    #[test]
    fn hsp_qualification_policy_reuses_core_bounds_and_fixed_wire_options() {
        let good = profile();
        good.validate().unwrap();
        assert_eq!(good.policy.to_policy().unwrap().max_chunk_points, 50);
        for case in 0..16 {
            let mut bad = good.clone();
            match case {
                0 => bad.policy.max_buffered_points = 4097,
                1 => bad.policy.max_buffered_points = 1,
                2 => bad.policy.device_capacity_points = 101,
                3 => bad.policy.device_capacity_points = 1,
                4 => bad.policy.max_chunk_points = 101,
                5 => bad.policy.max_chunk_points = 1,
                6 => bad.policy.device_capacity_points = 49,
                7 => bad.policy.min_lead_ms = 0,
                8 => bad.policy.max_future_ms = 99,
                9 => bad.policy.max_sample_age_ns = 0,
                10 => bad.policy.max_gap_ms = 0,
                11 => bad.playback_rate = 2,
                12 => bad.looping = true,
                13 => bad.pause_on_starving = true,
                14 => bad.append_flush = true,
                _ => bad.sole_controller = false,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        let wire = serde_json::to_vec(&good).unwrap();
        let decoded: HandyHspQualificationProfile = serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded, good);
        let mut authority = serde_json::to_value(good).unwrap();
        authority["accepted"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandyHspQualificationProfile>(authority).is_err());
    }
    #[test]
    fn hsp_qualification_transport_selection_is_closed_and_preserves_hssp_default() {
        assert!(HandyQualificationTransport::default().is_hssp());
        for selected in [
            HandyQualificationTransport::Hssp,
            HandyQualificationTransport::Hsp,
        ] {
            assert_eq!(
                HandyQualificationTransport::from_transport_id(selected.transport_id()).unwrap(),
                selected
            );
        }
        assert!(HandyQualificationTransport::from_transport_id("handy-rest-v3-hsp").is_err());
        assert!(serde_json::from_str::<HandyQualificationTransport>("\"http\"").is_err());
    }

    #[test]
    fn finite_drain_requires_explicit_model_positive_bounds_and_checked_existing_stop_budget() {
        let finite = HandyHspFiniteDrainQualification {
            schema_version: 1,
            model: HandyHspFiniteDrainModel::EarliestPhaseTailThenOwnedStop,
            maximum_drain_duration_ns: 1000,
            maximum_terminal_overrun_ns: 100,
            maximum_stop_dispatch_delay_ns: 20,
        };
        assert_eq!(finite.stop_budget_ns(Some(80)).unwrap(), 100);
        for stop in [None, Some(0), Some(81), Some(u64::MAX)] {
            assert!(finite.stop_budget_ns(stop).is_err());
        }
        for case in 0..5 {
            let mut bad = finite.clone();
            match case {
                0 => bad.schema_version = 2,
                1 => bad.maximum_drain_duration_ns = 0,
                2 => bad.maximum_terminal_overrun_ns = 0,
                3 => bad.maximum_stop_dispatch_delay_ns = 0,
                _ => {
                    bad.maximum_stop_dispatch_delay_ns = u64::MAX;
                    bad.maximum_terminal_overrun_ns = u64::MAX;
                }
            }
            assert!(bad.stop_budget_ns(Some(80)).is_err(), "case {case}");
        }
        for field in [
            "model",
            "schema_version",
            "maximum_drain_duration_ns",
            "maximum_terminal_overrun_ns",
            "maximum_stop_dispatch_delay_ns",
        ] {
            let mut missing = serde_json::to_value(&finite).unwrap();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<HandyHspFiniteDrainQualification>(missing).is_err());
        }
        let mut unknown = serde_json::to_value(&finite).unwrap();
        unknown["model"] = serde_json::json!("latest_phase_is_physical_completion");
        assert!(serde_json::from_value::<HandyHspFiniteDrainQualification>(unknown).is_err());
        let mut unknown = serde_json::to_value(finite).unwrap();
        unknown["stopping_bound_ns"] = serde_json::json!(1);
        assert!(serde_json::from_value::<HandyHspFiniteDrainQualification>(unknown).is_err());
    }

    #[test]
    fn absent_finite_drain_preserves_frozen_profile_bytes() {
        let expected = br#"{"policy":{"max_buffered_points":100,"device_capacity_points":100,"max_chunk_points":50,"min_lead_ms":100,"max_future_ms":1000,"max_sample_age_ns":10000000,"max_gap_ms":100},"playback_rate":1,"looping":false,"pause_on_starving":false,"append_flush":false,"sole_controller":true}"#;
        let p: HandyHspQualificationProfile = serde_json::from_slice(expected).unwrap();
        assert_eq!(p, profile());
        assert_eq!(serde_json::to_vec(&p).unwrap(), expected);
        assert!(p.finite_drain.is_none());
    }
}
