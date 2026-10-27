use super::*;
use crate::playback_runtime::{
    tests::{clock, fixture},
    QualifiedCommandTiming,
};
#[path = "device_browser_play_journal_tests.rs"]
mod browser_reference_tests;

struct Fixture {
    dir: tempfile::TempDir,
    journal: DeviceEffectJournal,
    runtime: HandyPlaybackRuntime,
    effect: PlaybackEffect,
    record: PlayOperationRecord,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let correlation: HandyProgramCorrelation = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap();
        let i = &correlation.inspection;
        let boot = i.engine_epoch.clone();
        let mut journal = DeviceEffectJournal::open(dir.path(), boot.clone()).unwrap();
        let actor = SessionId::new("actor").unwrap();
        let qualification_binding = i.accepted_qualification.clone().unwrap();
        let setup = SetupEffectRecord {
            boot: boot.clone(),
            device_session: i.device_session.clone(),
            actor_session: actor.clone(),
            request_id: correlation.setup_request_id.clone(),
            hosting_request_id: i.hosting_request_id.clone(),
            project_id: i.project_id.clone(),
            revision: i.revision,
            registration: i.registration.clone(),
            qualification: qualification_binding.clone(),
            master_sha256: i.master_sha256.clone(),
            script: i.script.clone(),
            hosted: i.hosted.clone(),
            route_sha256: "9".repeat(64),
            route_namespace_sha256: Some("8".repeat(64)),
            acknowledgment_required: true,
            preflight_mode: Some(correlation.setup_mode.clone()),
        };
        let prepared = journal.prepare_setup(setup).unwrap();
        let setup_record_id = ArtifactId::new(prepared.id()).unwrap();
        let claimed = journal.claim_setup(&prepared).unwrap();
        journal
            .complete_setup(
                &claimed,
                SetupCompletion::Acknowledged(correlation.acknowledgment.clone()),
            )
            .unwrap();
        let (_, mut admission, controller) = fixture();
        admission.project = i.project_id.clone();
        admission.revision = i.revision;
        admission.configuration.device = i.registration.device_id.clone();
        admission.program_artifact = i.hosted.artifact_id.clone();
        let qualification = DeviceQualification::new(
            admission.configuration.clone(),
            qualification_binding.dossier_id.clone(),
            QualificationAuthority::LocalHumanAcceptance,
            Some(7_000_000),
        )
        .unwrap();
        let timing =
            QualifiedCommandTiming::new(qualification, 10_000_000, 1_000_000, 100).unwrap();
        let mut runtime = HandyPlaybackRuntime::new(
            admission.configuration.device.clone(),
            i.device_session.clone(),
            timing,
        )
        .unwrap();
        runtime
            .arm(controller.clone(), admission.clone(), None)
            .unwrap();
        let pending = runtime.prepare_start(&controller, clock(), 1234).unwrap();
        let effect = runtime
            .dispatch_start(
                pending,
                &admission,
                &controller,
                &clock().domain,
                clock().monotonic_ns,
            )
            .unwrap();
        let preparation_request_id = RequestId::new("preparation").unwrap();
        let preparation_id = ArtifactId::new(digest(
            &serde_json::to_vec(&(
                &actor,
                boot.as_str(),
                &preparation_request_id,
                &setup_record_id,
            ))
            .unwrap(),
        ))
        .unwrap();
        let original_controller = HandyControllerLease {
            acquisition_request_id: RequestId::new("controller-acquire").unwrap(),
            controller_id: controller,
            preparation_request_id,
            preparation_id,
            project_id: admission.project.clone(),
            revision: admission.revision,
            engine_epoch: boot,
            device_session: effect.session_id().clone(),
            device_id: admission.configuration.device.clone(),
            generation: 0,
            duration_ms: 1000,
            deadline_boottime_ns: 1_000_000_000,
            state: HandyControllerState::Active,
            armed: true,
            physical_stop_pending: false,
            playback_enabled: false,
        };
        let record = PlayOperationRecord {
            browser_start: None,
            actor_session: actor,
            request_id: RequestId::new("play-original").unwrap(),
            request_fingerprint: digest(b"original-play-request"),
            original_controller,
            qualification: qualification_binding,
            setup_record_id,
            clock_request_id: RequestId::new("clock-request").unwrap(),
            clock: PlayClockRecord::from(&clock()),
            start_time_ms: 1234,
            effect: journal
                .receipt_binding()
                .record_for_effect(&effect)
                .unwrap(),
        };
        record.validate().unwrap();
        Self {
            dir,
            journal,
            runtime,
            effect,
            record,
        }
    }
    fn intent(&self) -> PlayIntent {
        self.journal
            .receipt_binding()
            .capture_play_intent(&self.runtime, &self.effect, self.record.clone())
            .unwrap()
    }
    fn prepare(&mut self) -> PreparedPlayReceipt {
        let intent = self.intent();
        self.journal.prepare_play(&intent).unwrap()
    }
    fn history(&self) -> PlayOperationObservation {
        self.journal
            .play_request_history(&self.record.actor_session, &self.record.request_id)
            .unwrap()
            .unwrap()
    }
    fn bytes(&self) -> Vec<Option<String>> {
        ["journal.sqlite3", "journal.sqlite3-wal"]
            .iter()
            .map(|name| {
                std::fs::read(self.dir.path().join("device-effects").join(name))
                    .ok()
                    .map(|b| digest(&b))
            })
            .collect()
    }
}

