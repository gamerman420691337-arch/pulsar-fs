//! Shared explicit read-only setup inspection; a report is never admission.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use std::path::Path;
pub struct SetupInspectionIntent {
    pub qualification: Option<HandyQualificationBinding>,
    pub project: ProjectId,
    pub revision: RevisionId,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub hosting_request_id: RequestId,
}
pub fn inspect(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &SetupInspectionIntent,
    consent: bool,
) -> Result<HandySetupInspection> {
    ensure!(
        consent,
        "Handy setup inspection requires explicit online consent"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandySetupInspection(report) = client.execute_identified(
        id.clone(),
        Command::InspectHandySetup {
            qualification: intent.qualification.clone(),
            engine_epoch: intent.engine_epoch.clone(),
            device_session: intent.device_session.clone(),
            hosting_request_id: intent.hosting_request_id.clone(),
            allow_online_inspection: true,
            owner_proof: proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("Handy setup inspection response mismatch");
    };
    report.validate()?;
    ensure!(
        report.accepted_qualification == intent.qualification
            && report.request_id == id
            && report.project_id == intent.project
            && report.revision == intent.revision
            && report.engine_epoch == intent.engine_epoch
            && report.device_session == intent.device_session
            && report.hosting_request_id == intent.hosting_request_id,
        "Handy setup inspection changed captured selection"
    );
    Ok(*report)
}

pub fn setup(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &SetupInspectionIntent,
    consent: bool,
) -> Result<HandySetupStatus> {
    ensure!(
        consent,
        "Handy setup requires explicit online setup consent"
    );
    let qualification = intent
        .qualification
        .clone()
        .context("setup requires an accepted qualification selection")?;
    qualification.validate()?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandySetupStatus(value) = client.execute_identified(
        id.clone(),
        Command::SetupHandyDevice {
            qualification: qualification.clone(),
            engine_epoch: intent.engine_epoch.clone(),
            device_session: intent.device_session.clone(),
            hosting_request_id: intent.hosting_request_id.clone(),
            allow_online_setup: true,
            owner_proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("setup response mismatch")
    };
    value.validate()?;
    ensure!(
        value.project_id == intent.project && value.setup_request_id == id,
        "setup response changed original request"
    );
    let h = value
        .history
        .as_ref()
        .context("setup returned no effect history")?;
    ensure!(
        h.engine_epoch == intent.engine_epoch
            && h.device_session == intent.device_session
            && h.revision == intent.revision
            && h.hosting_request_id == intent.hosting_request_id
            && h.qualification == qualification,
        "setup response changed captured selection"
    );
    Ok(*value)
}

pub fn cancel(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    setup_request_id: RequestId,
    record_id: ArtifactId,
) -> Result<HandySetupCancellation> {
    ensure!(
        id != setup_request_id,
        "cancellation requires its own request identity"
    );
    validate_sha256(record_id.as_str())?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandySetupCancellation(receipt) = client.execute_identified(
        id.clone(),
        Command::CancelHandySetup {
            setup_request_id: setup_request_id.clone(),
            record_id: record_id.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("setup cancellation response mismatch")
    };
    receipt.validate()?;
    ensure!(
        receipt.request_id == id
            && receipt.status.project_id == project
            && receipt.status.setup_request_id == setup_request_id
            && receipt.status.history.as_ref().map(|h| &h.record_id) == Some(&record_id),
        "setup cancellation changed captured selection"
    );
    ensure!(
        receipt.settled,
        "setup cancellation is not yet settled; retain the original request"
    );
    Ok(*receipt)
}

pub fn status(
    client: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    setup_request_id: RequestId,
) -> Result<HandySetupStatus> {
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandySetupStatus(value) = client.execute(
        Command::HandySetupStatus {
            setup_request_id: setup_request_id.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("setup status response mismatch")
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.setup_request_id == setup_request_id,
        "setup status changed original selection"
    );
    Ok(*value)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Api {
        calls: usize,
        reply: Option<ResponseBody>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unexpected unbound inspection")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            c: Command,
            p: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                c,
                Command::InspectHandySetup {
                    allow_online_inspection: true,
                    ..
                }
            ));
            assert!(p.is_some() && r.is_some());
            self.calls += 1;
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("no response"))
        }
    }
    #[test]
    fn handy_setup_client_refuses_changed_selection_and_setup_claims() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let key = directory.path().join("handy-owner.token");
        std::fs::write(&key, "a".repeat(64)).unwrap();
        std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let intent = SetupInspectionIntent {
            qualification: None,
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(1),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("session").unwrap(),
            hosting_request_id: RequestId::new("hosting").unwrap(),
        };
        let id = RequestId::new("inspect").unwrap();
        let mut api = Api {
            calls: 0,
            reply: None,
        };
        assert!(inspect(&mut api, directory.path(), id.clone(), &intent, false).is_err());
        assert_eq!(api.calls, 0);
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("b".repeat(64)).unwrap(),
            sha256: "b".repeat(64),
            byte_len: 30,
        };
        let report = HandySetupInspection {
            accepted_qualification: None,
            request_id: id.clone(),
            engine_epoch: intent.engine_epoch.clone(),
            project_id: intent.project.clone(),
            revision: intent.revision,
            device_session: intent.device_session.clone(),
            registration: HandyRegistration {
                device_id: DeviceId::new("device").unwrap(),
                revision: 1,
                settings: HandyRegistrationSettings {
                    label: "fixture".into(),
                    api_key_env: "KEY_A".into(),
                    connection_key_env: "KEY_B".into(),
                },
                disabled: false,
                binding_sha256: "c".repeat(64),
            },
            hosting_request_id: intent.hosting_request_id.clone(),
            master_sha256: "d".repeat(64),
            script: artifact.clone(),
            hosted: artifact,
            device: HandySetupDeviceObservation {
                operative_settings: serde_json::from_str(include_str!(
                    "../../pulsar-protocol/tests/fixtures/handy-operative-settings.json"
                ))
                .unwrap(),
                identity: HandyIdentity {
                    firmware: "4.0.16".into(),
                    model_number: 2,
                    model_name: "H02".into(),
                    model_variant: 0,
                    session_id: "remote".into(),
                    firmware_status: 0,
                },
                mode: HandyDeviceMode {
                    mode: 7,
                    mode_session_id: 1,
                },
                remote_hssp: None,
                remote_hsp: None,
                observed_at_unix_ms: 1,
            },
            observation_span_ns: 10,
            setup_performed: false,
            qualified: false,
        };
        api.reply = Some(ResponseBody::HandySetupInspection(Box::new(report.clone())));
        inspect(&mut api, directory.path(), id.clone(), &intent, true).unwrap();
        let mut wrong = report.clone();
        wrong.hosting_request_id = RequestId::new("other").unwrap();
        api.reply = Some(ResponseBody::HandySetupInspection(Box::new(wrong)));
        assert!(inspect(&mut api, directory.path(), id.clone(), &intent, true).is_err());
        let mut invalid_settings = report.clone();
        invalid_settings.device.operative_settings.stroke.max = 2.0;
        api.reply = Some(ResponseBody::HandySetupInspection(Box::new(
            invalid_settings,
        )));
        assert!(inspect(&mut api, directory.path(), id.clone(), &intent, true).is_err());
        let mut wrong = report;
        wrong.setup_performed = true;
        api.reply = Some(ResponseBody::HandySetupInspection(Box::new(wrong)));
        assert!(inspect(&mut api, directory.path(), id, &intent, true).is_err());
    }
}

