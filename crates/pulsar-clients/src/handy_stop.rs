//! Read-only stop observations. Native activity may lag, and history is incomplete.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;

/// Read the exact original controller's stop observations without acquiring
/// current execution authority or making a physical stopping claim.
pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    acquisition_request_id: RequestId,
) -> Result<HandyStopStatus> {
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyStop(value) = api.execute(
        Command::HandyStopStatus {
            acquisition_request_id: acquisition_request_id.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("stop status response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.acquisition_request_id == acquisition_request_id,
        "stop status subject differs"
    );
    Ok(*value)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[derive(Default)]
    struct Api {
        calls: Vec<(Command, Option<ProjectId>, Option<RevisionId>)>,
        reply: Option<ResponseBody>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.calls.push((command, project, revision));
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("missing mock reply"))
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("stop status must not submit an identified effect")
        }
    }
    fn report() -> HandyStopStatus {
        HandyStopStatus {
            project_id: ProjectId::new("project").unwrap(),
            acquisition_request_id: RequestId::new("acquire").unwrap(),
            controller_id: ClientId::new("controller").unwrap(),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("device-session").unwrap(),
            history: vec![HandyStopHistory {
                record_id: ArtifactId::new("a".repeat(64)).unwrap(),
                sequence: 1,
                outcome: Some(HandyStopOutcome::Acknowledged),
                emergency: false,
            }],
            native: None,
            history_complete: false,
        }
    }
    fn directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = directory.path().join("handy-owner.token");
        // Synthetic local proof only; no real capability or device credential.
        std::fs::write(&token, "a".repeat(64)).unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        directory
    }
    fn read(api: &mut Api, directory: &Path) -> Result<HandyStopStatus> {
        status(
            api,
            directory,
            ProjectId::new("project").unwrap(),
            RequestId::new("acquire").unwrap(),
        )
    }
    fn reply(value: HandyStopStatus) -> Api {
        Api {
            calls: Vec::new(),
            reply: Some(ResponseBody::HandyStop(Box::new(value))),
        }
    }

    #[test]
    fn handy_stop_client_reads_exact_original_subject_with_no_revision_or_effect_identity() {
        let directory = directory();
        let expected = report();
        let mut api = reply(expected.clone());
        assert_eq!(read(&mut api, directory.path()).unwrap(), expected);
        assert_eq!(api.calls.len(), 1);
        let (command, project, revision) = &api.calls[0];
        assert_eq!(project.as_ref(), Some(&expected.project_id));
        assert!(revision.is_none());
        assert!(
            matches!(command, Command::HandyStopStatus { acquisition_request_id, owner_proof }
            if acquisition_request_id == &expected.acquisition_request_id && owner_proof.len() == 64)
        );
    }

    #[test]
    fn handy_stop_client_refuses_missing_owner_proof_changed_subject_and_invalid_reports() {
        let empty = tempfile::tempdir().unwrap();
        let mut api = reply(report());
        assert!(read(&mut api, empty.path()).is_err());
        assert!(api.calls.is_empty());
        let directory = directory();
        for case in 0..6 {
            let mut value = report();
            match case {
                0 => value.project_id = ProjectId::new("other").unwrap(),
                1 => value.acquisition_request_id = RequestId::new("other").unwrap(),
                2 => value.history_complete = true,
                3 => value.history[0].sequence = 0,
                4 => value.history.push(value.history[0].clone()),
                _ => {
                    value.native = Some(HandyStopActivity {
                        record_id: ArtifactId::new("b".repeat(64)).unwrap(),
                        emergency: false,
                        phase: HandyStopPhase::Rejected,
                        outcome: Some(HandyStopOutcome::Acknowledged),
                        metadata_recorded: false,
                    })
                }
            }
            assert!(
                read(&mut reply(value), directory.path()).is_err(),
                "case {case}"
            );
        }
        let mut api = Api {
            calls: Vec::new(),
            reply: Some(ResponseBody::Ack),
        };
        assert!(read(&mut api, directory.path()).is_err());
    }

    #[test]
    fn handy_stop_client_preserves_observed_metadata_pending_and_rejected_activity() {
        let directory = directory();
        for (phase, outcome, metadata_recorded) in [
            (HandyStopPhase::Pending, None, false),
            (HandyStopPhase::InFlight, None, false),
            (HandyStopPhase::Rejected, None, false),
            (
                HandyStopPhase::Observed,
                Some(HandyStopOutcome::Acknowledged),
                false,
            ),
            (
                HandyStopPhase::Observed,
                Some(HandyStopOutcome::Unknown),
                false,
            ),
            (
                HandyStopPhase::Observed,
                Some(HandyStopOutcome::Acknowledged),
                true,
            ),
            (
                HandyStopPhase::Observed,
                Some(HandyStopOutcome::Unknown),
                true,
            ),
        ] {
            let mut expected = report();
            expected.native = Some(HandyStopActivity {
                record_id: ArtifactId::new("b".repeat(64)).unwrap(),
                emergency: true,
                phase,
                outcome,
                metadata_recorded,
            });
            assert_eq!(
                read(&mut reply(expected.clone()), directory.path()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn handy_stop_client_preserves_historical_uncertainty_without_native_authority() {
        let directory = directory();
        for outcome in [
            None,
            Some(HandyStopOutcome::Acknowledged),
            Some(HandyStopOutcome::Unknown),
        ] {
            let mut expected = report();
            expected.history[0].outcome = outcome;
            expected.history[0].emergency = outcome.is_some();
            assert_eq!(
                read(&mut reply(expected.clone()), directory.path()).unwrap(),
                expected
            );
        }
        let mut empty = report();
        empty.history.clear();
        assert_eq!(
            read(&mut reply(empty.clone()), directory.path()).unwrap(),
            empty
        );
        let mut authority = serde_json::to_value(report()).unwrap();
        authority["physical_stopped"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandyStopStatus>(authority).is_err());
    }
}
