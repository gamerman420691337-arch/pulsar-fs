//! Identified live HSP intents and read-only original status. The client never
//! manufactures stream readiness, source observations or physical qualification.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;

pub struct LiveIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub selection: HandyHspLiveSelection,
}
pub struct ContinuityIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub stream_request_id: RequestId,
    pub duration_ms: u32,
}
impl LiveIntent {
    pub fn validate(&self) -> Result<()> {
        self.selection.validate()?;
        Ok(())
    }
}
fn proof(directory: &Path) -> Result<String> {
    crate::project_package::read_local_owner_proof(directory, "handy-owner.token")
}
fn checked(
    body: ResponseBody,
    project: &ProjectId,
    original: &RequestId,
) -> Result<HandyHspLiveStatus> {
    let ResponseBody::HandyHspLive(value) = body else {
        anyhow::bail!("HSP live response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == *project && value.stream_request_id == *original,
        "HSP live response changed original subject"
    );
    Ok(*value)
}
fn checked_revision(value: &HandyHspLiveStatus, revision: RevisionId) -> Result<()> {
    ensure!(
        value
            .source_subject
            .as_ref()
            .is_none_or(|subject| subject.revision == revision)
            && value
                .stream
                .as_ref()
                .is_none_or(|stream| stream.live.revision == revision),
        "HSP live response changed selected project revision"
    );
    Ok(())
}

/// Retry only with the same request identity and complete original selection.
/// Environment acknowledgment names the exact selected source dossier's
/// browser environment. It is an operator assertion, not machine attestation,
/// and is independent of both preparation and online control consent.
pub fn bind(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &LiveIntent,
    acknowledge_current_environment: bool,
    allow_online_control: bool,
) -> Result<HandyHspLiveStatus> {
    ensure!(
        acknowledge_current_environment,
        "HSP live binding requires explicit current environment acknowledgment"
    );
    ensure!(
        allow_online_control,
        "HSP live binding requires explicit online control consent"
    );
    intent.validate()?;
    ensure!(
        id != intent.selection.preparation_request_id,
        "HSP live binding requires its own request identity"
    );
    let value = checked(
        api.execute_identified(
            id.clone(),
            Command::BindHandyHspLive {
                selection: intent.selection.clone(),
                acknowledge_current_environment,
                allow_online_control,
                owner_proof: proof(directory)?,
            },
            Some(intent.project.clone()),
            Some(intent.revision),
        )?,
        &intent.project,
        &id,
    )?;
    ensure!(
        value.selection == intent.selection,
        "HSP live response changed original source selection"
    );
    checked_revision(&value, intent.revision)?;
    Ok(value)
}

/// One explicit Play intent; the caller does not supply replacement timestamps,
/// samples, qualification or native authority.
pub fn play(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    revision: RevisionId,
    stream_request_id: RequestId,
) -> Result<HandyHspLiveStatus> {
    ensure!(
        id != stream_request_id,
        "HSP live Play requires its own request identity"
    );
    let value = checked(
        api.execute_identified(
            id.clone(),
            Command::PlayHandyHspLive {
                stream_request_id: stream_request_id.clone(),
                owner_proof: proof(directory)?,
            },
            Some(project.clone()),
            Some(revision),
        )?,
        &project,
        &stream_request_id,
    )?;
    checked_revision(&value, revision)?;
    ensure!(
        value
            .native
            .as_ref()
            .is_none_or(|native| { native.original_play_request_id.as_ref() == Some(&id) }),
        "HSP live Play response changed original intent"
    );
    Ok(value)
}

/// Stop is revision-free so source/project retirement cannot make the caller
/// invent a newer source selection. Acknowledgment never proves physical stop.
pub fn stop(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    stream_request_id: RequestId,
) -> Result<HandyHspLiveStatus> {
    ensure!(
        id != stream_request_id,
        "HSP live Stop requires its own request identity"
    );
    let value = checked(
        api.execute_identified(
            id.clone(),
            Command::StopHandyHspLive {
                stream_request_id: stream_request_id.clone(),
                owner_proof: proof(directory)?,
            },
            Some(project.clone()),
            None,
        )?,
        &project,
        &stream_request_id,
    )?;
    ensure!(
        value
            .native
            .as_ref()
            .is_none_or(|native| native.retired && native.original_stop_request_id.is_some()),
        "HSP live Stop response lacks retained retirement intent"
    );
    Ok(value)
}

/// Read existing admission/native/history observations without issuing an
/// identified effect, renewing a lease, or manufacturing an absent stream.
pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    stream_request_id: RequestId,
) -> Result<HandyHspLiveStatus> {
    checked(
        api.execute(
            Command::HandyHspLiveStatus {
                stream_request_id: stream_request_id.clone(),
                owner_proof: proof(directory)?,
            },
            Some(project.clone()),
            None,
        )?,
        &project,
        &stream_request_id,
    )
}

