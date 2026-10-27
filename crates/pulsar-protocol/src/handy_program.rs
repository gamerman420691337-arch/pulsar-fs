//! Fresh stream association. It does not prove remote script contents or grant playback.
use crate::*;
/// HSP state fields shared by setup replies and stopped-stream observations.
pub type HandyHspBuffer = HandySetupAcknowledgment;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyScriptSampleShape {
    pub points: u64,
    pub first_time_ms: u64,
    pub last_time_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> HandyProgramCorrelation {
        serde_json::from_str(include_str!(
            "../tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap()
    }
    #[test]
    fn handy_program_report_accepts_partial_streaming_buffer_but_rejects_authority_and_mismatch() {
        let good = fixture();
        good.validate().unwrap();
        assert!(good.expected_samples.points > good.second_buffer.max_points as u64);
        for index in 0..6 {
            let mut bad = fixture();
            match index {
                0 => bad.content_verified = true,
                1 => bad.playback_enabled = true,
                2 => bad.second_buffer.stream_id += 1,
                3 => bad.setup_mode.mode_session_id += 1,
                4 => bad.second_buffer.play_state = 0,
                _ => bad.inspection.accepted_qualification = None,
            };
            assert!(bad.validate().is_err());
        }
    }
    #[test]
    fn handy_program_command_is_read_only_revision_bound_and_requires_explicit_inspection_consent()
    {
        let value = fixture();
        let i = value.inspection;
        let mut r = Request::new(
            i.request_id,
            Command::InspectHandyProgram {
                setup_request_id: value.setup_request_id,
                record_id: value.record_id,
                qualification: i.accepted_qualification.unwrap(),
                engine_epoch: i.engine_epoch,
                device_session: i.device_session,
                hosting_request_id: i.hosting_request_id,
                allow_online_inspection: true,
                owner_proof: "a".repeat(64),
            },
        );
        assert!(
            !r.command.is_mutating()
                && r.command.requires_project()
                && r.command.requires_revision()
        );
        assert!(r.validate().is_err());
        r.project = Some(i.project_id);
        r.expected_revision = Some(i.revision);
        r.validate().unwrap();
        if let Command::InspectHandyProgram {
            allow_online_inspection,
            ..
        } = &mut r.command
        {
            *allow_online_inspection = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
    }
}
impl HandyScriptSampleShape {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.points < 2
            || self.points > HANDY_STROKE_SCRIPT_MAX_ACTIONS
            || self.first_time_ms >= self.last_time_ms
            || self.last_time_ms > (1u64 << 53) - 1
        {
            return Err(ProtocolError::invalid("invalid Handy script sample shape"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyProgramCorrelation {
    pub inspection: HandySetupInspection,
    pub setup_request_id: RequestId,
    pub record_id: ArtifactId,
    pub setup_mode: HandyDeviceMode,
    pub acknowledgment: HandySetupAcknowledgment,
    pub expected_samples: HandyScriptSampleShape,
    pub first_buffer: HandyHspBuffer,
    pub second_buffer: HandyHspBuffer,
    pub content_verified: bool,
    pub playback_enabled: bool,
}
impl HandyProgramCorrelation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.inspection.validate()?;
        self.expected_samples.validate()?;
        self.setup_mode.validate()?;
        self.acknowledgment.validate()?;
        validate_sha256(self.record_id.as_str())?;
        if self.content_verified
            || self.playback_enabled
            || self.inspection.accepted_qualification.is_none()
            || self.setup_request_id == self.inspection.request_id
            || self.setup_request_id == self.inspection.hosting_request_id
            || self.setup_mode != self.inspection.device.mode
        {
            return Err(ProtocolError::invalid("invalid Handy program correlation"));
        }
        for buffer in [&self.first_buffer, &self.second_buffer] {
            Self::check_buffer(&self.inspection.device, buffer)?;
            if buffer.stream_id != self.acknowledgment.stream_id {
                return Err(ProtocolError::invalid(
                    "Handy stream differs from acknowledged setup",
                ));
            }
        }
        // Buffer population/endpoints may change while a stopped HSSP stream fills.
        // Neither whole-script residency nor an undocumented tail-index origin is required.
        Ok(())
    }
    pub fn check_buffer(
        device: &HandySetupDeviceObservation,
        buffer: &HandyHspBuffer,
    ) -> Result<(), ProtocolError> {
        device.validate()?;
        buffer.validate()?;
        let state = device
            .remote_hssp
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("HSSP state missing"))?;
        if device.mode.mode != 1
            || buffer.play_state != 2
            || state.play_state != HandyRemotePlayState::Stopped
            || state.stream_id != buffer.stream_id
            || state.current_time_ms != buffer.current_time
            || state.playback_rate != buffer.playback_rate
            || state.looping != buffer.r#loop
            || state.pause_on_starving != buffer.pause_on_starving
        {
            return Err(ProtocolError::invalid(
                "stopped HSSP buffer observation differs",
            ));
        }
        Ok(())
    }
}
