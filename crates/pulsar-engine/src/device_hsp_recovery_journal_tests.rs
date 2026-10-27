// Authored metadata fixtures only. No device or physical acceptance is implied.
fn link_for(stream: &HspStreamRecord) -> crate::handy_hsp_records::HspQualificationLinkRecord {
    crate::handy_hsp_records::HspQualificationLinkRecord {
        stream_id: stream.id().unwrap(),
        qualification: HandyQualificationBinding {
            dossier_id: ArtifactId::new(stream.qualification_sha256.clone()).unwrap(),
            revision: 2,
        },
        subject_sha256: "6".repeat(64),
        profile_sha256: stream.profile_sha256.clone(),
        admission_sha256: "7".repeat(64),
    }
}
fn linked_fixture() -> (Fixture, HspEffect, PreparedHspReceipt) {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    let mut runtime = runtime();
    let effect = runtime.next_effect(100, Some(&facts())).unwrap().unwrap();
    let (stream, prepared) = journal
        .prepare_hsp_stream_effect_linked(&record(), &effect, &link_for(&record()))
        .unwrap();
    (
        Fixture {
            dir,
            journal,
            runtime,
            stream,
        },
        effect,
        prepared,
    )
}
fn authored_emergency(f: &mut Fixture) -> crate::handy_hsp_records::HspEmergencyRecord {
    f.runtime.stop();
    let stop = f.runtime.next_effect(100, None).unwrap().unwrap();
    crate::handy_hsp_records::HspEmergencyRecord {
        stream_id: f.stream.id().into(),
        effect: HspEffectRecord::capture(f.stream.id(), &record(), &stop).unwrap(),
        outcome: HspOutcomeRecord::Unknown,
        late_observation: None,
        registration: record().registration,
        device: record().device,
        mode: HandyDeviceMode {
            mode: 4,
            mode_session_id: 8,
        },
        route_sha256: record().route_sha256,
        route_namespace_sha256: record().route_namespace_sha256,
        provenance: crate::handy_hsp_records::HspEmergencyProvenance::NativeNoPrewrite,
    }
}
fn hsp_snapshot(f: &Fixture) -> super::super::reconciliation::CustodySnapshot {
    f.journal
        .custody_snapshot(
            &record().registration.device_id,
            &record().route_sha256,
            &record().route_namespace_sha256,
        )
        .unwrap()
}
fn hsp_resolution(
    f: &Fixture,
    basis: super::super::reconciliation::CustodySnapshot,
    snapshot: super::super::reconciliation::CustodySnapshot,
) -> super::super::reconciliation::CustodyResolutionRecord {
    let stream = record();
    let mut plan = HandyRecoveryPlan {
        plan_id: ArtifactId::new("a".repeat(64)).unwrap(),
        snapshot_id: ArtifactId::new(basis.id().unwrap()).unwrap(),
        project_id: stream.binding.identity.live.project.clone(),
        selection: HandyRecoverySelection {
            device_id: stream.registration.device_id.clone(),
            registration_revision: stream.registration.revision,
            engine_epoch: f.journal.boot.clone(),
            device_session: DeviceSessionId::new("authored-recovery").unwrap(),
            qualification: link_for(&stream).qualification,
        },
        registration: stream.registration.clone(),
        route_sha256: stream.route_sha256.clone(),
        route_namespace_sha256: stream.route_namespace_sha256.clone(),
        counts: basis.counts(),
        projects: basis.projects(),
        devices: basis.devices(),
        playback_enabled: false,
        contract: HandyRecoveryQualification {
            transport: HandyQualificationTransport::Hsp,
            model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
            remote_state: HandyRecoveryRemoteState::Stopped,
            prior_command_settle_ns: 1_000_000_000,
            stopped_sample_separation_ns: 1_000_000_000,
            maximum_sample_span_ns: 1_000_000_000,
        },
    };
    plan.plan_id = plan.identity().unwrap();
    let correlation: HandyProgramCorrelation = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
    ))
    .unwrap();
    let mut observation = correlation.inspection.device;
    observation.identity = stream.device;
    observation.mode = HandyDeviceMode {
        mode: 4,
        mode_session_id: 8,
    };
    observation.remote_hssp = None;
    observation.remote_hsp = Some(HandyHspObservation {
        play_state: HandyRemotePlayState::Stopped,
        stream_id: 42,
        current_time_ms: -100,
        points: 0,
        max_points: 8,
        current_point: -1,
        playback_rate: 1.,
        looping: false,
        pause_on_starving: false,
        first_point_time_ms: 0,
        last_point_time_ms: 0,
        tail_point_stream_index: -1,
        tail_point_stream_index_threshold: 1,
    });
    super::super::reconciliation::CustodyResolutionRecord {
        actor_session: stream.actor_session,
        request_id: RequestId::new("authored-hsp-resolution").unwrap(),
        request_fingerprint: "a".repeat(64),
        plan,
        basis,
        snapshot,
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
    }
}
#[test]
fn hsp_original_qualification_link_is_atomic_immutable_and_not_guessed() {
    let (mut f, effect, prepared) = linked_fixture();
    let before = record().to_json().unwrap();
    let membership = f.journal.hsp_stream_membership(f.stream.id()).unwrap();
    assert_eq!(membership.qualification_link, Some(link_for(&record())));
    let (_, replay) = f
        .journal
        .prepare_hsp_stream_effect_linked(&record(), &effect, &link_for(&record()))
        .unwrap();
    assert_eq!(replay, prepared);
    let mut altered = link_for(&record());
    altered.admission_sha256 = "8".repeat(64);
    assert!(f
        .journal
        .prepare_hsp_stream_effect_linked(&record(), &effect, &altered)
        .is_err());
    assert_eq!(
        f.journal
            .hsp_stream(f.stream.id())
            .unwrap()
            .record
            .to_json()
            .unwrap(),
        before
    );
    let mut old = Fixture::new();
    let (old_effect, _) = old.next();
    assert!(old
        .journal
        .prepare_hsp_stream_effect_linked(&record(), &old_effect, &link_for(&record()))
        .is_err());
    assert!(hsp_snapshot(&old).historical_qualifications().is_err());
}
#[test]
fn hsp_v10_upgrade_preserves_unknown_and_does_not_infer_qualification() {
    let mut f = Fixture::new();
    let (effect, prepared) = f.next();
    let claim = f.journal.claim_hsp(&prepared, &effect).unwrap();
    let completion = f
        .runtime
        .complete_settled(&effect, HspSettledTransport::Unknown, 100, Some(&facts()))
        .unwrap();
    f.journal.complete_hsp(&claim, &completion).unwrap();
    let original = f.journal.hsp_stream_membership(f.stream.id()).unwrap();
    f.journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members;DROP TABLE hsp_emergency_observations;DROP TABLE hsp_qualification_links;DROP TABLE hsp_recovery_census;PRAGMA user_version=10;").unwrap();
    let id = f.stream.id().to_owned();
    drop(f.journal);
    let migrated =
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("new-boot").unwrap()).unwrap();
    assert_eq!(
        migrated
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    assert_eq!(migrated.hsp_stream_membership(&id).unwrap(), original);
    assert!(original.qualification_link.is_none());
}
#[test]
fn hsp_retrospective_emergency_is_separate_idempotent_and_restart_durable() {
    let (mut f, effect, prepared) = linked_fixture();
    let claim = f.journal.claim_hsp(&prepared, &effect).unwrap();
    let completion = f
        .runtime
        .complete_settled(&effect, HspSettledTransport::Unknown, 100, Some(&facts()))
        .unwrap();
    f.journal.complete_hsp(&claim, &completion).unwrap();
    let emergency = authored_emergency(&mut f);
    let stop = f.runtime.in_flight().unwrap().clone();
    let stop_prepared = f.journal.prepare_hsp_effect(&f.stream, &stop).unwrap();
    let receipt = recovery::persist_authored_emergency(&mut f.journal, &emergency).unwrap();
    assert_eq!(
        recovery::persist_authored_emergency(&mut f.journal, &emergency).unwrap(),
        receipt
    );
    assert_eq!(
        f.journal.hsp_effect(prepared.id()).unwrap().outcome,
        Some(HspOutcomeRecord::Unknown)
    );
    assert_eq!(
        f.journal.hsp_effect(stop_prepared.id()).unwrap().state,
        HspJournalState::Prepared
    );
    assert!(f.journal.claim_hsp(&stop_prepared, &stop).is_err());
    assert!(f
        .journal
        .hsp_stream_membership(f.stream.id())
        .unwrap()
        .emergency
        .is_some());
    let mut wrong = emergency.clone();
    wrong.route_sha256 = "1".repeat(64);
    assert!(recovery::persist_authored_emergency(&mut f.journal, &wrong).is_err());
    drop(f.journal);
    let mut reopened =
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("new-boot").unwrap()).unwrap();
    assert_eq!(
        reopened.hsp_emergency_for_stream(f.stream.id()).unwrap(),
        Some(emergency.clone())
    );
    assert_eq!(
        recovery::persist_authored_emergency(&mut reopened, &emergency).unwrap(),
        receipt
    );
}
#[test]
fn hsp_resolution_releases_exact_custody_and_forbids_old_receipts() {
    let (mut f, effect, prepared) = linked_fixture();
    let claim = f.journal.claim_hsp(&prepared, &effect).unwrap();
    let completion = f
        .runtime
        .complete_settled(&effect, HspSettledTransport::Unknown, 100, Some(&facts()))
        .unwrap();
    f.journal.complete_hsp(&claim, &completion).unwrap();
    let basis = hsp_snapshot(&f);
    let emergency = authored_emergency(&mut f);
    recovery::persist_authored_emergency(&mut f.journal, &emergency).unwrap();
    let snapshot = hsp_snapshot(&f);
    basis.extends_with_stops(&snapshot).unwrap();
    let resolution = hsp_resolution(&f, basis, snapshot.clone());
    let id = f.journal.record_resolution(&resolution).unwrap();
    assert_eq!(f.journal.record_resolution(&resolution).unwrap(), id);
    let proof = f
        .journal
        .hsp_resolution_receipt(&id, &snapshot.hsp_streams)
        .unwrap();
    assert!(proof.matches_memberships(&snapshot.hsp_streams));
    assert!(proof.matches_target(
        &record().registration.device_id,
        &record().route_sha256,
        &record().route_namespace_sha256
    ));
    assert!(!proof.matches_target(
        &DeviceId::new("foreign-selector").unwrap(),
        &record().route_sha256,
        &record().route_namespace_sha256
    ));
    assert!(proof.contains_exact_memberships(&snapshot.hsp_streams));
    assert!(!proof.contains_exact_memberships(&[
        snapshot.hsp_streams[0].clone(),
        snapshot.hsp_streams[0].clone()
    ]));
    assert!(hsp_snapshot(&f).hsp_streams.is_empty());
    assert!(!has_device_custody(&f.journal.db, &record().registration.device_id).unwrap());
    ensure_legacy_route_available(
        &f.journal.db,
        &record().route_sha256,
        &record().route_namespace_sha256,
        &record().registration.device_id,
    )
    .unwrap();
    assert!(f.journal.claim_hsp(&prepared, &effect).is_err());
    assert!(f.journal.prepare_hsp_effect(&f.stream, &effect).is_err());
    assert!(f.journal.cancel_hsp_effect(&prepared).is_err());
    assert!(f.journal.complete_hsp(&claim, &completion).is_err());
    assert_eq!(
        f.journal.hsp_effect(prepared.id()).unwrap().outcome,
        Some(HspOutcomeRecord::Unknown)
    );
    assert!(f
        .journal
        .hsp_resolution_receipt(&"0".repeat(64), &snapshot.hsp_streams)
        .is_err());
    let mut next = record();
    next.request_id = RequestId::new("new-after-resolution").unwrap();
    next.binding = HspBindingRecord::capture(&binding(2));
    f.journal.prepare_hsp_stream(&next).unwrap();
}
#[test]
fn hsp_recovery_rejects_wrong_transport_foreign_route_and_changed_membership() {
    let (mut f, effect, prepared) = linked_fixture();
    f.acknowledge(&effect, &prepared, -1, false);
    let basis = hsp_snapshot(&f);
    let mut resolution = hsp_resolution(&f, basis.clone(), basis.clone());
    resolution.plan.contract.transport = HandyQualificationTransport::Hssp;
    resolution.plan.plan_id = resolution.plan.identity().unwrap();
    assert!(f.journal.record_resolution(&resolution).is_err());
    assert!(f
        .journal
        .custody_snapshot(
            &record().registration.device_id,
            &"1".repeat(64),
            &record().route_namespace_sha256
        )
        .is_err());
    assert!(f
        .journal
        .custody_snapshot(
            &record().registration.device_id,
            &record().route_sha256,
            &"1".repeat(64)
        )
        .is_err());
    let mut changed = basis.hsp_streams[0].clone();
    changed.effects[0].late_sha256 = Some("1".repeat(64));
    assert!(basis.hsp_streams[0].extends_with_stops(&changed).is_err());
}
#[test]
fn hsp_recovery_census_and_link_integrity_fail_closed() {
    for sql in [
        "UPDATE hsp_recovery_census SET links=0",
        "UPDATE hsp_qualification_links SET id=printf('%064d',0)",
        "DELETE FROM hsp_qualification_links",
    ] {
        let (f, _, _) = linked_fixture();
        f.journal.db.execute_batch(sql).unwrap();
        assert!(DeviceEffectJournal::validate_existing(&f.journal.db).is_err());
    }
}

