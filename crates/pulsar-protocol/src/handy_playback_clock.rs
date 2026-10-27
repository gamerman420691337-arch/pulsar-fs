//! Explicit device timing contract and historical combined-clock reports.
use crate::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyDeviceClockModel {
    /// The synchronous reply bounds the effective playback clock against the
    /// same server domain, including the declared user offset, without corrections
    /// during validity. Acceptance must substantiate this model and its bounds.
    SynchronousResponseEffectiveClock,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyDeviceClockQualification {
    pub model: HandyDeviceClockModel,
    pub user_offset_ms: i64,
    pub device_error_ns: u64,
    pub maximum_sync_age_ns: u64,
    pub maximum_sync_round_trip_ms: u64,
    pub drift_ppm: u32,
}
impl HandyDeviceClockQualification {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        HandyUserTimeOffset {
            offset_ms: self.user_offset_ms,
        }
        .validate()?;
        if self.maximum_sync_age_ns == 0 || self.maximum_sync_round_trip_ms > (1u64 << 53) - 1 {
            return Err(ProtocolError::invalid(
                "invalid declared device clock contract",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyClockSyncBinding {
    pub sync_request_id: RequestId,
    pub record_id: ArtifactId,
}
impl HandyClockSyncBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.record_id.as_str())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlaybackClockObservation {
    pub request_id: RequestId,
    pub controller: HandyControllerLease,
    pub qualification: HandyQualificationBinding,
    pub synchronization: HandyClockSyncBinding,
    pub synchronization_sent_boottime_ns: u64,
    pub synchronization_received_boottime_ns: u64,
    pub server_sent_boottime_ns: u64,
    pub server_received_boottime_ns: u64,
    pub clock_domain: ArtifactId,
    pub estimated_server_ms: u64,
    pub server_uncertainty_ns: u64,
    pub device_uncertainty_ns: u64,
    pub combined_uncertainty_ns: u64,
    pub admitted_user_offset_ms: i64,
    pub current: bool,
    pub playback_enabled: bool,
}
impl HandyPlaybackClockObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.controller.validate()?;
        self.qualification.validate()?;
        self.synchronization.validate()?;
        validate_sha256(self.clock_domain.as_str())?;
        HandyUserTimeOffset {
            offset_ms: self.admitted_user_offset_ms,
        }
        .validate()?;
        if self.controller.state != HandyControllerState::Active
            || self.playback_enabled
            || self.request_id == self.controller.acquisition_request_id
            || self.request_id == self.synchronization.sync_request_id
            || self.synchronization_received_boottime_ns < self.synchronization_sent_boottime_ns
            || self.server_sent_boottime_ns < self.synchronization_received_boottime_ns
            || self.server_received_boottime_ns < self.server_sent_boottime_ns
            || self.server_received_boottime_ns >= self.controller.deadline_boottime_ns
            || self.estimated_server_ms > (1u64 << 53) - 1
            || self
                .server_uncertainty_ns
                .checked_add(self.device_uncertainty_ns)
                != Some(self.combined_uncertainty_ns)
        {
            return Err(ProtocolError::invalid(
                "invalid combined playback clock observation",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_playback_clock_report_checks_exact_total_and_preserves_historical_intervals() {
        let good: HandyPlaybackClockObservation =
            serde_json::from_str(include_str!("../tests/fixtures/handy-playback-clock.json"))
                .unwrap();
        good.validate().unwrap();
        for case in 0..6 {
            let mut bad = good.clone();
            match case {
                0 => bad.playback_enabled = true,
                1 => bad.combined_uncertainty_ns -= 1,
                2 => bad.server_sent_boottime_ns = 1,
                3 => bad.server_received_boottime_ns = bad.controller.deadline_boottime_ns,
                4 => bad.device_uncertainty_ns = u64::MAX,
                _ => bad.request_id = bad.synchronization.sync_request_id.clone(),
            };
            assert!(bad.validate().is_err());
        }
        let mut historical = good;
        historical.current = false;
        historical.validate().unwrap();
    }
    #[test]
    fn handy_playback_clock_request_requires_explicit_sync_selection_and_consent() {
        let mut r = Request::new(
            RequestId::new("combined").unwrap(),
            Command::AcquireHandyPlaybackClock {
                acquisition_request_id: RequestId::new("acquire").unwrap(),
                controller_id: ClientId::new("controller").unwrap(),
                expected_generation: 0,
                synchronization: HandyClockSyncBinding {
                    sync_request_id: RequestId::new("sync").unwrap(),
                    record_id: ArtifactId::new("a".repeat(64)).unwrap(),
                },
                allow_online_timing: true,
                owner_proof: "b".repeat(64),
            },
        );
        assert!(r.command.is_mutating() && r.command.requires_revision());
        r.project = Some(ProjectId::new("project").unwrap());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::AcquireHandyPlaybackClock {
            allow_online_timing,
            ..
        } = &mut r.command
        {
            *allow_online_timing = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
    }
}
