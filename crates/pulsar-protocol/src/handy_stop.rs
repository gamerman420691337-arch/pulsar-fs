//! Bounded stop observations. Neither history nor native activity proves physical stopping.
use crate::*;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyStopOutcome {
    Acknowledged,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyStopPhase {
    Pending,
    InFlight,
    Observed,
    Rejected,
}

/// A native owner's last observed activity; this view may lag execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyStopActivity {
    pub record_id: ArtifactId,
    pub emergency: bool,
    pub phase: HandyStopPhase,
    pub outcome: Option<HandyStopOutcome>,
    pub metadata_recorded: bool,
}
impl HandyStopActivity {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.record_id.as_str())?;
        let observed = self.phase == HandyStopPhase::Observed;
        if observed != self.outcome.is_some() || (self.metadata_recorded && !observed) {
            return Err(ProtocolError::invalid(
                "invalid Handy stop activity observation",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyStopHistory {
    pub record_id: ArtifactId,
    pub sequence: u64,
    pub outcome: Option<HandyStopOutcome>,
    pub emergency: bool,
}
impl HandyStopHistory {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.record_id.as_str())?;
        if self.sequence == 0 || (self.emergency && self.outcome.is_none()) {
            return Err(ProtocolError::invalid(
                "invalid Handy stop history observation",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyStopStatus {
    pub project_id: ProjectId,
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub history: Vec<HandyStopHistory>,
    pub native: Option<HandyStopActivity>,
    /// Always false: this journal view is bounded, and unrecorded emergency
    /// attempts can disappear on restart.
    pub history_complete: bool,
}
impl HandyStopStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.history_complete || self.history.len() > 128 {
            return Err(ProtocolError::invalid(
                "Handy stop history must remain bounded and incomplete",
            ));
        }
        let mut records = HashSet::with_capacity(self.history.len());
        for entry in &self.history {
            entry.validate()?;
            if !records.insert(entry.record_id.as_str()) {
                return Err(ProtocolError::invalid(
                    "duplicate Handy stop history record",
                ));
            }
        }
        if let Some(activity) = &self.native {
            activity.validate()?;
            if let Some(record) = self
                .history
                .iter()
                .find(|r| r.record_id == activity.record_id)
            {
                if record.outcome.is_some()
                    && activity.outcome.is_some()
                    && (record.outcome != activity.outcome
                        || record.emergency != activity.emergency)
                {
                    return Err(ProtocolError::invalid(
                        "conflicting terminal Handy stop observations",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> HandyStopStatus {
        HandyStopStatus {
            project_id: ProjectId::new("project").unwrap(),
            acquisition_request_id: RequestId::new("acquire").unwrap(),
            controller_id: ClientId::new("controller").unwrap(),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("device-session").unwrap(),
            history: vec![HandyStopHistory {
                record_id: ArtifactId::new("a".repeat(64)).unwrap(),
                sequence: 1,
                outcome: None,
                emergency: false,
            }],
            native: None,
            history_complete: false,
        }
    }

    #[test]
    fn handy_stop_native_and_durable_terminal_observations_cannot_conflict() {
        let mut value = report();
        value.native = Some(HandyStopActivity {
            record_id: value.history[0].record_id.clone(),
            emergency: true,
            phase: HandyStopPhase::Observed,
            outcome: Some(HandyStopOutcome::Acknowledged),
            metadata_recorded: false,
        });
        // A prewrite can remain incomplete while an emergency result awaits metadata.
        value.validate().unwrap();
        value.history[0].outcome = Some(HandyStopOutcome::Unknown);
        assert!(value.validate().is_err());
        value.history[0].outcome = Some(HandyStopOutcome::Acknowledged);
        assert!(value.validate().is_err());
        value.history[0].emergency = true;
        value.validate().unwrap();
    }
    #[test]
    fn handy_stop_status_command_is_read_only_scoped_revision_free_and_redacted() {
        let mut request = Request::new(
            RequestId::new("read-stop").unwrap(),
            Command::HandyStopStatus {
                acquisition_request_id: RequestId::new("original-acquire").unwrap(),
                owner_proof: "synthetic-private-owner-proof".into(),
            },
        );
        request.auth_token = Some("synthetic-auth-token".into());
        assert!(request.command.requires_project());
        assert!(!request.command.requires_revision());
        assert!(!request.command.is_mutating());
        assert!(request.validate().is_err());
        request.project = Some(ProjectId::new("project").unwrap());
        request.validate().unwrap();
        for debug in [format!("{:?}", request), format!("{:?}", request.command)] {
            assert!(!debug.contains("synthetic-private-owner-proof"));
            assert!(!debug.contains("synthetic-auth-token"));
        }
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["command"]["operation"], "handy_stop_status");
        let decoded: Request = serde_json::from_value(json).unwrap();
        decoded.validate().unwrap();
        assert!(
            matches!(decoded.command, Command::HandyStopStatus { acquisition_request_id, .. }
            if acquisition_request_id.as_str() == "original-acquire")
        );
        for proof in [
            String::new(),
            " ".into(),
            "a".repeat(65),
            "bad\nproof".into(),
        ] {
            request.command = Command::HandyStopStatus {
                acquisition_request_id: RequestId::new("acquire").unwrap(),
                owner_proof: proof,
            };
            assert!(request.validate().is_err());
        }
        request.command = Command::HandyStopStatus {
            acquisition_request_id: RequestId::new("acquire").unwrap(),
            owner_proof: "a".repeat(64),
        };
        request.validate().unwrap();
    }

    #[test]
    fn handy_stop_activity_preserves_pending_metadata_observed_and_rejected_states() {
        for phase in [
            HandyStopPhase::Pending,
            HandyStopPhase::InFlight,
            HandyStopPhase::Observed,
            HandyStopPhase::Rejected,
        ] {
            for outcome in [
                None,
                Some(HandyStopOutcome::Acknowledged),
                Some(HandyStopOutcome::Unknown),
            ] {
                for metadata_recorded in [false, true] {
                    for emergency in [false, true] {
                        let activity = HandyStopActivity {
                            record_id: ArtifactId::new("a".repeat(64)).unwrap(),
                            emergency,
                            phase,
                            outcome,
                            metadata_recorded,
                        };
                        let observed = phase == HandyStopPhase::Observed;
                        assert_eq!(
                            activity.validate().is_ok(),
                            observed == outcome.is_some() && (!metadata_recorded || observed)
                        );
                    }
                }
            }
        }
        let mut value = report();
        value.native = Some(HandyStopActivity {
            record_id: value.history[0].record_id.clone(),
            emergency: false,
            phase: HandyStopPhase::Observed,
            outcome: Some(HandyStopOutcome::Unknown),
            metadata_recorded: false,
        });
        value.validate().unwrap();
        value.native.as_mut().unwrap().record_id = ArtifactId::new("not-a-digest").unwrap();
        assert!(value.validate().is_err());
    }

    #[test]
    fn handy_stop_history_requires_bounded_unique_records_and_terminal_emergency_metadata() {
        let mut value = report();
        value.history = (1..=128)
            .map(|sequence| HandyStopHistory {
                record_id: ArtifactId::new(format!("{sequence:064x}")).unwrap(),
                sequence,
                outcome: None,
                emergency: false,
            })
            .collect();
        value.validate().unwrap();
        value.history.push(HandyStopHistory {
            record_id: ArtifactId::new("f".repeat(64)).unwrap(),
            sequence: 129,
            outcome: None,
            emergency: false,
        });
        assert!(value.validate().is_err());
        for case in 0..5 {
            let mut invalid = report();
            match case {
                0 => invalid.history_complete = true,
                1 => invalid.history[0].sequence = 0,
                2 => invalid.history[0].record_id = ArtifactId::new("invalid-digest").unwrap(),
                3 => invalid.history.push(invalid.history[0].clone()),
                _ => invalid.history[0].emergency = true,
            }
            assert!(invalid.validate().is_err());
        }
        for outcome in [HandyStopOutcome::Acknowledged, HandyStopOutcome::Unknown] {
            let mut value = report();
            value.history[0].emergency = true;
            value.history[0].outcome = Some(outcome);
            value.validate().unwrap();
        }
        let mut empty = report();
        empty.history.clear();
        empty.validate().unwrap();
        empty.history_complete = true;
        assert!(empty.validate().is_err());
    }

    #[test]
    fn handy_stop_wire_round_trip_refuses_authority_shaped_extensions() {
        let value = report();
        let body = ResponseBody::HandyStop(Box::new(value.clone()));
        let wire = serde_json::to_value(body).unwrap();
        assert_eq!(wire["kind"], "handy_stop");
        let ResponseBody::HandyStop(decoded) = serde_json::from_value(wire).unwrap() else {
            panic!("stop status response variant changed");
        };
        assert_eq!(*decoded, value);
        decoded.validate().unwrap();
        for field in [
            "physical_stopped",
            "physical_stop_pending",
            "playback_enabled",
            "current",
            "qualified",
        ] {
            let mut malformed = serde_json::to_value(&value).unwrap();
            malformed[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyStopStatus>(malformed).is_err());
        }
        let mut malformed = serde_json::to_value(&value).unwrap();
        malformed["history"][0]["physical_stopped"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandyStopStatus>(malformed).is_err());
        let native = serde_json::json!({
            "record_id": "a".repeat(64), "emergency": false, "phase": "observed",
            "outcome": "acknowledged", "metadata_recorded": true, "current": true,
        });
        assert!(serde_json::from_value::<HandyStopActivity>(native).is_err());
    }
}
