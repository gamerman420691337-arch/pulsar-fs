//! Explicit play effects and original history; replies never restore runtime authority.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;

pub struct PlaybackIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub generation: u64,
    pub clock_request_id: RequestId,
    pub start_time_ms: u64,
}

impl PlaybackIntent {
    pub fn validate(&self, id: &RequestId) -> Result<()> {
        ensure!(
            id != &self.acquisition_request_id
                && id != &self.clock_request_id
                && self.generation <= i64::MAX as u64
                && self.start_time_ms < (1u64 << 53),
            "invalid playback request selection"
        );
        Ok(())
    }
}

/// Submit one identified play request. A returned acknowledgment is transport history.
/// Missing history can be an original reservation without preparation, never motion success.
pub fn start(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &PlaybackIntent,
    allow_play: bool,
) -> Result<HandyPlaybackStatus> {
    ensure!(
        allow_play,
        "Handy playback requires explicit effect consent"
    );
    intent.validate(&id)?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyPlayback(value) = api.execute_identified(
        id.clone(),
        Command::StartHandyPlayback {
            acquisition_request_id: intent.acquisition_request_id.clone(),
            controller_id: intent.controller_id.clone(),
            expected_generation: intent.generation,
            clock_request_id: intent.clock_request_id.clone(),
            start_time_ms: intent.start_time_ms,
            allow_play: true,
            owner_proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("playback response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == intent.project && value.play_request_id == id,
        "playback receipt subject differs"
    );
    if let Some(history) = &value.history {
        let controller = &history.original_controller;
        ensure!(
            controller.project_id == intent.project
                && controller.revision == intent.revision
                && controller.acquisition_request_id == intent.acquisition_request_id
                && controller.controller_id == intent.controller_id
                && controller.generation == intent.generation
                && history.clock_request_id == intent.clock_request_id
                && history.start_time_ms == intent.start_time_ms,
            "playback captured selection differs"
        );
    }
    Ok(*value)
}

/// Read original operation history, without claiming current controller or physical state.
pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyPlaybackStatus> {
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyPlayback(value) = api.execute(
        Command::HandyPlaybackStatus {
            play_request_id: original.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("playback status response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.play_request_id == original,
        "playback status subject differs"
    );
    Ok(*value)
}

pub(crate) fn validate_cancellation(
    id: &RequestId,
    original: &RequestId,
    record: &ArtifactId,
) -> Result<()> {
    ensure!(
        id != original,
        "cancellation requires its own request identity"
    );
    validate_sha256(record.as_str())?;
    Ok(())
}

/// Cancel only an unclaimed preparation. This is not a physical stop operation.
pub fn cancel(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    original: RequestId,
    record: ArtifactId,
) -> Result<HandyPlaybackStatus> {
    validate_cancellation(&id, &original, &record)?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyPlayback(value) = api.execute_identified(
        id,
        Command::CancelHandyPlayback {
            play_request_id: original.clone(),
            record_id: record.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("playback cancellation response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project
            && value.play_request_id == original
            && value.history.as_ref().is_some_and(|h| {
                h.record_id == record && h.state == HandyPlaybackOperationState::Cancelled
            }),
        "playback cancellation is not the selected settled record"
    );
    Ok(*value)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    type Call = (
        Option<RequestId>,
        Command,
        Option<ProjectId>,
        Option<RevisionId>,
    );
    #[derive(Default)]
    struct Api {
        calls: Vec<Call>,
        reply: Option<ResponseBody>,
    }
    impl Api {
        fn with_report(report: HandyPlaybackStatus) -> Self {
            Self {
                calls: Vec::new(),
                reply: Some(ResponseBody::HandyPlayback(Box::new(report))),
            }
        }
        fn respond(&mut self, call: Call) -> Result<ResponseBody> {
            self.calls.push(call);
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("missing mock reply"))
        }
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.respond((None, command, project, revision))
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.respond((Some(id), command, project, revision))
        }
    }

    fn fixture() -> (PlaybackIntent, HandyPlaybackStatus) {
        let controller: HandyControllerReceipt = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-controller.json"
        ))
        .unwrap();
        let controller = controller.lease;
        let clock_request_id = RequestId::new("clock-fixture").unwrap();
        let intent = PlaybackIntent {
            project: controller.project_id.clone(),
            revision: controller.revision,
            acquisition_request_id: controller.acquisition_request_id.clone(),
            controller_id: controller.controller_id.clone(),
            generation: controller.generation,
            clock_request_id: clock_request_id.clone(),
            start_time_ms: 125,
        };
        let report = HandyPlaybackStatus {
            project_id: intent.project.clone(),
            play_request_id: RequestId::new("play-fixture").unwrap(),
            history: Some(HandyPlaybackHistory {
                browser_start: None,
                record_id: ArtifactId::new("b".repeat(64)).unwrap(),
                original_controller: controller,
                qualification: HandyQualificationBinding {
                    dossier_id: ArtifactId::new("c".repeat(64)).unwrap(),
                    revision: 2,
                },
                setup_record_id: ArtifactId::new("d".repeat(64)).unwrap(),
                clock_request_id,
                start_time_ms: intent.start_time_ms,
                state: HandyPlaybackOperationState::Prepared,
            }),
        };
        report.validate().unwrap();
        (intent, report)
    }

    fn private_directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = directory.path().join("handy-owner.token");
        // Synthetic owner-proof fixture, never a real capability or credential.
        std::fs::write(&token, "a".repeat(64)).unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        directory
    }

    #[test]
    fn handy_play_client_checks_consent_and_selection_before_owner_proof() {
        let directory = tempfile::tempdir().unwrap();
        for case in 0..5 {
            let (mut intent, report) = fixture();
            let mut id = report.play_request_id;
            match case {
                0 => {}
                1 => id = intent.acquisition_request_id.clone(),
                2 => id = intent.clock_request_id.clone(),
                3 => intent.generation = i64::MAX as u64 + 1,
                _ => intent.start_time_ms = 1u64 << 53,
            }
            let mut api = Api::default();
            let error = start(&mut api, directory.path(), id, &intent, case != 0).unwrap_err();
            assert_eq!(
                error.to_string(),
                if case == 0 {
                    "Handy playback requires explicit effect consent"
                } else {
                    "invalid playback request selection"
                }
            );
            assert!(api.calls.is_empty());
        }
    }

    #[test]
    fn handy_play_client_preserves_request_and_all_captured_selection() {
        let directory = private_directory();
        let (intent, good) = fixture();
        for case in 0..13 {
            let mut report = good.clone();
            match case {
                0 => {}
                1 => report.history = None,
                2 => report.project_id = ProjectId::new("other").unwrap(),
                3 => report.play_request_id = RequestId::new("other").unwrap(),
                4 => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .project_id = ProjectId::new("other").unwrap()
                }
                5 => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .revision = RevisionId::new(2)
                }
                6 => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .acquisition_request_id = RequestId::new("other").unwrap()
                }
                7 => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .controller_id = ClientId::new("other").unwrap()
                }
                8 => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .generation += 1
                }
                9 => {
                    report.history.as_mut().unwrap().clock_request_id =
                        RequestId::new("other").unwrap()
                }
                10 => report.history.as_mut().unwrap().start_time_ms += 1,
                11 => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .playback_enabled = true
                }
                _ => {
                    report.history.as_mut().unwrap().record_id =
                        ArtifactId::new("invalid-hash").unwrap()
                }
            }
            let mut api = Api::with_report(report);
            let result = start(
                &mut api,
                directory.path(),
                good.play_request_id.clone(),
                &intent,
                true,
            );
            assert_eq!(result.is_ok(), case < 2, "case {case}");
            let (id, command, project, revision) = &api.calls[0];
            assert_eq!(id.as_ref(), Some(&good.play_request_id));
            assert_eq!(project.as_ref(), Some(&intent.project));
            assert_eq!(*revision, Some(intent.revision));
            let Command::StartHandyPlayback {
                acquisition_request_id,
                controller_id,
                expected_generation,
                clock_request_id,
                start_time_ms,
                allow_play,
                owner_proof,
            } = command
            else {
                panic!("wrong play command")
            };
            assert_eq!(acquisition_request_id, &intent.acquisition_request_id);
            assert_eq!(controller_id, &intent.controller_id);
            assert_eq!(*expected_generation, intent.generation);
            assert_eq!(clock_request_id, &intent.clock_request_id);
            assert_eq!(*start_time_ms, intent.start_time_ms);
            assert!(*allow_play && owner_proof.len() == 64);
        }
    }

    #[test]
    fn handy_play_client_preserves_historical_states_without_enabling_authority() {
        let directory = private_directory();
        let (intent, good) = fixture();
        for state in [
            HandyPlaybackOperationState::Prepared,
            HandyPlaybackOperationState::Claimed,
            HandyPlaybackOperationState::Acknowledged,
            HandyPlaybackOperationState::Unknown,
            HandyPlaybackOperationState::Cancelled,
        ] {
            let mut report = good.clone();
            report.history.as_mut().unwrap().state = state;
            let mut api = Api::with_report(report.clone());
            let result = start(
                &mut api,
                directory.path(),
                good.play_request_id.clone(),
                &intent,
                true,
            )
            .unwrap();
            assert_eq!(result.history.unwrap().state, state);
            let mut api = Api::with_report(report);
            let result = status(
                &mut api,
                directory.path(),
                good.project_id.clone(),
                good.play_request_id.clone(),
            )
            .unwrap();
            let history = result.history.unwrap();
            assert_eq!(history.state, state);
            assert!(!history.original_controller.playback_enabled);
            let (id, command, project, revision) = &api.calls[0];
            assert!(id.is_none() && revision.is_none());
            assert_eq!(project.as_ref(), Some(&good.project_id));
            assert!(
                matches!(command, Command::HandyPlaybackStatus { play_request_id, .. } if play_request_id == &good.play_request_id)
            );
        }
    }

    #[test]
    fn handy_play_status_requires_original_subject_and_valid_history() {
        let directory = private_directory();
        let (_, good) = fixture();
        for case in 0..4 {
            let mut report = good.clone();
            match case {
                0 => report.history = None,
                1 => report.project_id = ProjectId::new("other").unwrap(),
                2 => report.play_request_id = RequestId::new("other").unwrap(),
                _ => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .playback_enabled = true
                }
            }
            let result = status(
                &mut Api::with_report(report),
                directory.path(),
                good.project_id.clone(),
                good.play_request_id.clone(),
            );
            assert_eq!(result.is_ok(), case == 0, "case {case}");
        }
    }

    #[test]
    fn handy_play_cancel_requires_valid_selection_before_owner_proof() {
        let directory = tempfile::tempdir().unwrap();
        let (_, good) = fixture();
        let mut api = Api::default();
        let record = good.history.unwrap().record_id;
        let error = cancel(
            &mut api,
            directory.path(),
            good.play_request_id.clone(),
            good.project_id.clone(),
            good.play_request_id.clone(),
            record,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cancellation requires its own request identity"
        );
        let error = cancel(
            &mut api,
            directory.path(),
            RequestId::new("cancel").unwrap(),
            good.project_id,
            good.play_request_id,
            ArtifactId::new("invalid-hash").unwrap(),
        )
        .unwrap_err();
        assert!(!error.to_string().contains("owner"));
        assert!(api.calls.is_empty());
    }

    #[test]
    fn handy_play_cancel_accepts_only_exact_cancelled_history() {
        let directory = private_directory();
        let (_, mut good) = fixture();
        good.history.as_mut().unwrap().state = HandyPlaybackOperationState::Cancelled;
        let record = good.history.as_ref().unwrap().record_id.clone();
        let cancellation = RequestId::new("cancel-fixture").unwrap();
        for case in 0..10 {
            let mut report = good.clone();
            match case {
                0 => {}
                1 => report.history = None,
                2 => report.project_id = ProjectId::new("other").unwrap(),
                3 => report.play_request_id = RequestId::new("other").unwrap(),
                4 => {
                    report.history.as_mut().unwrap().record_id =
                        ArtifactId::new("e".repeat(64)).unwrap()
                }
                5 => report.history.as_mut().unwrap().state = HandyPlaybackOperationState::Prepared,
                6 => report.history.as_mut().unwrap().state = HandyPlaybackOperationState::Claimed,
                7 => {
                    report.history.as_mut().unwrap().state =
                        HandyPlaybackOperationState::Acknowledged
                }
                8 => report.history.as_mut().unwrap().state = HandyPlaybackOperationState::Unknown,
                _ => {
                    report
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .playback_enabled = true
                }
            }
            let mut api = Api::with_report(report);
            let result = cancel(
                &mut api,
                directory.path(),
                cancellation.clone(),
                good.project_id.clone(),
                good.play_request_id.clone(),
                record.clone(),
            );
            assert_eq!(result.is_ok(), case == 0, "case {case}");
            let (id, command, project, revision) = &api.calls[0];
            assert_eq!(id.as_ref(), Some(&cancellation));
            assert_eq!(project.as_ref(), Some(&good.project_id));
            assert!(revision.is_none());
            assert!(
                matches!(command, Command::CancelHandyPlayback { play_request_id, record_id, .. } if play_request_id == &good.play_request_id && record_id == &record)
            );
        }
    }

    #[test]
    fn handy_play_operations_reject_wrong_response_variants() {
        let directory = private_directory();
        let (intent, good) = fixture();
        let wrong_reply = || Api {
            calls: Vec::new(),
            reply: Some(ResponseBody::Ack),
        };
        assert!(start(
            &mut wrong_reply(),
            directory.path(),
            good.play_request_id.clone(),
            &intent,
            true
        )
        .is_err());
        assert!(status(
            &mut wrong_reply(),
            directory.path(),
            good.project_id.clone(),
            good.play_request_id.clone()
        )
        .is_err());
        assert!(cancel(
            &mut wrong_reply(),
            directory.path(),
            RequestId::new("cancel").unwrap(),
            good.project_id,
            good.play_request_id,
            good.history.unwrap().record_id
        )
        .is_err());
    }
}
