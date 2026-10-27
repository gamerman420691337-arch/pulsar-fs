use super::*;
fn fixture() -> (
    tempfile::TempDir,
    DeviceEffectJournal,
    ClockSyncEffectRecord,
) {
    let dir = tempfile::tempdir().unwrap();
    let c: HandyProgramCorrelation = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
    ))
    .unwrap();
    let mut l: HandyControllerReceipt = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/handy-controller.json"
    ))
    .unwrap();
    let i = c.inspection;
    l.lease.device_session = i.device_session.clone();
    let mut j = DeviceEffectJournal::open(dir.path(), i.engine_epoch.clone()).unwrap();
    let actor = SessionId::new("fixture-owner").unwrap();
    let parent = SetupEffectRecord {
        boot: i.engine_epoch.clone(),
        device_session: i.device_session.clone(),
        actor_session: actor.clone(),
        request_id: c.setup_request_id,
        hosting_request_id: i.hosting_request_id,
        project_id: i.project_id,
        revision: i.revision,
        registration: i.registration.clone(),
        qualification: i.accepted_qualification.clone().unwrap(),
        master_sha256: i.master_sha256,
        script: i.script,
        hosted: i.hosted,
        route_sha256: "9".repeat(64),
        route_namespace_sha256: Some("8".repeat(64)),
        acknowledgment_required: true,
        preflight_mode: Some(c.setup_mode),
    };
    let prepared = j.prepare_setup(parent).unwrap();
    let claim = j.claim_setup(&prepared).unwrap();
    j.complete_setup(&claim, SetupCompletion::Acknowledged(c.acknowledgment))
        .unwrap();
    let record = ClockSyncEffectRecord {
        actor_session: actor,
        request_id: RequestId::new("sync-fixture").unwrap(),
        original_controller: l.lease,
        registration: i.registration,
        preparation_qualification: i.accepted_qualification.unwrap(),
        setup_record_id: ArtifactId::new(prepared.id()).unwrap(),
        user_offset_before: HandyUserTimeOffset { offset_ms: 0 },
        route_sha256: "9".repeat(64),
        route_namespace_sha256: "8".repeat(64),
    };
    (dir, j, record)
}
fn ack() -> HandyClockSyncAcknowledgment {
    HandyClockSyncAcknowledgment {
        info: HandyDeviceTimeInfo {
            device_uptime_ms: 100,
            reported_clock_offset_ms: 125,
            last_sync_round_trip_ms: 10,
        },
        sent_boottime_ns: 1,
        received_boottime_ns: 2,
    }
}
#[test]
fn clock_sync_journal_single_claim_exact_completion_and_independent_setup_parent() {
    let (_dir, mut j, r) = fixture();
    let p = j.prepare_clock_sync(r.clone()).unwrap();
    assert!(j
        .clock_sync_route_available(
            &r.route_sha256,
            &r.route_namespace_sha256,
            &r.registration.device_id,
            None
        )
        .is_err());
    let c = j.claim_clock_sync(&p).unwrap();
    assert!(j.claim_clock_sync(&p).is_err());
    assert!(j.cancel_clock_sync_unclaimed(p.id()).is_err());
    j.clock_sync_route_available(
        &r.route_sha256,
        &r.route_namespace_sha256,
        &r.registration.device_id,
        Some(&c),
    )
    .unwrap();
    assert!(j
        .clock_sync_route_available(
            &"7".repeat(64),
            &r.route_namespace_sha256,
            &r.registration.device_id,
            Some(&c)
        )
        .is_err());
    j.complete_clock_sync(&c, ClockSyncCompletion::Acknowledged(ack()))
        .unwrap();
    j.complete_clock_sync(&c, ClockSyncCompletion::Acknowledged(ack()))
        .unwrap();
    assert!(j
        .complete_clock_sync(&c, ClockSyncCompletion::Unknown)
        .is_err());
    j.clock_sync_route_available(
        &r.route_sha256,
        &r.route_namespace_sha256,
        &r.registration.device_id,
        None,
    )
    .unwrap();
    assert!(j.setup_route_reserved(&r.route_sha256).unwrap());
    assert_eq!(
        j.clock_sync_request_history(&r.actor_session, &r.request_id)
            .unwrap()
            .unwrap()
            .acknowledgment,
        Some(ack())
    );
}
#[test]
fn clock_sync_journal_unknown_custody_survives_boot_and_alias_or_namespace_changes() {
    let (dir, mut j, r) = fixture();
    let p = j.prepare_clock_sync(r.clone()).unwrap();
    let c = j.claim_clock_sync(&p).unwrap();
    j.complete_clock_sync(&c, ClockSyncCompletion::Unknown)
        .unwrap();
    for (route, namespace, device) in [
        (
            &r.route_sha256,
            &r.route_namespace_sha256,
            DeviceId::new("alias").unwrap(),
        ),
        (
            &"7".repeat(64),
            &r.route_namespace_sha256,
            r.registration.device_id.clone(),
        ),
        (
            &"7".repeat(64),
            &"6".repeat(64),
            DeviceId::new("other").unwrap(),
        ),
    ] {
        assert!(j
            .clock_sync_route_available(route, namespace, &device, None)
            .is_err());
    }
    drop(j);
    let mut next =
        DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("next-boot").unwrap()).unwrap();
    assert!(next.claim_clock_sync(&p).is_err());
    assert!(next
        .complete_clock_sync(&c, ClockSyncCompletion::Unknown)
        .is_err());
    assert!(next
        .clock_sync_request_history(&r.actor_session, &r.request_id)
        .unwrap()
        .unwrap()
        .state
        .route_reserved());
    assert!(next
        .clock_sync_route_available(
            &r.route_sha256,
            &r.route_namespace_sha256,
            &r.registration.device_id,
            None
        )
        .is_err());
}
#[test]
fn clock_sync_journal_cancels_only_prepared_and_refuses_observation_corruption() {
    let (_dir, mut j, r) = fixture();
    let p = j.prepare_clock_sync(r.clone()).unwrap();
    j.cancel_clock_sync_unclaimed(p.id()).unwrap();
    j.cancel_clock_sync_unclaimed(p.id()).unwrap();
    assert!(j.claim_clock_sync(&p).is_err());
    let mut next = r.clone();
    next.request_id = RequestId::new("next-sync").unwrap();
    let p = j.prepare_clock_sync(next.clone()).unwrap();
    let c = j.claim_clock_sync(&p).unwrap();
    j.complete_clock_sync(&c, ClockSyncCompletion::Acknowledged(ack()))
        .unwrap();
    j.db.execute(
        "UPDATE clock_sync_effects SET observation_sha256=?1 WHERE id=?2",
        params!["0".repeat(64), p.id()],
    )
    .unwrap();
    assert!(j
        .clock_sync_request_history(&next.actor_session, &next.request_id)
        .is_err());
    assert!(DeviceEffectJournal::validate_existing(&j.db).is_err());
}
#[test]
fn clock_sync_journal_v3_migration_preserves_setup_history_and_rejects_unexpected_layout() {
    let (dir, j, r) = fixture();
    let before: String =
        j.db.query_row(
            "SELECT record FROM setup_effects WHERE id=?1",
            [r.setup_record_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    j.db.execute_batch(
        "DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members; DROP TABLE custody_resolution_census; DROP TABLE custody_resolutions; DROP TABLE setup_custody_routes; DROP TABLE clock_sync_custody_routes; DROP INDEX setup_active_route; CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'; DROP TABLE emergency_stop_observations; DROP TABLE emergency_stop_census; DROP TABLE play_operations; DROP TABLE clock_sync_effects; PRAGMA user_version=3;",
    )
    .unwrap();
    drop(j);
    let j =
        DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("next-boot").unwrap()).unwrap();
    assert_eq!(
        j.db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        j.db.query_row(
            "SELECT record FROM setup_effects WHERE id=?1",
            [r.setup_record_id.as_str()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        before
    );
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE other(route TEXT);CREATE INDEX clock_sync_active_route ON other(route);PRAGMA user_version=3;").unwrap();
    assert!(upgrade(&mut db, 3).is_err());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='clock_sync_effects'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn clock_sync_journal_failed_completion_keeps_claim_and_metadata_retry_never_reclaims() {
    let (_dir, mut j, r) = fixture();
    let p = j.prepare_clock_sync(r.clone()).unwrap();
    let c = j.claim_clock_sync(&p).unwrap();
    j.db.execute_batch("CREATE TEMP TRIGGER fail_sync BEFORE UPDATE ON clock_sync_effects WHEN NEW.state='acknowledged' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(j
        .complete_clock_sync(&c, ClockSyncCompletion::Acknowledged(ack()))
        .is_err());
    assert_eq!(
        j.clock_sync_request_history(&r.actor_session, &r.request_id)
            .unwrap()
            .unwrap()
            .state,
        HandyClockSyncState::Claimed
    );
    j.db.execute_batch("DROP TRIGGER fail_sync").unwrap();
    j.complete_clock_sync(&c, ClockSyncCompletion::Acknowledged(ack()))
        .unwrap();
    assert!(j.claim_clock_sync(&p).is_err());
}
