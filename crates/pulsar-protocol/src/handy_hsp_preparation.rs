//! Original HSP preparation observations. Synchronization acknowledgment,
//! retained history and native phase metadata never arm or authorize playback.
use crate::*;

pub const HANDY_HSP_PREPARATION_MAX_TIMEOUT_MS: u64 = 60_000;

pub fn validate_handy_hsp_preparation_selection(
    engine_epoch: &DeviceSessionId,
    device_session: &DeviceSessionId,
    qualification: &HandyQualificationBinding,
    timeout_ms: u64,
) -> Result<(), ProtocolError> {
    qualification.validate()?;
    if engine_epoch == device_session
        || !(1..=HANDY_HSP_PREPARATION_MAX_TIMEOUT_MS).contains(&timeout_ms)
    {
        return Err(ProtocolError::invalid(
            "invalid HSP preparation selection or timeout",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspPreparationPhase {
    Inspecting,
    Synchronizing,
    Checking,
    Ready,
    Retired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspPreparationHistory {
    pub record_id: ArtifactId,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub revision: RevisionId,
    pub device_id: DeviceId,
    pub registration_revision: u64,
    pub qualification: HandyQualificationBinding,
    pub subject_sha256: String,
    pub profile_sha256: String,
    pub deadline_boottime_ns: u64,
    pub state: HandyClockSyncState,
    pub acknowledgment: Option<HandyClockSyncAcknowledgment>,
    pub linked_stream_id: Option<ArtifactId>,
    pub custody_transferred: bool,
    pub resolved: bool,
}
impl HandyHspPreparationHistory {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.record_id.as_str())?;
        validate_sha256(&self.subject_sha256)?;
        validate_sha256(&self.profile_sha256)?;
        self.qualification.validate()?;
        if let Some(link) = &self.linked_stream_id {
            validate_sha256(link.as_str())?;
        }
        if let Some(ack) = &self.acknowledgment {
            ack.validate()?;
            if ack.received_boottime_ns >= self.deadline_boottime_ns {
                return Err(ProtocolError::invalid(
                    "HSP preparation acknowledgment exceeds original deadline",
                ));
            }
        }
        if self.engine_epoch == self.device_session
            || self.registration_revision == 0
            || self.registration_revision > i64::MAX as u64
            || self.deadline_boottime_ns == 0
            || (self.state == HandyClockSyncState::Acknowledged) != self.acknowledgment.is_some()
            || (self.linked_stream_id.is_some() && self.state != HandyClockSyncState::Acknowledged)
            || (self.custody_transferred && self.linked_stream_id.is_none())
        {
            return Err(ProtocolError::invalid(
                "inconsistent HSP preparation history",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspPreparationNativeStatus {
    pub phase: HandyHspPreparationPhase,
    pub worker_active: bool,
    pub cancel_requested: bool,
    pub metadata_pending: bool,
    pub clock_valid_until_boottime_ns: Option<u64>,
    pub error: Option<ProtocolError>,
}
impl HandyHspPreparationNativeStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.clock_valid_until_boottime_ns == Some(0)
            || (self.phase == HandyHspPreparationPhase::Ready
                && (self.clock_valid_until_boottime_ns.is_none()
                    || self.error.is_some()
                    || self.cancel_requested))
            || self.error.as_ref().is_some_and(|e| {
                e.message.is_empty() || e.message.len() > 2048 || e.message.contains('\0')
            })
        {
            return Err(ProtocolError::invalid(
                "inconsistent HSP preparation native observation",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspPreparationStatus {
    pub project_id: ProjectId,
    pub preparation_request_id: RequestId,
    /// A known original request reservation can exist before this history.
    pub history: Option<HandyHspPreparationHistory>,
    /// Absent after restart; history never reconstructs a ready native owner.
    pub native: Option<HandyHspPreparationNativeStatus>,
    pub armed: bool,
    pub playback_enabled: bool,
}
impl HandyHspPreparationStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.armed || self.playback_enabled {
            return Err(ProtocolError::invalid(
                "HSP preparation metadata is not playback authority",
            ));
        }
        if let Some(history) = &self.history {
            history.validate()?;
        }
        if let Some(native) = &self.native {
            native.validate()?;
            if native.phase == HandyHspPreparationPhase::Ready {
                let history = self.history.as_ref().ok_or_else(|| {
                    ProtocolError::invalid("ready HSP preparation lacks original history")
                })?;
                if history.state != HandyClockSyncState::Acknowledged
                    || history.resolved
                    || history.custody_transferred
                    || history.linked_stream_id.is_some()
                    || native
                        .clock_valid_until_boottime_ns
                        .is_none_or(|until| until > history.deadline_boottime_ns)
                {
                    return Err(ProtocolError::invalid(
                        "ready HSP preparation differs from original unconsumed history",
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
    fn history() -> HandyHspPreparationHistory {
        HandyHspPreparationHistory {
            record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("device-session").unwrap(),
            revision: RevisionId::new(2),
            device_id: DeviceId::new("handy").unwrap(),
            registration_revision: 1,
            qualification: HandyQualificationBinding {
                dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
                revision: 2,
            },
            subject_sha256: "c".repeat(64),
            profile_sha256: "d".repeat(64),
            deadline_boottime_ns: 1000,
            state: HandyClockSyncState::Unknown,
            acknowledgment: None,
            linked_stream_id: None,
            custody_transferred: false,
            resolved: false,
        }
    }
    fn report() -> HandyHspPreparationStatus {
        HandyHspPreparationStatus {
            project_id: ProjectId::new("project").unwrap(),
            preparation_request_id: RequestId::new("prepare").unwrap(),
            history: Some(history()),
            native: Some(HandyHspPreparationNativeStatus {
                phase: HandyHspPreparationPhase::Retired,
                worker_active: false,
                cancel_requested: true,
                metadata_pending: true,
                clock_valid_until_boottime_ns: None,
                error: Some(ProtocolError::new(
                    ErrorCode::UnknownOutcome,
                    "original synchronization is unknown",
                )),
            }),
            armed: false,
            playback_enabled: false,
        }
    }
    #[test]
    fn hsp_preparation_commands_require_effect_consent_scopes_and_distinct_cancel_identity() {
        let mut r = Request::new(
            RequestId::new("prepare").unwrap(),
            Command::PrepareHandyHsp {
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                device_session: DeviceSessionId::new("device-session").unwrap(),
                qualification: history().qualification,
                timeout_ms: 1000,
                allow_online_synchronization: true,
                owner_proof: "synthetic-owner-proof".into(),
            },
        );
        assert!(
            r.command.requires_project()
                && r.command.requires_revision()
                && r.command.is_mutating()
        );
        assert!(r.validate().is_err());
        r.project = Some(ProjectId::new("project").unwrap());
        assert!(r.validate().is_err());
        r.expected_revision = Some(RevisionId::new(2));
        r.auth_token = Some("synthetic-auth-token".into());
        r.validate().unwrap();
        for debug in [format!("{r:?}"), format!("{:?}", r.command)] {
            assert!(!debug.contains("synthetic-owner-proof"));
            assert!(!debug.contains("synthetic-auth-token"));
        }
        let wire = serde_json::to_value(&r).unwrap();
        assert_eq!(wire["command"]["operation"], "prepare_handy_hsp");
        let decoded: Request = serde_json::from_value(wire).unwrap();
        decoded.validate().unwrap();
        assert!(matches!(
            decoded.command,
            Command::PrepareHandyHsp {
                timeout_ms: 1000,
                allow_online_synchronization: true,
                ..
            }
        ));
        let original = r.clone();
        for case in 0..6 {
            let mut bad = original.clone();
            let Command::PrepareHandyHsp {
                engine_epoch,
                device_session,
                qualification,
                timeout_ms,
                allow_online_synchronization,
                owner_proof,
            } = &mut bad.command
            else {
                unreachable!();
            };
            match case {
                0 => *allow_online_synchronization = false,
                1 => *timeout_ms = 0,
                2 => *timeout_ms = 60_001,
                3 => *device_session = engine_epoch.clone(),
                4 => qualification.revision = 1,
                _ => owner_proof.clear(),
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        r.expected_revision = None;
        r.command = Command::HandyHspPreparationStatus {
            preparation_request_id: RequestId::new("prepare").unwrap(),
            owner_proof: "a".repeat(64),
        };
        assert!(
            r.command.requires_project()
                && !r.command.requires_revision()
                && !r.command.is_mutating()
        );
        r.validate().unwrap();
        r.command = Command::CancelHandyHspPreparation {
            preparation_request_id: RequestId::new("prepare").unwrap(),
            owner_proof: "a".repeat(64),
        };
        assert!(
            r.command.requires_project()
                && !r.command.requires_revision()
                && r.command.is_mutating()
        );
        assert!(r.validate().is_err());
        r.request_id = RequestId::new("cancel").unwrap();
        r.validate().unwrap();
    }
    #[test]
    fn hsp_preparation_history_preserves_unknown_and_requires_exact_acknowledgment_state() {
        for state in [
            HandyClockSyncState::Prepared,
            HandyClockSyncState::Claimed,
            HandyClockSyncState::Unknown,
            HandyClockSyncState::Cancelled,
        ] {
            let mut value = history();
            value.state = state;
            value.validate().unwrap();
        }
        for case in 0..9 {
            let mut value = history();
            match case {
                0 => value.state = HandyClockSyncState::Acknowledged,
                1 => value.record_id = ArtifactId::new("bad").unwrap(),
                2 => value.subject_sha256.clear(),
                3 => value.profile_sha256.clear(),
                4 => value.engine_epoch = value.device_session.clone(),
                5 => value.registration_revision = 0,
                6 => value.deadline_boottime_ns = 0,
                7 => value.custody_transferred = true,
                _ => value.linked_stream_id = Some(ArtifactId::new("e".repeat(64)).unwrap()),
            }
            assert!(value.validate().is_err(), "case {case}");
        }
    }
    #[test]
    fn hsp_preparation_history_acknowledgment_link_and_transfer_remain_observations() {
        let mut h = history();
        h.acknowledgment = Some(HandyClockSyncAcknowledgment {
            info: HandyDeviceTimeInfo {
                device_uptime_ms: 10,
                reported_clock_offset_ms: -1,
                last_sync_round_trip_ms: 1,
            },
            sent_boottime_ns: 10,
            received_boottime_ns: 20,
        });
        assert!(h.validate().is_err());
        h.state = HandyClockSyncState::Acknowledged;
        h.validate().unwrap();
        h.linked_stream_id = Some(ArtifactId::new("e".repeat(64)).unwrap());
        h.validate().unwrap(); // An exact link can precede custody transfer.
        h.custody_transferred = true;
        h.validate().unwrap();
        h.resolved = true;
        h.validate().unwrap(); // Resolution is history, not restored readiness.
        h.acknowledgment.as_mut().unwrap().received_boottime_ns = 9;
        assert!(h.validate().is_err());
    }
    #[test]
    fn hsp_preparation_ready_requires_unconsumed_acknowledged_history_with_original_deadline() {
        let mut good = report();
        let h = good.history.as_mut().unwrap();
        h.state = HandyClockSyncState::Acknowledged;
        h.acknowledgment = Some(HandyClockSyncAcknowledgment {
            info: HandyDeviceTimeInfo {
                device_uptime_ms: 10,
                reported_clock_offset_ms: -1,
                last_sync_round_trip_ms: 1,
            },
            sent_boottime_ns: 10,
            received_boottime_ns: 20,
        });
        let native = good.native.as_mut().unwrap();
        native.phase = HandyHspPreparationPhase::Ready;
        native.clock_valid_until_boottime_ns = Some(h.deadline_boottime_ns);
        native.cancel_requested = false;
        native.error = None;
        for worker_active in [false, true] {
            good.native.as_mut().unwrap().worker_active = worker_active;
            good.validate().unwrap(); // Installing readiness can precede worker join.
        }
        for case in 0..10 {
            let mut bad = good.clone();
            match case {
                0 => bad.history = None,
                1..=4 => {
                    let history = bad.history.as_mut().unwrap();
                    history.state = [
                        HandyClockSyncState::Prepared,
                        HandyClockSyncState::Claimed,
                        HandyClockSyncState::Unknown,
                        HandyClockSyncState::Cancelled,
                    ][case - 1];
                    history.acknowledgment = None;
                }
                5 => bad.history.as_mut().unwrap().resolved = true,
                6 => {
                    let history = bad.history.as_mut().unwrap();
                    history.linked_stream_id = Some(ArtifactId::new("e".repeat(64)).unwrap());
                    history.custody_transferred = true;
                }
                7 => {
                    bad.history.as_mut().unwrap().linked_stream_id =
                        Some(ArtifactId::new("e".repeat(64)).unwrap())
                }
                8 => bad.native.as_mut().unwrap().clock_valid_until_boottime_ns = Some(1001),
                _ => bad.native.as_mut().unwrap().cancel_requested = true,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        let mut ack = good.history.unwrap();
        ack.acknowledgment.as_mut().unwrap().received_boottime_ns = 999;
        ack.validate().unwrap();
        ack.acknowledgment.as_mut().unwrap().received_boottime_ns = 1000;
        assert!(ack.validate().is_err());
        ack.acknowledgment.as_mut().unwrap().received_boottime_ns = 1001;
        assert!(ack.validate().is_err());
    }
    #[test]
    fn hsp_preparation_wire_keeps_empty_reservation_and_pending_metadata_without_authority() {
        for (history, native) in [
            (None, None),
            (Some(history()), None),
            (report().history, report().native),
        ] {
            let value = HandyHspPreparationStatus {
                history,
                native,
                ..report()
            };
            value.validate().unwrap();
            let wire =
                serde_json::to_value(ResponseBody::HandyHspPreparation(Box::new(value.clone())))
                    .unwrap();
            assert_eq!(wire["kind"], "handy_hsp_preparation");
            let ResponseBody::HandyHspPreparation(decoded) = serde_json::from_value(wire).unwrap()
            else {
                panic!();
            };
            assert_eq!(*decoded, value);
        }
        for field in ["armed", "playback_enabled"] {
            let mut wire = serde_json::to_value(report()).unwrap();
            wire[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyHspPreparationStatus>(wire)
                .unwrap()
                .validate()
                .is_err());
        }
        for field in ["native_authority", "physical_stopped", "credentials"] {
            let mut wire = serde_json::to_value(report()).unwrap();
            wire[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyHspPreparationStatus>(wire).is_err());
        }
        let mut ready = report().native.unwrap();
        ready.phase = HandyHspPreparationPhase::Ready;
        assert!(ready.validate().is_err());
        ready.error = None;
        ready.cancel_requested = false;
        ready.clock_valid_until_boottime_ns = Some(1000);
        ready.validate().unwrap();
        ready.clock_valid_until_boottime_ns = Some(0);
        assert!(ready.validate().is_err());
    }
}
