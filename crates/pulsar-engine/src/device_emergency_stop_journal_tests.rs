use super::*;
use crate::playback_runtime::tests::{clock, fixture};

struct Fixture {
    dir: tempfile::TempDir,
    journal: DeviceEffectJournal,
    runtime: HandyPlaybackRuntime,
    stop: PlaybackEffect,
    intent: EffectIntent,
    play: EffectIntent,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("emergency-boot").unwrap())
                .unwrap();
        let (mut runtime, admission, controller) = fixture();
        let clock = clock();
        let pending = runtime
            .prepare_start(&controller, clock.clone(), 1234)
            .unwrap();
        let effect = runtime
            .dispatch_start(
                pending,
                &admission,
                &controller,
                &clock.domain,
                clock.monotonic_ns,
            )
            .unwrap();
        let play = journal
            .receipt_binding()
            .capture_intent(&runtime, &effect)
            .unwrap();
        let stop = runtime.revoke_controller().unwrap().unwrap();
        let intent = journal
            .receipt_binding()
            .capture_intent(&runtime, &stop)
            .unwrap();
        Self {
            dir,
            journal,
            runtime,
            stop,
            intent,
            play,
        }
    }
    fn counts(&self) -> (u64, u64, u64) {
        self.journal.db.query_row("SELECT (SELECT count(*) FROM effects),(SELECT count(*) FROM emergency_stop_observations),(SELECT observation_count FROM emergency_stop_census WHERE singleton=1)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
    }
    fn bytes(&self) -> Vec<Option<String>> {
        ["journal.sqlite3", "journal.sqlite3-wal"]
            .iter()
            .map(|n| {
                std::fs::read(self.dir.path().join("device-effects").join(n))
                    .ok()
                    .map(|b| digest(&b))
            })
            .collect()
    }
}

#[test]
fn emergency_stop_retrospective_metadata_is_immutable_and_restores_no_runtime_authority() {
    for outcome in [DispatchOutcome::Acknowledged, DispatchOutcome::Unknown] {
        let mut f = Fixture::new();
        f.runtime.complete_stop(f.stop.clone(), outcome).unwrap();
        let id = f.intent.record_id().unwrap();
        assert!(!f.journal.emergency_stop_recorded(&id).unwrap());
        f.journal
            .try_record_emergency_stop(&f.intent, outcome)
            .unwrap();
        f.journal
            .try_record_emergency_stop(&f.intent, outcome)
            .unwrap();
        assert_eq!(f.counts(), (1, 1, 1));
        assert!(f.journal.emergency_stop_recorded(&id).unwrap());
        let opposite = if outcome == DispatchOutcome::Acknowledged {
            DispatchOutcome::Unknown
        } else {
            DispatchOutcome::Acknowledged
        };
        assert!(f
            .journal
            .try_record_emergency_stop(&f.intent, opposite)
            .is_err());
        let history = f
            .journal
            .history(&f.stop.admission().configuration.device, 1)
            .unwrap();
        assert_eq!(history[0].id, id);
        assert_eq!(history[0].outcome, Some(outcome));
        assert_eq!(f.runtime.session().state(), DeviceState::StopUnconfirmed);
        assert!(f.runtime.session().physical_stop_pending());
        assert!(f.runtime.session().controller().is_none());
        let next =
            DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("new-boot").unwrap())
                .unwrap();
        assert!(next.emergency_stop_recorded(&id).unwrap());
        assert!(!f.journal.current_owner().unwrap());
        assert!(f
            .journal
            .try_record_emergency_stop(&f.intent, outcome)
            .is_err());
    }
}

#[test]
fn emergency_stop_failed_completion_rolls_back_effect_marker_and_census() {
    for prewritten in [false, true] {
        let mut f = Fixture::new();
        if prewritten {
            f.journal.prepare_intent(&f.intent).unwrap();
        }
        let before = f.counts();
        f.journal.db.execute_batch("CREATE TRIGGER fail_emergency_complete BEFORE UPDATE OF outcome ON effects BEGIN SELECT RAISE(ABORT,'synthetic emergency metadata failure'); END;").unwrap();
        assert!(f
            .journal
            .try_record_emergency_stop(&f.intent, DispatchOutcome::Acknowledged)
            .is_err());
        assert_eq!(f.counts(), before);
        if prewritten {
            assert!(f
                .journal
                .history(&f.stop.admission().configuration.device, 1)
                .unwrap()[0]
                .outcome
                .is_none());
        }
        f.journal
            .db
            .execute_batch("DROP TRIGGER fail_emergency_complete")
            .unwrap();
        f.journal
            .try_record_emergency_stop(&f.intent, DispatchOutcome::Acknowledged)
            .unwrap();
        assert_eq!(f.counts(), (1, 1, 1));
    }
}

