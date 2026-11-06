//! Read-only device clock observations. No synchronization age or error is inferred.
use crate::*;
const MAX_MS: u64 = (1u64 << 53) - 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyDeviceTimeInfo {
    pub device_uptime_ms: u64,
    pub reported_clock_offset_ms: i64,
    pub last_sync_round_trip_ms: u64,
}
impl HandyDeviceTimeInfo {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.device_uptime_ms > MAX_MS
            || self.reported_clock_offset_ms.unsigned_abs() > MAX_MS
            || self.last_sync_round_trip_ms > MAX_MS
        {
            return Err(ProtocolError::invalid(
                "device timing exceeds exact numeric bounds",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyUserTimeOffset {
    pub offset_ms: i64,
}
impl HandyUserTimeOffset {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.offset_ms.unsigned_abs() > MAX_MS {
            return Err(ProtocolError::invalid(
                "user timing offset exceeds exact numeric bounds",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyDeviceTimingSample {
    pub info: HandyDeviceTimeInfo,
    pub user_offset: HandyUserTimeOffset,
    /// Overall span covering two separate GETs; not an atomic remote snapshot.
    pub read_started_boottime_ns: u64,
    pub read_finished_boottime_ns: u64,
}
impl HandyDeviceTimingSample {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.info.validate()?;
        self.user_offset.validate()?;
        if self.read_finished_boottime_ns < self.read_started_boottime_ns {
            return Err(ProtocolError::invalid(
                "device timing observation clock reversed",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyDeviceTimingInspection {
    pub request_id: RequestId,
    pub controller: HandyControllerLease,
    pub preparation_qualification: HandyQualificationBinding,
    pub before: HandySetupDeviceObservation,
    pub first: HandyDeviceTimingSample,
    pub second: HandyDeviceTimingSample,
    pub after: HandySetupDeviceObservation,
    pub inspection_started_boottime_ns: u64,
    pub inspection_finished_boottime_ns: u64,
    pub synchronization_verified: bool,
    pub playback_enabled: bool,
}
impl HandyDeviceTimingInspection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.controller.validate()?;
        self.preparation_qualification.validate()?;
        self.before.validate()?;
        self.after.validate()?;
        self.first.validate()?;
        self.second.validate()?;
        if self.synchronization_verified
            || self.playback_enabled
            || self.controller.state != HandyControllerState::Active
            || !self.before.same_state(&self.after)
            || self.inspection_started_boottime_ns > self.first.read_started_boottime_ns
            || self.first.read_finished_boottime_ns > self.second.read_started_boottime_ns
            || self.second.read_finished_boottime_ns > self.inspection_finished_boottime_ns
            || self.inspection_finished_boottime_ns >= self.controller.deadline_boottime_ns
            || self.second.info.device_uptime_ms < self.first.info.device_uptime_ms
        {
            return Err(ProtocolError::invalid(
                "invalid unqualified device timing inspection",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_device_timing_report_preserves_unknown_synchronization_and_ordered_context() {
        let good: HandyDeviceTimingInspection =
            serde_json::from_str(include_str!("../tests/fixtures/handy-device-timing.json"))
                .unwrap();
        good.validate().unwrap();
        for case in 0..7 {
            let mut bad = good.clone();
            match case {
                0 => bad.synchronization_verified = true,
                1 => bad.playback_enabled = true,
                2 => bad.first.read_finished_boottime_ns = 100,
                3 => bad.inspection_finished_boottime_ns = bad.controller.deadline_boottime_ns,
                4 => bad.second.info.device_uptime_ms = 99,
                5 => bad.after.identity.session_id = "changed".into(),
                _ => bad.first.user_offset.offset_ms = i64::MIN,
            }
            assert!(bad.validate().is_err());
        }
    }
    #[test]
    fn handy_device_timing_numbers_preserve_sign_and_refuse_inexact_ranges() {
        for sign in [-1, 1] {
            HandyUserTimeOffset {
                offset_ms: sign * MAX_MS as i64,
            }
            .validate()
            .unwrap();
            HandyDeviceTimeInfo {
                device_uptime_ms: 0,
                reported_clock_offset_ms: sign * 125,
                last_sync_round_trip_ms: 0,
            }
            .validate()
            .unwrap();
        }
        assert!(HandyUserTimeOffset {
            offset_ms: i64::MIN
        }
        .validate()
        .is_err());
        for (time, offset, rtd) in [(MAX_MS + 1, 0, 0), (0, i64::MAX, 0), (0, 0, MAX_MS + 1)] {
            assert!(HandyDeviceTimeInfo {
                device_uptime_ms: time,
                reported_clock_offset_ms: offset,
                last_sync_round_trip_ms: rtd
            }
            .validate()
            .is_err());
        }
    }
    #[test]
    fn handy_device_timing_command_is_read_only_scoped_and_requires_consent() {
        let mut r = Request::new(
            RequestId::new("inspect-time").unwrap(),
            Command::InspectHandyDeviceTiming {
                acquisition_request_id: RequestId::new("acquire").unwrap(),
                controller_id: ClientId::new("controller").unwrap(),
                expected_generation: 0,
                allow_online_inspection: true,
                owner_proof: "a".repeat(64),
            },
        );
        assert!(!r.command.is_mutating() && r.command.requires_revision());
        assert!(r.validate().is_err());
        r.project = Some(ProjectId::new("project").unwrap());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::InspectHandyDeviceTiming {
            allow_online_inspection,
            ..
        } = &mut r.command
        {
            *allow_online_inspection = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
    }
}
