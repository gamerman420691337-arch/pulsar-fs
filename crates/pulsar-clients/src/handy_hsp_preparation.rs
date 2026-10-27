//! Explicit HSP preparation and exact original status/cancellation. Metadata
//! cannot restore native readiness or authorize setup, stream points or playback.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;

pub struct PreparationIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub qualification: HandyQualificationBinding,
    pub timeout_ms: u64,
}
impl PreparationIntent {
    pub fn validate(&self) -> Result<()> {
        validate_handy_hsp_preparation_selection(
            &self.engine_epoch,
            &self.device_session,
            &self.qualification,
            self.timeout_ms,
        )?;
        Ok(())
    }
}
fn checked(
    body: ResponseBody,
    project: &ProjectId,
    original: &RequestId,
) -> Result<HandyHspPreparationStatus> {
    let ResponseBody::HandyHspPreparation(value) = body else {
        anyhow::bail!("HSP preparation response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == *project && value.preparation_request_id == *original,
        "HSP preparation response changed original subject"
    );
    Ok(*value)
}

/// Submit a single identified synchronization intent with its explicit budget.
/// A caller retry must retain the same request ID and complete original inputs.
pub fn prepare(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &PreparationIntent,
    allow_online_synchronization: bool,
) -> Result<HandyHspPreparationStatus> {
    ensure!(
        allow_online_synchronization,
        "HSP preparation requires explicit online synchronization consent"
    );
    intent.validate()?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let value = checked(
        api.execute_identified(
            id.clone(),
            Command::PrepareHandyHsp {
                engine_epoch: intent.engine_epoch.clone(),
                device_session: intent.device_session.clone(),
                qualification: intent.qualification.clone(),
                timeout_ms: intent.timeout_ms,
                allow_online_synchronization: true,
                owner_proof,
            },
            Some(intent.project.clone()),
            Some(intent.revision),
        )?,
        &intent.project,
        &id,
    )?;
    if let Some(history) = &value.history {
        ensure!(
            history.engine_epoch == intent.engine_epoch
                && history.device_session == intent.device_session
                && history.revision == intent.revision
                && history.qualification == intent.qualification,
            "HSP preparation history changed captured selection"
        );
    }
    Ok(value)
}

pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyHspPreparationStatus> {
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    checked(
        api.execute(
            Command::HandyHspPreparationStatus {
                preparation_request_id: original.clone(),
                owner_proof,
            },
            Some(project.clone()),
            None,
        )?,
        &project,
        &original,
    )
}

/// Cancellation carries no replacement timeout, new qualification, or playback
/// operation. The original joined/durable outcome remains authoritative.
pub fn cancel(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyHspPreparationStatus> {
    ensure!(
        id != original,
        "HSP preparation cancellation requires its own request identity"
    );
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    checked(
        api.execute_identified(
            id,
            Command::CancelHandyHspPreparation {
                preparation_request_id: original.clone(),
                owner_proof,
            },
            Some(project.clone()),
            None,
        )?,
        &project,
        &original,
    )
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
        fn reply(value: HandyHspPreparationStatus) -> Self {
            Self {
                calls: Vec::new(),
                reply: Some(ResponseBody::HandyHspPreparation(Box::new(value))),
            }
        }
        fn respond(&mut self, call: Call) -> Result<ResponseBody> {
            self.calls.push(call);
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("original preparation not found"))
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
    fn intent() -> PreparationIntent {
        PreparationIntent {
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(2),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("device-session").unwrap(),
            qualification: HandyQualificationBinding {
                dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
                revision: 2,
            },
            timeout_ms: 1000,
        }
    }
    fn report() -> HandyHspPreparationStatus {
        let i = intent();
        HandyHspPreparationStatus {
            project_id: i.project,
            preparation_request_id: RequestId::new("prepare").unwrap(),
            history: Some(HandyHspPreparationHistory {
                record_id: ArtifactId::new("a".repeat(64)).unwrap(),
                engine_epoch: i.engine_epoch,
                device_session: i.device_session,
                revision: i.revision,
                device_id: DeviceId::new("handy").unwrap(),
                registration_revision: 1,
                qualification: i.qualification,
                subject_sha256: "c".repeat(64),
                profile_sha256: "d".repeat(64),
                deadline_boottime_ns: 1000,
                state: HandyClockSyncState::Unknown,
                acknowledgment: None,
                linked_stream_id: None,
                custody_transferred: false,
                resolved: false,
            }),
            native: Some(HandyHspPreparationNativeStatus {
                phase: HandyHspPreparationPhase::Retired,
                worker_active: false,
                cancel_requested: true,
                metadata_pending: true,
                clock_valid_until_boottime_ns: None,
                error: None,
            }),
            armed: false,
            playback_enabled: false,
        }
    }
    fn directory() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let proof = dir.path().join("handy-owner.token");
        std::fs::write(&proof, "a".repeat(64)).unwrap();
        std::fs::set_permissions(proof, std::fs::Permissions::from_mode(0o600)).unwrap();
        dir
    }
    #[test]
    fn hsp_preparation_client_preserves_identified_selection_and_explicit_consent() {
        let directory = directory();
        let expected = report();
        let mut api = Api::reply(expected.clone());
        assert_eq!(
            prepare(
                &mut api,
                directory.path(),
                expected.preparation_request_id.clone(),
                &intent(),
                true
            )
            .unwrap(),
            expected
        );
        assert_eq!(api.calls.len(), 1);
        let (id, command, project, revision) = &api.calls[0];
        assert_eq!(id.as_ref(), Some(&expected.preparation_request_id));
        assert_eq!(project.as_ref(), Some(&expected.project_id));
        assert_eq!(*revision, Some(RevisionId::new(2)));
        assert!(matches!(
            command,
            Command::PrepareHandyHsp {
                timeout_ms: 1000,
                allow_online_synchronization: true,
                ..
            }
        ));
        for case in 0..4 {
            let mut i = intent();
            let mut api = Api::reply(report());
            match case {
                0 => {}
                1 => i.timeout_ms = 0,
                2 => i.device_session = i.engine_epoch.clone(),
                _ => i.qualification.revision = 1,
            }
            assert!(prepare(
                &mut api,
                directory.path(),
                RequestId::new("prepare").unwrap(),
                &i,
                case != 0
            )
            .is_err());
            assert!(api.calls.is_empty());
        }
        let missing = tempfile::tempdir().unwrap();
        let mut api = Api::reply(report());
        assert!(prepare(
            &mut api,
            missing.path(),
            RequestId::new("prepare").unwrap(),
            &intent(),
            true
        )
        .is_err());
        assert!(api.calls.is_empty());
    }
    #[test]
    fn hsp_preparation_client_rejects_changed_subject_selection_and_authority_flags() {
        let directory = directory();
        for case in 0..8 {
            let mut value = report();
            match case {
                0 => value.project_id = ProjectId::new("other").unwrap(),
                1 => value.preparation_request_id = RequestId::new("other").unwrap(),
                2 => {
                    value.history.as_mut().unwrap().engine_epoch =
                        DeviceSessionId::new("other").unwrap()
                }
                3 => {
                    value.history.as_mut().unwrap().device_session =
                        DeviceSessionId::new("other").unwrap()
                }
                4 => value.history.as_mut().unwrap().revision = RevisionId::new(3),
                5 => value.history.as_mut().unwrap().qualification.revision = 3,
                6 => value.armed = true,
                _ => value.playback_enabled = true,
            }
            assert!(
                prepare(
                    &mut Api::reply(value),
                    directory.path(),
                    RequestId::new("prepare").unwrap(),
                    &intent(),
                    true
                )
                .is_err(),
                "case {case}"
            );
        }
    }
    #[test]
    fn hsp_preparation_client_status_and_cancel_keep_original_outcome_without_new_deadline() {
        let directory = directory();
        for cancel_operation in [false, true] {
            let expected = report();
            let mut api = Api::reply(expected.clone());
            let actual = if cancel_operation {
                cancel(
                    &mut api,
                    directory.path(),
                    RequestId::new("cancel").unwrap(),
                    expected.project_id.clone(),
                    expected.preparation_request_id.clone(),
                )
            } else {
                status(
                    &mut api,
                    directory.path(),
                    expected.project_id.clone(),
                    expected.preparation_request_id.clone(),
                )
            }
            .unwrap();
            assert_eq!(actual, expected);
            let (id, command, project, revision) = &api.calls[0];
            assert_eq!(project.as_ref(), Some(&expected.project_id));
            assert!(revision.is_none());
            if cancel_operation {
                assert_eq!(id.as_ref().unwrap().as_str(), "cancel");
                assert!(
                    matches!(command,Command::CancelHandyHspPreparation {preparation_request_id,..} if preparation_request_id==&expected.preparation_request_id)
                );
            } else {
                assert!(id.is_none());
                assert!(!command.is_mutating());
            }
        }
        let original = RequestId::new("prepare").unwrap();
        let mut api = Api::reply(report());
        assert!(cancel(
            &mut api,
            directory.path(),
            original.clone(),
            intent().project,
            original
        )
        .is_err());
        assert!(api.calls.is_empty());
        let mut absent = Api::default();
        assert_eq!(
            status(
                &mut absent,
                directory.path(),
                intent().project,
                RequestId::new("prepare").unwrap()
            )
            .unwrap_err()
            .to_string(),
            "original preparation not found"
        );
        let mut reservation = report();
        reservation.history = None;
        reservation.native = None;
        assert_eq!(
            status(
                &mut Api::reply(reservation.clone()),
                directory.path(),
                intent().project,
                reservation.preparation_request_id.clone()
            )
            .unwrap(),
            reservation
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
            panic!("preparation hydration must not send a command");
        }
        fn open_bulk(
            &mut self,
            _: &TransferLease,
            _: std::time::Instant,
        ) -> Result<Box<dyn crate::motion::BulkIo>> {
            panic!("preparation hydration must not open bulk I/O");
        }
        fn bulk_endpoint(&self) -> &Path {
            panic!("preparation hydration must not resolve bulk I/O");
        }
    }
    #[test]
    fn hsp_preparation_client_hydrates_only_matching_valid_project_without_io() {
        let expected = report();
        let mut cache = crate::motion::MotionCache::default();
        for project in [None, Some(ProjectId::new("other").unwrap())] {
            assert!(cache
                .hydrate(
                    pulsar_protocol::ResponseBody::HandyHspPreparation(Box::new(expected.clone())),
                    project.as_ref(),
                    None,
                    &mut NoIo
                )
                .is_err());
        }
        let ResponseBody::HandyHspPreparation(actual) = cache
            .hydrate(
                pulsar_protocol::ResponseBody::HandyHspPreparation(Box::new(expected.clone())),
                Some(&expected.project_id),
                None,
                &mut NoIo,
            )
            .unwrap()
        else {
            panic!();
        };
        assert_eq!(*actual, expected);
        let mut invalid = expected.clone();
        invalid.armed = true;
        assert!(cache
            .hydrate(
                pulsar_protocol::ResponseBody::HandyHspPreparation(Box::new(invalid)),
                Some(&expected.project_id),
                None,
                &mut NoIo
            )
            .is_err());
    }
}