#[test]
fn emergency_stop_prepared_legacy_row_keeps_its_original_bytes_and_identity() {
    let mut f = Fixture::new();
    let receipt = f.journal.prepare_intent(&f.intent).unwrap();
    let before: String = f
        .journal
        .db
        .query_row(
            "SELECT record FROM effects WHERE id=?1",
            [&receipt.id],
            |r| r.get(0),
        )
        .unwrap();
    f.journal
        .try_record_emergency_stop(&f.intent, DispatchOutcome::Unknown)
        .unwrap();
    let after: String = f
        .journal
        .db
        .query_row(
            "SELECT record FROM effects WHERE id=?1",
            [&receipt.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(f.intent.record_id().unwrap(), receipt.id);
    assert!(f.journal.emergency_stop_recorded(&receipt.id).unwrap());
    assert_eq!(f.counts(), (1, 1, 1));
}

#[test]
fn emergency_stop_rejects_play_and_intent_substitution_without_writing() {
    let mut f = Fixture::new();
    assert!(f
        .journal
        .try_record_emergency_stop(&f.play, DispatchOutcome::Unknown)
        .is_err());
    let mut pretend = f.play.clone();
    pretend.record.stop = true;
    assert!(f
        .journal
        .try_record_emergency_stop(&pretend, DispatchOutcome::Unknown)
        .is_err());
    let binding = f.journal.receipt_binding();
    binding.validate_intent(&f.stop, &f.intent).unwrap();
    for mode in 0..3 {
        let mut changed = f.intent.clone();
        match mode {
            0 => changed.record.boot = DeviceSessionId::new("other-boot").unwrap(),
            1 => changed.record.sequence += 1,
            _ => changed.record.admission.project = ProjectId::new("other-project").unwrap(),
        }
        assert!(binding.validate_intent(&f.stop, &changed).is_err());
    }
    f.runtime
        .complete_stop(f.stop.clone(), DispatchOutcome::Unknown)
        .unwrap();
    binding.validate_intent(&f.stop, &f.intent).unwrap();
    assert!(!f.runtime.effect_is_current(&f.stop));
    assert_eq!(f.counts(), (0, 0, 0));
}

#[test]
fn emergency_stop_writer_contention_is_prompt_and_restores_the_shared_timeout() {
    let mut f = Fixture::new();
    f.journal
        .db
        .busy_timeout(Duration::from_millis(1750))
        .unwrap();
    let writer = Connection::open(f.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(f.journal.current_owner().unwrap());
    let begin = std::time::Instant::now();
    assert!(f
        .journal
        .try_record_emergency_stop(&f.intent, DispatchOutcome::Unknown)
        .is_err());
    assert!(begin.elapsed() < Duration::from_millis(750));
    assert_eq!(
        f.journal
            .db
            .query_row("PRAGMA busy_timeout", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        1750
    );
    assert_eq!(f.counts(), (0, 0, 0));
    writer.execute_batch("COMMIT").unwrap();
    f.journal
        .try_record_emergency_stop(&f.intent, DispatchOutcome::Unknown)
        .unwrap();
    assert_eq!(
        f.journal
            .db
            .query_row("PRAGMA busy_timeout", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        1750
    );
}

#[test]
fn emergency_stop_current_owner_distinguishes_foreign_epoch_from_unavailable_metadata() {
    let mut f = Fixture::new();
    assert!(f.journal.current_owner().unwrap());
    f.journal
        .db
        .execute("UPDATE owner SET boot='foreign-boot'", [])
        .unwrap();
    assert!(!f.journal.current_owner().unwrap());
    assert!(f
        .journal
        .try_record_emergency_stop(&f.intent, DispatchOutcome::Unknown)
        .is_err());
    f.journal
        .db
        .execute("UPDATE owner SET boot=''", [])
        .unwrap();
    assert!(f.journal.current_owner().is_err());
    f.journal.db.execute_batch("DROP TABLE owner").unwrap();
    assert!(f.journal.current_owner().is_err());
    assert_eq!(f.counts(), (0, 0, 0));
}

#[test]
fn emergency_stop_missing_or_corrupt_history_refuses_without_replacing_owner_or_bytes() {
    for sql in [
        "DELETE FROM emergency_stop_observations",
        "UPDATE emergency_stop_observations SET provenance_sha256='bad'",
        "UPDATE emergency_stop_observations SET outcome='unknown'",
        "UPDATE effects SET outcome='unknown'",
        "DELETE FROM effects",
        "DELETE FROM emergency_stop_census",
        "UPDATE emergency_stop_census SET observation_count=0",
        "DROP TABLE emergency_stop_observations",
    ] {
        let mut f = Fixture::new();
        f.journal
            .try_record_emergency_stop(&f.intent, DispatchOutcome::Acknowledged)
            .unwrap();
        f.journal.db.execute_batch(sql).unwrap();
        let before = f.bytes();
        assert!(f
            .journal
            .emergency_stop_recorded(&f.intent.record_id().unwrap())
            .is_err());
        assert!(f
            .journal
            .try_record_emergency_stop(&f.intent, DispatchOutcome::Acknowledged)
            .is_err());
        assert!(DeviceEffectJournal::open(
            f.dir.path(),
            DeviceSessionId::new("must-not-own").unwrap()
        )
        .is_err());
        assert!(f.journal.current_owner().unwrap());
        assert_eq!(f.bytes(), before);
    }
}

#[test]
fn emergency_stop_marker_cannot_describe_a_play_even_with_recomputed_hashes() {
    let mut f = Fixture::new();
    f.journal
        .try_record_emergency_stop(&f.intent, DispatchOutcome::Unknown)
        .unwrap();
    let mut play = f.intent.record.clone();
    play.stop = false;
    let text = serde_json::to_string(&play).unwrap();
    let id = digest(text.as_bytes());
    f.journal
        .db
        .execute("UPDATE effects SET id=?1,record=?2", params![id, text])
        .unwrap();
    f.journal
        .db
        .execute(
            "UPDATE emergency_stop_observations SET effect_id=?1,provenance_sha256=?2",
            params![id, provenance(&id, "unknown").unwrap()],
        )
        .unwrap();
    assert!(DeviceEffectJournal::validate_existing(&f.journal.db).is_err());
}

#[test]
fn emergency_stop_version_six_upgrade_preserves_legacy_effect_without_inventing_provenance() {
    let mut f = Fixture::new();
    let receipt = f.journal.prepare_intent(&f.intent).unwrap();
    f.journal
        .complete(&receipt, DispatchOutcome::Unknown)
        .unwrap();
    let before: String = f
        .journal
        .db
        .query_row(
            "SELECT record FROM effects WHERE id=?1",
            [&receipt.id],
            |r| r.get(0),
        )
        .unwrap();
    f.journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members; DROP TABLE custody_resolution_census; DROP TABLE custody_resolutions; DROP TABLE setup_custody_routes; DROP TABLE clock_sync_custody_routes; DROP INDEX setup_active_route; CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'; DROP INDEX clock_sync_active_route; CREATE UNIQUE INDEX clock_sync_active_route ON clock_sync_effects(route) WHERE state IN ('prepared','claimed','unknown'); DROP TABLE emergency_stop_observations; DROP TABLE emergency_stop_census; PRAGMA user_version=6;").unwrap();
    let next =
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("version-ten").unwrap())
            .unwrap();
    assert_eq!(
        next.db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        next.db
            .query_row(
                "SELECT record FROM effects WHERE id=?1",
                [&receipt.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        before
    );
    assert!(!next.emergency_stop_recorded(&receipt.id).unwrap());
    assert_eq!(
        next.history(&f.stop.admission().configuration.device, 1)
            .unwrap()[0]
            .outcome,
        Some(DispatchOutcome::Unknown)
    );
    assert!(!f.journal.current_owner().unwrap());
}

#[test]
fn emergency_stop_new_schema_cannot_hide_behind_an_older_version() {
    let f = Fixture::new();
    f.journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members; DROP TABLE custody_resolution_census; DROP TABLE custody_resolutions; DROP TABLE setup_custody_routes; DROP TABLE clock_sync_custody_routes; DROP INDEX setup_active_route; CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'; DROP INDEX clock_sync_active_route; CREATE UNIQUE INDEX clock_sync_active_route ON clock_sync_effects(route) WHERE state IN ('prepared','claimed','unknown'); PRAGMA user_version=6").unwrap();
    let before = f.bytes();
    assert!(
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("must-not-own").unwrap())
            .is_err()
    );
    assert_eq!(f.bytes(), before);
}