/// Explicitly admit ongoing observations for this original stream. Repeating
/// the same request inspects its original outcome; it cannot extend its duration.
pub fn enable_continuity(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &ContinuityIntent,
    allow_online_observations: bool,
) -> Result<HandyHspContinuityStatus> {
    validate_handy_hsp_continuity_selection(
        &intent.stream_request_id,
        &id,
        intent.duration_ms,
        allow_online_observations,
    )?;
    let ResponseBody::HandyHspContinuity(value) = api.execute_identified(
        id.clone(),
        Command::EnableHandyHspContinuity {
            stream_request_id: intent.stream_request_id.clone(),
            duration_ms: intent.duration_ms,
            allow_online_observations,
            owner_proof: proof(directory)?,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("HSP continuity response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == intent.project
            && value.stream_request_id == intent.stream_request_id
            && value.enable_request_id == id
            && value.duration_ms == intent.duration_ms,
        "HSP continuity response changed original intent"
    );
    Ok(*value)
}

/// Read the original admission without polling, refreshing or creating a clock.
pub fn continuity_status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original_request_id: RequestId,
) -> Result<HandyHspContinuityStatus> {
    let ResponseBody::HandyHspContinuity(value) = api.execute(
        Command::HandyHspContinuityStatus {
            original_request_id: original_request_id.clone(),
            owner_proof: proof(directory)?,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("HSP continuity response differs");
    };
    value.validate()?;
    ensure!(
        value.project_id == project && value.enable_request_id == original_request_id,
        "HSP continuity status changed original admission"
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
        fn reply(value: HandyHspLiveStatus) -> Self {
            Self {
                calls: Vec::new(),
                reply: Some(ResponseBody::HandyHspLive(Box::new(value))),
            }
        }
        fn respond(&mut self, call: Call) -> Result<ResponseBody> {
            self.calls.push(call);
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("original live stream not found"))
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
    fn intent() -> LiveIntent {
        LiveIntent {
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(2),
            selection: HandyHspLiveSelection {
                finite_drain_policy_sha256: None,
                preparation_request_id: RequestId::new("prepare").unwrap(),
                capture_epoch: AttemptId::new("capture").unwrap(),
                inference_attempt_id: AttemptId::new("inference").unwrap(),
                browser_timing: ArtifactId::new("opaque-browser-timing").unwrap(),
                source_qualification: HandyHspSourceQualificationBinding {
                    dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                    revision: 2,
                },
            },
        }
    }
    fn report() -> HandyHspLiveStatus {
        let i = intent();
        HandyHspLiveStatus {
            project_id: i.project,
            stream_request_id: RequestId::new("stream").unwrap(),
            selection: i.selection,
            source_subject: None,
            origin: None,
            native: None,
            stream: None,
            error: None,
            qualified: false,
        }
    }
    fn acted() -> HandyHspLiveStatus {
        let mut value = report();
        value.native = Some(HandyHspLiveNativeStatus {
            active_source: None,
            retired: true,
            original_play_request_id: Some(RequestId::new("play").unwrap()),
            original_stop_request_id: Some(RequestId::new("stop").unwrap()),
            delivered_samples: 3,
            error: Some(ProtocolError::new(
                ErrorCode::UnknownOutcome,
                "outcome unknown",
            )),
        });
        value
    }
    fn directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = directory.path().join("handy-owner.token");
        // Synthetic fixture only; not a device credential or real capability.
        std::fs::write(&token, "a".repeat(64)).unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        directory
    }
    fn same(actual: &HandyHspLiveStatus, expected: &HandyHspLiveStatus) {
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[test]
    fn hsp_live_client_bind_preserves_identified_selection_and_requires_consent() {
        let directory = directory();
        let expected = report();
        let mut api = Api::reply(expected.clone());
        let actual = bind(
            &mut api,
            directory.path(),
            expected.stream_request_id.clone(),
            &intent(),
            true,
            true,
        )
        .unwrap();
        same(&actual, &expected);
        let [(id, command, project, revision)] = api.calls.as_slice() else {
            panic!()
        };
        assert_eq!(id.as_ref(), Some(&expected.stream_request_id));
        assert_eq!(project.as_ref(), Some(&expected.project_id));
        assert_eq!(*revision, Some(intent().revision));
        assert!(
            matches!(command, Command::BindHandyHspLive { selection, acknowledge_current_environment: true, allow_online_control: true, owner_proof } if selection == &expected.selection && owner_proof.len() == 64)
        );
        for case in 0..5 {
            let mut i = intent();
            let mut id = RequestId::new("stream").unwrap();
            match case {
                0 => {}
                1 => i.selection.source_qualification.revision = 1,
                2 => i.selection.inference_attempt_id = i.selection.capture_epoch.clone(),
                3 => id = i.selection.preparation_request_id.clone(),
                _ => {}
            }
            let mut api = Api::reply(report());
            assert!(bind(&mut api, directory.path(), id, &i, case != 4, case != 0).is_err());
            assert!(api.calls.is_empty());
        }
        let missing = tempfile::tempdir().unwrap();
        let mut api = Api::reply(report());
        assert!(bind(
            &mut api,
            missing.path(),
            RequestId::new("stream").unwrap(),
            &intent(),
            true,
            true
        )
        .is_err());
        assert!(api.calls.is_empty());
        let error = bind(
            &mut api,
            missing.path(),
            RequestId::new("unacknowledged").unwrap(),
            &intent(),
            false,
            true,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("current environment acknowledgment"));
        assert!(api.calls.is_empty());
    }

    #[test]
    fn hsp_live_client_bind_rejects_substituted_subject_selection_and_qualification() {
        let directory = directory();
        for case in 0..8 {
            let mut value = report();
            match case {
                0 => value.project_id = ProjectId::new("other").unwrap(),
                1 => value.stream_request_id = RequestId::new("other").unwrap(),
                2 => value.selection.capture_epoch = AttemptId::new("other").unwrap(),
                3 => value.selection.inference_attempt_id = AttemptId::new("other").unwrap(),
                4 => value.selection.browser_timing = ArtifactId::new("other").unwrap(),
                5 => value.selection.preparation_request_id = RequestId::new("other").unwrap(),
                6 => value.selection.source_qualification.revision = 3,
                _ => value.qualified = true,
            }
            assert!(
                bind(
                    &mut Api::reply(value),
                    directory.path(),
                    RequestId::new("stream").unwrap(),
                    &intent(),
                    true,
                    true
                )
                .is_err(),
                "case {case}"
            );
        }
    }

    #[test]
    fn hsp_live_client_play_and_stop_keep_exact_effect_ids_and_distinct_revision_scope() {
        let directory = directory();
        let expected = acted();
        for stopping in [false, true] {
            let mut api = Api::reply(expected.clone());
            let id = RequestId::new(if stopping { "stop" } else { "play" }).unwrap();
            let actual = if stopping {
                stop(
                    &mut api,
                    directory.path(),
                    id.clone(),
                    expected.project_id.clone(),
                    expected.stream_request_id.clone(),
                )
            } else {
                play(
                    &mut api,
                    directory.path(),
                    id.clone(),
                    expected.project_id.clone(),
                    intent().revision,
                    expected.stream_request_id.clone(),
                )
            }
            .unwrap();
            same(&actual, &expected);
            let [(actual_id, command, project, revision)] = api.calls.as_slice() else {
                panic!()
            };
            assert_eq!(actual_id.as_ref(), Some(&id));
            assert_eq!(project.as_ref(), Some(&expected.project_id));
            assert_eq!(*revision, (!stopping).then_some(intent().revision));
            if stopping {
                assert!(
                    matches!(command, Command::StopHandyHspLive { stream_request_id, .. } if stream_request_id == &expected.stream_request_id)
                );
            } else {
                assert!(
                    matches!(command, Command::PlayHandyHspLive { stream_request_id, .. } if stream_request_id == &expected.stream_request_id)
                );
            }
        }
        let mut api = Api::reply(expected.clone());
        assert!(play(
            &mut api,
            directory.path(),
            expected.stream_request_id.clone(),
            expected.project_id.clone(),
            intent().revision,
            expected.stream_request_id.clone()
        )
        .is_err());
        assert!(stop(
            &mut api,
            directory.path(),
            expected.stream_request_id.clone(),
            expected.project_id.clone(),
            expected.stream_request_id.clone()
        )
        .is_err());
        assert!(api.calls.is_empty());
    }

    #[test]
    fn hsp_live_client_refuses_substituted_or_missing_native_effect_receipts() {
        let directory = directory();
        for missing in [false, true] {
            let mut value = acted();
            let native = value.native.as_mut().unwrap();
            native.original_play_request_id =
                (!missing).then(|| RequestId::new("different-play").unwrap());
            native.original_stop_request_id =
                (!missing).then(|| RequestId::new("different-stop").unwrap());
            assert!(play(
                &mut Api::reply(value.clone()),
                directory.path(),
                RequestId::new("play").unwrap(),
                value.project_id.clone(),
                intent().revision,
                value.stream_request_id.clone()
            )
            .is_err());
            let stopped = stop(
                &mut Api::reply(value.clone()),
                directory.path(),
                RequestId::new("stop").unwrap(),
                value.project_id.clone(),
                value.stream_request_id.clone(),
            );
            assert_eq!(stopped.is_err(), missing);
            if let Ok(stopped) = stopped {
                assert_eq!(
                    stopped.native.unwrap().original_stop_request_id,
                    Some(RequestId::new("different-stop").unwrap())
                );
            }
        }
        let mut active = acted();
        active.native.as_mut().unwrap().retired = false;
        assert!(stop(
            &mut Api::reply(active.clone()),
            directory.path(),
            RequestId::new("stop").unwrap(),
            active.project_id,
            active.stream_request_id
        )
        .is_err());
    }

    #[test]
    fn hsp_live_client_status_is_read_only_and_preserves_absence_and_unknown() {
        let directory = directory();
        let mut failed = report();
        failed.error = Some(ProtocolError::new(
            ErrorCode::UnknownOutcome,
            "original admission outcome is unknown",
        ));
        for expected in [report(), acted(), failed] {
            let mut api = Api::reply(expected.clone());
            let actual = status(
                &mut api,
                directory.path(),
                expected.project_id.clone(),
                expected.stream_request_id.clone(),
            )
            .unwrap();
            same(&actual, &expected);
            let [(id, command, project, revision)] = api.calls.as_slice() else {
                panic!()
            };
            assert!(id.is_none() && revision.is_none());
            assert_eq!(project.as_ref(), Some(&expected.project_id));
            assert!(
                matches!(command, Command::HandyHspLiveStatus { stream_request_id, .. } if stream_request_id == &expected.stream_request_id)
            );
            assert!(!command.is_mutating());
        }
        for case in 0..4 {
            let mut value = report();
            match case {
                0 => value.project_id = ProjectId::new("other").unwrap(),
                1 => value.stream_request_id = RequestId::new("other").unwrap(),
                2 => value.qualified = true,
                _ => {}
            }
            let mut api = if case == 3 {
                Api {
                    calls: Vec::new(),
                    reply: Some(ResponseBody::Ack),
                }
            } else {
                Api::reply(value)
            };
            assert!(
                status(
                    &mut api,
                    directory.path(),
                    intent().project,
                    RequestId::new("stream").unwrap()
                )
                .is_err(),
                "case {case}"
            );
        }
        let mut absent = Api::default();
        let error = status(
            &mut absent,
            directory.path(),
            intent().project,
            RequestId::new("stream").unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "original live stream not found");
        assert_eq!(absent.calls.len(), 1);
    }
    include!("handy_hsp_continuity_client_tests.rs");
}
