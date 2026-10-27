//! Preparation reports are historical metadata, never controller or playback authority.
use crate::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyProgramSelection {
    pub setup_request_id: RequestId,
    pub record_id: ArtifactId,
    pub qualification: HandyQualificationBinding,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub hosting_request_id: RequestId,
}
impl HandyProgramSelection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.record_id.as_str())?;
        self.qualification.validate()?;
        if self.setup_request_id == self.hosting_request_id
            || self.engine_epoch == self.device_session
        {
            return Err(ProtocolError::invalid("invalid Handy program selection"));
        }
        Ok(())
    }
    pub fn inspection_command(&self, owner_proof: String) -> Command {
        Command::InspectHandyProgram {
            setup_request_id: self.setup_request_id.clone(),
            record_id: self.record_id.clone(),
            qualification: self.qualification.clone(),
            engine_epoch: self.engine_epoch.clone(),
            device_session: self.device_session.clone(),
            hosting_request_id: self.hosting_request_id.clone(),
            allow_online_inspection: true,
            owner_proof,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyPlaybackPreparationState {
    Prepared,
    Retired,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlaybackPreparation {
    pub request_id: RequestId,
    pub preparation_id: ArtifactId,
    pub correlation: HandyProgramCorrelation,
    pub admission: PlaybackAdmission,
    pub state: HandyPlaybackPreparationState,
    pub physical_stop_pending: bool,
    pub armed: bool,
    pub playback_enabled: bool,
}
impl HandyPlaybackPreparation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.correlation.validate()?;
        validate_sha256(self.preparation_id.as_str())?;
        let i = &self.correlation.inspection;
        if self.armed
            || self.playback_enabled
            || self.request_id != i.request_id
            || self.admission.project != i.project_id
            || self.admission.revision != i.revision
            || self.admission.configuration.device != i.registration.device_id
            || self.admission.program_artifact != i.hosted.artifact_id
        {
            return Err(ProtocolError::invalid(
                "invalid nonplaying Handy preparation",
            ));
        }
        Ok(())
    }
    pub fn selection(&self) -> Result<HandyProgramSelection, ProtocolError> {
        self.validate()?;
        let i = &self.correlation.inspection;
        Ok(HandyProgramSelection {
            setup_request_id: self.correlation.setup_request_id.clone(),
            record_id: self.correlation.record_id.clone(),
            qualification: i
                .accepted_qualification
                .clone()
                .expect("validated correlation"),
            engine_epoch: i.engine_epoch.clone(),
            device_session: i.device_session.clone(),
            hosting_request_id: i.hosting_request_id.clone(),
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlaybackRetirement {
    pub request_id: RequestId,
    pub preparation: HandyPlaybackPreparation,
}
impl HandyPlaybackRetirement {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.preparation.validate()?;
        if self.request_id == self.preparation.request_id
            || self.preparation.state != HandyPlaybackPreparationState::Retired
        {
            return Err(ProtocolError::invalid(
                "invalid Handy preparation retirement",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_preparation_metadata_never_grants_arm_or_play_and_binds_exact_execution_artifact() {
        let good: HandyPlaybackPreparation = serde_json::from_str(include_str!(
            "../tests/fixtures/handy-playback-preparation.json"
        ))
        .unwrap();
        good.validate().unwrap();
        good.selection().unwrap().validate().unwrap();
        for index in 0..4 {
            let mut bad = good.clone();
            match index {
                0 => bad.armed = true,
                1 => bad.playback_enabled = true,
                2 => bad.admission.program_artifact = ArtifactId::new("a".repeat(64)).unwrap(),
                _ => bad.admission.revision = RevisionId::new(2),
            };
            assert!(bad.validate().is_err());
        }
        let mut retired = good;
        retired.state = HandyPlaybackPreparationState::Retired;
        retired.validate().unwrap();
        let mut receipt = HandyPlaybackRetirement {
            request_id: RequestId::new("retire").unwrap(),
            preparation: retired,
        };
        receipt.validate().unwrap();
        receipt.request_id = receipt.preparation.request_id.clone();
        assert!(receipt.validate().is_err());
    }
    #[test]
    fn handy_preparation_commands_require_current_revision_and_consent_only_for_preparation() {
        let good: HandyPlaybackPreparation = serde_json::from_str(include_str!(
            "../tests/fixtures/handy-playback-preparation.json"
        ))
        .unwrap();
        let mut r = Request::new(
            good.request_id.clone(),
            Command::PrepareHandyPlayback {
                program: good.selection().unwrap(),
                allow_online_inspection: true,
                owner_proof: "a".repeat(64),
            },
        );
        assert!(r.command.is_mutating() && r.command.requires_revision());
        r.project = Some(good.admission.project.clone());
        r.expected_revision = Some(good.admission.revision);
        r.validate().unwrap();
        if let Command::PrepareHandyPlayback {
            allow_online_inspection,
            ..
        } = &mut r.command
        {
            *allow_online_inspection = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
        r.command = Command::HandyPlaybackPreparationStatus {
            preparation_request_id: good.request_id,
            owner_proof: "a".repeat(64),
        };
        r.expected_revision = None;
        assert!(!r.command.is_mutating() && !r.command.requires_revision());
        r.validate().unwrap();
    }
}
