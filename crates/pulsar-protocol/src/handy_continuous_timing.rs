//! Qualified reported-playtime observations, never native authority constructors.
use crate::*;

pub const HANDY_CONTINUOUS_TIMING_MAX_OBSERVATION_AGE_NS: u64 = 30_000_000_000;
const MAX_EXACT_MS: u64 = (1u64 << 53) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyContinuousTimingModel {
    /// Physical qualification must bound reported HSSP stream playtime against
    /// execution for the exact firmware and operative settings, including the
    /// non-atomic response brackets. This is neither a vendor guarantee nor
    /// physical position proof, and says nothing about uptime/offset algebra.
    HsspPlaytimeRequestBracket,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyContinuousTimingQualification {
    pub model: HandyContinuousTimingModel,
    pub timestamp_error_ns: u64,
    pub maximum_observation_age_ns: u64,
    pub maximum_sample_span_ns: u64,
    pub drift_ppm: u32,
}
impl HandyContinuousTimingQualification {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.timestamp_error_ns == 0
            || !(1..=HANDY_CONTINUOUS_TIMING_MAX_OBSERVATION_AGE_NS)
                .contains(&self.maximum_observation_age_ns)
            || self.maximum_sample_span_ns == 0
            || self.maximum_sample_span_ns > self.maximum_observation_age_ns
        {
            return Err(ProtocolError::invalid(
                "invalid continuous timing qualification",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyContinuousTimingSelection {
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub expected_generation: u64,
    pub play_request_id: RequestId,
    pub play_record_id: ArtifactId,
}
impl HandyContinuousTimingSelection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.play_record_id.as_str())?;
        if self.acquisition_request_id == self.play_request_id
            || self.expected_generation > i64::MAX as u64
        {
            return Err(ProtocolError::invalid(
                "invalid continuous timing selection",
            ));
        }
        Ok(())
    }
}

/// Immutable original play reference. Its original combined uncertainty is
/// retained in every alignment calculation and cannot shrink on refresh.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyContinuousTimeReference {
    pub start_time_ms: u64,
    pub server_time_ms: u64,
    pub uncertainty_ns: u64,
}
impl HandyContinuousTimeReference {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.start_time_ms > MAX_EXACT_MS || self.server_time_ms > MAX_EXACT_MS {
            return Err(ProtocolError::invalid(
                "original continuous timing reference exceeds exact numeric bounds",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyContinuousServerSample {
    pub sent_boottime_ns: u64,
    pub received_boottime_ns: u64,
    pub server_time_ms: u64,
}
impl HandyContinuousServerSample {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.received_boottime_ns < self.sent_boottime_ns || self.server_time_ms > MAX_EXACT_MS {
            return Err(ProtocolError::invalid("invalid continuous server sample"));
        }
        Ok(())
    }
}

/// Validate the stable context surrounding advancing reported playtime. This
/// deliberately does not change the stopped/Active-only observation contracts.
pub fn validate_handy_playing_context(
    before: &HandySetupDeviceObservation,
    after: &HandySetupDeviceObservation,
) -> Result<(), ProtocolError> {
    before.validate()?;
    after.validate()?;
    let (Some(first), Some(second)) = (&before.remote_hssp, &after.remote_hssp) else {
        return Err(ProtocolError::invalid(
            "continuous timing requires HSSP observations",
        ));
    };
    if before.identity != after.identity
        || before.operative_settings != after.operative_settings
        || before.mode != after.mode
        || before.mode.mode != 1
        || first.play_state != HandyRemotePlayState::Playing
        || second.play_state != HandyRemotePlayState::Playing
        || first.stream_id != second.stream_id
        || first.playback_rate != 1.0
        || second.playback_rate != 1.0
        || first.looping
        || second.looping
        || first.pause_on_starving != second.pause_on_starving
        || second.current_time_ms < first.current_time_ms
    {
        return Err(ProtocolError::invalid(
            "continuous timing playing context changed",
        ));
    }
    Ok(())
}

/// Historical arithmetic only. Actual samples, qualification and the immutable
/// original reference must still be established by the native Engine.
pub fn handy_continuous_alignment_error_ns(
    reference: &HandyContinuousTimeReference,
    observed_phase_ms: u64,
    phase_uncertainty_ns: u64,
    estimated_server_ms: u64,
    server_uncertainty_ns: u64,
) -> Result<u64, ProtocolError> {
    reference.validate()?;
    let invalid =
        || ProtocolError::invalid("continuous timing alignment exceeds checked numeric bounds");
    if observed_phase_ms > MAX_EXACT_MS || estimated_server_ms > MAX_EXACT_MS {
        return Err(invalid());
    }
    let expected_phase = u128::from(reference.start_time_ms)
        .checked_add(u128::from(estimated_server_ms))
        .and_then(|value| value.checked_sub(u128::from(reference.server_time_ms)))
        .filter(|value| *value <= u128::from(MAX_EXACT_MS))
        .ok_or_else(invalid)?;
    u128::from(observed_phase_ms)
        .abs_diff(expected_phase)
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(u128::from(reference.uncertainty_ns)))
        .and_then(|value| value.checked_add(u128::from(phase_uncertainty_ns)))
        .and_then(|value| value.checked_add(u128::from(server_uncertainty_ns)))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(invalid)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyContinuousTimingObservation {
    pub request_id: RequestId,
    pub controller: HandyControllerLease,
    pub play_request_id: RequestId,
    pub play_record_id: ArtifactId,
    pub qualification: HandyQualificationBinding,
    pub synchronization: HandyClockSyncBinding,
    pub sequence: u64,
    pub contract: HandyContinuousTimingQualification,
    pub reference: HandyContinuousTimeReference,
    pub before: HandySetupDeviceObservation,
    pub first: HandyDeviceTimingSample,
    pub second: HandyDeviceTimingSample,
    pub after: HandySetupDeviceObservation,
    pub inspection_started_boottime_ns: u64,
    pub inspection_finished_boottime_ns: u64,
    pub server_before: HandyContinuousServerSample,
    pub server_after: HandyContinuousServerSample,
    pub observed_at_boottime_ns: u64,
    pub valid_until_boottime_ns: u64,
    pub observed_phase_ms: u64,
    pub phase_uncertainty_ns: u64,
    pub estimated_server_ms: u64,
    pub server_uncertainty_ns: u64,
    pub alignment_error_ns: u64,
    /// Whether the Engine still retains the exact native observation when read.
    /// This historical DTO cannot reconstruct that native timing source.
    pub current: bool,
    pub playback_enabled: bool,
}
impl HandyContinuousTimingObservation {
    pub fn selection(&self) -> HandyContinuousTimingSelection {
        HandyContinuousTimingSelection {
            acquisition_request_id: self.controller.acquisition_request_id.clone(),
            controller_id: self.controller.controller_id.clone(),
            expected_generation: self.controller.generation,
            play_request_id: self.play_request_id.clone(),
            play_record_id: self.play_record_id.clone(),
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.controller.validate()?;
        self.selection().validate()?;
        self.qualification.validate()?;
        self.synchronization.validate()?;
        self.contract.validate()?;
        self.reference.validate()?;
        self.first.validate()?;
        self.second.validate()?;
        self.server_before.validate()?;
        self.server_after.validate()?;
        validate_handy_playing_context(&self.before, &self.after)?;
        let span = |start: u64, end: u64| {
            end.checked_sub(start)
                .is_some_and(|value| value <= self.contract.maximum_sample_span_ns)
        };
        let maximum_validity = self
            .inspection_started_boottime_ns
            .checked_add(self.contract.maximum_observation_age_ns);
        if self.controller.state != HandyControllerState::Playing
            || self.playback_enabled
            || self.sequence == 0
            || self.request_id == self.play_request_id
            || self.request_id == self.controller.acquisition_request_id
            || self.server_before.received_boottime_ns > self.inspection_started_boottime_ns
            || self.inspection_started_boottime_ns > self.first.read_started_boottime_ns
            || self.first.read_finished_boottime_ns > self.second.read_started_boottime_ns
            || self.second.read_finished_boottime_ns > self.inspection_finished_boottime_ns
            || self.inspection_finished_boottime_ns > self.server_after.sent_boottime_ns
            || self.second.info.device_uptime_ms < self.first.info.device_uptime_ms
            || !span(
                self.inspection_started_boottime_ns,
                self.inspection_finished_boottime_ns,
            )
            || !span(
                self.server_before.sent_boottime_ns,
                self.server_before.received_boottime_ns,
            )
            || !span(
                self.server_after.sent_boottime_ns,
                self.server_after.received_boottime_ns,
            )
            || self.observed_at_boottime_ns != self.server_after.received_boottime_ns
            || self.observed_at_boottime_ns >= self.controller.deadline_boottime_ns
            || self.valid_until_boottime_ns <= self.observed_at_boottime_ns
            || maximum_validity.is_none_or(|maximum| self.valid_until_boottime_ns > maximum)
            || self.alignment_error_ns
                != handy_continuous_alignment_error_ns(
                    &self.reference,
                    self.observed_phase_ms,
                    self.phase_uncertainty_ns,
                    self.estimated_server_ms,
                    self.server_uncertainty_ns,
                )?
        {
            return Err(ProtocolError::invalid(
                "invalid continuous timing observation",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation() -> HandyContinuousTimingObservation {
        let mut device: HandyDeviceTimingInspection =
            serde_json::from_str(include_str!("../tests/fixtures/handy-device-timing.json"))
                .unwrap();
        device.controller.state = HandyControllerState::Playing;
        device.controller.physical_stop_pending = true;
        device.controller.deadline_boottime_ns = 1_050_000_000;
        for (sample, phase) in [(&mut device.before, 100), (&mut device.after, 102)] {
            let hssp = sample.remote_hssp.as_mut().unwrap();
            hssp.play_state = HandyRemotePlayState::Playing;
            hssp.current_time_ms = phase;
            hssp.playback_rate = 1.0;
            hssp.looping = false;
        }
        device.first.read_started_boottime_ns = 1_003_000_000;
        device.first.read_finished_boottime_ns = 1_004_000_000;
        device.second.read_started_boottime_ns = 1_005_000_000;
        device.second.read_finished_boottime_ns = 1_006_000_000;
        let reference = HandyContinuousTimeReference {
            start_time_ms: 0,
            server_time_ms: 1000,
            uncertainty_ns: 1000,
        };
        let alignment_error_ns =
            handy_continuous_alignment_error_ns(&reference, 101, 2_000_000, 1102, 1_000_000)
                .unwrap();
        let value = HandyContinuousTimingObservation {
            request_id: RequestId::new("refresh-fixture").unwrap(),
            controller: device.controller,
            play_request_id: RequestId::new("play-fixture").unwrap(),
            play_record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            qualification: device.preparation_qualification,
            synchronization: HandyClockSyncBinding {
                sync_request_id: RequestId::new("sync-fixture").unwrap(),
                record_id: ArtifactId::new("b".repeat(64)).unwrap(),
            },
            sequence: 1,
            contract: HandyContinuousTimingQualification {
                model: HandyContinuousTimingModel::HsspPlaytimeRequestBracket,
                timestamp_error_ns: 1000,
                maximum_observation_age_ns: 100_000_000,
                maximum_sample_span_ns: 10_000_000,
                drift_ppm: 10,
            },
            reference,
            before: device.before,
            first: device.first,
            second: device.second,
            after: device.after,
            inspection_started_boottime_ns: 1_002_000_000,
            inspection_finished_boottime_ns: 1_008_000_000,
            server_before: HandyContinuousServerSample {
                sent_boottime_ns: 1_000_000_000,
                received_boottime_ns: 1_001_000_000,
                server_time_ms: 1092,
            },
            server_after: HandyContinuousServerSample {
                sent_boottime_ns: 1_009_000_000,
                received_boottime_ns: 1_010_000_000,
                server_time_ms: 1102,
            },
            observed_at_boottime_ns: 1_010_000_000,
            valid_until_boottime_ns: 1_100_000_000,
            observed_phase_ms: 101,
            phase_uncertainty_ns: 2_000_000,
            estimated_server_ms: 1102,
            server_uncertainty_ns: 1_000_000,
            alignment_error_ns,
            current: true,
            playback_enabled: false,
        };
        value.validate().unwrap();
        value
    }

    #[test]
    fn handy_continuous_contract_and_selection_enforce_explicit_bounds() {
        let good = observation();
        for case in 0..5 {
            let mut contract = good.contract.clone();
            match case {
                0 => contract.timestamp_error_ns = 0,
                1 => contract.maximum_observation_age_ns = 0,
                2 => {
                    contract.maximum_observation_age_ns =
                        HANDY_CONTINUOUS_TIMING_MAX_OBSERVATION_AGE_NS + 1
                }
                3 => contract.maximum_sample_span_ns = 0,
                _ => contract.maximum_sample_span_ns = contract.maximum_observation_age_ns + 1,
            }
            assert!(contract.validate().is_err());
        }
        let mut boundary = good.contract.clone();
        boundary.maximum_observation_age_ns = 30_000_000_000;
        boundary.maximum_sample_span_ns = boundary.maximum_observation_age_ns;
        boundary.validate().unwrap();
        for case in 0..3 {
            let mut selection = good.selection();
            match case {
                0 => selection.play_record_id = ArtifactId::new("invalid-digest").unwrap(),
                1 => selection.play_request_id = selection.acquisition_request_id.clone(),
                _ => selection.expected_generation = i64::MAX as u64 + 1,
            }
            assert!(selection.validate().is_err());
        }
        let mut boundary = good.selection();
        boundary.expected_generation = i64::MAX as u64;
        boundary.validate().unwrap();
    }

    #[test]
    fn handy_continuous_alignment_keeps_original_uncertainty_and_checks_all_arithmetic() {
        let reference = HandyContinuousTimeReference {
            start_time_ms: 50,
            server_time_ms: 1000,
            uncertainty_ns: 5,
        };
        assert_eq!(
            handy_continuous_alignment_error_ns(&reference, 99, 7, 1050, 11).unwrap(),
            1_000_023
        );
        assert_eq!(
            handy_continuous_alignment_error_ns(&reference, 100, 7, 1050, 11).unwrap(),
            23
        );
        let mut invalid = reference.clone();
        invalid.server_time_ms = 2000;
        assert!(handy_continuous_alignment_error_ns(&invalid, 100, 0, 1000, 0).is_err());
        let mut invalid = reference.clone();
        invalid.start_time_ms = MAX_EXACT_MS;
        invalid.server_time_ms = 0;
        assert!(handy_continuous_alignment_error_ns(&invalid, 100, 0, 1, 0).is_err());
        let mut invalid = reference.clone();
        invalid.uncertainty_ns = u64::MAX;
        assert!(handy_continuous_alignment_error_ns(&invalid, 100, 1, 1050, 0).is_err());
        assert!(handy_continuous_alignment_error_ns(&reference, MAX_EXACT_MS, 0, 1000, 0).is_err());
        assert!(
            handy_continuous_alignment_error_ns(&reference, MAX_EXACT_MS + 1, 0, 1000, 0).is_err()
        );
        assert!(
            handy_continuous_alignment_error_ns(&reference, 100, 0, MAX_EXACT_MS + 1, 0).is_err()
        );
        let boundary = HandyContinuousTimeReference {
            start_time_ms: MAX_EXACT_MS,
            server_time_ms: MAX_EXACT_MS,
            uncertainty_ns: 2,
        };
        assert_eq!(
            handy_continuous_alignment_error_ns(&boundary, MAX_EXACT_MS, 3, MAX_EXACT_MS, 4)
                .unwrap(),
            9
        );
    }

    #[test]
    fn handy_continuous_playing_context_allows_progress_but_not_stream_or_configuration_changes() {
        let good = observation();
        assert!(!good.before.same_state(&good.after));
        validate_handy_playing_context(&good.before, &good.after).unwrap();
        for case in 0..11 {
            let mut before = good.before.clone();
            let mut after = good.after.clone();
            match case {
                0 => after.identity.firmware.push('x'),
                1 => after.mode.mode_session_id += 1,
                2 => {
                    before.remote_hssp.as_mut().unwrap().play_state = HandyRemotePlayState::Stopped
                }
                3 => after.remote_hssp.as_mut().unwrap().play_state = HandyRemotePlayState::Paused,
                4 => after.remote_hssp.as_mut().unwrap().stream_id += 1,
                5 => before.remote_hssp.as_mut().unwrap().playback_rate = 0.5,
                6 => after.remote_hssp.as_mut().unwrap().playback_rate = 1.5,
                7 => after.remote_hssp.as_mut().unwrap().looping = true,
                8 => {
                    after.remote_hssp.as_mut().unwrap().pause_on_starving =
                        !before.remote_hssp.as_ref().unwrap().pause_on_starving
                }
                9 => after.remote_hssp.as_mut().unwrap().current_time_ms = 99,
                _ => {
                    let mut settings = serde_json::to_value(&after.operative_settings).unwrap();
                    settings["slider"]["x_limit_stop"] = serde_json::json!(99.0);
                    after.operative_settings = serde_json::from_value(settings).unwrap();
                }
            }
            assert!(
                validate_handy_playing_context(&before, &after).is_err(),
                "case {case}"
            );
        }
        let mut before = good.before;
        let mut after = good.after;
        before.remote_hssp.as_mut().unwrap().pause_on_starving = true;
        after.remote_hssp.as_mut().unwrap().pause_on_starving = true;
        after.observed_at_unix_ms += 1;
        validate_handy_playing_context(&before, &after).unwrap();
    }

    #[test]
    fn handy_continuous_observation_checks_chronology_validity_and_authority_flags() {
        let good = observation();
        assert!(good.valid_until_boottime_ns > good.controller.deadline_boottime_ns);
        let mut historical = good.clone();
        historical.current = false;
        historical.validate().unwrap();
        for case in 0..24 {
            let mut bad = good.clone();
            match case {
                0 => {
                    bad.controller.state = HandyControllerState::Active;
                    bad.controller.physical_stop_pending = false;
                }
                1 => bad.playback_enabled = true,
                2 => bad.sequence = 0,
                3 => bad.request_id = bad.play_request_id.clone(),
                4 => bad.request_id = bad.controller.acquisition_request_id.clone(),
                5 => bad.play_record_id = ArtifactId::new("invalid").unwrap(),
                6 => bad.qualification.revision = 1,
                7 => bad.synchronization.record_id = ArtifactId::new("invalid").unwrap(),
                8 => bad.observed_at_boottime_ns -= 1,
                9 => bad.valid_until_boottime_ns = bad.observed_at_boottime_ns,
                10 => {
                    bad.valid_until_boottime_ns = bad.inspection_started_boottime_ns
                        + bad.contract.maximum_observation_age_ns
                        + 1
                }
                11 => bad.controller.deadline_boottime_ns = bad.observed_at_boottime_ns,
                12 => {
                    bad.server_before.received_boottime_ns = bad.inspection_started_boottime_ns + 1
                }
                13 => bad.inspection_started_boottime_ns = bad.first.read_started_boottime_ns + 1,
                14 => bad.first.read_finished_boottime_ns = bad.second.read_started_boottime_ns + 1,
                15 => {
                    bad.second.read_finished_boottime_ns = bad.inspection_finished_boottime_ns + 1
                }
                16 => bad.inspection_finished_boottime_ns = bad.server_after.sent_boottime_ns + 1,
                17 => bad.contract.maximum_sample_span_ns = 5_000_000,
                18 => {
                    bad.server_before.sent_boottime_ns = bad.server_before.received_boottime_ns
                        - bad.contract.maximum_sample_span_ns
                        - 1
                }
                19 => {
                    bad.server_after.received_boottime_ns =
                        bad.server_after.sent_boottime_ns + bad.contract.maximum_sample_span_ns + 1;
                    bad.observed_at_boottime_ns = bad.server_after.received_boottime_ns;
                }
                20 => bad.first.read_finished_boottime_ns = bad.first.read_started_boottime_ns - 1,
                21 => bad.second.info.device_uptime_ms = bad.first.info.device_uptime_ms - 1,
                22 => bad.reference.uncertainty_ns -= 1,
                _ => bad.alignment_error_ns += 1,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
    }

    #[test]
    fn handy_continuous_validity_and_server_bounds_fail_closed_on_overflow() {
        let mut value = observation();
        let shift = u64::MAX - 1_050_000_000;
        value.server_before.sent_boottime_ns += shift;
        value.server_before.received_boottime_ns += shift;
        value.inspection_started_boottime_ns += shift;
        value.inspection_finished_boottime_ns += shift;
        value.first.read_started_boottime_ns += shift;
        value.first.read_finished_boottime_ns += shift;
        value.second.read_started_boottime_ns += shift;
        value.second.read_finished_boottime_ns += shift;
        value.server_after.sent_boottime_ns += shift;
        value.server_after.received_boottime_ns += shift;
        value.observed_at_boottime_ns += shift;
        value.controller.deadline_boottime_ns = u64::MAX;
        value.valid_until_boottime_ns = u64::MAX - 1;
        assert!(value.validate().is_err());
        for sample in [
            HandyContinuousServerSample {
                sent_boottime_ns: 2,
                received_boottime_ns: 1,
                server_time_ms: 0,
            },
            HandyContinuousServerSample {
                sent_boottime_ns: 0,
                received_boottime_ns: 1,
                server_time_ms: MAX_EXACT_MS + 1,
            },
        ] {
            assert!(sample.validate().is_err());
        }
    }

    #[test]
    fn handy_continuous_wire_commands_require_consent_original_selection_and_redacted_proofs() {
        let report = observation();
        let mut request = Request::new(
            report.request_id.clone(),
            Command::RefreshHandyPlaybackTiming {
                selection: report.selection(),
                allow_online_inspection: true,
                owner_proof: "synthetic-private-owner-proof".into(),
            },
        )
        .in_project(
            report.controller.project_id.clone(),
            Some(report.controller.revision),
        );
        request.validate().unwrap();
        assert!(
            request.command.is_mutating()
                && request.command.requires_project()
                && request.command.requires_revision()
        );
        assert!(!format!("{request:?}").contains("synthetic-private-owner-proof"));
        let restored: Request =
            serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
        restored.validate().unwrap();
        for case in 0..5 {
            let mut bad = request.clone();
            match case {
                0 => bad.expected_revision = None,
                1 => bad.project = None,
                2 => bad.request_id = report.play_request_id.clone(),
                3 => bad.request_id = report.controller.acquisition_request_id.clone(),
                _ => {
                    let Command::RefreshHandyPlaybackTiming {
                        allow_online_inspection,
                        ..
                    } = &mut bad.command
                    else {
                        unreachable!();
                    };
                    *allow_online_inspection = false;
                }
            }
            assert!(bad.validate().is_err());
        }
        request.expected_revision = None;
        request.command = Command::HandyContinuousTimingStatus {
            refresh_request_id: report.request_id.clone(),
            owner_proof: "proof".into(),
        };
        request.validate().unwrap();
        assert!(!request.command.is_mutating() && !request.command.requires_revision());
        for proof in [String::new(), "x".repeat(65), "invalid\nproof".into()] {
            for status in [false, true] {
                let command = if status {
                    Command::HandyContinuousTimingStatus {
                        refresh_request_id: report.request_id.clone(),
                        owner_proof: proof.clone(),
                    }
                } else {
                    Command::RefreshHandyPlaybackTiming {
                        selection: report.selection(),
                        allow_online_inspection: true,
                        owner_proof: proof.clone(),
                    }
                };
                let bad = Request::new(report.request_id.clone(), command).in_project(
                    report.controller.project_id.clone(),
                    Some(report.controller.revision),
                );
                assert!(bad.validate().is_err());
            }
        }
        let wire = serde_json::to_value(ResponseBody::HandyContinuousTiming(Box::new(
            report.clone(),
        )))
        .unwrap();
        assert_eq!(wire["kind"], "handy_continuous_timing");
        let ResponseBody::HandyContinuousTiming(roundtrip) = serde_json::from_value(wire).unwrap()
        else {
            panic!();
        };
        roundtrip.validate().unwrap();
        for field in [
            "authority_restored",
            "physical_position_verified",
            "synchronization_verified",
        ] {
            let mut bad = serde_json::to_value(&report).unwrap();
            bad[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyContinuousTimingObservation>(bad).is_err());
        }
    }
}