#[test]
fn original_play_claim_is_single_use_and_completion_is_atomic_and_immutable() {
    let mut f = Fixture::new();
    let intent = f.intent();
    let first = f.journal.prepare_play(&intent).unwrap();
    let repeat = f.journal.prepare_play(&intent).unwrap();
    assert_eq!(f.history().state, PlayOperationState::Prepared);
    let claimed = f.journal.claim_play(first).unwrap();
    assert_eq!(f.history().state, PlayOperationState::Claimed);
    assert!(f.journal.claim_play(repeat).is_err());
    assert!(f.journal.prepare_play(&intent).is_err());
    assert!(f.journal.prepare(&f.runtime, &f.effect).is_err());
    assert!(f
        .journal
        .complete(claimed.effect_receipt(), DispatchOutcome::Acknowledged)
        .is_err());
    f.journal
        .complete_play_operation(&claimed, DispatchOutcome::Acknowledged)
        .unwrap();
    f.journal
        .complete_play_operation(&claimed, DispatchOutcome::Acknowledged)
        .unwrap();
    assert!(f
        .journal
        .complete_play_operation(&claimed, DispatchOutcome::Unknown)
        .is_err());
    let observed = f.history();
    assert_eq!(observed.state, PlayOperationState::Acknowledged);
    assert_eq!(observed.id, f.record.id().unwrap());
    assert_eq!(observed.record.clock, f.record.clock);
    let effects = f
        .journal
        .history(&f.record.original_controller.device_id, 10)
        .unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].outcome, Some(DispatchOutcome::Acknowledged));
}

#[test]
fn play_route_custody_survives_acknowledgment_alias_namespace_and_restart() {
    for state in 0..4 {
        let mut f = Fixture::new();
        let prepared = f.prepare();
        if state > 0 {
            let claimed = f.journal.claim_play(prepared).unwrap();
            if state > 1 {
                f.journal
                    .complete_play_operation(
                        &claimed,
                        if state == 2 {
                            DispatchOutcome::Acknowledged
                        } else {
                            DispatchOutcome::Unknown
                        },
                    )
                    .unwrap();
            }
        }
        for (route, namespace, device) in [
            (
                "9".repeat(64),
                "8".repeat(64),
                DeviceId::new("alias").unwrap(),
            ),
            (
                "7".repeat(64),
                "8".repeat(64),
                f.record.original_controller.device_id.clone(),
            ),
            (
                "7".repeat(64),
                "6".repeat(64),
                DeviceId::new("other").unwrap(),
            ),
        ] {
            assert!(f
                .journal
                .play_route_available(&route, &namespace, &device, None)
                .is_err());
        }
        f.journal
            .play_route_available(
                &"7".repeat(64),
                &"8".repeat(64),
                &DeviceId::new("other").unwrap(),
                None,
            )
            .unwrap();
        let mut next =
            DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("next-boot").unwrap())
                .unwrap();
        assert!(next
            .play_route_available(
                &"9".repeat(64),
                &"8".repeat(64),
                &f.record.original_controller.device_id,
                None
            )
            .is_err());
        if state == 0 {
            next.cancel_unclaimed_play(
                &f.record.actor_session,
                &f.record.request_id,
                &f.record.id().unwrap(),
            )
            .unwrap();
            next.play_route_available(
                &"9".repeat(64),
                &"8".repeat(64),
                &f.record.original_controller.device_id,
                None,
            )
            .unwrap();
        }
    }
}