#[cfg(all(test, unix))]
mod status_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Api {
        reply: Option<ResponseBody>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            c: Command,
            p: Option<ProjectId>,
            rev: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(c, Command::HandySetupStatus { .. }));
            assert!(p.is_some());
            assert!(rev.is_none());
            Ok(self.reply.take().unwrap())
        }
    }
    #[test]
    fn handy_setup_status_client_checks_original_selection_and_rejects_authority_claims() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let key = dir.path().join("handy-owner.token");
        std::fs::write(&key, "a".repeat(64)).unwrap();
        std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let project = ProjectId::new("project").unwrap();
        let original = RequestId::new("original").unwrap();
        let value = HandySetupStatus {
            project_id: project.clone(),
            setup_request_id: original.clone(),
            history: None,
            loaded: false,
            qualified: false,
        };
        for case in 0..4 {
            let mut reply = value.clone();
            match case {
                1 => reply.setup_request_id = RequestId::new("other").unwrap(),
                2 => reply.project_id = ProjectId::new("other").unwrap(),
                3 => reply.loaded = true,
                _ => {}
            }
            let mut api = Api {
                reply: Some(ResponseBody::HandySetupStatus(Box::new(reply))),
            };
            assert_eq!(
                status(&mut api, dir.path(), project.clone(), original.clone()).is_ok(),
                case == 0
            );
        }
    }
}

#[cfg(all(test, unix))]
mod setup_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Api {
        calls: usize,
        changed: bool,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unbound setup")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            c: Command,
            p: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            let Command::SetupHandyDevice {
                mut qualification,
                engine_epoch,
                device_session,
                hosting_request_id,
                allow_online_setup,
                ..
            } = c
            else {
                anyhow::bail!("wrong command")
            };
            assert!(allow_online_setup);
            self.calls += 1;
            if self.changed {
                qualification.revision += 1;
            }
            let artifact = ArtifactIdentity {
                artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
                sha256: "a".repeat(64),
                byte_len: 40,
            };
            Ok(ResponseBody::HandySetupStatus(Box::new(HandySetupStatus {
                project_id: p.unwrap(),
                setup_request_id: id,
                history: Some(HandySetupHistory {
                    record_id: ArtifactId::new("b".repeat(64)).unwrap(),
                    engine_epoch,
                    device_session,
                    revision: r.unwrap(),
                    device_id: DeviceId::new("device").unwrap(),
                    registration_revision: 1,
                    qualification,
                    hosting_request_id,
                    master_sha256: "c".repeat(64),
                    script: artifact.clone(),
                    hosted: artifact,
                    state: HandySetupState::Claimed,
                    acknowledgment: None,
                }),
                loaded: false,
                qualified: false,
            })))
        }
    }
    #[test]
    fn handy_setup_client_requires_consent_and_preserves_original_qualification_selection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut intent = SetupInspectionIntent {
            qualification: None,
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(1),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("session").unwrap(),
            hosting_request_id: RequestId::new("hosting").unwrap(),
        };
        let id = RequestId::new("setup").unwrap();
        let mut api = Api {
            calls: 0,
            changed: false,
        };
        assert!(setup(&mut api, dir.path(), id.clone(), &intent, false).is_err());
        assert!(setup(&mut api, dir.path(), id.clone(), &intent, true).is_err());
        assert_eq!(api.calls, 0);
        let path = dir.path().join("handy-owner.token");
        std::fs::write(&path, "a".repeat(64)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        intent.qualification = Some(HandyQualificationBinding {
            dossier_id: ArtifactId::new("d".repeat(64)).unwrap(),
            revision: 2,
        });
        assert!(setup(&mut api, dir.path(), id.clone(), &intent, true).is_ok());
        api.changed = true;
        assert!(setup(&mut api, dir.path(), id, &intent, true).is_err());
    }
}

