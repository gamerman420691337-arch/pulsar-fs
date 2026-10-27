//! Historical setup metadata. No state here is loaded-program or playback authority.
use crate::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandySetupState {
    Prepared,
    Claimed,
    Acknowledged,
    Unknown,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandySetupHistory {
    pub record_id: ArtifactId,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub revision: RevisionId,
    pub device_id: DeviceId,
    pub registration_revision: u64,
    pub qualification: HandyQualificationBinding,
    pub hosting_request_id: RequestId,
    pub master_sha256: String,
    pub script: ArtifactIdentity,
    pub hosted: ArtifactIdentity,
    pub state: HandySetupState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledgment: Option<HandySetupAcknowledgment>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandySetupStatus {
    pub project_id: ProjectId,
    pub setup_request_id: RequestId,
    pub history: Option<HandySetupHistory>,
    pub loaded: bool,
    pub qualified: bool,
}
impl HandySetupStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.loaded || self.qualified {
            return Err(ProtocolError::invalid(
                "setup history cannot grant loaded or qualified authority",
            ));
        }
        if let Some(h) = &self.history {
            if let Some(acknowledgment) = &h.acknowledgment {
                acknowledgment.validate()?;
                if h.state != HandySetupState::Acknowledged {
                    return Err(ProtocolError::invalid(
                        "setup acknowledgment requires acknowledged history",
                    ));
                }
            }
            validate_sha256(h.record_id.as_str())?;
            validate_sha256(&h.master_sha256)?;
            h.qualification.validate()?;
            if h.engine_epoch == h.device_session
                || h.hosting_request_id == self.setup_request_id
                || h.registration_revision == 0
                || h.registration_revision > i64::MAX as u64
            {
                return Err(ProtocolError::invalid("invalid setup history binding"));
            }
            for (a, limit) in [
                (&h.script, HANDY_STROKE_SCRIPT_MAX_BYTES),
                (&h.hosted, HANDY_HOSTED_SCRIPT_MAX_BYTES),
            ] {
                a.validate()?;
                if a.byte_len == 0 || a.byte_len > limit || a.artifact_id.as_str() != a.sha256 {
                    return Err(ProtocolError::invalid(
                        "invalid setup history content identity",
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
    #[test]
    fn handy_setup_status_is_read_only_and_cannot_report_loaded_authority() {
        let mut r = Request::new(
            RequestId::new("query").unwrap(),
            Command::HandySetupStatus {
                setup_request_id: RequestId::new("original").unwrap(),
                owner_proof: "a".repeat(64),
            },
        );
        assert!(!r.command.is_mutating());
        assert!(r.command.requires_project());
        assert!(!r.command.requires_revision());
        assert!(r.validate().is_err());
        r.project = Some(ProjectId::new("project").unwrap());
        r.validate().unwrap();
        let mut value = HandySetupStatus {
            project_id: r.project.unwrap(),
            setup_request_id: RequestId::new("original").unwrap(),
            history: None,
            loaded: false,
            qualified: false,
        };
        value.validate().unwrap();
        value.loaded = true;
        assert!(value.validate().is_err());
        value.loaded = false;
        value.qualified = true;
        assert!(value.validate().is_err());
    }
}

#[cfg(test)]
mod setup_command_tests {
    use super::*;
    #[test]
    fn setup_command_requires_revision_qualification_and_explicit_effect_consent() {
        let mut r = Request::new(
            RequestId::new("setup").unwrap(),
            Command::SetupHandyDevice {
                qualification: HandyQualificationBinding {
                    dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                    revision: 2,
                },
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                device_session: DeviceSessionId::new("session").unwrap(),
                hosting_request_id: RequestId::new("hosting").unwrap(),
                allow_online_setup: true,
                owner_proof: "b".repeat(64),
            },
        );
        assert!(
            r.command.is_mutating()
                && r.command.requires_project()
                && r.command.requires_revision()
        );
        assert!(r.validate().is_err());
        r.project = Some(ProjectId::new("project").unwrap());
        assert!(r.validate().is_err());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::SetupHandyDevice {
            allow_online_setup, ..
        } = &mut r.command
        {
            *allow_online_setup = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
    }
}

/// Durable local cancellation receipt. It never reports physical stopping.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandySetupCancellation {
    pub request_id: RequestId,
    pub status: HandySetupStatus,
    pub settled: bool,
}
impl HandySetupCancellation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.status.validate()?;
        let history =
            self.status.history.as_ref().ok_or_else(|| {
                ProtocolError::invalid("cancellation requires exact setup history")
            })?;
        if self.request_id == self.status.setup_request_id
            || !matches!(
                history.state,
                HandySetupState::Prepared | HandySetupState::Cancelled
            )
            || (self.settled && history.state != HandySetupState::Cancelled)
        {
            return Err(ProtocolError::invalid(
                "invalid unclaimed setup cancellation receipt",
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[test]
    fn handy_setup_cancellation_requires_separate_identity_record_pin_and_project_without_revision()
    {
        let mut r = Request::new(
            RequestId::new("cancel").unwrap(),
            Command::CancelHandySetup {
                setup_request_id: RequestId::new("setup").unwrap(),
                record_id: ArtifactId::new("a".repeat(64)).unwrap(),
                owner_proof: "b".repeat(64),
            },
        );
        assert!(
            r.command.is_mutating()
                && r.command.requires_project()
                && !r.command.requires_revision()
        );
        assert!(r.validate().is_err());
        r.project = Some(ProjectId::new("project").unwrap());
        r.validate().unwrap();
        r.request_id = RequestId::new("setup").unwrap();
        assert!(r.validate().is_err());
        r.request_id = RequestId::new("cancel").unwrap();
        if let Command::CancelHandySetup { record_id, .. } = &mut r.command {
            *record_id = ArtifactId::new("not-a-digest").unwrap();
        }
        assert!(r.validate().is_err());
    }
}

/// Sanitized historical HSP response. No device-content hash or current playback authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandySetupAcknowledgment {
    pub play_state: u8,
    pub pause_on_starving: bool,
    pub points: u64,
    pub max_points: f64,
    pub current_point: u64,
    pub current_time: u64,
    pub r#loop: bool,
    pub playback_rate: f64,
    pub first_point_time: u64,
    pub last_point_time: u64,
    pub stream_id: u64,
    pub tail_point_stream_index: u64,
    pub tail_point_stream_index_threshold: u64,
}
impl HandySetupAcknowledgment {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        const MAX_EXACT: u64 = 9_007_199_254_740_991;
        if !matches!(self.play_state, 0 | 2)
            || self.r#loop
            || self.playback_rate != 1.0
            || !self.max_points.is_finite()
            || self.max_points < 0.0
            || self.max_points.fract() != 0.0
            || self.max_points > MAX_EXACT as f64
            || self.points as f64 > self.max_points
            || self.first_point_time > self.last_point_time
            || [
                self.points,
                self.current_point,
                self.current_time,
                self.first_point_time,
                self.last_point_time,
                self.stream_id,
                self.tail_point_stream_index,
                self.tail_point_stream_index_threshold,
            ]
            .iter()
            .any(|v| *v > MAX_EXACT)
        {
            return Err(ProtocolError::invalid(
                "invalid historical setup acknowledgment",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod acknowledgment_tests {
    use super::*;
    #[test]
    fn handy_setup_acknowledgment_is_bounded_historical_data_with_no_invented_fields() {
        let raw = serde_json::json!({"play_state":2,"pause_on_starving":false,"points":2,"max_points":2.0,"current_point":0,"current_time":0,"loop":false,"playback_rate":1.0,"first_point_time":0,"last_point_time":1000,"stream_id":8,"tail_point_stream_index":1,"tail_point_stream_index_threshold":1});
        let good: HandySetupAcknowledgment = serde_json::from_value(raw.clone()).unwrap();
        good.validate().unwrap();
        for field in raw.as_object().unwrap().keys() {
            let mut partial = raw.clone();
            partial.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<HandySetupAcknowledgment>(partial).is_err());
        }
        for (field, value) in [
            ("play_state", serde_json::json!(1)),
            ("loop", serde_json::json!(true)),
            ("points", serde_json::json!(3)),
            ("max_points", serde_json::json!(2.5)),
            ("current_time", serde_json::json!(9_007_199_254_740_992u64)),
        ] {
            let mut invalid = raw.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<HandySetupAcknowledgment>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
        let mut infinite = good;
        infinite.max_points = f64::INFINITY;
        assert!(infinite.validate().is_err());
        let mut extra = raw;
        extra["loaded"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandySetupAcknowledgment>(extra).is_err());
    }
}