#[test]
fn play_custody_blocks_sync_preparation_and_a_preexisting_claim() {
    let mut f = Fixture::new();
    let parent = f
        .journal
        .setup_history(f.record.setup_record_id.as_str())
        .unwrap()
        .record;
    let sync = ClockSyncEffectRecord {
        actor_session: f.record.actor_session.clone(),
        request_id: RequestId::new("later-sync").unwrap(),
        original_controller: f.record.original_controller.clone(),
        registration: parent.registration.clone(),
        preparation_qualification: f.record.qualification.clone(),
        setup_record_id: f.record.setup_record_id.clone(),
        user_offset_before: HandyUserTimeOffset { offset_ms: 0 },
        route_sha256: parent.route_sha256.clone(),
        route_namespace_sha256: parent.route_namespace_sha256.clone().unwrap(),
    };
    let sync_pending = f.journal.prepare_clock_sync(sync.clone()).unwrap();
    f.prepare();
    assert!(f.journal.claim_clock_sync(&sync_pending).is_err());
    let mut sync = sync;
    sync.request_id = RequestId::new("new-sync").unwrap();
    assert!(f.journal.prepare_clock_sync(sync).is_err());
}

#[test]
fn full_play_witness_checks_immutable_subject_and_monotonic_history() {
    let mut f = Fixture::new();
    let p = f.prepare();
    let prepared = f.history().status().unwrap();
    let before = f.bytes();
    DeviceEffectJournal::verify_play_statuses(
        f.dir.path(),
        &[(f.record.actor_session.clone(), prepared.clone())],
    )
    .unwrap();
    assert_eq!(f.bytes(), before);
    let mut changed = prepared.clone();
    changed.history.as_mut().unwrap().clock_request_id = RequestId::new("wrong-clock").unwrap();
    assert!(f.history().check_witness(&changed).is_err());
    changed = prepared.clone();
    changed.history.as_mut().unwrap().state =
        pulsar_protocol::HandyPlaybackOperationState::Acknowledged;
    assert!(f.history().check_witness(&changed).is_err());
    let claimed = f.journal.claim_play(p).unwrap();
    f.journal
        .complete_play_operation(&claimed, DispatchOutcome::Unknown)
        .unwrap();
    f.history().check_witness(&prepared).unwrap();
    assert!(f.history().check_witness(&changed).is_err());
}

