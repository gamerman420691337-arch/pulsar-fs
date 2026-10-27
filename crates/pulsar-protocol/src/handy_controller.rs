//! Engine-issued controller leases. Reports cannot restore native authority.
use crate::*;
pub const HANDY_CONTROLLER_MIN_LEASE_MS: u64 = 100;
pub const HANDY_CONTROLLER_MAX_LEASE_MS: u64 = 30_000;
pub fn validate_handy_controller_duration(ms: u64) -> Result<(), ProtocolError> {
    if !(HANDY_CONTROLLER_MIN_LEASE_MS..=HANDY_CONTROLLER_MAX_LEASE_MS).contains(&ms) {
        return Err(ProtocolError::invalid(
            "controller lease duration must be 100..30000 ms",
        ));
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyControllerState {
    Active,
    Playing,
    Retired,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyControllerLease {
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub preparation_request_id: RequestId,
    pub preparation_id: ArtifactId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub device_id: DeviceId,
    pub generation: u64,
    pub duration_ms: u64,
    pub deadline_boottime_ns: u64,
    pub state: HandyControllerState,
    pub armed: bool,
    pub physical_stop_pending: bool,
    pub playback_enabled: bool,
}
impl HandyControllerLease {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.preparation_id.as_str())?;
        validate_handy_controller_duration(self.duration_ms)?;
        if self.playback_enabled
            || self.armed != (self.state != HandyControllerState::Retired)
            || (self.state == HandyControllerState::Active && self.physical_stop_pending)
            || (self.state == HandyControllerState::Playing && !self.physical_stop_pending)
            || self.deadline_boottime_ns == 0
            || self.engine_epoch == self.device_session
            || self.acquisition_request_id == self.preparation_request_id
            || self.generation > i64::MAX as u64
        {
            return Err(ProtocolError::invalid("invalid Handy controller lease"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyControllerReceipt {
    pub request_id: RequestId,
    pub lease: HandyControllerLease,
}
impl HandyControllerReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.lease.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_controller_reports_bind_lifetime_and_never_enable_physical_playback() {
        let good: HandyControllerReceipt =
            serde_json::from_str(include_str!("../tests/fixtures/handy-controller.json")).unwrap();
        good.validate().unwrap();
        for case in 0..5 {
            let mut bad = good.clone();
            match case {
                0 => bad.lease.playback_enabled = true,
                1 => bad.lease.physical_stop_pending = true,
                2 => bad.lease.duration_ms = 30_001,
                3 => bad.lease.deadline_boottime_ns = 0,
                _ => bad.lease.state = HandyControllerState::Retired,
            };
            assert!(bad.validate().is_err());
        }
        let mut retired = good;
        retired.lease.state = HandyControllerState::Retired;
        retired.lease.armed = false;
        retired.validate().unwrap();
    }
    #[test]
    fn handy_controller_acquisition_needs_local_arm_and_inspection_consent() {
        let mut r = Request::new(
            RequestId::new("acquire").unwrap(),
            Command::AcquireHandyController {
                preparation_request_id: RequestId::new("prepare").unwrap(),
                preparation_id: ArtifactId::new("a".repeat(64)).unwrap(),
                duration_ms: 1000,
                allow_arm: true,
                allow_online_inspection: true,
                owner_proof: "b".repeat(64),
            },
        );
        assert!(r.command.is_mutating() && r.command.requires_revision());
        r.project = Some(ProjectId::new("project").unwrap());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::AcquireHandyController { allow_arm, .. } = &mut r.command {
            *allow_arm = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
        r.command = Command::HandyControllerStatus {
            acquisition_request_id: RequestId::new("acquire").unwrap(),
            owner_proof: "b".repeat(64),
        };
        r.expected_revision = None;
        assert!(!r.command.is_mutating() && !r.command.requires_revision());
        r.validate().unwrap();
    }
}