#[test]
fn hsp_late_observation_invalidates_original_recovery_membership() {
    let (mut f, effect, prepared) = linked_fixture();
    let claim = f.journal.claim_hsp(&prepared, &effect).unwrap();
    let original = f
        .runtime
        .complete_settled(&effect, HspSettledTransport::Unknown, 100, Some(&facts()))
        .unwrap();
    f.journal.complete_hsp(&claim, &original).unwrap();
    let basis = hsp_snapshot(&f);
    let late = settle(&mut f.runtime, &effect, -1, false);
    assert_eq!(late.outcome, HspObservedOutcome::Unknown);
    assert!(late.late_observation.is_some());
    f.journal.complete_hsp(&claim, &late).unwrap();
    let current = hsp_snapshot(&f);
    assert!(basis.extends_with_stops(&current).is_err());
    assert!(f
        .journal
        .record_resolution(&hsp_resolution(&f, basis, current))
        .is_err());
    assert_eq!(
        f.journal.hsp_effect(prepared.id()).unwrap().outcome,
        Some(HspOutcomeRecord::Unknown)
    );
}
#[test]
fn hsp_recovery_bounds_and_resolution_membership_are_checked_on_reopen() {
    let (mut f, effect, prepared) = linked_fixture();
    f.acknowledge(&effect, &prepared, -1, false);
    assert!(f
        .journal
        .db
        .execute_batch("UPDATE hsp_recovery_census SET links=65")
        .is_err());
    let snapshot = hsp_snapshot(&f);
    let resolution = hsp_resolution(&f, snapshot.clone(), snapshot);
    f.journal.record_resolution(&resolution).unwrap();
    f.journal
        .db
        .execute_batch("UPDATE hsp_resolution_members SET membership_sha256=printf('%064d',0)")
        .unwrap();
    assert!(DeviceEffectJournal::validate_existing(&f.journal.db).is_err());
    drop(f.journal);
    assert!(
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("new-boot").unwrap()).is_err()
    );
}

#[test]
fn hsp_drain_can_cancel_original_unclaimed_append_without_losing_membership() {
    let (mut f, effect, prepared) = linked_fixture();
    f.acknowledge(&effect, &prepared, -1, false);
    offer(&mut f.runtime, 0, 1);
    let (_, append) = f.next();
    let basis = hsp_snapshot(&f);
    f.journal.cancel_hsp_effect(&append).unwrap();
    let after = hsp_snapshot(&f);
    basis.extends_with_stops(&after).unwrap();
    assert_eq!(after.hsp_streams[0].effects.len(), 2);
    assert_eq!(after.hsp_streams[0].effects[1].state, "cancelled");
    assert!(after.hsp_streams[0].effects[1].outcome_sha256.is_none());
}