#[test]
fn version_five_upgrade_keeps_original_operation_and_effect() {
    let mut f = Fixture::new();
    let p = f.prepare();
    f.journal.claim_play(p).unwrap();
    let before = f.history();
    f.journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members; DROP TABLE custody_resolution_census; DROP TABLE custody_resolutions; DROP TABLE setup_custody_routes; DROP TABLE clock_sync_custody_routes; DROP INDEX setup_active_route; CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'; DROP INDEX clock_sync_active_route; CREATE UNIQUE INDEX clock_sync_active_route ON clock_sync_effects(route) WHERE state IN ('prepared','claimed','unknown'); DROP TABLE emergency_stop_observations; DROP TABLE emergency_stop_census; PRAGMA user_version=5").unwrap();
    let next =
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("version-ten").unwrap())
            .unwrap();
    assert_eq!(
        next.db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    let after = next
        .play_request_history(&f.record.actor_session, &f.record.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(
        serde_json::to_value(&after.record).unwrap(),
        serde_json::to_value(&before.record).unwrap()
    );
    assert_eq!(after.state, PlayOperationState::Claimed);
}

#[test]
fn typed_completion_refuses_another_operation_receipt_without_losing_observation() {
    let mut f = Fixture::new();
    let p = f.prepare();
    let claimed = f.journal.claim_play(p).unwrap();
    let mut completion = f
        .runtime
        .observe_play_completion(
            &f.effect,
            claimed.effect_receipt(),
            DispatchOutcome::Acknowledged,
            &f.journal.receipt_binding(),
        )
        .unwrap()
        .observation;
    let mut other = Fixture::new();
    other.record.request_id = RequestId::new("another-play").unwrap();
    let p = other.prepare();
    let other_claim = other.journal.claim_play(p).unwrap();
    assert!(completion
        .try_persist_play_operation(&other_claim, &mut f.journal)
        .is_err());
    assert!(completion.journal_pending());
    assert_eq!(f.history().state, PlayOperationState::Claimed);
    completion
        .try_persist_play_operation(&claimed, &mut f.journal)
        .unwrap();
    assert!(!completion.journal_pending());
    assert_eq!(f.history().state, PlayOperationState::Acknowledged);
}

#[test]
fn only_exact_unclaimed_play_can_be_cancelled_and_no_runtime_authority_changes() {
    for claim in [false, true] {
        let mut f = Fixture::new();
        let intent = f.intent();
        let prepared = f.journal.prepare_play(&intent).unwrap();
        let duplicate = f.journal.prepare_play(&intent).unwrap();
        let claimed = claim.then(|| f.journal.claim_play(prepared).unwrap());
        let id = f.record.id().unwrap();
        assert!(f
            .journal
            .cancel_unclaimed_play(&SessionId::new("other").unwrap(), &f.record.request_id, &id)
            .is_err());
        let cancelled =
            f.journal
                .cancel_unclaimed_play(&f.record.actor_session, &f.record.request_id, &id);
        assert_eq!(cancelled.is_ok(), !claim);
        if !claim {
            f.journal
                .cancel_unclaimed_play(&f.record.actor_session, &f.record.request_id, &id)
                .unwrap();
            assert_eq!(f.history().state, PlayOperationState::Cancelled);
        } else {
            f.journal
                .complete_play_operation(claimed.as_ref().unwrap(), DispatchOutcome::Unknown)
                .unwrap();
            assert!(f
                .journal
                .cancel_unclaimed_play(&f.record.actor_session, &f.record.request_id, &id)
                .is_err());
        }
        assert!(f.journal.claim_play(duplicate).is_err());
        assert!(f.journal.prepare_play(&intent).is_err());
        assert_eq!(f.runtime.session().state(), DeviceState::Playing);
        assert!(f.runtime.session().physical_stop_pending());
    }
}

#[test]
fn restart_preserves_history_but_fences_intents_claims_and_completions() {
    for did_claim in [false, true] {
        let mut f = Fixture::new();
        let intent = f.intent();
        let prepared = f.journal.prepare_play(&intent).unwrap();
        let duplicate = f.journal.prepare_play(&intent).unwrap();
        let claimed = did_claim.then(|| f.journal.claim_play(prepared).unwrap());
        let mut replacement =
            DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("new-boot").unwrap())
                .unwrap();
        assert!(f.journal.claim_play(duplicate).is_err());
        assert!(replacement.prepare_play(&intent).is_err());
        if let Some(claimed) = claimed {
            assert!(replacement
                .complete_play_operation(&claimed, DispatchOutcome::Unknown)
                .is_err());
            assert!(f
                .journal
                .complete_play_operation(&claimed, DispatchOutcome::Unknown)
                .is_err());
        }
        let history = replacement
            .play_request_history(&f.record.actor_session, &f.record.request_id)
            .unwrap()
            .unwrap();
        assert_eq!(history.record.clock, f.record.clock);
        assert_eq!(
            history.state,
            if did_claim {
                PlayOperationState::Claimed
            } else {
                PlayOperationState::Prepared
            }
        );
        assert_eq!(
            replacement
                .cancel_unclaimed_play(&f.record.actor_session, &f.record.request_id, &history.id)
                .is_ok(),
            !did_claim
        );
    }
}

#[test]
fn failed_operation_insert_rolls_back_the_effect_and_retry_keeps_original_identity() {
    let mut f = Fixture::new();
    let intent = f.intent();
    f.journal.db.execute_batch("CREATE TRIGGER refuse_play BEFORE INSERT ON play_operations BEGIN SELECT RAISE(ABORT,'synthetic operation failure'); END;").unwrap();
    assert!(f.journal.try_prepare_play(&intent).is_err());
    assert!(f
        .journal
        .history(&f.record.original_controller.device_id, 10)
        .unwrap()
        .is_empty());
    assert!(f
        .journal
        .play_request_history(&f.record.actor_session, &f.record.request_id)
        .unwrap()
        .is_none());
    f.journal
        .db
        .execute_batch("DROP TRIGGER refuse_play")
        .unwrap();
    f.journal.try_prepare_play(&intent).unwrap();
    assert_eq!(f.history().id, f.record.id().unwrap());
}