#[cfg(all(test, unix))]
mod cancellation_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Api {
        changed: u8,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unbound cancellation")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            c: Command,
            p: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(r.is_none());
            let Command::CancelHandySetup {
                setup_request_id,
                mut record_id,
                ..
            } = c
            else {
                anyhow::bail!("wrong command")
            };
            if self.changed == 1 {
                record_id = ArtifactId::new("f".repeat(64)).unwrap();
            }
            let a = ArtifactIdentity {
                artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
                sha256: "a".repeat(64),
                byte_len: 40,
            };
            let history = HandySetupHistory {
                record_id,
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                device_session: DeviceSessionId::new("session").unwrap(),
                revision: RevisionId::new(1),
                device_id: DeviceId::new("device").unwrap(),
                registration_revision: 1,
                qualification: HandyQualificationBinding {
                    dossier_id: ArtifactId::new("d".repeat(64)).unwrap(),
                    revision: 2,
                },
                hosting_request_id: RequestId::new("hosting").unwrap(),
                master_sha256: "b".repeat(64),
                script: a.clone(),
                hosted: a,
                state: HandySetupState::Cancelled,
                acknowledgment: None,
            };
            Ok(ResponseBody::HandySetupCancellation(Box::new(
                HandySetupCancellation {
                    request_id: id,
                    status: HandySetupStatus {
                        project_id: p.unwrap(),
                        setup_request_id,
                        history: Some(history),
                        loaded: false,
                        qualified: false,
                    },
                    settled: self.changed != 2,
                },
            )))
        }
    }
    #[test]
    fn handy_setup_cancel_client_requires_exact_selection_and_settled_acknowledgment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let key = dir.path().join("handy-owner.token");
        std::fs::write(&key, "a".repeat(64)).unwrap();
        std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600)).unwrap();
        for changed in 0..3 {
            let mut api = Api { changed };
            assert_eq!(
                cancel(
                    &mut api,
                    dir.path(),
                    RequestId::new("cancel").unwrap(),
                    ProjectId::new("project").unwrap(),
                    RequestId::new("setup").unwrap(),
                    ArtifactId::new("c".repeat(64)).unwrap()
                )
                .is_ok(),
                changed == 0
            );
        }
    }
}

pub struct ProgramInspectionIntent {
    pub setup: SetupInspectionIntent,
    pub setup_request_id: RequestId,
    pub record_id: ArtifactId,
}

