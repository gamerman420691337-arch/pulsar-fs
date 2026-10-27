//! Remote observations for preflight, never setup, qualification or stopping evidence.
use crate::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyRemotePlayState {
    NotInitialized,
    Playing,
    Stopped,
    Paused,
    Starving,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHsspObservation {
    pub play_state: HandyRemotePlayState,
    pub stream_id: u64,
    pub current_time_ms: u64,
    pub playback_rate: f64,
    pub looping: bool,
    pub pause_on_starving: bool,
}
impl HandyHsspObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.stream_id > (1u64 << 53) - 1
            || self.current_time_ms > (1u64 << 53) - 1
            || !self.playback_rate.is_finite()
            || self.playback_rate <= 0.0
        {
            return Err(ProtocolError::invalid("invalid remote HSSP observation"));
        }
        Ok(())
    }
}
/// Bounded HSP metadata. Signed sentinels and pre-Play rates remain observations,
/// never playback authority or evidence of physical stopping.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspObservation {
    pub play_state: HandyRemotePlayState,
    pub stream_id: u32,
    pub current_time_ms: i32,
    pub points: u32,
    pub max_points: u32,
    pub current_point: i32,
    pub playback_rate: f64,
    pub looping: bool,
    pub pause_on_starving: bool,
    pub first_point_time_ms: u32,
    pub last_point_time_ms: u32,
    pub tail_point_stream_index: i32,
    pub tail_point_stream_index_threshold: u32,
}
impl HandyHspObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.current_point < -1
            || self.tail_point_stream_index < -1
            || !self.playback_rate.is_finite()
        {
            return Err(ProtocolError::invalid("invalid remote HSP observation"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyDeviceMode {
    pub mode: u8,
    pub mode_session_id: u64,
}
impl HandyDeviceMode {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.mode > 9 || self.mode_session_id > (1u64 << 53) - 1 {
            return Err(ProtocolError::invalid("invalid Handy mode observation"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandySetupDeviceObservation {
    pub identity: HandyIdentity,
    pub mode: HandyDeviceMode,
    pub operative_settings: HandyOperativeSettings,
    pub remote_hssp: Option<HandyHsspObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_hsp: Option<HandyHspObservation>,
    pub observed_at_unix_ms: u64,
}
impl HandySetupDeviceObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.identity.validate()?;
        self.mode.validate()?;
        self.operative_settings.validate()?;
        if self.observed_at_unix_ms == 0 || (self.mode.mode == 1) != self.remote_hssp.is_some() {
            return Err(ProtocolError::invalid(
                "inconsistent Handy setup observation",
            ));
        }
        if let Some(hssp) = &self.remote_hssp {
            hssp.validate()?;
        }
        if let Some(hsp) = &self.remote_hsp {
            if self.mode.mode != 4 || self.remote_hssp.is_some() {
                return Err(ProtocolError::invalid("inconsistent HSP mode observation"));
            }
            hsp.validate()?;
        }
        Ok(())
    }
    pub fn same_state(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.mode == other.mode
            && self.operative_settings == other.operative_settings
            && self.remote_hssp == other.remote_hssp
            && self.remote_hsp == other.remote_hsp
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandySetupInspection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_qualification: Option<HandyQualificationBinding>,
    pub request_id: RequestId,
    pub engine_epoch: DeviceSessionId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub device_session: DeviceSessionId,
    pub registration: HandyRegistration,
    pub hosting_request_id: RequestId,
    pub master_sha256: String,
    pub script: ArtifactIdentity,
    pub hosted: ArtifactIdentity,
    pub device: HandySetupDeviceObservation,
    pub observation_span_ns: u64,
    pub setup_performed: bool,
    pub qualified: bool,
}
impl HandySetupInspection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(binding) = &self.accepted_qualification {
            binding.validate()?;
        }
        self.registration.validate()?;
        validate_sha256(&self.master_sha256)?;
        self.script.validate()?;
        self.hosted.validate()?;
        self.device.validate()?;
        if self.setup_performed
            || self.qualified
            || self.registration.disabled
            || self.engine_epoch == self.device_session
            || self.script.artifact_id.as_str() != self.script.sha256
            || self.hosted.artifact_id.as_str() != self.hosted.sha256
            || self.script.byte_len == 0
            || self.script.byte_len > HANDY_STROKE_SCRIPT_MAX_BYTES
            || self.hosted.byte_len == 0
            || self.hosted.byte_len > HANDY_HOSTED_SCRIPT_MAX_BYTES
        {
            return Err(ProtocolError::invalid(
                "invalid read-only Handy setup inspection",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_setup_inspection_is_read_only_but_requires_revision_and_consent() {
        let mut r = Request::new(
            RequestId::new("inspect").unwrap(),
            Command::InspectHandySetup {
                qualification: None,
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                device_session: DeviceSessionId::new("device-session").unwrap(),
                hosting_request_id: RequestId::new("hosting").unwrap(),
                allow_online_inspection: true,
                owner_proof: "a".repeat(64),
            },
        );
        assert!(!r.command.is_mutating());
        assert!(r.command.requires_project() && r.command.requires_revision());
        r.project = Some(ProjectId::new("project").unwrap());
        assert!(r.validate().is_err());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::InspectHandySetup {
            allow_online_inspection,
            ..
        } = &mut r.command
        {
            *allow_online_inspection = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
    }
    #[test]
    fn handy_setup_observation_preserves_remote_mode_and_rejects_invalid_state() {
        let identity = HandyIdentity {
            firmware: "4.0.16".into(),
            model_number: 2,
            model_name: "H02".into(),
            model_variant: 0,
            session_id: "remote".into(),
            firmware_status: 0,
        };
        let mut sample = HandySetupDeviceObservation {
            operative_settings: serde_json::from_str(include_str!(
                "../tests/fixtures/handy-operative-settings.json"
            ))
            .unwrap(),
            identity,
            mode: HandyDeviceMode {
                mode: 7,
                mode_session_id: 1,
            },
            remote_hssp: None,
            remote_hsp: None,
            observed_at_unix_ms: 1,
        };
        sample.validate().unwrap();
        assert!(serde_json::to_value(&sample)
            .unwrap()
            .get("remote_hsp")
            .is_none());
        sample.mode.mode = 1;
        assert!(sample.validate().is_err());
        sample.remote_hssp = Some(HandyHsspObservation {
            play_state: HandyRemotePlayState::Stopped,
            stream_id: 1,
            current_time_ms: 0,
            playback_rate: 1.0,
            looping: false,
            pause_on_starving: false,
        });
        sample.validate().unwrap();
        let mut later = sample.clone();
        later.observed_at_unix_ms = 2;
        assert!(sample.same_state(&later));
        later.mode.mode_session_id += 1;
        assert!(!sample.same_state(&later));
        sample.remote_hssp.as_mut().unwrap().playback_rate = f64::NAN;
        assert!(sample.validate().is_err());
        sample.mode.mode = 4;
        sample.remote_hssp = None;
        // Earlier mode4 records lacked this observation. They remain readable
        // history; HSP qualification/recovery separately requires its presence.
        sample.validate().unwrap();
        sample.remote_hsp = Some(HandyHspObservation {
            play_state: HandyRemotePlayState::NotInitialized,
            stream_id: 0,
            current_time_ms: -250,
            points: 0,
            max_points: 100,
            current_point: -1,
            playback_rate: 0.0,
            looping: false,
            pause_on_starving: false,
            first_point_time_ms: 0,
            last_point_time_ms: 0,
            tail_point_stream_index: -1,
            tail_point_stream_index_threshold: 0,
        });
        sample.validate().unwrap();
        let mut changed = sample.clone();
        changed.remote_hsp.as_mut().unwrap().stream_id = 9;
        assert!(!sample.same_state(&changed));
        changed.mode.mode = 7;
        assert!(changed.validate().is_err());
        sample.remote_hsp.as_mut().unwrap().current_point = -2;
        assert!(sample.validate().is_err());
    }
}
