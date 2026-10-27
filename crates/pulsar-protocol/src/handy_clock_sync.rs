//! Historical synchronization effects. No timing accuracy or live authority follows from a reply.
use crate::*;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_clock_sync_history_requires_exact_observation_state_and_never_grants_authority() {
        let good: HandyClockSyncStatus =
            serde_json::from_str(include_str!("../tests/fixtures/handy-clock-sync.json")).unwrap();
        good.validate().unwrap();
        for case in 0..5 {
            let mut bad = good.clone();
            match case {
                0 => bad.synchronization_verified = true,
                1 => bad.playback_enabled = true,
                2 => bad.project_id = ProjectId::new("other").unwrap(),
                3 => bad.history.as_mut().unwrap().state = HandyClockSyncState::Acknowledged,
                _ => bad.history.as_mut().unwrap().user_offset_before.offset_ms = i64::MIN,
            };
            assert!(bad.validate().is_err());
        }
        assert!(HandyClockSyncState::Unknown.route_reserved());
        assert!(!HandyClockSyncState::Acknowledged.route_reserved());
    }
    #[test]
    fn handy_clock_sync_command_requires_effect_consent_and_recovery_has_no_revision_requirement() {
        let mut r = Request::new(
            RequestId::new("sync").unwrap(),
            Command::SynchronizeHandyClock {
                acquisition_request_id: RequestId::new("acquire").unwrap(),
                controller_id: ClientId::new("controller").unwrap(),
                expected_generation: 0,
                allow_online_synchronization: true,
                owner_proof: "a".repeat(64),
            },
        );
        assert!(r.command.is_mutating() && r.command.requires_revision());
        r.project = Some(ProjectId::new("project").unwrap());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::SynchronizeHandyClock {
            allow_online_synchronization,
            ..
        } = &mut r.command
        {
            *allow_online_synchronization = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
        r.command = Command::HandyClockSyncStatus {
            sync_request_id: RequestId::new("sync").unwrap(),
            owner_proof: "a".repeat(64),
        };
        r.expected_revision = None;
        assert!(!r.command.is_mutating() && !r.command.requires_revision());
        r.validate().unwrap();
        r.command = Command::CancelHandyClockSync {
            sync_request_id: RequestId::new("original").unwrap(),
            record_id: ArtifactId::new("b".repeat(64)).unwrap(),
            owner_proof: "a".repeat(64),
        };
        assert!(r.command.is_mutating() && !r.command.requires_revision());
        r.validate().unwrap();
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyClockSyncState {
    Prepared,
    Claimed,
    Acknowledged,
    Unknown,
    Cancelled,
}
impl HandyClockSyncState {
    pub fn route_reserved(self) -> bool {
        matches!(self, Self::Prepared | Self::Claimed | Self::Unknown)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyClockSyncAcknowledgment {
    pub info: HandyDeviceTimeInfo,
    pub sent_boottime_ns: u64,
    pub received_boottime_ns: u64,
}
impl HandyClockSyncAcknowledgment {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.info.validate()?;
        if self.received_boottime_ns < self.sent_boottime_ns {
            return Err(ProtocolError::invalid(
                "clock synchronization observation reversed",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyClockSyncHistory {
    pub record_id: ArtifactId,
    pub original_controller: HandyControllerLease,
    pub preparation_qualification: HandyQualificationBinding,
    pub setup_record_id: ArtifactId,
    pub user_offset_before: HandyUserTimeOffset,
    pub state: HandyClockSyncState,
    pub acknowledgment: Option<HandyClockSyncAcknowledgment>,
}
impl HandyClockSyncHistory {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.record_id.as_str())?;
        validate_sha256(self.setup_record_id.as_str())?;
        self.original_controller.validate()?;
        self.preparation_qualification.validate()?;
        self.user_offset_before.validate()?;
        if self.original_controller.state != HandyControllerState::Active
            || (self.state == HandyClockSyncState::Acknowledged) != self.acknowledgment.is_some()
        {
            return Err(ProtocolError::invalid("invalid synchronization history"));
        }
        if let Some(a) = &self.acknowledgment {
            a.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyClockSyncStatus {
    pub project_id: ProjectId,
    pub sync_request_id: RequestId,
    pub history: Option<HandyClockSyncHistory>,
    pub synchronization_verified: bool,
    pub playback_enabled: bool,
}
impl HandyClockSyncStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.synchronization_verified || self.playback_enabled {
            return Err(ProtocolError::invalid(
                "synchronization observation is not timing authority",
            ));
        }
        if let Some(h) = &self.history {
            h.validate()?;
            if h.original_controller.project_id != self.project_id
                || h.original_controller.acquisition_request_id == self.sync_request_id
            {
                return Err(ProtocolError::invalid("synchronization subject differs"));
            }
        }
        Ok(())
    }
}