#[cfg(all(test, unix))]
mod program_tests {
    use super::*;
    struct Api {
        reply: Option<HandyProgramCorrelation>,
        calls: usize,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unbound call")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            c: Command,
            p: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert_eq!(id.as_str(), "program-inspect");
            assert!(p.is_some() && r.is_some());
            assert!(matches!(
                c,
                Command::InspectHandyProgram {
                    allow_online_inspection: true,
                    ..
                }
            ));
            self.calls += 1;
            Ok(ResponseBody::HandyProgramCorrelation(Box::new(
                self.reply.take().unwrap(),
            )))
        }
    }
    #[test]
    fn handy_program_client_checks_complete_selection_and_refuses_without_consent() {
        use std::os::unix::fs::PermissionsExt;
        let good: HandyProgramCorrelation = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap();
        let i = &good.inspection;
        let selection = ProgramInspectionIntent {
            setup: SetupInspectionIntent {
                qualification: i.accepted_qualification.clone(),
                project: i.project_id.clone(),
                revision: i.revision,
                engine_epoch: i.engine_epoch.clone(),
                device_session: i.device_session.clone(),
                hosting_request_id: i.hosting_request_id.clone(),
            },
            setup_request_id: good.setup_request_id.clone(),
            record_id: good.record_id.clone(),
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.path().join("handy-owner.token");
        std::fs::write(&path, "a".repeat(64)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut api = Api {
            reply: None,
            calls: 0,
        };
        assert!(inspect_program(
            &mut api,
            dir.path(),
            i.request_id.clone(),
            &selection,
            false
        )
        .is_err());
        assert_eq!(api.calls, 0);
        for variant in 0..6 {
            let mut reply = good.clone();
            match variant {
                0 => {}
                1 => reply.record_id = ArtifactId::new("f".repeat(64)).unwrap(),
                2 => reply.inspection.engine_epoch = DeviceSessionId::new("other-boot").unwrap(),
                3 => reply.inspection.revision = RevisionId::new(2),
                4 => reply.playback_enabled = true,
                _ => {
                    reply
                        .inspection
                        .accepted_qualification
                        .as_mut()
                        .unwrap()
                        .revision += 1
                }
            };
            let mut api = Api {
                reply: Some(reply),
                calls: 0,
            };
            assert_eq!(
                inspect_program(&mut api, dir.path(), i.request_id.clone(), &selection, true)
                    .is_ok(),
                variant == 0
            );
            assert_eq!(api.calls, 1);
        }
    }
}
pub fn inspect_program(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &ProgramInspectionIntent,
    consent: bool,
) -> Result<HandyProgramCorrelation> {
    ensure!(
        consent,
        "Handy program inspection requires explicit online consent"
    );
    ensure!(
        id != intent.setup_request_id,
        "program inspection needs a separate request ID"
    );
    validate_sha256(intent.record_id.as_str())?;
    let qualification = intent
        .setup
        .qualification
        .clone()
        .context("program inspection requires accepted qualification")?;
    qualification.validate()?;
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyProgramCorrelation(report) = client.execute_identified(
        id.clone(),
        Command::InspectHandyProgram {
            setup_request_id: intent.setup_request_id.clone(),
            record_id: intent.record_id.clone(),
            qualification,
            engine_epoch: intent.setup.engine_epoch.clone(),
            device_session: intent.setup.device_session.clone(),
            hosting_request_id: intent.setup.hosting_request_id.clone(),
            allow_online_inspection: true,
            owner_proof: proof,
        },
        Some(intent.setup.project.clone()),
        Some(intent.setup.revision),
    )?
    else {
        anyhow::bail!("Handy program inspection response mismatch");
    };
    report.validate()?;
    let inspected = &report.inspection;
    ensure!(
        report.setup_request_id == intent.setup_request_id
            && report.record_id == intent.record_id
            && inspected.request_id == id
            && inspected.project_id == intent.setup.project
            && inspected.revision == intent.setup.revision
            && inspected.engine_epoch == intent.setup.engine_epoch
            && inspected.device_session == intent.setup.device_session
            && inspected.hosting_request_id == intent.setup.hosting_request_id
            && inspected.accepted_qualification == intent.setup.qualification,
        "Handy program inspection changed captured selection"
    );
    Ok(*report)
}

pub fn prepare_playback(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &ProgramInspectionIntent,
    consent: bool,
) -> Result<HandyPlaybackPreparation> {
    ensure!(
        consent,
        "Handy playback preparation requires explicit online inspection consent"
    );
    let program = HandyProgramSelection {
        setup_request_id: intent.setup_request_id.clone(),
        record_id: intent.record_id.clone(),
        qualification: intent
            .setup
            .qualification
            .clone()
            .context("accepted qualification required")?,
        engine_epoch: intent.setup.engine_epoch.clone(),
        device_session: intent.setup.device_session.clone(),
        hosting_request_id: intent.setup.hosting_request_id.clone(),
    };
    program.validate()?;
    ensure!(
        id != program.setup_request_id,
        "preparation requires a separate request ID"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyPlaybackPreparation(value) = client.execute_identified(
        id.clone(),
        Command::PrepareHandyPlayback {
            program: program.clone(),
            allow_online_inspection: true,
            owner_proof: proof,
        },
        Some(intent.setup.project.clone()),
        Some(intent.setup.revision),
    )?
    else {
        anyhow::bail!("preparation response differs");
    };
    value.validate()?;
    ensure!(
        value.request_id == id
            && value.selection()? == program
            && value.admission.project == intent.setup.project
            && value.admission.revision == intent.setup.revision,
        "preparation changed captured selection"
    );
    Ok(*value)
}
pub fn playback_status(
    client: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyPlaybackPreparation> {
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyPlaybackPreparation(value) = client.execute(
        Command::HandyPlaybackPreparationStatus {
            preparation_request_id: original.clone(),
            owner_proof: proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("preparation status response differs");
    };
    value.validate()?;
    ensure!(
        value.request_id == original && value.admission.project == project,
        "preparation status changed original selection"
    );
    Ok(*value)
}
pub fn retire_playback(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    original: RequestId,
    preparation_id: ArtifactId,
) -> Result<HandyPlaybackRetirement> {
    validate_sha256(preparation_id.as_str())?;
    ensure!(id != original, "retirement requires a separate request ID");
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyPlaybackRetirement(value) = client.execute_identified(
        id.clone(),
        Command::RetireHandyPlaybackPreparation {
            preparation_request_id: original.clone(),
            preparation_id: preparation_id.clone(),
            owner_proof: proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("preparation retirement response differs");
    };
    value.validate()?;
    ensure!(
        value.request_id == id
            && value.preparation.request_id == original
            && value.preparation.preparation_id == preparation_id
            && value.preparation.admission.project == project,
        "retirement changed captured selection"
    );
    Ok(*value)
}

#[cfg(all(test, unix))]
mod playback_preparation_tests {
    use super::*;
    struct Api {
        reply: Option<HandyPlaybackPreparation>,
        calls: usize,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unexpected unbound preparation")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            c: Command,
            _: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                c,
                Command::PrepareHandyPlayback {
                    allow_online_inspection: true,
                    ..
                }
            ));
            assert!(r.is_some());
            self.calls += 1;
            Ok(ResponseBody::HandyPlaybackPreparation(Box::new(
                self.reply.take().unwrap(),
            )))
        }
    }
    #[test]
    fn handy_preparation_client_rejects_changed_selection_and_accepts_retired_replay_without_rearming(
    ) {
        use std::os::unix::fs::PermissionsExt;
        let good: HandyPlaybackPreparation = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-playback-preparation.json"
        ))
        .unwrap();
        let i = &good.correlation.inspection;
        let intent = ProgramInspectionIntent {
            setup: SetupInspectionIntent {
                qualification: i.accepted_qualification.clone(),
                project: i.project_id.clone(),
                revision: i.revision,
                engine_epoch: i.engine_epoch.clone(),
                device_session: i.device_session.clone(),
                hosting_request_id: i.hosting_request_id.clone(),
            },
            setup_request_id: good.correlation.setup_request_id.clone(),
            record_id: good.correlation.record_id.clone(),
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let key = dir.path().join("handy-owner.token");
        std::fs::write(&key, "a".repeat(64)).unwrap();
        std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut api = Api {
            reply: None,
            calls: 0,
        };
        assert!(prepare_playback(
            &mut api,
            dir.path(),
            good.request_id.clone(),
            &intent,
            false
        )
        .is_err());
        assert_eq!(api.calls, 0);
        for variant in 0..4 {
            let mut reply = good.clone();
            match variant {
                0 => {}
                1 => reply.state = HandyPlaybackPreparationState::Retired,
                2 => reply.request_id = RequestId::new("wrong-original").unwrap(),
                _ => reply.armed = true,
            };
            let mut api = Api {
                reply: Some(reply),
                calls: 0,
            };
            assert_eq!(
                prepare_playback(&mut api, dir.path(), good.request_id.clone(), &intent, true)
                    .is_ok(),
                variant < 2
            );
            assert_eq!(api.calls, 1);
        }
    }
}

pub struct ControllerAcquireIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub preparation_request_id: RequestId,
    pub preparation_id: ArtifactId,
    pub duration_ms: u64,
}
pub struct ServerClockIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub generation: u64,
}
pub struct DeviceTimingIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub generation: u64,
}
#[cfg(all(test, unix))]
mod playback_clock_client_tests {
    use super::*;
    struct Api(Option<HandyPlaybackClockObservation>, usize);
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("missing clock request ID")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            c: Command,
            _: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                c,
                Command::AcquireHandyPlaybackClock {
                    allow_online_timing: true,
                    ..
                }
            ));
            assert!(r.is_some());
            self.1 += 1;
            Ok(ResponseBody::HandyPlaybackClock(Box::new(
                self.0.take().unwrap(),
            )))
        }
    }
    #[test]
    fn handy_playback_clock_client_checks_both_selections_and_requires_consent() {
        use std::os::unix::fs::PermissionsExt;
        let good: HandyPlaybackClockObservation = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-playback-clock.json"
        ))
        .unwrap();
        let l = &good.controller;
        let intent = DeviceTimingIntent {
            project: l.project_id.clone(),
            revision: l.revision,
            acquisition_request_id: l.acquisition_request_id.clone(),
            controller_id: l.controller_id.clone(),
            generation: l.generation,
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.path().join("handy-owner.token");
        std::fs::write(&path, "a".repeat(64)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut api = Api(None, 0);
        assert!(acquire_playback_clock(
            &mut api,
            dir.path(),
            good.request_id.clone(),
            &intent,
            &good.synchronization,
            false
        )
        .is_err());
        assert_eq!(api.1, 0);
        for case in 0..6 {
            let mut value = good.clone();
            match case {
                0 => {}
                1 => value.current = false,
                2 => value.controller.generation += 1,
                3 => value.synchronization.record_id = ArtifactId::new("d".repeat(64)).unwrap(),
                4 => value.request_id = RequestId::new("other").unwrap(),
                _ => value.playback_enabled = true,
            };
            assert_eq!(
                acquire_playback_clock(
                    &mut Api(Some(value), 0),
                    dir.path(),
                    good.request_id.clone(),
                    &intent,
                    &good.synchronization,
                    true
                )
                .is_ok(),
                case < 2
            );
        }
    }
}
pub fn acquire_playback_clock(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &DeviceTimingIntent,
    sync: &HandyClockSyncBinding,
    online: bool,
) -> Result<HandyPlaybackClockObservation> {
    ensure!(
        online,
        "playback clock acquisition requires explicit online timing consent"
    );
    sync.validate()?;
    ensure!(
        id != intent.acquisition_request_id
            && id != sync.sync_request_id
            && intent.generation <= i64::MAX as u64,
        "invalid playback clock request identity or generation"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyPlaybackClock(value) = api.execute_identified(
        id.clone(),
        Command::AcquireHandyPlaybackClock {
            acquisition_request_id: intent.acquisition_request_id.clone(),
            controller_id: intent.controller_id.clone(),
            expected_generation: intent.generation,
            synchronization: sync.clone(),
            allow_online_timing: true,
            owner_proof: proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("playback clock response differs");
    };
    value.validate()?;
    let l = &value.controller;
    ensure!(
        value.request_id == id
            && l.project_id == intent.project
            && l.revision == intent.revision
            && l.acquisition_request_id == intent.acquisition_request_id
            && l.controller_id == intent.controller_id
            && l.generation == intent.generation
            && value.synchronization == *sync,
        "playback clock selection differs"
    );
    Ok(*value)
}
#[cfg(all(test, unix))]
mod clock_sync_client_tests {
    use super::*;
    struct Api(Option<HandyClockSyncStatus>, usize);
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("missing sync ID")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            c: Command,
            _: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                c,
                Command::SynchronizeHandyClock {
                    allow_online_synchronization: true,
                    ..
                }
            ));
            assert!(r.is_some());
            self.1 += 1;
            Ok(ResponseBody::HandyClockSync(Box::new(
                self.0.take().unwrap(),
            )))
        }
    }
    #[test]
    fn handy_clock_sync_client_preserves_selection_and_refuses_without_effect_consent() {
        use std::os::unix::fs::PermissionsExt;
        let good: HandyClockSyncStatus = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-clock-sync.json"
        ))
        .unwrap();
        let l = &good.history.as_ref().unwrap().original_controller;
        let intent = DeviceTimingIntent {
            project: l.project_id.clone(),
            revision: l.revision,
            acquisition_request_id: l.acquisition_request_id.clone(),
            controller_id: l.controller_id.clone(),
            generation: l.generation,
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.path().join("handy-owner.token");
        std::fs::write(&path, "a".repeat(64)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut api = Api(None, 0);
        assert!(synchronize_clock(
            &mut api,
            dir.path(),
            good.sync_request_id.clone(),
            &intent,
            false
        )
        .is_err());
        assert_eq!(api.1, 0);
        for case in 0..6 {
            let mut value = good.clone();
            match case {
                0 => {}
                1 => value.history.as_mut().unwrap().state = HandyClockSyncState::Unknown,
                2 => {
                    value
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .generation += 1
                }
                3 => value.sync_request_id = RequestId::new("other").unwrap(),
                4 => value.playback_enabled = true,
                _ => {
                    value
                        .history
                        .as_mut()
                        .unwrap()
                        .original_controller
                        .controller_id = ClientId::new("other").unwrap()
                }
            };
            assert_eq!(
                synchronize_clock(
                    &mut Api(Some(value), 0),
                    dir.path(),
                    good.sync_request_id.clone(),
                    &intent,
                    true
                )
                .is_ok(),
                case < 2
            );
        }
    }
}
pub fn synchronize_clock(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &DeviceTimingIntent,
    online: bool,
) -> Result<HandyClockSyncStatus> {
    ensure!(
        online,
        "clock synchronization requires explicit effect consent"
    );
    ensure!(
        id != intent.acquisition_request_id && intent.generation <= i64::MAX as u64,
        "invalid synchronization request identity or generation"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyClockSync(value) = api.execute_identified(
        id.clone(),
        Command::SynchronizeHandyClock {
            acquisition_request_id: intent.acquisition_request_id.clone(),
            controller_id: intent.controller_id.clone(),
            expected_generation: intent.generation,
            allow_online_synchronization: true,
            owner_proof: proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("synchronization response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == intent.project && value.sync_request_id == id,
        "synchronization receipt subject differs"
    );
    if let Some(h) = &value.history {
        let l = &h.original_controller;
        ensure!(
            l.acquisition_request_id == intent.acquisition_request_id
                && l.controller_id == intent.controller_id
                && l.generation == intent.generation
                && l.revision == intent.revision,
            "synchronization controller selection differs"
        );
    }
    Ok(*value)
}
pub fn clock_sync_status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyClockSyncStatus> {
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyClockSync(value) = api.execute(
        Command::HandyClockSyncStatus {
            sync_request_id: original.clone(),
            owner_proof: proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("synchronization status response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.sync_request_id == original,
        "synchronization status subject differs"
    );
    Ok(*value)
}
pub fn cancel_clock_sync(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    original: RequestId,
    record: ArtifactId,
) -> Result<HandyClockSyncStatus> {
    validate_sha256(record.as_str())?;
    ensure!(
        id != original,
        "cancellation requires its own request identity"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyClockSync(value) = api.execute_identified(
        id,
        Command::CancelHandyClockSync {
            sync_request_id: original.clone(),
            record_id: record.clone(),
            owner_proof: proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("synchronization cancellation response differs");
    };
    value.validate()?;
    ensure!(value.project_id==project && value.sync_request_id==original && value.history.as_ref().is_some_and(|h|h.record_id==record && h.state==HandyClockSyncState::Cancelled),"synchronization cancellation is not the selected settled record");
    Ok(*value)
}

#[cfg(all(test, unix))]
mod device_timing_client_tests {
    use super::*;
    struct Api(Option<HandyDeviceTimingInspection>, usize);
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("missing inspection ID")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            c: Command,
            _: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                c,
                Command::InspectHandyDeviceTiming {
                    allow_online_inspection: true,
                    ..
                }
            ));
            assert!(r.is_some());
            self.1 += 1;
            Ok(ResponseBody::HandyDeviceTiming(Box::new(
                self.0.take().unwrap(),
            )))
        }
    }
    #[test]
    fn handy_device_timing_client_requires_consent_and_exact_controller_selection() {
        use std::os::unix::fs::PermissionsExt;
        let good: HandyDeviceTimingInspection = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-device-timing.json"
        ))
        .unwrap();
        let l = &good.controller;
        let intent = DeviceTimingIntent {
            project: l.project_id.clone(),
            revision: l.revision,
            acquisition_request_id: l.acquisition_request_id.clone(),
            controller_id: l.controller_id.clone(),
            generation: l.generation,
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = dir.path().join("handy-owner.token");
        std::fs::write(&token, "a".repeat(64)).unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut api = Api(None, 0);
        assert!(inspect_device_timing(
            &mut api,
            dir.path(),
            good.request_id.clone(),
            &intent,
            false
        )
        .is_err());
        assert_eq!(api.1, 0);
        for case in 0..8 {
            let mut report = good.clone();
            match case {
                0 => {}
                1 => report.controller.generation += 1,
                2 => report.controller.project_id = ProjectId::new("other").unwrap(),
                3 => report.controller.controller_id = ClientId::new("other").unwrap(),
                4 => report.controller.acquisition_request_id = RequestId::new("other").unwrap(),
                5 => report.request_id = RequestId::new("other").unwrap(),
                6 => report.synchronization_verified = true,
                _ => report.playback_enabled = true,
            };
            assert_eq!(
                inspect_device_timing(
                    &mut Api(Some(report), 0),
                    dir.path(),
                    good.request_id.clone(),
                    &intent,
                    true
                )
                .is_ok(),
                case == 0
            );
        }
    }
}
pub fn inspect_device_timing(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &DeviceTimingIntent,
    online: bool,
) -> Result<HandyDeviceTimingInspection> {
    ensure!(
        online,
        "device timing inspection requires explicit online consent"
    );
    ensure!(
        intent.generation <= i64::MAX as u64,
        "invalid controller generation"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyDeviceTiming(value) = api.execute_identified(
        id.clone(),
        Command::InspectHandyDeviceTiming {
            acquisition_request_id: intent.acquisition_request_id.clone(),
            controller_id: intent.controller_id.clone(),
            expected_generation: intent.generation,
            allow_online_inspection: true,
            owner_proof: proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("device timing response differs");
    };
    value.validate()?;
    let l = &value.controller;
    ensure!(
        value.request_id == id
            && l.project_id == intent.project
            && l.revision == intent.revision
            && l.acquisition_request_id == intent.acquisition_request_id
            && l.controller_id == intent.controller_id
            && l.generation == intent.generation,
        "device timing response selection differs"
    );
    Ok(*value)
}
pub fn sample_server_clock(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &ServerClockIntent,
    online: bool,
) -> Result<HandyServerClockObservation> {
    ensure!(
        online,
        "server-clock acquisition needs explicit online sampling consent"
    );
    ensure!(
        id != intent.acquisition_request_id && intent.generation <= i64::MAX as u64,
        "invalid sampling request identity or generation"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyServerClock(value) = api.execute_identified(
        id.clone(),
        Command::SampleHandyServerClock {
            acquisition_request_id: intent.acquisition_request_id.clone(),
            controller_id: intent.controller_id.clone(),
            expected_generation: intent.generation,
            allow_online_sampling: true,
            owner_proof: proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("server-clock response differs");
    };
    value.validate()?;
    let l = &value.controller;
    ensure!(
        value.request_id == id
            && l.project_id == intent.project
            && l.revision == intent.revision
            && l.acquisition_request_id == intent.acquisition_request_id
            && l.controller_id == intent.controller_id
            && l.generation == intent.generation,
        "server-clock response selection differs"
    );
    Ok(*value)
}

#[cfg(all(test, unix))]
mod server_clock_client_tests {
    use super::*;
    struct Api(Option<HandyServerClockObservation>, usize);
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unidentified sample")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            c: Command,
            _: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                c,
                Command::SampleHandyServerClock {
                    allow_online_sampling: true,
                    ..
                }
            ));
            assert!(r.is_some());
            self.1 += 1;
            Ok(ResponseBody::HandyServerClock(Box::new(
                self.0.take().unwrap(),
            )))
        }
    }
    #[test]
    fn handy_server_clock_client_checks_exact_selection_and_consent() {
        use std::os::unix::fs::PermissionsExt;
        let lease: HandyControllerReceipt = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-controller.json"
        ))
        .unwrap();
        let good = HandyServerClockObservation {
            request_id: RequestId::new("sample").unwrap(),
            controller: lease.lease,
            qualification: HandyQualificationBinding {
                dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                revision: 2,
            },
            clock_domain: ArtifactId::new("b".repeat(64)).unwrap(),
            sent_boottime_ns: 1,
            received_boottime_ns: 2,
            estimated_server_ms: 1000,
            uncertainty_ns: 10,
            current: true,
            playback_enabled: false,
        };
        let l = &good.controller;
        let intent = ServerClockIntent {
            project: l.project_id.clone(),
            revision: l.revision,
            acquisition_request_id: l.acquisition_request_id.clone(),
            controller_id: l.controller_id.clone(),
            generation: l.generation,
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = dir.path().join("handy-owner.token");
        std::fs::write(&token, "c".repeat(64)).unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut api = Api(None, 0);
        assert!(sample_server_clock(
            &mut api,
            dir.path(),
            good.request_id.clone(),
            &intent,
            false
        )
        .is_err());
        assert_eq!(api.1, 0);
        for case in 0..8 {
            let mut reply = good.clone();
            match case {
                0 => {}
                1 => reply.current = false,
                2 => reply.controller.generation += 1,
                3 => reply.request_id = RequestId::new("other").unwrap(),
                4 => reply.controller.project_id = ProjectId::new("other").unwrap(),
                5 => reply.controller.controller_id = ClientId::new("other").unwrap(),
                6 => reply.controller.acquisition_request_id = RequestId::new("other").unwrap(),
                _ => reply.playback_enabled = true,
            }
            let mut api = Api(Some(reply), 0);
            assert_eq!(
                sample_server_clock(&mut api, dir.path(), good.request_id.clone(), &intent, true)
                    .is_ok(),
                case < 2
            );
        }
    }
}
pub fn controller_acquire(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &ControllerAcquireIntent,
    arm: bool,
    online: bool,
) -> Result<HandyControllerReceipt> {
    ensure!(
        arm && online,
        "controller acquisition needs explicit local arming and online inspection consent"
    );
    validate_handy_controller_duration(intent.duration_ms)?;
    validate_sha256(intent.preparation_id.as_str())?;
    ensure!(
        id != intent.preparation_request_id,
        "acquisition needs its own request ID"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyController(value) = api.execute_identified(
        id.clone(),
        Command::AcquireHandyController {
            preparation_request_id: intent.preparation_request_id.clone(),
            preparation_id: intent.preparation_id.clone(),
            duration_ms: intent.duration_ms,
            allow_arm: true,
            allow_online_inspection: true,
            owner_proof: proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("controller response differs");
    };
    value.validate()?;
    let l = &value.lease;
    ensure!(
        value.request_id == id
            && l.acquisition_request_id == id
            && l.preparation_request_id == intent.preparation_request_id
            && l.preparation_id == intent.preparation_id
            && l.project_id == intent.project
            && l.revision == intent.revision
            && l.duration_ms == intent.duration_ms,
        "controller response changed acquisition selection"
    );
    Ok(*value)
}
pub struct ControllerChangeIntent {
    pub project: ProjectId,
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub renewal: Option<(RevisionId, u64)>,
}
pub fn controller_change(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &ControllerChangeIntent,
) -> Result<HandyControllerReceipt> {
    ensure!(
        id != intent.acquisition_request_id,
        "controller change needs its own request ID"
    );
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let (command, revision) = if let Some((revision, generation)) = intent.renewal {
        (
            Command::RenewHandyController {
                acquisition_request_id: intent.acquisition_request_id.clone(),
                controller_id: intent.controller_id.clone(),
                expected_generation: generation,
                owner_proof: proof,
            },
            Some(revision),
        )
    } else {
        (
            Command::ReleaseHandyController {
                acquisition_request_id: intent.acquisition_request_id.clone(),
                controller_id: intent.controller_id.clone(),
                owner_proof: proof,
            },
            None,
        )
    };
    let ResponseBody::HandyController(value) =
        api.execute_identified(id.clone(), command, Some(intent.project.clone()), revision)?
    else {
        anyhow::bail!("controller change response differs");
    };
    value.validate()?;
    let l = &value.lease;
    ensure!(
        value.request_id == id
            && l.acquisition_request_id == intent.acquisition_request_id
            && l.controller_id == intent.controller_id
            && l.project_id == intent.project,
        "controller change selection differs"
    );
    if let Some((revision, generation)) = intent.renewal {
        ensure!(
            l.revision == revision
                && (l.state == HandyControllerState::Retired || l.generation > generation),
            "controller renewal generation differs"
        );
    } else {
        ensure!(
            l.state == HandyControllerState::Retired,
            "controller release was not retired"
        );
    }
    Ok(*value)
}
pub fn controller_status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyControllerReceipt> {
    let proof = crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyController(value) = api.execute(
        Command::HandyControllerStatus {
            acquisition_request_id: original.clone(),
            owner_proof: proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("controller status response differs");
    };
    value.validate()?;
    ensure!(
        value.lease.project_id == project && value.lease.acquisition_request_id == original,
        "controller status changed original selection"
    );
    Ok(*value)
}

#[cfg(all(test, unix))]
mod controller_tests {
    use super::*;
    struct Api {
        reply: Option<HandyControllerReceipt>,
        calls: usize,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unbound acquisition")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            c: Command,
            _: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(matches!(
                c,
                Command::AcquireHandyController {
                    allow_arm: true,
                    allow_online_inspection: true,
                    ..
                }
            ));
            assert!(r.is_some());
            self.calls += 1;
            Ok(ResponseBody::HandyController(Box::new(
                self.reply.take().unwrap(),
            )))
        }
    }
    #[test]
    fn handy_controller_client_checks_selection_and_requires_both_consents() {
        use std::os::unix::fs::PermissionsExt;
        let good: HandyControllerReceipt = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-controller.json"
        ))
        .unwrap();
        let l = &good.lease;
        let intent = ControllerAcquireIntent {
            project: l.project_id.clone(),
            revision: l.revision,
            preparation_request_id: l.preparation_request_id.clone(),
            preparation_id: l.preparation_id.clone(),
            duration_ms: l.duration_ms,
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let key = dir.path().join("handy-owner.token");
        std::fs::write(&key, "a".repeat(64)).unwrap();
        std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600)).unwrap();
        for (arm, online) in [(false, true), (true, false)] {
            let mut api = Api {
                reply: None,
                calls: 0,
            };
            assert!(controller_acquire(
                &mut api,
                dir.path(),
                good.request_id.clone(),
                &intent,
                arm,
                online
            )
            .is_err());
            assert_eq!(api.calls, 0);
        }
        for case in 0..4 {
            let mut reply = good.clone();
            match case {
                0 => {}
                1 => {
                    reply.lease.state = HandyControllerState::Retired;
                    reply.lease.armed = false;
                }
                2 => reply.lease.duration_ms = 2000,
                _ => reply.lease.playback_enabled = true,
            };
            let mut api = Api {
                reply: Some(reply),
                calls: 0,
            };
            assert_eq!(
                controller_acquire(
                    &mut api,
                    dir.path(),
                    good.request_id.clone(),
                    &intent,
                    true,
                    true
                )
                .is_ok(),
                case < 2
            );
        }
    }
}
