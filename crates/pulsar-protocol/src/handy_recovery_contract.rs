//! Qualified recovery declarations and historical samples, never executable proof.
use crate::*;

pub const HANDY_RECOVERY_MAX_WINDOW_NS: u64 = 30_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyRecoveryModel {
    /// Physical qualification must establish that preceding remote effects settle
    /// within the declared bound after host dispatch drains, and that matching
    /// stopped samples under its operating assumptions establish quiescence.
    /// This is not a vendor guarantee or a default inference from REST replies.
    DrainedHostThenStoppedSamples,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyRecoveryRemoteState {
    Stopped,
    NotInitialized,
}
impl HandyRecoveryRemoteState {
    fn matches(self, state: HandyRemotePlayState) -> bool {
        matches!(
            (self, state),
            (Self::Stopped, HandyRemotePlayState::Stopped)
                | (Self::NotInitialized, HandyRemotePlayState::NotInitialized)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRecoveryQualification {
    #[serde(default, skip_serializing_if = "HandyQualificationTransport::is_hssp")]
    pub transport: HandyQualificationTransport,
    pub model: HandyRecoveryModel,
    pub remote_state: HandyRecoveryRemoteState,
    pub prior_command_settle_ns: u64,
    pub stopped_sample_separation_ns: u64,
    pub maximum_sample_span_ns: u64,
}
impl HandyRecoveryQualification {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let window = u128::from(self.prior_command_settle_ns)
            + u128::from(self.stopped_sample_separation_ns)
            + 2 * u128::from(self.maximum_sample_span_ns);
        if self.prior_command_settle_ns == 0
            || self.stopped_sample_separation_ns == 0
            || self.maximum_sample_span_ns == 0
            || window > u128::from(HANDY_RECOVERY_MAX_WINDOW_NS)
        {
            return Err(ProtocolError::invalid(
                "invalid Handy recovery timing contract",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRecoverySample {
    pub started_boottime_ns: u64,
    pub finished_boottime_ns: u64,
    pub observation: HandySetupDeviceObservation,
}
impl HandyRecoverySample {
    fn validate(&self, contract: &HandyRecoveryQualification) -> Result<(), ProtocolError> {
        self.observation.validate()?;
        if self
            .finished_boottime_ns
            .checked_sub(self.started_boottime_ns)
            .is_none_or(|span| span > contract.maximum_sample_span_ns)
            || !match contract.transport {
                HandyQualificationTransport::Hssp => {
                    self.observation.mode.mode == 1
                        && self
                            .observation
                            .remote_hssp
                            .as_ref()
                            .is_some_and(|state| contract.remote_state.matches(state.play_state))
                }
                HandyQualificationTransport::Hsp => {
                    self.observation.mode.mode == 4
                        && self
                            .observation
                            .remote_hsp
                            .as_ref()
                            .is_some_and(|state| contract.remote_state.matches(state.play_state))
                }
            }
        {
            return Err(ProtocolError::invalid("invalid Handy recovery sample"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRecoveryEvidence {
    pub drained_boottime_ns: u64,
    pub first: HandyRecoverySample,
    pub second: HandyRecoverySample,
}
impl HandyRecoveryEvidence {
    /// Checks historical consistency only. Native drain ownership, clock origin,
    /// current acceptance, identity/settings and authority must be verified by
    /// the Engine; deserializing this record cannot establish physical stopping.
    pub fn validate(&self, contract: &HandyRecoveryQualification) -> Result<(), ProtocolError> {
        contract.validate()?;
        self.first.validate(contract)?;
        self.second.validate(contract)?;
        if self
            .first
            .started_boottime_ns
            .checked_sub(self.drained_boottime_ns)
            .is_none_or(|elapsed| elapsed < contract.prior_command_settle_ns)
            || self
                .second
                .started_boottime_ns
                .checked_sub(self.first.finished_boottime_ns)
                .is_none_or(|elapsed| elapsed < contract.stopped_sample_separation_ns)
            || self
                .second
                .finished_boottime_ns
                .checked_sub(self.drained_boottime_ns)
                .is_none_or(|elapsed| elapsed > HANDY_RECOVERY_MAX_WINDOW_NS)
            || !self.first.observation.same_state(&self.second.observation)
        {
            return Err(ProtocolError::invalid(
                "inconsistent Handy recovery observations",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (HandyRecoveryQualification, HandyRecoveryEvidence) {
        let c: HandyProgramCorrelation = serde_json::from_str(include_str!(
            "../tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap();
        let observation = c.inspection.device;
        (
            HandyRecoveryQualification {
                transport: HandyQualificationTransport::Hssp,
                model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
                remote_state: HandyRecoveryRemoteState::Stopped,
                prior_command_settle_ns: 1_000_000_000,
                stopped_sample_separation_ns: 1_000_000_000,
                maximum_sample_span_ns: 1_000_000_000,
            },
            HandyRecoveryEvidence {
                drained_boottime_ns: 1_000_000_000,
                first: HandyRecoverySample {
                    started_boottime_ns: 2_000_000_000,
                    finished_boottime_ns: 2_100_000_000,
                    observation: observation.clone(),
                },
                second: HandyRecoverySample {
                    started_boottime_ns: 3_100_000_000,
                    finished_boottime_ns: 3_200_000_000,
                    observation,
                },
            },
        )
    }

    #[test]
    fn recovery_transport_is_explicit_for_hsp_and_cannot_cross_match_stopped_samples() {
        let (old, old_evidence) = fixture();
        let old_bytes = serde_json::to_vec(&old).unwrap();
        assert_eq!(
            std::str::from_utf8(&old_bytes).unwrap(),
            r#"{"model":"drained_host_then_stopped_samples","remote_state":"stopped","prior_command_settle_ns":1000000000,"stopped_sample_separation_ns":1000000000,"maximum_sample_span_ns":1000000000}"#
        );
        let decoded: HandyRecoveryQualification = serde_json::from_slice(&old_bytes).unwrap();
        assert!(decoded.transport.is_hssp());
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), old_bytes);
        let mut hsp = old.clone();
        hsp.transport = HandyQualificationTransport::Hsp;
        assert!(old_evidence.validate(&hsp).is_err());
        let mut wire = serde_json::to_value(old_evidence).unwrap();
        for which in ["first", "second"] {
            wire[which]["observation"]["mode"]["mode"] = serde_json::json!(4);
            wire[which]["observation"]["remote_hssp"] = serde_json::Value::Null;
            wire[which]["observation"]["remote_hsp"] = serde_json::json!({
                "play_state":"stopped", "stream_id":7, "current_time_ms":-1,
                "points":0, "max_points":100, "current_point":-1,
                "playback_rate":0.0, "looping":false, "pause_on_starving":false,
                "first_point_time_ms":0, "last_point_time_ms":0,
                "tail_point_stream_index":-1, "tail_point_stream_index_threshold":0,
            });
        }
        let evidence: HandyRecoveryEvidence = serde_json::from_value(wire).unwrap();
        evidence.validate(&hsp).unwrap();
        assert!(evidence.validate(&old).is_err());
        let mut absent = evidence.clone();
        absent.first.observation.remote_hsp = None;
        absent.second.observation.remote_hsp = None;
        assert!(absent.validate(&hsp).is_err());
        let mut changed = evidence;
        changed
            .second
            .observation
            .remote_hsp
            .as_mut()
            .unwrap()
            .stream_id += 1;
        assert!(changed.validate(&hsp).is_err());
    }
    #[test]
    fn recovery_contract_bounds_untrusted_arithmetic_and_requires_each_interval() {
        let (good, _) = fixture();
        good.validate().unwrap();
        for case in 0..6 {
            let mut bad = good.clone();
            match case {
                0 => bad.prior_command_settle_ns = 0,
                1 => bad.stopped_sample_separation_ns = 0,
                2 => bad.maximum_sample_span_ns = 0,
                3 => bad.prior_command_settle_ns = u64::MAX,
                4 => bad.maximum_sample_span_ns = u64::MAX,
                _ => bad.stopped_sample_separation_ns = HANDY_RECOVERY_MAX_WINDOW_NS,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
    }

    #[test]
    fn recovery_history_checks_drain_order_span_separation_state_and_configuration() {
        let (contract, good) = fixture();
        good.validate(&contract).unwrap();
        for case in 0..12 {
            let mut bad = good.clone();
            match case {
                0 => bad.first.started_boottime_ns -= 1,
                1 => bad.first.finished_boottime_ns = bad.first.started_boottime_ns - 1,
                2 => bad.first.finished_boottime_ns += contract.maximum_sample_span_ns,
                3 => bad.second.started_boottime_ns -= 1,
                4 => bad.second.finished_boottime_ns = bad.second.started_boottime_ns - 1,
                5 => bad.second.finished_boottime_ns += contract.maximum_sample_span_ns,
                6 => bad.drained_boottime_ns = u64::MAX,
                7 => bad.second.observation.mode.mode_session_id += 1,
                8 => bad.second.observation.identity.firmware.push('x'),
                9 => {
                    bad.first
                        .observation
                        .remote_hssp
                        .as_mut()
                        .unwrap()
                        .play_state = HandyRemotePlayState::Paused;
                    bad.second.observation = bad.first.observation.clone();
                }
                10 => {
                    bad.second.started_boottime_ns += HANDY_RECOVERY_MAX_WINDOW_NS;
                    bad.second.finished_boottime_ns += HANDY_RECOVERY_MAX_WINDOW_NS;
                }
                _ => {
                    bad.second
                        .observation
                        .remote_hssp
                        .as_mut()
                        .unwrap()
                        .stream_id += 1
                }
            }
            assert!(bad.validate(&contract).is_err(), "case {case}");
        }
        let mut independent_wall_clock = good.clone();
        independent_wall_clock
            .second
            .observation
            .observed_at_unix_ms += 1;
        independent_wall_clock.validate(&contract).unwrap();
        let mut not_initialized = good;
        not_initialized
            .first
            .observation
            .remote_hssp
            .as_mut()
            .unwrap()
            .play_state = HandyRemotePlayState::NotInitialized;
        not_initialized.second.observation = not_initialized.first.observation.clone();
        assert!(not_initialized.validate(&contract).is_err());
        let mut explicit = contract;
        explicit.remote_state = HandyRecoveryRemoteState::NotInitialized;
        not_initialized.validate(&explicit).unwrap();
    }
}
