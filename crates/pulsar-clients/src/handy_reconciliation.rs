//! Original reconciliation selections and history. Replies never restore native authority.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;

pub fn plan(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    selection: &HandyRecoverySelection,
) -> Result<HandyRecoveryPlan> {
    selection.validate()?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyRecoveryPlan(value) = api.execute(
        Command::PlanHandyReconciliation {
            selection: selection.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("reconciliation plan response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.selection == *selection,
        "reconciliation plan changed captured selection"
    );
    Ok(*value)
}

pub fn start(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    plan_id: ArtifactId,
    allow_online_inspection: bool,
    allow_resolution: bool,
) -> Result<HandyRecoveryStatus> {
    ensure!(
        allow_online_inspection && allow_resolution,
        "Handy reconciliation requires explicit online inspection and resolution consent"
    );
    validate_sha256(plan_id.as_str())?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyRecovery(value) = api.execute_identified(
        id.clone(),
        Command::ReconcileHandyEffects {
            plan_id: plan_id.clone(),
            allow_online_inspection: true,
            allow_resolution: true,
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("reconciliation response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project
            && value.reconciliation_request_id == id
            && value.plan.plan_id == plan_id,
        "reconciliation changed original request or plan"
    );
    Ok(*value)
}

pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyRecoveryStatus> {
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyRecovery(value) = api.execute(
        Command::HandyReconciliationStatus {
            reconciliation_request_id: original.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("reconciliation status response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.reconciliation_request_id == original,
        "reconciliation status changed original subject"
    );
    Ok(*value)
}

/// Preserve the original outcome, including Completed when resolution won the race.
pub fn cancel(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyRecoveryStatus> {
    ensure!(
        id != original,
        "reconciliation cancellation requires its own request identity"
    );
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyRecovery(value) = api.execute_identified(
        id,
        Command::CancelHandyReconciliation {
            reconciliation_request_id: original.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("reconciliation cancellation response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.reconciliation_request_id == original,
        "reconciliation cancellation changed original subject"
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
        fn new(reply: ResponseBody) -> Self {
            Self {
                calls: Vec::new(),
                reply: Some(reply),
            }
        }
        fn respond(&mut self, call: Call) -> Result<ResponseBody> {
            self.calls.push(call);
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("missing mock response"))
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
    fn directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = directory.path().join("handy-owner.token");
        // Synthetic test proof only, never a real capability or credential.
        std::fs::write(&token, "a".repeat(64)).unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        directory
    }
    fn fixture() -> HandyRecoveryStatus {
        let correlation: HandyProgramCorrelation = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap();
        let registration = correlation.inspection.registration;
        let observation = correlation.inspection.device;
        let mut plan = HandyRecoveryPlan {
            plan_id: ArtifactId::new("a".repeat(64)).unwrap(),
            snapshot_id: ArtifactId::new("b".repeat(64)).unwrap(),
            project_id: ProjectId::new("requested-project").unwrap(),
            selection: HandyRecoverySelection {
                device_id: registration.device_id.clone(),
                registration_revision: registration.revision,
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                device_session: DeviceSessionId::new("session").unwrap(),
                qualification: HandyQualificationBinding {
                    dossier_id: ArtifactId::new("c".repeat(64)).unwrap(),
                    revision: 2,
                },
            },
            devices: vec![registration.device_id.clone()],
            registration,
            route_sha256: "d".repeat(64),
            route_namespace_sha256: "e".repeat(64),
            counts: HandyCustodyCounts {
                hsp_streams: 0,
                hsp_preparations: 0,
                setup: 1,
                clock_sync: 1,
                play: 1,
                effects: 1,
            },
            projects: vec![ProjectId::new("historical-project").unwrap()],
            contract: HandyRecoveryQualification {
                transport: HandyQualificationTransport::Hssp,
                model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
                remote_state: HandyRecoveryRemoteState::Stopped,
                prior_command_settle_ns: 1_000_000_000,
                stopped_sample_separation_ns: 1_000_000_000,
                maximum_sample_span_ns: 1_000_000_000,
            },
            playback_enabled: false,
        };
        plan.plan_id = plan.identity().unwrap();
        let resolution = HandyRecoveryResolution {
            record_id: ArtifactId::new("f".repeat(64)).unwrap(),
            basis_snapshot_id: plan.snapshot_id.clone(),
            snapshot_id: ArtifactId::new("1".repeat(64)).unwrap(),
            counts: plan.counts.clone(),
            evidence: HandyRecoveryEvidence {
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
        };
        let value = HandyRecoveryStatus {
            project_id: plan.project_id.clone(),
            reconciliation_request_id: RequestId::new("reconcile").unwrap(),
            plan,
            state: HandyRecoveryState::Completed,
            resolution: Some(resolution),
            error: None,
            playback_enabled: false,
            authority_restored: false,
        };
        value.validate().unwrap();
        value
    }
    fn recovery(value: HandyRecoveryStatus) -> Api {
        Api::new(ResponseBody::HandyRecovery(Box::new(value)))
    }
    fn same(left: &HandyRecoveryStatus, right: &HandyRecoveryStatus) {
        assert_eq!(
            serde_json::to_value(left).unwrap(),
            serde_json::to_value(right).unwrap()
        );
    }

    #[test]
    fn handy_reconciliation_consent_and_selection_fail_before_owner_proof_access() {
        let directory = tempfile::tempdir().unwrap();
        let value = fixture();
        let mut api = Api::default();
        for (online, resolution) in [(false, false), (true, false), (false, true)] {
            let error = start(
                &mut api,
                directory.path(),
                value.reconciliation_request_id.clone(),
                value.project_id.clone(),
                value.plan.plan_id.clone(),
                online,
                resolution,
            )
            .unwrap_err();
            assert!(error
                .to_string()
                .contains("requires explicit online inspection and resolution consent"));
        }
        assert!(start(
            &mut api,
            directory.path(),
            value.reconciliation_request_id.clone(),
            value.project_id.clone(),
            ArtifactId::new("not-a-digest").unwrap(),
            true,
            true
        )
        .is_err());
        let mut invalid = value.plan.selection.clone();
        invalid.registration_revision = 0;
        let error = plan(
            &mut api,
            directory.path(),
            value.project_id.clone(),
            &invalid,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("invalid Handy recovery selection"));
        let error = cancel(
            &mut api,
            directory.path(),
            value.reconciliation_request_id.clone(),
            value.project_id,
            value.reconciliation_request_id,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("requires its own request identity"));
        assert!(api.calls.is_empty());
    }

    #[test]
    fn handy_reconciliation_plan_reads_and_validates_exact_captured_selection() {
        let directory = directory();
        let original = fixture().plan;
        for case in 0..4 {
            let mut value = original.clone();
            match case {
                0 => {}
                1 => value.project_id = ProjectId::new("other-project").unwrap(),
                2 => {
                    value.selection.device_session = DeviceSessionId::new("other-session").unwrap()
                }
                _ => value.playback_enabled = true,
            }
            value.plan_id = value.identity().unwrap();
            let mut api = Api::new(ResponseBody::HandyRecoveryPlan(Box::new(value)));
            let result = plan(
                &mut api,
                directory.path(),
                original.project_id.clone(),
                &original.selection,
            );
            assert_eq!(result.is_ok(), case == 0);
            let (id, command, project, revision) = &api.calls[0];
            assert!(id.is_none() && revision.is_none());
            assert_eq!(project.as_ref(), Some(&original.project_id));
            assert!(
                matches!(command, Command::PlanHandyReconciliation { selection, owner_proof }
                if selection == &original.selection && owner_proof.len() == 64)
            );
            assert_eq!(api.calls.len(), 1);
        }
    }

    #[test]
    fn handy_reconciliation_history_and_completed_cancellation_race_are_preserved_without_retry() {
        let directory = directory();
        let original = fixture();
        for state in [
            HandyRecoveryState::Pending,
            HandyRecoveryState::Completed,
            HandyRecoveryState::Failed,
            HandyRecoveryState::Cancelled,
            HandyRecoveryState::Interrupted,
        ] {
            let mut expected = original.clone();
            expected.state = state;
            if state != HandyRecoveryState::Completed {
                expected.resolution = None;
            }
            if matches!(
                state,
                HandyRecoveryState::Failed | HandyRecoveryState::Interrupted
            ) {
                expected.error = Some(ProtocolError::invalid("synthetic historical failure"));
            }
            let mut api = recovery(expected.clone());
            let actual = start(
                &mut api,
                directory.path(),
                original.reconciliation_request_id.clone(),
                original.project_id.clone(),
                original.plan.plan_id.clone(),
                true,
                true,
            )
            .unwrap();
            same(&actual, &expected);
            assert_eq!(api.calls.len(), 1);
            let (id, command, project, revision) = &api.calls[0];
            assert_eq!(id.as_ref(), Some(&original.reconciliation_request_id));
            assert_eq!(project.as_ref(), Some(&original.project_id));
            assert!(revision.is_none());
            assert!(
                matches!(command, Command::ReconcileHandyEffects { plan_id, allow_online_inspection: true, allow_resolution: true, .. }
                if plan_id == &original.plan.plan_id)
            );

            let mut api = recovery(expected.clone());
            same(
                &status(
                    &mut api,
                    directory.path(),
                    original.project_id.clone(),
                    original.reconciliation_request_id.clone(),
                )
                .unwrap(),
                &expected,
            );
            assert_eq!(api.calls.len(), 1);
            let (id, command, project, revision) = &api.calls[0];
            assert!(id.is_none() && revision.is_none());
            assert_eq!(project.as_ref(), Some(&original.project_id));
            assert!(
                matches!(command, Command::HandyReconciliationStatus { reconciliation_request_id, .. }
                if reconciliation_request_id == &original.reconciliation_request_id)
            );

            let mut api = recovery(expected.clone());
            let cancellation = RequestId::new("cancel").unwrap();
            same(
                &cancel(
                    &mut api,
                    directory.path(),
                    cancellation.clone(),
                    original.project_id.clone(),
                    original.reconciliation_request_id.clone(),
                )
                .unwrap(),
                &expected,
            );
            assert_eq!(api.calls.len(), 1);
            let (id, command, project, revision) = &api.calls[0];
            assert_eq!(id.as_ref(), Some(&cancellation));
            assert!(revision.is_none());
            assert_eq!(project.as_ref(), Some(&original.project_id));
            assert!(
                matches!(command, Command::CancelHandyReconciliation { reconciliation_request_id, .. }
                if reconciliation_request_id == &original.reconciliation_request_id)
            );
        }
    }

    #[test]
    fn handy_reconciliation_clients_reject_changed_subject_plan_and_authority_claims() {
        let directory = directory();
        let original = fixture();
        for case in 0..4 {
            let mut value = original.clone();
            match case {
                0 => {
                    value.project_id = ProjectId::new("other-project").unwrap();
                    value.plan.project_id = value.project_id.clone();
                    value.plan.plan_id = value.plan.identity().unwrap();
                }
                1 => value.reconciliation_request_id = RequestId::new("other-request").unwrap(),
                2 => value.authority_restored = true,
                _ => value.resolution.as_mut().unwrap().counts.effects = 0,
            }
            assert!(start(
                &mut recovery(value.clone()),
                directory.path(),
                original.reconciliation_request_id.clone(),
                original.project_id.clone(),
                original.plan.plan_id.clone(),
                true,
                true
            )
            .is_err());
            assert!(status(
                &mut recovery(value.clone()),
                directory.path(),
                original.project_id.clone(),
                original.reconciliation_request_id.clone()
            )
            .is_err());
            assert!(cancel(
                &mut recovery(value),
                directory.path(),
                RequestId::new("cancel").unwrap(),
                original.project_id.clone(),
                original.reconciliation_request_id.clone()
            )
            .is_err());
        }
        let mut different_plan = original.clone();
        different_plan.plan.route_sha256 = "9".repeat(64);
        different_plan.plan.plan_id = different_plan.plan.identity().unwrap();
        different_plan.validate().unwrap();
        assert!(start(
            &mut recovery(different_plan),
            directory.path(),
            original.reconciliation_request_id,
            original.project_id,
            original.plan.plan_id,
            true,
            true
        )
        .is_err());
    }

    #[test]
    fn handy_reconciliation_clients_reject_wrong_response_variants() {
        let directory = directory();
        let original = fixture();
        assert!(plan(
            &mut Api::new(ResponseBody::Ack),
            directory.path(),
            original.project_id.clone(),
            &original.plan.selection
        )
        .is_err());
        assert!(start(
            &mut Api::new(ResponseBody::Ack),
            directory.path(),
            original.reconciliation_request_id.clone(),
            original.project_id.clone(),
            original.plan.plan_id,
            true,
            true
        )
        .is_err());
        assert!(status(
            &mut Api::new(ResponseBody::Ack),
            directory.path(),
            original.project_id.clone(),
            original.reconciliation_request_id.clone()
        )
        .is_err());
        assert!(cancel(
            &mut Api::new(ResponseBody::Ack),
            directory.path(),
            RequestId::new("cancel").unwrap(),
            original.project_id,
            original.reconciliation_request_id
        )
        .is_err());
    }
}
