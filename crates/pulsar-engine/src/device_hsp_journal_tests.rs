use super::*;
use crate::handy_hsp_records::HspBindingRecord;
use crate::handy_hsp_runtime::{
    HandyHspRuntime, HspAuthorityWindow, HspBinding, HspDispatchFacts, HspEffectKind,
    HspObservedOutcome, HspPlayReference, HspSettledTransport,
};
use pulsar_core::{
    HspPhaseWindow, HspPoint, HspPolicy, HspSample, HspStreamIdentity, LiveIdentity,
};

fn binding(generation: u64) -> HspBinding {
    HspBinding::new(
        HspStreamIdentity {
            generation,
            stream_id: 42,
            live: LiveIdentity {
                epoch: AttemptId::new("synthetic-hsp-epoch").unwrap(),
                project: ProjectId::new("synthetic-project").unwrap(),
                revision: RevisionId::new(1),
                source: SourceVersionId::new("synthetic-source").unwrap(),
            },
        },
        ClientId::new("synthetic-owner").unwrap(),
        DeviceSessionId::new("synthetic-device-session").unwrap(),
        ArtifactId::new("synthetic-native-clock").unwrap(),
    )
}
fn record() -> HspStreamRecord {
    HspStreamRecord {
        boot: DeviceSessionId::new("synthetic-boot").unwrap(),
        actor_session: SessionId::new("synthetic-actor").unwrap(),
        request_id: RequestId::new("synthetic-request").unwrap(),
        registration: HandyRegistration {
            device_id: DeviceId::new("synthetic-device").unwrap(),
            revision: 1,
            settings: HandyRegistrationSettings {
                label: "Synthetic journal test; no physical evidence".into(),
                api_key_env: "FIXTURE_API".into(),
                connection_key_env: "FIXTURE_ROUTE".into(),
            },
            disabled: false,
            binding_sha256: "a".repeat(64),
        },
        device: HandyIdentity {
            firmware: "4.0".into(),
            model_number: 2,
            model_name: "Handy".into(),
            model_variant: 0,
            session_id: "native-session".into(),
            firmware_status: 0,
        },
        route_sha256: "b".repeat(64),
        route_namespace_sha256: "c".repeat(64),
        binding: HspBindingRecord::capture(&binding(1)),
        qualification_sha256: "d".repeat(64),
        profile_sha256: "e".repeat(64),
    }
}
fn facts() -> HspDispatchFacts {
    HspDispatchFacts::new(
        binding(1),
        HspPlayReference::new(
            100,
            HspPhaseWindow {
                earliest_ms: -100,
                latest_ms: -100,
            },
            -100,
            10100,
        )
        .unwrap(),
        HspAuthorityWindow::new(2000, 2000, 2000),
    )
    .unwrap()
}
fn runtime() -> HandyHspRuntime {
    HandyHspRuntime::new(
        binding(1),
        HspPolicy {
            max_buffered_points: 8,
            device_capacity_points: 8,
            max_chunk_points: 2,
            min_lead_ms: 10,
            max_future_ms: 1_000_000,
            max_sample_age_ns: 1000,
            max_gap_ms: 200,
        },
    )
    .unwrap()
}
fn response(tail: i32, playing: bool) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"result":{
        "play_state":if playing {1} else {2},"points":if tail<0 {0} else {2},"max_points":8,
        "current_point":if tail<0 {-1} else {0},"current_time":-100,"loop":false,"playback_rate":1,
        "first_point_time":0,"last_point_time":tail.max(0) as u32*100,"stream_id":42,
        "tail_point_stream_index":tail,"tail_point_stream_index_threshold":1,"pause_on_starving":false
    }})).unwrap()
}
fn settle(
    runtime: &mut HandyHspRuntime,
    effect: &HspEffect,
    tail: i32,
    playing: bool,
) -> HspCompletionRecord {
    runtime
        .complete_settled(
            effect,
            HspSettledTransport::Response {
                status: 200,
                body: &response(tail, playing),
            },
            100,
            Some(&facts()),
        )
        .unwrap()
}
fn offer(runtime: &mut HandyHspRuntime, start: u32, sequence: u64) {
    for offset in 0..2 {
        runtime
            .offer(
                &binding(1).identity().live,
                HspSample {
                    point: HspPoint::new(start + offset * 100, 50).unwrap(),
                    source_sequence: sequence + u64::from(offset),
                    captured_at_ns: 100,
                    accepted_at_ns: 100,
                    continuous_from_previous: true,
                },
                100,
                &facts(),
            )
            .unwrap();
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    journal: DeviceEffectJournal,
    runtime: HandyHspRuntime,
    stream: HspStreamReceipt,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
        let stream = journal.prepare_hsp_stream(&record()).unwrap();
        Self {
            dir,
            journal,
            runtime: runtime(),
            stream,
        }
    }
    fn next(&mut self) -> (HspEffect, PreparedHspReceipt) {
        let effect = self
            .runtime
            .next_effect(100, Some(&facts()))
            .unwrap()
            .unwrap();
        let prepared = self
            .journal
            .prepare_hsp_effect(&self.stream, &effect)
            .unwrap();
        (effect, prepared)
    }
    fn acknowledge(
        &mut self,
        effect: &HspEffect,
        prepared: &PreparedHspReceipt,
        tail: i32,
        playing: bool,
    ) -> ClaimedHspReceipt {
        let claim = self.journal.claim_hsp(prepared, effect).unwrap();
        let completion = settle(&mut self.runtime, effect, tail, playing);
        assert!(matches!(
            completion.outcome,
            HspObservedOutcome::Acknowledged(_)
        ));
        self.journal.complete_hsp(&claim, &completion).unwrap();
        claim
    }
    fn setup(&mut self) {
        let (effect, p) = self.next();
        self.acknowledge(&effect, &p, -1, false);
    }
}
#[test]
fn ordered_setup_multiple_appends_explicit_play_stop_and_original_status() {
    let mut f = Fixture::new();
    f.setup();
    for i in 0..2 {
        offer(&mut f.runtime, i * 200, u64::from(i) * 2 + 1);
        let (effect, p) = f.next();
        let original = effect.request().body().unwrap().to_vec();
        f.acknowledge(&effect, &p, i as i32 * 2 + 1, false);
        let replay = f.journal.prepare_hsp_effect(&f.stream, &effect).unwrap();
        assert_eq!(replay, p);
        assert!(f.journal.claim_hsp(&replay, &effect).is_err());
        assert_eq!(
            f.journal.hsp_effect(p.id()).unwrap().record.request.body,
            Some(original)
        );
    }
    f.runtime.request_play(100, &facts()).unwrap();
    let (play, p) = f.next();
    assert_eq!(play.kind(), HspEffectKind::Play);
    f.acknowledge(&play, &p, 3, true);
    f.runtime.stop();
    let (stop, p) = f.next();
    f.acknowledge(&stop, &p, 3, false);
    assert_eq!(
        f.journal.hsp_stream(f.stream.id()).unwrap().state,
        HspStreamJournalState::Custody
    );
    assert!(f.journal.cancel_hsp_stream(&f.stream).is_err());
    assert!(ensure_legacy_route_available(
        &f.journal.db,
        &record().route_sha256,
        &record().route_namespace_sha256,
        &record().registration.device_id
    )
    .is_err());
}
#[test]
fn exact_clones_cannot_claim_twice_or_match_foreign_sealed_effect() {
    let mut f = Fixture::new();
    let (effect, p) = f.next();
    let mut foreign = HandyHspRuntime::new(
        binding(2),
        HspPolicy {
            max_buffered_points: 8,
            device_capacity_points: 8,
            max_chunk_points: 2,
            min_lead_ms: 10,
            max_future_ms: 1000,
            max_sample_age_ns: 1000,
            max_gap_ms: 200,
        },
    )
    .unwrap();
    let foreign_facts = HspDispatchFacts::new(
        binding(2),
        HspPlayReference::new(
            100,
            HspPhaseWindow {
                earliest_ms: -100,
                latest_ms: -100,
            },
            -100,
            10100,
        )
        .unwrap(),
        HspAuthorityWindow::new(2000, 2000, 2000),
    )
    .unwrap();
    let foreign = foreign
        .next_effect(100, Some(&foreign_facts))
        .unwrap()
        .unwrap();
    assert!(f.journal.claim_hsp(&p, &foreign).is_err());
    f.journal.claim_hsp(&p, &effect).unwrap();
    assert!(f.journal.claim_hsp(&p.clone(), &effect.clone()).is_err());
    assert!(f.journal.cancel_hsp_effect(&p).is_err());
}
#[test]
fn unknown_is_sticky_late_annotation_once_and_only_stop_follows() {
    let mut f = Fixture::new();
    f.setup();
    offer(&mut f.runtime, 0, 1);
    let (append, p) = f.next();
    let claim = f.journal.claim_hsp(&p, &append).unwrap();
    let unknown = f
        .runtime
        .complete_settled(&append, HspSettledTransport::Unknown, 100, Some(&facts()))
        .unwrap();
    f.journal.complete_hsp(&claim, &unknown).unwrap();
    let late = settle(&mut f.runtime, &append, 1, false);
    let observation = f.journal.complete_hsp(&claim, &late).unwrap();
    assert_eq!(observation.state, HspJournalState::Unknown);
    assert!(matches!(
        observation.late_observation,
        Some(HspOutcomeRecord::Acknowledged(_))
    ));
    f.journal.complete_hsp(&claim, &unknown).unwrap();
    let mut changed = late.clone();
    changed.outcome = changed.late_observation.unwrap();
    changed.late_observation = None;
    assert!(f.journal.complete_hsp(&claim, &changed).is_err());
    let mut different = late;
    different.late_observation = Some(HspObservedOutcome::Unknown);
    assert!(f.journal.complete_hsp(&claim, &different).is_err());
    // A different runtime with the same binding cannot continue this history.
    let mut other = runtime();
    let e = other.next_effect(100, Some(&facts())).unwrap().unwrap();
    settle(&mut other, &e, -1, false);
    offer(&mut other, 0, 1);
    let e = other.next_effect(100, Some(&facts())).unwrap().unwrap();
    settle(&mut other, &e, 1, false);
    offer(&mut other, 200, 3);
    let e = other.next_effect(100, Some(&facts())).unwrap().unwrap();
    assert!(f.journal.prepare_hsp_effect(&f.stream, &e).is_err());
    let (stop, p) = f.next();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    f.acknowledge(&stop, &p, 1, false);
    assert_eq!(
        f.journal.hsp_effect(claim.id()).unwrap().state,
        HspJournalState::Unknown
    );
}
#[test]
fn claimed_transport_must_settle_before_stop_and_completion_can_retry_metadata() {
    let mut f = Fixture::new();
    let (effect, p) = f.next();
    let claim = f.journal.claim_hsp(&p, &effect).unwrap();
    let completion = f
        .runtime
        .complete_settled(&effect, HspSettledTransport::Unknown, 100, Some(&facts()))
        .unwrap();
    let stop = f.runtime.next_effect(100, None).unwrap().unwrap();
    assert!(f.journal.prepare_hsp_effect(&f.stream, &stop).is_err());
    let writer = Connection::open(f.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let begin = std::time::Instant::now();
    assert!(f.journal.complete_hsp(&claim, &completion).is_err());
    assert!(begin.elapsed() < Duration::from_millis(750));
    assert!(f.journal.claim_hsp(&p, &effect).is_err());
    writer.execute_batch("ROLLBACK").unwrap();
    f.journal.complete_hsp(&claim, &completion).unwrap();
    f.journal.prepare_hsp_effect(&f.stream, &stop).unwrap();
}
#[test]
fn definitely_unclaimed_cancellation_releases_route_without_restoring_authority() {
    let mut f = Fixture::new();
    assert!(has_device_custody(&f.journal.db, &record().registration.device_id).unwrap());
    let (effect, p) = f.next();
    f.journal.cancel_hsp_effect(&p).unwrap();
    assert!(!has_device_custody(&f.journal.db, &record().registration.device_id).unwrap());
    assert_eq!(
        f.journal.hsp_stream(f.stream.id()).unwrap().state,
        HspStreamJournalState::Cancelled
    );
    assert!(f.journal.claim_hsp(&p, &effect).is_err());
    ensure_legacy_route_available(
        &f.journal.db,
        &record().route_sha256,
        &record().route_namespace_sha256,
        &record().registration.device_id,
    )
    .unwrap();
    let mut next = record();
    next.request_id = RequestId::new("replacement-request").unwrap();
    next.binding = HspBindingRecord::capture(&binding(2));
    f.journal.prepare_hsp_stream(&next).unwrap();
}
#[test]
fn empty_reservation_can_cancel_but_original_request_cannot_be_rebound() {
    let mut f = Fixture::new();
    let mut changed = record();
    changed.profile_sha256 = "f".repeat(64);
    assert!(f.journal.prepare_hsp_stream(&changed).is_err());
    f.journal.cancel_hsp_stream(&f.stream).unwrap();
    assert_eq!(
        f.journal.hsp_stream(f.stream.id()).unwrap().state,
        HspStreamJournalState::Cancelled
    );
    assert!(f.journal.prepare_hsp_stream(&changed).is_err());
}
#[test]
fn restart_preserves_original_claim_and_fences_both_old_receipts_and_writers() {
    let mut f = Fixture::new();
    let (effect, p) = f.next();
    let claim = f.journal.claim_hsp(&p, &effect).unwrap();
    let mut newer = DeviceEffectJournal::open(
        f.dir.path(),
        DeviceSessionId::new("synthetic-next-boot").unwrap(),
    )
    .unwrap();
    assert_eq!(
        newer.hsp_effect(p.id()).unwrap().state,
        HspJournalState::Claimed
    );
    assert!(newer.claim_hsp(&p, &effect).is_err());
    assert!(f.journal.claim_hsp(&p, &effect).is_err());
    let c = HspCompletionRecord {
        effect,
        outcome: HspObservedOutcome::Unknown,
        late_observation: None,
    };
    assert!(newer.complete_hsp(&claim, &c).is_err());
    assert!(f.journal.complete_hsp(&claim, &c).is_err());
    assert!(newer.prepare_hsp_stream(&record()).is_err());
}
#[test]
fn atomic_prepare_rolls_back_stream_on_foreign_effect_and_preserves_writer_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    let effect = runtime().next_effect(100, Some(&facts())).unwrap().unwrap();
    let mut changed = record();
    changed.binding = HspBindingRecord::capture(&binding(2));
    assert!(journal
        .prepare_hsp_stream_effect(&changed, &effect)
        .is_err());
    assert_eq!(catalog(&journal.db).unwrap().census.streams, 0);
    journal
        .db
        .busy_timeout(Duration::from_millis(1750))
        .unwrap();
    let writer = Connection::open(dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let begin = std::time::Instant::now();
    assert!(journal
        .prepare_hsp_stream_effect(&record(), &effect)
        .is_err());
    assert!(begin.elapsed() < Duration::from_millis(750));
    assert_eq!(
        journal
            .db
            .query_row("PRAGMA busy_timeout", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        1750
    );
    writer.execute_batch("ROLLBACK").unwrap();
    journal
        .prepare_hsp_stream_effect(&record(), &effect)
        .unwrap();
}
#[test]
fn independent_device_route_and_namespace_exclusion_is_fail_closed() {
    let mut f = Fixture::new();
    for mut other in [record(), record(), record()] {
        other.request_id = RequestId::new("other").unwrap();
        assert!(f.journal.prepare_hsp_stream(&other).is_err());
    }
    let mut other = record();
    other.request_id = RequestId::new("other").unwrap();
    other.route_sha256 = "1".repeat(64);
    assert!(f.journal.prepare_hsp_stream(&other).is_err());
    other.registration.device_id = DeviceId::new("other-device").unwrap();
    other.route_namespace_sha256 = "2".repeat(64);
    assert!(f.journal.prepare_hsp_stream(&other).is_err());
    assert!(ensure_legacy_route_available(
        &f.journal.db,
        &other.route_sha256,
        &other.route_namespace_sha256,
        &other.registration.device_id
    )
    .is_err());
    other.route_namespace_sha256 = record().route_namespace_sha256;
    other.binding = HspBindingRecord::capture(&binding(2));
    f.journal.prepare_hsp_stream(&other).unwrap();
}
#[test]
fn startup_and_reads_reject_record_membership_census_schema_and_outcome_tampering() {
    for sql in [
        "UPDATE hsp_effects SET record=replace(record,'/hsp/setup','/hsp/other')",
        "UPDATE hsp_effects SET stream=printf('%064d',0)",
        "UPDATE hsp_effects SET state='claimed'",
        "UPDATE hsp_streams SET state='cancelled'",
        "UPDATE hsp_census SET ordinary=ordinary+1",
        "UPDATE hsp_effects SET late=printf('%03000d',0)",
        "CREATE TRIGGER hsp_injected AFTER INSERT ON hsp_effects BEGIN SELECT 1; END",
        "CREATE TABLE hsp_shadow(value TEXT)",
        "CREATE TABLE HSP_SHADOW(value TEXT)",
        "DROP INDEX hsp_effect_sequence",
        "PRAGMA user_version=13",
    ] {
        let mut f = Fixture::new();
        let (_, p) = f.next();
        f.journal.db.execute_batch(sql).unwrap();
        assert!(f.journal.hsp_effect(p.id()).is_err(), "{sql}");
        assert!(
            DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("restart").unwrap())
                .is_err(),
            "{sql}"
        );
    }
}
#[test]
fn migration_from_nine_preserves_legacy_rows_and_refuses_preexisting_hsp_objects() {
    let dir = tempfile::tempdir().unwrap();
    let journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; PRAGMA user_version=9;").unwrap();
    let original: String = journal
        .db
        .query_row("SELECT boot FROM owner", [], |r| r.get(0))
        .unwrap();
    drop(journal);
    let migrated = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    assert_eq!(
        migrated
            .db
            .query_row("SELECT boot FROM owner", [], |r| r.get::<_, String>(0))
            .unwrap(),
        original
    );
    assert_eq!(
        migrated
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    migrated.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; CREATE TABLE hsp_intruder(value TEXT); PRAGMA user_version=9;").unwrap();
    drop(migrated);
    assert!(DeviceEffectJournal::open(dir.path(), record().boot).is_err());
    let db = Connection::open_with_flags(
        dir.path().join("device-effects/journal.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        9
    );
}

#[test]
fn external_writer_invalidates_cache_and_failed_transaction_cannot_poison_it() {
    let mut f = Fixture::new();
    f.setup();
    // Prime cache, then mutate from a different connection without changing the
    // journal connection's total_changes counter.
    f.journal.hsp_stream(f.stream.id()).unwrap();
    let writer = Connection::open(f.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer
        .execute("UPDATE hsp_census SET ordinary=ordinary+1", [])
        .unwrap();
    assert!(f.journal.hsp_stream(f.stream.id()).is_err());
    writer
        .execute("UPDATE hsp_census SET ordinary=ordinary-1", [])
        .unwrap();
    f.journal.hsp_stream(f.stream.id()).unwrap();
    // Complete a first append only in the supplied-fact runtime, making the
    // next append skip an unpersisted ordinary sequence. The SQL insert and
    // census update occur before chain validation rejects and rolls them back.
    offer(&mut f.runtime, 0, 1);
    let missing = f.runtime.next_effect(100, Some(&facts())).unwrap().unwrap();
    settle(&mut f.runtime, &missing, 1, false);
    offer(&mut f.runtime, 200, 3);
    let skipped = f.runtime.next_effect(100, Some(&facts())).unwrap().unwrap();
    assert!(f.journal.prepare_hsp_effect(&f.stream, &skipped).is_err());
    assert_eq!(
        f.journal
            .hsp_stream(f.stream.id())
            .unwrap()
            .effect_ids
            .len(),
        1
    );
    assert_eq!(catalog(&f.journal.db).unwrap().census.ordinary, 1);
}
#[test]
fn ordinary_count_and_bytes_never_consume_reserved_stop_space() {
    for mut census in [
        Census {
            streams: STREAM_CAPACITY,
            ordinary: ORDINARY_CAPACITY,
            ..Census::default()
        },
        Census {
            streams: STREAM_CAPACITY,
            ordinary_bytes: ORDINARY_BYTES_LIMIT,
            ..Census::default()
        },
    ] {
        assert!(reserve_effect(
            &mut census,
            HspEffectRecordKind::Append,
            OUTCOME_RESERVATION + 100
        )
        .is_err());
        for _ in 0..STREAM_CAPACITY {
            reserve_effect(
                &mut census,
                HspEffectRecordKind::Stop,
                HSP_EFFECT_RECORD_LIMIT as u64 + OUTCOME_RESERVATION,
            )
            .unwrap();
        }
        assert_eq!(census.stops, STREAM_CAPACITY);
        assert!(reserve_effect(
            &mut census,
            HspEffectRecordKind::Stop,
            OUTCOME_RESERVATION + 100
        )
        .is_err());
    }
}
#[test]
fn stream_capacity_still_leaves_existing_stream_its_stop() {
    let mut f = Fixture::new();
    for index in 1..STREAM_CAPACITY {
        let mut value = record();
        value.request_id = RequestId::new(format!("stream-{index}")).unwrap();
        value.binding = HspBindingRecord::capture(&binding(index + 1));
        value.registration.device_id = DeviceId::new(format!("device-{index}")).unwrap();
        value.route_sha256 = format!("{index:064x}");
        f.journal.prepare_hsp_stream(&value).unwrap();
    }
    let mut value = record();
    value.request_id = RequestId::new("overflow-stream").unwrap();
    value.binding = HspBindingRecord::capture(&binding(STREAM_CAPACITY + 1));
    value.registration.device_id = DeviceId::new("overflow-device").unwrap();
    value.route_sha256 = "f".repeat(64);
    assert!(f.journal.prepare_hsp_stream(&value).is_err());
    f.runtime.stop();
    let (stop, p) = f.next();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    f.journal.claim_hsp(&p, &stop).unwrap();
    assert_eq!(catalog(&f.journal.db).unwrap().census.stops, 1);
}
fn legacy_record() -> SetupEffectRecord {
    let hsp = record();
    let artifact = |character: char| {
        let sha = character.to_string().repeat(64);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha.clone()).unwrap(),
            sha256: sha,
            byte_len: 40,
        }
    };
    SetupEffectRecord {
        boot: hsp.boot,
        device_session: DeviceSessionId::new("legacy-session").unwrap(),
        actor_session: hsp.actor_session,
        request_id: RequestId::new("legacy-setup").unwrap(),
        hosting_request_id: RequestId::new("legacy-hosted").unwrap(),
        project_id: ProjectId::new("synthetic-project").unwrap(),
        revision: RevisionId::new(1),
        registration: hsp.registration,
        qualification: HandyQualificationBinding {
            dossier_id: ArtifactId::new("d".repeat(64)).unwrap(),
            revision: 2,
        },
        master_sha256: "c".repeat(64),
        script: artifact('d'),
        hosted: artifact('e'),
        route_sha256: hsp.route_sha256,
        route_namespace_sha256: Some(hsp.route_namespace_sha256),
        acknowledgment_required: true,
        preflight_mode: Some(HandyDeviceMode {
            mode: 1,
            mode_session_id: 10,
        }),
    }
}
#[test]
fn legacy_and_hsp_admission_exclude_each_other_and_foreign_namespace() {
    let mut f = Fixture::new();
    assert!(f.journal.prepare_setup(legacy_record()).is_err());
    f.journal.cancel_hsp_stream(&f.stream).unwrap();
    let setup = f.journal.prepare_setup(legacy_record()).unwrap();
    let mut hsp = record();
    hsp.request_id = RequestId::new("new-hsp-request").unwrap();
    hsp.binding = HspBindingRecord::capture(&binding(2));
    assert!(f.journal.prepare_hsp_stream(&hsp).is_err());
    hsp.registration.device_id = DeviceId::new("unrelated-hsp-device").unwrap();
    hsp.route_sha256 = "f".repeat(64);
    hsp.route_namespace_sha256 = "1".repeat(64);
    assert!(f.journal.prepare_hsp_stream(&hsp).is_err());
    f.journal.cancel_setup_unclaimed(setup.id()).unwrap();
    f.journal.prepare_hsp_stream(&hsp).unwrap();
}
#[test]
fn migration_preserves_populated_legacy_record_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    let prepared = journal.prepare_setup(legacy_record()).unwrap();
    let claim = journal.claim_setup(&prepared).unwrap();
    journal
        .complete_setup(&claim, SetupCompletion::Unknown)
        .unwrap();
    let original: (String, String) = journal
        .db
        .query_row(
            "SELECT record,state FROM setup_effects WHERE id=?1",
            [prepared.id()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; PRAGMA user_version=9;").unwrap();
    drop(journal);
    let migrated =
        DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("migration-boot").unwrap())
            .unwrap();
    let retained: (String, String) = migrated
        .db
        .query_row(
            "SELECT record,state FROM setup_effects WHERE id=?1",
            [prepared.id()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(retained, original);
}

#[test]
fn rehashed_append_chain_tampering_is_rejected_by_original_membership() {
    let mut f = Fixture::new();
    f.setup();
    for i in 0..2 {
        offer(&mut f.runtime, i * 200, u64::from(i) * 2 + 1);
        let (effect, p) = f.next();
        f.acknowledge(&effect, &p, i as i32 * 2 + 1, false);
    }
    let all = catalog(&f.journal.db).unwrap();
    let id = all.streams[f.stream.id()].effect_ids.last().unwrap();
    let mut altered = all.effects[id].record.clone();
    // Keep the wire points and tail unchanged while lying about the original
    // source sequence across the chunk boundary. Per-record validation permits
    // ascending sample sequences; the persisted chain rejects overlap.
    for (index, sample) in altered
        .chunk
        .as_mut()
        .unwrap()
        .samples
        .iter_mut()
        .enumerate()
    {
        sample.source_sequence = index as u64 + 1;
    }
    altered.validate(&record()).unwrap();
    let new_id = altered.id().unwrap();
    let prior = &all.effects[id];
    let outcome = outcome_text(prior.outcome.as_ref().unwrap()).unwrap();
    f.journal.db.execute("UPDATE hsp_effects SET id=?1,record=?2,outcome_sha256=?3,lifecycle_sha256=?4 WHERE id=?5",params![new_id,String::from_utf8(altered.to_json().unwrap()).unwrap(),outcome_hash(&new_id,&outcome,false).unwrap(),lifecycle(&new_id,"acknowledged",Some(&outcome),None).unwrap(),id]).unwrap();
    // Length changes are reflected in the census so the failure reaches chain
    // validation instead of merely detecting an inconsistent byte counter.
    let difference =
        altered.to_json().unwrap().len() as i64 - prior.record.to_json().unwrap().len() as i64;
    f.journal
        .db
        .execute(
            "UPDATE hsp_census SET ordinary_bytes=ordinary_bytes+?1",
            [difference],
        )
        .unwrap();
    assert!(f
        .journal
        .hsp_stream(f.stream.id())
        .unwrap_err()
        .to_string()
        .contains("continuity"));
}

#[test]
fn a_runtime_binding_has_one_original_stream_parent_even_after_cancellation() {
    let mut f = Fixture::new();
    let mut changed = record();
    changed.request_id = RequestId::new("changed-request").unwrap();
    changed.registration.device_id = DeviceId::new("changed-device").unwrap();
    changed.route_sha256 = "f".repeat(64);
    assert!(f
        .journal
        .prepare_hsp_stream(&changed)
        .unwrap_err()
        .to_string()
        .contains("runtime binding"));
    f.journal.cancel_hsp_stream(&f.stream).unwrap();
    assert_eq!(f.journal.prepare_hsp_stream(&record()).unwrap(), f.stream);
    assert!(f
        .journal
        .prepare_hsp_stream(&changed)
        .unwrap_err()
        .to_string()
        .contains("runtime binding"));
    changed.binding = HspBindingRecord::capture(&binding(2));
    f.journal.prepare_hsp_stream(&changed).unwrap();
}

#[test]
fn rehashed_cross_family_custody_injected_by_external_writer_fails_reads_and_startup() {
    for variant in 0..4 {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
        journal.prepare_setup(legacy_record()).unwrap();
        // Prime the connection-specific cache before another connection injects
        // internally valid, rehashed HSP history past the admission guard.
        journal.hsp_read(|_| Ok(())).unwrap();
        let mut value = record();
        if variant == 1 || variant == 3 {
            value.registration.device_id = DeviceId::new("alias-device").unwrap();
        }
        if variant >= 2 {
            value.route_sha256 = "f".repeat(64);
        }
        if variant == 3 {
            value.route_namespace_sha256 = "1".repeat(64);
        }
        let id = value.id().unwrap();
        let text = String::from_utf8(value.to_json().unwrap()).unwrap();
        let mut writer =
            Connection::open(dir.path().join("device-effects/journal.sqlite3")).unwrap();
        let tx = writer
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        tx.execute(
            "INSERT INTO hsp_streams VALUES(?1,?2,?3,?4,?5,?6,'prepared')",
            params![
                id,
                value.request_identity().unwrap(),
                value.route_sha256,
                value.route_namespace_sha256,
                value.registration.device_id.as_str(),
                text
            ],
        )
        .unwrap();
        tx.execute(
            "UPDATE hsp_census SET streams=1,stream_bytes=?1",
            [text.len() as u64],
        )
        .unwrap();
        tx.commit().unwrap();
        assert!(journal.hsp_stream(&id).is_err(), "variant {variant}");
        assert!(
            DeviceEffectJournal::open(
                dir.path(),
                DeviceSessionId::new("cross-custody-restart").unwrap()
            )
            .is_err(),
            "variant {variant}"
        );
    }
}

#[test]
fn cancellation_proof_binds_exact_prepared_effect_and_is_idempotent() {
    let mut f = Fixture::new();
    let (effect, p) = f.next();
    let mut other = runtime();
    let setup = other.next_effect(100, Some(&facts())).unwrap().unwrap();
    settle(&mut other, &setup, -1, false);
    offer(&mut other, 0, 1);
    let foreign = other.next_effect(100, Some(&facts())).unwrap().unwrap();
    assert!(f.journal.try_cancel_hsp_effect(&p, &foreign).is_err());
    assert_eq!(
        f.journal.hsp_effect(p.id()).unwrap().state,
        HspJournalState::Prepared
    );
    let proof = f.journal.try_cancel_hsp_effect(&p, &effect).unwrap();
    assert!(proof.matches_effect(&effect));
    assert!(!proof.matches_effect(&foreign));
    assert_eq!(proof.id(), p.id());
    assert_eq!(proof.stream_id(), p.stream_id());
    assert_eq!(
        f.journal
            .try_cancel_hsp_effect(&p.clone(), &effect.clone())
            .unwrap(),
        proof
    );
    assert!(f.journal.claim_hsp(&p, &effect).is_err());
    assert_eq!(
        f.journal.hsp_effect(p.id()).unwrap().state,
        HspJournalState::Cancelled
    );
    assert!(!has_device_custody(&f.journal.db, &record().registration.device_id).unwrap());
}
#[test]
fn cancellation_proof_refuses_cloned_receipt_after_claim_and_both_terminal_outcomes() {
    for outcome in 0..3 {
        let mut f = Fixture::new();
        let (effect, p) = f.next();
        let replay = p.clone();
        let claim = f.journal.claim_hsp(&p, &effect).unwrap();
        if outcome > 0 {
            let completion = if outcome == 1 {
                settle(&mut f.runtime, &effect, -1, false)
            } else {
                f.runtime
                    .complete_settled(&effect, HspSettledTransport::Unknown, 100, Some(&facts()))
                    .unwrap()
            };
            f.journal.complete_hsp(&claim, &completion).unwrap();
        }
        let before = f.journal.hsp_effect(p.id()).unwrap();
        assert!(f.journal.try_cancel_hsp_effect(&replay, &effect).is_err());
        let after = f.journal.hsp_effect(p.id()).unwrap();
        assert_eq!(before.state, after.state);
        assert_eq!(before.outcome, after.outcome);
        assert!(has_device_custody(&f.journal.db, &record().registration.device_id).unwrap());
    }
}
#[test]
fn cancellation_proof_refuses_foreign_boot_and_writer_contention() {
    let mut f = Fixture::new();
    let (effect, p) = f.next();
    let writer = Connection::open(f.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let begin = std::time::Instant::now();
    assert!(f.journal.try_cancel_hsp_effect(&p, &effect).is_err());
    assert!(begin.elapsed() < Duration::from_millis(750));
    assert_eq!(
        f.journal.hsp_effect(p.id()).unwrap().state,
        HspJournalState::Prepared
    );
    writer.execute_batch("ROLLBACK").unwrap();
    let mut newer = DeviceEffectJournal::open(
        f.dir.path(),
        DeviceSessionId::new("cancellation-next-boot").unwrap(),
    )
    .unwrap();
    assert!(newer.try_cancel_hsp_effect(&p, &effect).is_err());
    assert!(f.journal.try_cancel_hsp_effect(&p, &effect).is_err());
    assert_eq!(
        newer.hsp_effect(p.id()).unwrap().state,
        HspJournalState::Prepared
    );
}
#[test]
fn cancelling_known_unsent_append_retains_earlier_claim_custody() {
    let mut f = Fixture::new();
    f.setup();
    offer(&mut f.runtime, 0, 1);
    let (append, p) = f.next();
    let proof = f.journal.try_cancel_hsp_effect(&p, &append).unwrap();
    assert!(proof.matches_effect(&append));
    assert_eq!(
        f.journal.hsp_effect(p.id()).unwrap().state,
        HspJournalState::Cancelled
    );
    assert_eq!(
        f.journal.hsp_stream(f.stream.id()).unwrap().state,
        HspStreamJournalState::Custody
    );
    assert!(has_device_custody(&f.journal.db, &record().registration.device_id).unwrap());
    assert!(f.journal.cancel_hsp_stream(&f.stream).is_err());
    f.runtime.retire_unclaimed(&append, &proof).unwrap();
    assert_eq!(f.runtime.unclaimed_effect(), Some(&append));
    assert!(f.runtime.uncertain_effect().is_none());
    let (stop, prepared_stop) = f.next();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    f.acknowledge(&stop, &prepared_stop, -1, false);
    assert_eq!(
        f.journal.hsp_effect(p.id()).unwrap().state,
        HspJournalState::Cancelled
    );
    assert_eq!(
        f.journal.hsp_stream(f.stream.id()).unwrap().state,
        HspStreamJournalState::Custody
    );
}

include!("device_hsp_recovery_journal_tests.rs");

include!("device_hsp_provenance_journal_tests.rs");
