//! Exact original play timing observations. No automatic retry or authority reconstruction.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;

pub struct RefreshIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub selection: HandyContinuousTimingSelection,
}
impl RefreshIntent {
    pub fn validate(&self, id: &RequestId) -> Result<()> {
        self.selection.validate()?;
        ensure!(
            id != &self.selection.play_request_id && id != &self.selection.acquisition_request_id,
            "continuous timing refresh requires its own request identity"
        );
        Ok(())
    }
}

pub fn refresh(
    api: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &RefreshIntent,
    allow_online_inspection: bool,
) -> Result<HandyContinuousTimingObservation> {
    ensure!(
        allow_online_inspection,
        "continuous timing refresh requires explicit online inspection consent"
    );
    intent.validate(&id)?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyContinuousTiming(value) = api.execute_identified(
        id.clone(),
        Command::RefreshHandyPlaybackTiming {
            selection: intent.selection.clone(),
            allow_online_inspection: true,
            owner_proof,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("continuous timing response differs");
    };
    value.validate()?;
    ensure!(
        value.request_id == id
            && value.controller.project_id == intent.project
            && value.controller.revision == intent.revision
            && value.selection() == intent.selection,
        "continuous timing response changed captured selection"
    );
    Ok(*value)
}

pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    original: RequestId,
) -> Result<HandyContinuousTimingObservation> {
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    let ResponseBody::HandyContinuousTiming(value) = api.execute(
        Command::HandyContinuousTimingStatus {
            refresh_request_id: original.clone(),
            owner_proof,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("continuous timing status response differs");
    };
    value.validate()?;
    ensure!(
        value.request_id == original && value.controller.project_id == project,
        "continuous timing status changed original subject"
    );
    Ok(*value)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    type ReadCall = (Command, Option<ProjectId>, Option<RevisionId>);
    type WriteCall = (RequestId, Command, Option<ProjectId>, Option<RevisionId>);
    #[derive(Default)]
    struct Api {
        reads: Vec<ReadCall>,
        writes: Vec<WriteCall>,
        reply: Option<ResponseBody>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.reads.push((command, project, revision));
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("synthetic unavailable reply"))
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.writes.push((id, command, project, revision));
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("synthetic unavailable reply"))
        }
    }
    fn directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let proof = directory.path().join("handy-owner.token");
        // Synthetic local fixture, never a real capability or device credential.
        std::fs::write(&proof, "a".repeat(64)).unwrap();
        std::fs::set_permissions(proof, std::fs::Permissions::from_mode(0o600)).unwrap();
        directory
    }
    fn intent(value: &HandyContinuousTimingObservation) -> RefreshIntent {
        RefreshIntent {
            project: value.controller.project_id.clone(),
            revision: value.controller.revision,
            selection: value.selection(),
        }
    }
    fn reply(value: HandyContinuousTimingObservation) -> Api {
        Api {
            reply: Some(ResponseBody::HandyContinuousTiming(Box::new(value))),
            ..Api::default()
        }
    }
    fn refresh_report(api: &mut Api, path: &Path) -> Result<HandyContinuousTimingObservation> {
        let value = observation();
        refresh(api, path, value.request_id.clone(), &intent(&value), true)
    }
    fn read_report(api: &mut Api, path: &Path) -> Result<HandyContinuousTimingObservation> {
        let value = observation();
        status(api, path, value.controller.project_id, value.request_id)
    }

    #[test]
    fn handy_continuous_client_checks_consent_and_selection_before_owner_proof() {
        let empty = tempfile::tempdir().unwrap();
        let value = observation();
        let mut api = reply(value.clone());
        let err = refresh(
            &mut api,
            empty.path(),
            value.request_id.clone(),
            &intent(&value),
            false,
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("explicit online inspection consent"));
        for case in 0..4 {
            let mut selected = intent(&value);
            let mut id = value.request_id.clone();
            match case {
                0 => id = selected.selection.play_request_id.clone(),
                1 => id = selected.selection.acquisition_request_id.clone(),
                2 => selected.selection.expected_generation = i64::MAX as u64 + 1,
                _ => selected.selection.play_record_id = ArtifactId::new("invalid").unwrap(),
            }
            let expected = selected.validate(&id).unwrap_err().to_string();
            let err = refresh(&mut api, empty.path(), id, &selected, true).unwrap_err();
            assert_eq!(err.to_string(), expected);
        }
        assert!(api.reads.is_empty() && api.writes.is_empty());
        assert!(refresh_report(&mut api, empty.path()).is_err());
        assert!(api.reads.is_empty() && api.writes.is_empty());
    }

    #[test]
    fn handy_continuous_client_submits_one_exact_identified_refresh_and_preserves_history() {
        let directory = directory();
        for current in [true, false] {
            let mut expected = observation();
            expected.current = current;
            let mut api = reply(expected.clone());
            let actual = refresh_report(&mut api, directory.path()).unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert!(api.reads.is_empty());
            assert_eq!(api.writes.len(), 1);
            let (id, command, project, revision) = &api.writes[0];
            assert_eq!(id, &expected.request_id);
            assert_eq!(project.as_ref(), Some(&expected.controller.project_id));
            assert_eq!(*revision, Some(expected.controller.revision));
            assert!(
                matches!(command,Command::RefreshHandyPlaybackTiming {selection,allow_online_inspection:true,owner_proof}
                if selection==&expected.selection() && owner_proof.len()==64)
            );
        }
    }

    #[test]
    fn handy_continuous_client_refuses_changed_subject_or_invalid_response_without_retry() {
        let directory = directory();
        for case in 0..11 {
            let mut value = observation();
            match case {
                0 => value.controller.project_id = ProjectId::new("other-project").unwrap(),
                1 => value.controller.revision = RevisionId::new(2),
                2 => value.request_id = RequestId::new("other-refresh").unwrap(),
                3 => {
                    value.controller.acquisition_request_id =
                        RequestId::new("other-acquisition").unwrap()
                }
                4 => value.controller.controller_id = ClientId::new("other-controller").unwrap(),
                5 => value.controller.generation += 1,
                6 => value.play_request_id = RequestId::new("other-play").unwrap(),
                7 => value.play_record_id = ArtifactId::new("c".repeat(64)).unwrap(),
                8 => value.playback_enabled = true,
                9 => value.controller.playback_enabled = true,
                _ => value.alignment_error_ns += 1,
            }
            let mut api = reply(value);
            assert!(
                refresh_report(&mut api, directory.path()).is_err(),
                "case {case}"
            );
            assert!(api.reads.is_empty());
            assert_eq!(api.writes.len(), 1);
        }
        for response in [Some(ResponseBody::Ack), None] {
            let mut api = Api {
                reply: response,
                ..Api::default()
            };
            assert!(refresh_report(&mut api, directory.path()).is_err());
            assert_eq!(api.writes.len(), 1);
        }
    }

    #[test]
    fn handy_continuous_client_status_is_read_only_and_preserves_expired_history() {
        let directory = directory();
        let mut expected = observation();
        expected.current = false;
        let mut api = reply(expected.clone());
        let actual = read_report(&mut api, directory.path()).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert!(api.writes.is_empty());
        assert_eq!(api.reads.len(), 1);
        let (command, project, revision) = &api.reads[0];
        assert_eq!(project.as_ref(), Some(&expected.controller.project_id));
        assert!(revision.is_none());
        assert!(
            matches!(command,Command::HandyContinuousTimingStatus {refresh_request_id,owner_proof}
            if refresh_request_id==&expected.request_id && owner_proof.len()==64)
        );
    }

    #[test]
    fn handy_continuous_client_status_rejects_wrong_original_project_or_authority() {
        let directory = directory();
        for case in 0..4 {
            let mut value = observation();
            match case {
                0 => value.controller.project_id = ProjectId::new("other-project").unwrap(),
                1 => value.request_id = RequestId::new("other-refresh").unwrap(),
                2 => value.playback_enabled = true,
                _ => value.sequence = 0,
            }
            let mut api = reply(value);
            assert!(read_report(&mut api, directory.path()).is_err());
            assert!(api.writes.is_empty());
            assert_eq!(api.reads.len(), 1);
        }
        let empty = tempfile::tempdir().unwrap();
        let mut api = reply(observation());
        assert!(read_report(&mut api, empty.path()).is_err());
        assert!(api.reads.is_empty());
    }
    fn observation() -> HandyContinuousTimingObservation {
        let mut device: HandyDeviceTimingInspection = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-device-timing.json"
        ))
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
}