#[test]
fn failed_completion_update_rolls_back_both_rows_and_retries_metadata_only() {
    let mut f = Fixture::new();
    let p = f.prepare();
    let claimed = f.journal.claim_play(p).unwrap();
    f.journal.db.execute_batch("CREATE TRIGGER refuse_outcome BEFORE UPDATE OF state ON play_operations BEGIN SELECT RAISE(ABORT,'synthetic outcome failure'); END;").unwrap();
    assert!(f
        .journal
        .try_complete_play_operation(&claimed, DispatchOutcome::Unknown)
        .is_err());
    assert_eq!(f.history().state, PlayOperationState::Claimed);
    assert_eq!(
        f.journal
            .history(&f.record.original_controller.device_id, 10)
            .unwrap()[0]
            .outcome,
        None
    );
    f.journal
        .db
        .execute_batch("DROP TRIGGER refuse_outcome")
        .unwrap();
    f.journal
        .try_complete_play_operation(&claimed, DispatchOutcome::Unknown)
        .unwrap();
    assert_eq!(f.history().state, PlayOperationState::Unknown);
    assert!(f
        .journal
        .claim_play(PreparedPlayReceipt {
            id: f.record.id().unwrap(),
            boot: f.record.effect.boot.clone()
        })
        .is_err());
}

#[test]
fn legacy_prepared_or_completed_effects_cannot_be_reclassified_as_unclaimed_operations() {
    for outcome in [
        None,
        Some(DispatchOutcome::Unknown),
        Some(DispatchOutcome::Acknowledged),
    ] {
        let mut f = Fixture::new();
        let intent = f.intent();
        let receipt = f.journal.prepare(&f.runtime, &f.effect).unwrap();
        if let Some(outcome) = outcome {
            f.journal.complete(&receipt, outcome).unwrap();
        }
        assert!(f.journal.prepare_play(&intent).is_err());
        assert!(f
            .journal
            .play_request_history(&f.record.actor_session, &f.record.request_id)
            .unwrap()
            .is_none());
        assert_eq!(
            f.journal
                .history(&f.record.original_controller.device_id, 10)
                .unwrap()[0]
                .outcome,
            outcome
        );
    }
}

#[test]
fn changed_original_request_or_mismatched_effect_metadata_never_acquires_a_claim() {
    let mut f = Fixture::new();
    f.prepare();
    let mut changed = f.record.clone();
    changed.request_fingerprint = digest(b"changed-request");
    let intent = f
        .journal
        .receipt_binding()
        .capture_play_intent(&f.runtime, &f.effect, changed)
        .unwrap();
    assert!(f.journal.prepare_play(&intent).is_err());
    for mode in 0..8 {
        let mut record = f.record.clone();
        match mode {
            0 => record.start_time_ms += 1,
            1 => record.clock.monotonic_ns += 1,
            2 => record.original_controller.generation = u64::MAX,
            3 => record.effect.admission.project = ProjectId::new("wrong-project").unwrap(),
            4 => {
                record.original_controller.controller_id =
                    ClientId::new("wrong-controller").unwrap()
            }
            5 => record.qualification.revision = 1,
            6 => record.effect.stop = true,
            _ => record.effect.request_sha256 = digest(b"different-body"),
        }
        assert!(f
            .journal
            .receipt_binding()
            .capture_play_intent(&f.runtime, &f.effect, record)
            .is_err());
    }
    assert_eq!(f.history().state, PlayOperationState::Prepared);
}

#[test]
fn malformed_layout_or_history_refuses_startup_without_replacing_owner_or_bytes() {
    for mode in 0..5 {
        let mut f = Fixture::new();
        f.prepare();
        f.journal
            .db
            .execute_batch(match mode {
                0 => "DROP INDEX play_operation_request",
                1 => "UPDATE play_operations SET record='{}'",
                2 => "UPDATE play_operations SET state='acknowledged'",
                3 => "UPDATE effects SET outcome='unknown'",
                _ => "DELETE FROM effects",
            })
            .unwrap();
        let before = f.bytes();
        assert!(DeviceEffectJournal::open(
            f.dir.path(),
            DeviceSessionId::new("forbidden-replacement").unwrap()
        )
        .is_err());
        assert_eq!(f.bytes(), before);
        assert_eq!(
            f.journal
                .db
                .query_row("SELECT boot FROM owner", [], |r| r.get::<_, String>(0))
                .unwrap(),
            f.record.effect.boot.as_str()
        );
    }
}

