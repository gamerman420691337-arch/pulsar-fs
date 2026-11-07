//! Original Handy play history. Reports never restore native execution authority.
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyPlaybackOperationState {
    Prepared,
    Claimed,
    Acknowledged,
    Unknown,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlaybackHistory {
    pub record_id: ArtifactId,
    pub original_controller: HandyControllerLease,
    pub qualification: HandyQualificationBinding,
    pub setup_record_id: ArtifactId,
    pub clock_request_id: RequestId,
    pub start_time_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_start: Option<HandyBrowserStartDecision>,
    pub state: HandyPlaybackOperationState,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlaybackStatus {
    pub project_id: ProjectId,
    pub play_request_id: RequestId,
    pub history: Option<HandyPlaybackHistory>,
}
impl HandyPlaybackStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(h) = &self.history {
            validate_sha256(h.record_id.as_str())?;
            validate_sha256(h.setup_record_id.as_str())?;
            h.original_controller.validate()?;
            h.qualification.validate()?;
            if let Some(browser) = &h.browser_start {
                browser.validate()?;
                if browser.start_time_ms() != h.start_time_ms
                    || browser.subject.device_qualification != h.qualification
                    || browser.subject.project_id != self.project_id
                    || browser.subject.revision != h.original_controller.revision
                    || browser.timing_binding.engine_epoch.as_str()
                        != h.original_controller.engine_epoch.as_str()
                {
                    return Err(ProtocolError::invalid(
                        "browser start history differs from original play",
                    ));
                }
            }
            if h.original_controller.project_id != self.project_id
                || h.original_controller.state != HandyControllerState::Active
                || h.original_controller.physical_stop_pending
                || h.start_time_ms >= (1u64 << 53)
                || h.clock_request_id == self.play_request_id
            {
                return Err(ProtocolError::invalid(
                    "invalid original Handy playback history",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn start() -> Request {
        let mut r = Request::new(
            RequestId::new("play").unwrap(),
            Command::StartHandyPlayback {
                acquisition_request_id: RequestId::new("acquire").unwrap(),
                controller_id: ClientId::new("controller").unwrap(),
                expected_generation: 0,
                clock_request_id: RequestId::new("clock").unwrap(),
                start_time_ms: 0,
                allow_play: true,
                owner_proof: "private-owner-proof".into(),
            },
        );
        r.project = Some(ProjectId::new("project").unwrap());
        r.expected_revision = Some(RevisionId::new(1));
        r
    }
    #[test]
    fn handy_play_start_requires_consent_exact_selection_and_revision() {
        let r = start();
        r.validate().unwrap();
        assert!(r.command.is_mutating());
        assert!(!format!("{:?}", r.command).contains("private-owner-proof"));
        for mode in 0..5 {
            let mut bad = r.clone();
            match mode {
                0 => bad.expected_revision = None,
                1 => {
                    if let Command::StartHandyPlayback { allow_play, .. } = &mut bad.command {
                        *allow_play = false;
                    }
                }
                2 => {
                    if let Command::StartHandyPlayback { start_time_ms, .. } = &mut bad.command {
                        *start_time_ms = 1u64 << 53;
                    }
                }
                3 => {
                    if let Command::StartHandyPlayback {
                        clock_request_id, ..
                    } = &mut bad.command
                    {
                        *clock_request_id = bad.request_id.clone();
                    }
                }
                _ => {
                    if let Command::StartHandyPlayback {
                        expected_generation,
                        ..
                    } = &mut bad.command
                    {
                        *expected_generation = u64::MAX;
                    }
                }
            };
            assert!(bad.validate().is_err());
        }
    }
    #[test]
    fn handy_play_history_and_cancel_are_revision_free_but_keep_original_ids() {
        let mut r = start();
        r.expected_revision = None;
        r.command = Command::HandyPlaybackStatus {
            play_request_id: RequestId::new("original").unwrap(),
            owner_proof: "proof".into(),
        };
        r.validate().unwrap();
        assert!(!r.command.is_mutating());
        r.command = Command::CancelHandyPlayback {
            play_request_id: RequestId::new("original").unwrap(),
            record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            owner_proof: "proof".into(),
        };
        r.validate().unwrap();
        assert!(r.command.is_mutating());
        r.request_id = RequestId::new("original").unwrap();
        assert!(r.validate().is_err());
    }
    #[test]
    fn handy_play_history_preserves_an_original_armed_snapshot_not_current_playing_authority() {
        let lease: HandyControllerReceipt =
            serde_json::from_str(include_str!("../tests/fixtures/handy-controller.json")).unwrap();
        let mut value = HandyPlaybackStatus {
            project_id: lease.lease.project_id.clone(),
            play_request_id: RequestId::new("play").unwrap(),
            history: Some(HandyPlaybackHistory {
                record_id: ArtifactId::new("a".repeat(64)).unwrap(),
                original_controller: lease.lease,
                qualification: HandyQualificationBinding {
                    dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
                    revision: 2,
                },
                setup_record_id: ArtifactId::new("c".repeat(64)).unwrap(),
                clock_request_id: RequestId::new("clock").unwrap(),
                start_time_ms: 0,
                state: HandyPlaybackOperationState::Claimed,
                browser_start: None,
            }),
        };
        value.validate().unwrap();
        let h = value.history.as_mut().unwrap();
        h.original_controller.state = HandyControllerState::Playing;
        h.original_controller.physical_stop_pending = true;
        h.original_controller.validate().unwrap();
        assert!(value.validate().is_err());
        value.history = None;
        value.validate().unwrap();
    }
}
