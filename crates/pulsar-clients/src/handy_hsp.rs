//! Read-only HSP stream status; no native ownership, replay, or device I/O.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;

/// Inspect the original stream for this authenticated actor. Missing history or
/// authority stays an Engine error; the client never manufactures an idle stream.
pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    stream_request_id: RequestId,
) -> Result<HandyHspStatus> {
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyHsp(value) = api.execute(
        Command::HandyHspStatus {
            stream_request_id: stream_request_id.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("HSP status response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.stream_request_id == stream_request_id,
        "HSP status subject differs"
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
                .ok_or_else(|| anyhow::anyhow!("stream not found"))
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("HSP status must not submit an identified effect")
        }
    }
    fn report() -> HandyHspStatus {
        HandyHspStatus {
            project_id: ProjectId::new("project").unwrap(),
            stream_request_id: RequestId::new("original-stream").unwrap(),
            stream_record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            generation: 1,
            stream_id: 9,
            live: LiveIdentity {
                epoch: AttemptId::new("capture-epoch").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(1),
                source: SourceVersionId::new("source-version").unwrap(),
            },
            native: Some(HandyHspNativeStatus {
                finite_drain: None,
                runtime_state: HandyHspRuntimeState::StopUnknown,
                native_active: false,
                current_effect: None,
                accepted_samples: 2,
                pending_points: 0,
                retained_points: 2,
                buffered_points: 2,
                total_added: 2,
                last_joined: Some(HandyHspJoinedObservation {
                    effect: HandyHspEffectStatus {
                        sequence: 4,
                        kind: HandyHspEffectKind::Stop,
                    },
                    outcome: HandyHspJoinedOutcome::Unknown,
                }),
                metadata_pending: true,
                stop_pending: false,
                stop_blocked: true,
                emergency_observed: true,
                post_setup_bound: true,
                retired: true,
            }),
            qualified: false,
            emergency_history_durable: false,
        }
    }
    fn directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = directory.path().join("handy-owner.token");
        // Synthetic fixture only; not a real capability or device credential.
        std::fs::write(&token, "a".repeat(64)).unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        directory
    }
    fn reply(value: HandyHspStatus) -> Api {
        Api {
            calls: Vec::new(),
            reply: Some(ResponseBody::HandyHsp(Box::new(value))),
        }
    }
    fn read(api: &mut Api, directory: &Path) -> Result<HandyHspStatus> {
        status(
            api,
            directory,
            ProjectId::new("project").unwrap(),
            RequestId::new("original-stream").unwrap(),
        )
    }

    #[test]
    fn handy_hsp_client_reads_original_subject_without_effect_identity_or_revision() {
        let directory = directory();
        let expected = report();
        let mut api = reply(expected.clone());
        assert_eq!(read(&mut api, directory.path()).unwrap(), expected);
        assert_eq!(api.calls.len(), 1);
        let (command, project, revision) = &api.calls[0];
        assert_eq!(project.as_ref(), Some(&expected.project_id));
        assert!(revision.is_none());
        assert!(
            matches!(command, Command::HandyHspStatus {stream_request_id,owner_proof}
            if stream_request_id==&expected.stream_request_id && owner_proof.len()==64)
        );
    }

    #[test]
    fn handy_hsp_client_refuses_missing_proof_wrong_subject_and_fabricated_qualification() {
        let missing = tempfile::tempdir().unwrap();
        let mut api = reply(report());
        assert!(read(&mut api, missing.path()).is_err());
        assert!(api.calls.is_empty());
        let directory = directory();
        for case in 0..6 {
            let mut value = report();
            match case {
                0 => value.project_id = ProjectId::new("other").unwrap(),
                1 => value.stream_request_id = RequestId::new("other").unwrap(),
                2 => value.qualified = true,
                3 => {
                    value.emergency_history_durable = true;
                    value.native.as_mut().unwrap().emergency_observed = false;
                }
                4 => value.native.as_mut().unwrap().buffered_points = 4097,
                _ => {
                    value
                        .native
                        .as_mut()
                        .unwrap()
                        .last_joined
                        .as_mut()
                        .unwrap()
                        .effect
                        .sequence = 0
                }
            }
            assert!(
                read(&mut reply(value), directory.path()).is_err(),
                "case {case}"
            );
        }
        let mut wrong = Api {
            calls: Vec::new(),
            reply: Some(ResponseBody::Ack),
        };
        assert!(read(&mut wrong, directory.path()).is_err());
        let mut not_found = Api::default();
        let error = read(&mut not_found, directory.path()).unwrap_err();
        assert_eq!(error.to_string(), "stream not found");
        assert_eq!(not_found.calls.len(), 1);
    }

    #[test]
    fn handy_hsp_client_preserves_unknown_stop_emergency_and_metadata_pending_observations() {
        let directory = directory();
        for outcome in [
            HandyHspJoinedOutcome::Acknowledged,
            HandyHspJoinedOutcome::Unknown,
            HandyHspJoinedOutcome::Rejected,
        ] {
            for pending in [false, true] {
                let mut expected = report();
                let native = expected.native.as_mut().unwrap();
                native.last_joined.as_mut().unwrap().outcome = outcome;
                native.metadata_pending = pending;
                assert_eq!(
                    read(&mut reply(expected.clone()), directory.path()).unwrap(),
                    expected
                );
            }
        }
        let mut historical = report();
        historical.native = None;
        historical.emergency_history_durable = true;
        assert_eq!(
            read(&mut reply(historical.clone()), directory.path()).unwrap(),
            historical
        );
    }

    struct NoIo;
    impl crate::motion::MotionIo for NoIo {
        fn control(
            &mut self,
            _: Command,
            _: ProjectId,
            _: Option<RevisionId>,
        ) -> Result<pulsar_protocol::ResponseBody> {
            panic!("HSP status hydration must not send another command");
        }
        fn open_bulk(
            &mut self,
            _: &TransferLease,
            _: std::time::Instant,
        ) -> Result<Box<dyn crate::motion::BulkIo>> {
            panic!("HSP status hydration must not open bulk transport");
        }
        fn bulk_endpoint(&self) -> &Path {
            panic!("HSP status hydration must not resolve a bulk endpoint");
        }
    }

    #[test]
    fn handy_hsp_client_hydration_requires_project_and_valid_snapshot_without_io() {
        let expected = report();
        for project in [None, Some(ProjectId::new("other").unwrap())] {
            let mut cache = crate::motion::MotionCache::default();
            assert!(cache
                .hydrate(
                    pulsar_protocol::ResponseBody::HandyHsp(Box::new(expected.clone())),
                    project.as_ref(),
                    None,
                    &mut NoIo,
                )
                .is_err());
        }
        let mut cache = crate::motion::MotionCache::default();
        let ResponseBody::HandyHsp(actual) = cache
            .hydrate(
                pulsar_protocol::ResponseBody::HandyHsp(Box::new(expected.clone())),
                Some(&expected.project_id),
                None,
                &mut NoIo,
            )
            .unwrap()
        else {
            panic!("HSP status variant changed during hydration");
        };
        assert_eq!(*actual, expected);
        let mut invalid = expected.clone();
        invalid.qualified = true;
        assert!(cache
            .hydrate(
                pulsar_protocol::ResponseBody::HandyHsp(Box::new(invalid)),
                Some(&expected.project_id),
                None,
                &mut NoIo,
            )
            .is_err());
    }
}