#[test]
fn witness_checks_are_read_only_and_refuse_missing_or_substituted_history() {
    let mut f = Fixture::new();
    f.prepare();
    let before = f.bytes();
    let witness = (
        f.record.actor_session.clone(),
        f.record.request_id.clone(),
        f.record.id().unwrap(),
    );
    DeviceEffectJournal::verify_play_records(f.dir.path(), std::slice::from_ref(&witness)).unwrap();
    let mut wrong = witness.clone();
    wrong.0 = SessionId::new("other").unwrap();
    assert!(DeviceEffectJournal::verify_play_records(f.dir.path(), &[wrong]).is_err());
    assert_eq!(f.bytes(), before);
    let missing = tempfile::tempdir().unwrap();
    assert!(DeviceEffectJournal::verify_play_records(missing.path(), &[witness]).is_err());
    assert!(!missing.path().join("device-effects").exists());
}

#[test]
fn version_four_upgrade_preserves_legacy_rows_without_creating_play_operations() {
    let mut f = Fixture::new();
    let receipt = f.journal.prepare(&f.runtime, &f.effect).unwrap();
    f.journal
        .complete(&receipt, DispatchOutcome::Unknown)
        .unwrap();
    let before: String = f
        .journal
        .db
        .query_row("SELECT record FROM effects", [], |r| r.get(0))
        .unwrap();
    f.journal
        .db
        .execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members; DROP TABLE custody_resolution_census; DROP TABLE custody_resolutions; DROP TABLE setup_custody_routes; DROP TABLE clock_sync_custody_routes; DROP INDEX setup_active_route; CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'; DROP INDEX clock_sync_active_route; CREATE UNIQUE INDEX clock_sync_active_route ON clock_sync_effects(route) WHERE state IN ('prepared','claimed','unknown'); DROP TABLE emergency_stop_observations; DROP TABLE emergency_stop_census; DROP TABLE play_operations; PRAGMA user_version=4")
        .unwrap();
    let upgraded =
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("upgraded").unwrap()).unwrap();
    assert_eq!(
        upgraded
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        upgraded
            .db
            .query_row("SELECT record FROM effects", [], |r| r.get::<_, String>(0))
            .unwrap(),
        before
    );
    assert!(upgraded
        .play_request_history(&f.record.actor_session, &f.record.request_id)
        .unwrap()
        .is_none());
    assert_eq!(
        upgraded
            .history(&f.record.original_controller.device_id, 10)
            .unwrap()[0]
            .outcome,
        Some(DispatchOutcome::Unknown)
    );
}

#[test]
fn missing_setup_parent_and_changed_preparation_binding_cannot_record_a_play() {
    let mut f = Fixture::new();
    let mut record = f.record.clone();
    record.setup_record_id = ArtifactId::new("e".repeat(64)).unwrap();
    assert!(f
        .journal
        .receipt_binding()
        .capture_play_intent(&f.runtime, &f.effect, record.clone())
        .is_err());
    record.original_controller.preparation_id = ArtifactId::new(digest(
        &serde_json::to_vec(&(
            &record.actor_session,
            record.original_controller.engine_epoch.as_str(),
            &record.original_controller.preparation_request_id,
            &record.setup_record_id,
        ))
        .unwrap(),
    ))
    .unwrap();
    let intent = f
        .journal
        .receipt_binding()
        .capture_play_intent(&f.runtime, &f.effect, record)
        .unwrap();
    assert!(f.journal.prepare_play(&intent).is_err());
    assert!(f
        .journal
        .history(&f.record.original_controller.device_id, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn setup_parent_corruption_invalidates_play_history_before_restart_changes_owner() {
    let mut f = Fixture::new();
    f.prepare();
    f.journal
        .db
        .execute(
            "UPDATE setup_effects SET record='{}' WHERE id=?1",
            [f.record.setup_record_id.as_str()],
        )
        .unwrap();
    assert!(f
        .journal
        .play_request_history(&f.record.actor_session, &f.record.request_id)
        .is_err());
    let before = f.bytes();
    assert!(DeviceEffectJournal::open(
        f.dir.path(),
        DeviceSessionId::new("invalid-parent").unwrap()
    )
    .is_err());
    assert_eq!(f.bytes(), before);
}
