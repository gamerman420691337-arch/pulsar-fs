use super::*;
use crate::playback_runtime::{
    tests::{clock, fixture as runtime_fixture},
    QualifiedCommandTiming,
};

struct Fixture {
    dir: tempfile::TempDir,
    journal: DeviceEffectJournal,
    setup: SetupEffectRecord,
    setup_receipt: PreparedSetupReceipt,
    setup_claim: ClaimedSetupReceipt,
    correlation: HandyProgramCorrelation,
}
impl Fixture {
    fn new() -> Self {
        Self::with_setup(true)
    }
    fn with_setup(acknowledged: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let correlation: HandyProgramCorrelation = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap();
        let i = &correlation.inspection;
        let mut journal = DeviceEffectJournal::open(dir.path(), i.engine_epoch.clone()).unwrap();
        let setup = SetupEffectRecord {
            boot: i.engine_epoch.clone(),
            device_session: i.device_session.clone(),
            actor_session: SessionId::new("actor").unwrap(),
            request_id: correlation.setup_request_id.clone(),
            hosting_request_id: i.hosting_request_id.clone(),
            project_id: i.project_id.clone(),
            revision: i.revision,
            registration: i.registration.clone(),
            qualification: i.accepted_qualification.clone().unwrap(),
            master_sha256: i.master_sha256.clone(),
            script: i.script.clone(),
            hosted: i.hosted.clone(),
            route_sha256: "9".repeat(64),
            route_namespace_sha256: Some("8".repeat(64)),
            acknowledgment_required: true,
            preflight_mode: Some(correlation.setup_mode.clone()),
        };
        let setup_receipt = journal.prepare_setup(setup.clone()).unwrap();
        let setup_claim = journal.claim_setup(&setup_receipt).unwrap();
        journal
            .complete_setup(
                &setup_claim,
                if acknowledged {
                    SetupCompletion::Acknowledged(correlation.acknowledgment.clone())
                } else {
                    SetupCompletion::Unknown
                },
            )
            .unwrap();
        Self {
            dir,
            journal,
            setup,
            setup_receipt,
            setup_claim,
            correlation,
        }
    }
    fn snapshot(&self) -> CustodySnapshot {
        self.journal
            .custody_snapshot(
                &self.setup.registration.device_id,
                &self.setup.route_sha256,
                self.setup.route_namespace_sha256.as_ref().unwrap(),
            )
            .unwrap()
    }
    fn record(&self, basis: CustodySnapshot, snapshot: CustodySnapshot) -> CustodyResolutionRecord {
        let mut plan = HandyRecoveryPlan {
            plan_id: ArtifactId::new("a".repeat(64)).unwrap(),
            snapshot_id: ArtifactId::new(basis.id().unwrap()).unwrap(),
            project_id: self.setup.project_id.clone(),
            selection: HandyRecoverySelection {
                device_id: self.setup.registration.device_id.clone(),
                registration_revision: self.setup.registration.revision,
                engine_epoch: self.journal.boot.clone(),
                device_session: DeviceSessionId::new("recovery-session").unwrap(),
                qualification: self.setup.qualification.clone(),
            },
            registration: self.setup.registration.clone(),
            route_sha256: self.setup.route_sha256.clone(),
            route_namespace_sha256: self.setup.route_namespace_sha256.clone().unwrap(),
            counts: basis.counts(),
            projects: basis.projects(),
            devices: basis.devices(),
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
        let observation = self.correlation.inspection.device.clone();
        CustodyResolutionRecord {
            actor_session: self.setup.actor_session.clone(),
            request_id: RequestId::new("resolution-original").unwrap(),
            request_fingerprint: digest(b"resolution-original"),
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
    fn controller(&self) -> HandyControllerLease {
        let mut l: HandyControllerLease = serde_json::from_str::<HandyControllerReceipt>(
            include_str!("../../pulsar-protocol/tests/fixtures/handy-controller.json"),
        )
        .unwrap()
        .lease;
        l.engine_epoch = self.setup.boot.clone();
        l.device_session = self.setup.device_session.clone();
        l.device_id = self.setup.registration.device_id.clone();
        l.project_id = self.setup.project_id.clone();
        l.revision = self.setup.revision;
        l.preparation_request_id = RequestId::new("preparation").unwrap();
        l.preparation_id = ArtifactId::new(digest(
            &serde_json::to_vec(&(
                &self.setup.actor_session,
                l.engine_epoch.as_str(),
                &l.preparation_request_id,
                self.setup_receipt.id(),
            ))
            .unwrap(),
        ))
        .unwrap();
        l.deadline_boottime_ns = 1_000_000_000;
        l
    }
    fn sync(&mut self) -> (PreparedClockSyncReceipt, ClaimedClockSyncReceipt) {
        let record = ClockSyncEffectRecord {
            actor_session: self.setup.actor_session.clone(),
            request_id: RequestId::new("sync-original").unwrap(),
            original_controller: self.controller(),
            registration: self.setup.registration.clone(),
            preparation_qualification: self.setup.qualification.clone(),
            setup_record_id: ArtifactId::new(self.setup_receipt.id()).unwrap(),
            user_offset_before: HandyUserTimeOffset { offset_ms: 0 },
            route_sha256: self.setup.route_sha256.clone(),
            route_namespace_sha256: self.setup.route_namespace_sha256.clone().unwrap(),
        };
        let p = self.journal.prepare_clock_sync(record).unwrap();
        let c = self.journal.claim_clock_sync(&p).unwrap();
        self.journal
            .complete_clock_sync(&c, ClockSyncCompletion::Unknown)
            .unwrap();
        (p, c)
    }
    fn runtime(&self) -> (HandyPlaybackRuntime, PlaybackAdmission, ClientId) {
        let (_, mut admission, controller) = runtime_fixture();
        admission.project = self.setup.project_id.clone();
        admission.revision = self.setup.revision;
        admission.configuration.device = self.setup.registration.device_id.clone();
        admission.program_artifact = self.setup.hosted.artifact_id.clone();
        let qualification = DeviceQualification::new(
            admission.configuration.clone(),
            self.setup.qualification.dossier_id.clone(),
            QualificationAuthority::LocalHumanAcceptance,
            Some(7_000_000),
        )
        .unwrap();
        let timing =
            QualifiedCommandTiming::new(qualification, 10_000_000, 1_000_000, 100).unwrap();
        let mut runtime = HandyPlaybackRuntime::new(
            admission.configuration.device.clone(),
            self.setup.device_session.clone(),
            timing,
        )
        .unwrap();
        runtime
            .arm(controller.clone(), admission.clone(), None)
            .unwrap();
        (runtime, admission, controller)
    }
    fn stop_intent(&self) -> EffectIntent {
        let (mut r, a, c) = self.runtime();
        let mapping = clock();
        let p = r.prepare_start(&c, mapping.clone(), 1234).unwrap();
        r.dispatch_start(p, &a, &c, &mapping.domain, mapping.monotonic_ns)
            .unwrap();
        let stop = r.revoke_controller().unwrap().unwrap();
        self.journal
            .receipt_binding()
            .capture_intent(&r, &stop)
            .unwrap()
    }
    fn play_intent(&self) -> (PlayIntent, PlayOperationRecord) {
        let (mut runtime, admission, controller) = self.runtime();
        let mapping = clock();
        let pending = runtime
            .prepare_start(&controller, mapping.clone(), 1234)
            .unwrap();
        let effect = runtime
            .dispatch_start(
                pending,
                &admission,
                &controller,
                &mapping.domain,
                mapping.monotonic_ns,
            )
            .unwrap();
        let mut lease = self.controller();
        lease.controller_id = controller;
        let record = PlayOperationRecord {
            browser_start: None,
            actor_session: self.setup.actor_session.clone(),
            request_id: RequestId::new("play-original").unwrap(),
            request_fingerprint: digest(b"play-original"),
            original_controller: lease,
            qualification: self.setup.qualification.clone(),
            setup_record_id: ArtifactId::new(self.setup_receipt.id()).unwrap(),
            clock_request_id: RequestId::new("clock-original").unwrap(),
            clock: PlayClockRecord::from(&mapping),
            start_time_ms: 1234,
            effect: self
                .journal
                .receipt_binding()
                .record_for_effect(&effect)
                .unwrap(),
        };
        let intent = self
            .journal
            .receipt_binding()
            .capture_play_intent(&runtime, &effect, record.clone())
            .unwrap();
        (intent, record)
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
fn reconciliation_keeps_original_bytes_and_outcomes_and_reuses_only_resolved_routes() {
    let mut f = Fixture::new();
    let (sync, claim) = f.sync();
    let stop = f.stop_intent();
    f.journal
        .try_record_emergency_stop(&stop, DispatchOutcome::Unknown)
        .unwrap();
    let before = f.snapshot();
    assert_eq!(
        before.counts(),
        HandyCustodyCounts {
            hsp_streams: 0,
            hsp_preparations: 0,
            setup: 1,
            clock_sync: 1,
            play: 0,
            effects: 1
        }
    );
    let record = f.record(before.clone(), before.clone());
    let originals: Vec<(String, String)> = {
        let mut q=f.journal.db.prepare("SELECT record,state FROM setup_effects UNION ALL SELECT record,state FROM clock_sync_effects UNION ALL SELECT record,outcome FROM effects").unwrap();
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    let id = f.journal.record_resolution(&record).unwrap();
    assert_eq!(f.journal.record_resolution(&record).unwrap(), id);
    assert!(f.snapshot().members.is_empty());
    assert!(!f
        .journal
        .setup_route_reserved(&f.setup.route_sha256)
        .unwrap());
    assert!(!f
        .journal
        .has_device_effects(&f.setup.registration.device_id)
        .unwrap());
    f.journal
        .clock_sync_route_available(
            &f.setup.route_sha256,
            f.setup.route_namespace_sha256.as_ref().unwrap(),
            &f.setup.registration.device_id,
            None,
        )
        .unwrap();
    f.journal
        .play_route_available(
            &f.setup.route_sha256,
            f.setup.route_namespace_sha256.as_ref().unwrap(),
            &f.setup.registration.device_id,
            None,
        )
        .unwrap();
    assert!(f.journal.claim_setup(&f.setup_receipt).is_err());
    assert!(f
        .journal
        .cancel_setup_unclaimed(f.setup_receipt.id())
        .is_err());
    assert!(f.journal.prepare_setup(f.setup.clone()).is_err());
    assert!(f.journal.claim_clock_sync(&sync).is_err());
    assert!(f.journal.cancel_clock_sync_unclaimed(sync.id()).is_err());
    f.journal
        .complete_setup(
            &f.setup_claim,
            SetupCompletion::Acknowledged(f.correlation.acknowledgment.clone()),
        )
        .unwrap();
    f.journal
        .complete_clock_sync(&claim, ClockSyncCompletion::Unknown)
        .unwrap();
    assert!(f
        .journal
        .complete_setup(&f.setup_claim, SetupCompletion::Unknown)
        .is_err());
    f.journal
        .try_record_emergency_stop(&stop, DispatchOutcome::Unknown)
        .unwrap();
    assert!(f
        .journal
        .try_record_emergency_stop(&stop, DispatchOutcome::Acknowledged)
        .is_err());
    assert!(f.journal.try_register_rejected_stop(&stop).is_err());
    let after: Vec<(String, String)> = {
        let mut q=f.journal.db.prepare("SELECT record,state FROM setup_effects UNION ALL SELECT record,state FROM clock_sync_effects UNION ALL SELECT record,outcome FROM effects").unwrap();
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(originals, after);
    let mut new = f.setup.clone();
    new.request_id = RequestId::new("new-setup").unwrap();
    new.device_session = DeviceSessionId::new("new-device-session").unwrap();
    let fresh = f.journal.prepare_setup(new).unwrap();
    assert!(f
        .journal
        .setup_route_reserved(&f.setup.route_sha256)
        .unwrap());
    assert!(!f
        .journal
        .is_resolved(CustodyKind::Setup, fresh.id())
        .unwrap());
    assert_eq!(f.journal.record_resolution(&record).unwrap(), id);
    assert!(!f
        .journal
        .is_resolved(CustodyKind::Setup, fresh.id())
        .unwrap());
    let saved = f
        .journal
        .resolution_history(&record.actor_session, &record.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.0, id);
    assert_eq!(saved.1.snapshot, before);
    let bytes = f.bytes();
    assert_eq!(
        DeviceEffectJournal::verify_resolution(
            f.dir.path(),
            &record.actor_session,
            &record.request_id,
            &id
        )
        .unwrap()
        .id()
        .unwrap(),
        id
    );
    assert!(DeviceEffectJournal::verify_resolution(
        f.dir.path(),
        &SessionId::new("other-actor").unwrap(),
        &record.request_id,
        &id
    )
    .is_err());
    assert_eq!(f.bytes(), bytes);
    let mut next = DeviceEffectJournal::open(
        f.dir.path(),
        DeviceSessionId::new("restarted-owner").unwrap(),
    )
    .unwrap();
    assert_eq!(next.record_resolution(&record).unwrap(), id);
    assert!(next.setup_route_reserved(&f.setup.route_sha256).unwrap());
}

#[test]
fn reconciliation_atomic_failure_rolls_back_resolution_membership_and_route_release() {
    let mut f = Fixture::new();
    let stop = f.stop_intent();
    f.journal.try_register_rejected_stop(&stop).unwrap();
    let basis = f.snapshot();
    let record = f.record(basis.clone(), basis.clone());
    f.journal
        .db
        .execute_batch(
            "CREATE UNIQUE INDEX injected_member_failure ON custody_resolution_members(resolution)",
        )
        .unwrap();
    assert!(f.journal.record_resolution(&record).is_err());
    assert_eq!(f.snapshot(), basis);
    assert!(f
        .journal
        .setup_route_reserved(&f.setup.route_sha256)
        .unwrap());
    assert!(f
        .journal
        .resolution_history(&record.actor_session, &record.request_id)
        .unwrap()
        .is_none());
    assert_eq!(census(&f.journal.db).unwrap(), (0, 0));
    f.journal
        .db
        .execute_batch("DROP INDEX injected_member_failure")
        .unwrap();
    f.journal.record_resolution(&record).unwrap();
    assert!(f.snapshot().members.is_empty());
}

#[test]
fn reconciliation_snapshot_identity_tracks_members_but_not_ordinary_outcome_advancement() {
    let mut f = Fixture::new();
    let stop = f.stop_intent();
    f.journal.try_register_rejected_stop(&stop).unwrap();
    let basis = f.snapshot();
    let id = basis.id().unwrap();
    let receipt = f.journal.prepare_intent(&stop).unwrap();
    f.journal
        .complete(&receipt, DispatchOutcome::Unknown)
        .unwrap();
    let final_snapshot = f.snapshot();
    assert_eq!(final_snapshot.id().unwrap(), id);
    assert_ne!(basis, final_snapshot);
    basis.extends_with_stops(&final_snapshot).unwrap();
    let stale = f.record(basis.clone(), basis.clone());
    assert!(f.journal.record_resolution(&stale).is_err());
    let current = f.record(basis, final_snapshot);
    f.journal.record_resolution(&current).unwrap();
    assert!(f
        .journal
        .complete(&receipt, DispatchOutcome::Acknowledged)
        .is_err());
    f.journal
        .complete(&receipt, DispatchOutcome::Unknown)
        .unwrap();
}

#[test]
fn reconciliation_allows_only_subject_matched_stop_extensions_and_exact_request_retries() {
    let mut f = Fixture::new();
    let basis = f.snapshot();
    let stop = f.stop_intent();
    f.journal.try_register_rejected_stop(&stop).unwrap();
    let after = f.snapshot();
    basis.extends_with_stops(&after).unwrap();
    assert_eq!(basis.added_stop_records(&after).unwrap().len(), 1);
    assert_eq!(
        after.historical_qualifications().unwrap(),
        vec![f.setup.qualification.clone()]
    );
    let record = f.record(basis.clone(), after);
    f.journal.record_resolution(&record).unwrap();
    let mut changed = record.clone();
    changed.request_fingerprint = digest(b"different");
    assert!(f.journal.record_resolution(&changed).is_err());
    let mut f = Fixture::new();
    let basis = f.snapshot();
    f.sync();
    let after = f.snapshot();
    assert!(basis.extends_with_stops(&after).is_err());
}

#[test]
fn reconciliation_refuses_unknown_legacy_routing_namespace_or_device_route_ambiguity() {
    let mut f = Fixture::new();
    let stop = f.stop_intent();
    let mut unknown = stop.clone();
    unknown.record.device_session = DeviceSessionId::new("unassociated-session").unwrap();
    f.journal.try_register_rejected_stop(&unknown).unwrap();
    assert!(f
        .journal
        .custody_snapshot(
            &f.setup.registration.device_id,
            &f.setup.route_sha256,
            f.setup.route_namespace_sha256.as_ref().unwrap()
        )
        .is_err());
    let f = Fixture::new();
    assert!(f
        .journal
        .custody_snapshot(
            &f.setup.registration.device_id,
            &"7".repeat(64),
            f.setup.route_namespace_sha256.as_ref().unwrap()
        )
        .is_err());
    assert!(f
        .journal
        .custody_snapshot(
            &f.setup.registration.device_id,
            &f.setup.route_sha256,
            &"6".repeat(64)
        )
        .is_err());
    let alias = DeviceId::new("logical-alias").unwrap();
    let observed = f
        .journal
        .custody_snapshot(
            &alias,
            &f.setup.route_sha256,
            f.setup.route_namespace_sha256.as_ref().unwrap(),
        )
        .unwrap();
    assert!(observed.devices().contains(&alias));
    assert!(observed.devices().contains(&f.setup.registration.device_id));
    assert_eq!(observed.members.len(), 1);
}

#[test]
fn reconciliation_missing_members_corrupt_markers_or_changed_originals_refuse_restart() {
    for sql in [
        "DELETE FROM custody_resolution_members",
        "DELETE FROM custody_resolutions",
        "DELETE FROM custody_resolution_census",
        "UPDATE custody_resolution_members SET membership_sha256='bad'",
        "UPDATE custody_resolutions SET record='{}'",
        "UPDATE setup_observations SET sha256='bad'",
        "INSERT INTO setup_custody_routes SELECT route,id FROM setup_effects",
    ] {
        let mut f = Fixture::new();
        let basis = f.snapshot();
        let record = f.record(basis.clone(), basis);
        let id = f.journal.record_resolution(&record).unwrap();
        f.journal.db.execute_batch(sql).unwrap();
        let before = f.bytes();
        assert!(DeviceEffectJournal::open(
            f.dir.path(),
            DeviceSessionId::new("must-not-own").unwrap()
        )
        .is_err());
        assert!(DeviceEffectJournal::verify_resolution(
            f.dir.path(),
            &record.actor_session,
            &record.request_id,
            &id
        )
        .is_err());
        assert_eq!(f.bytes(), before);
        assert!(f.journal.current_owner().unwrap());
    }
}

#[test]
fn reconciliation_version_seven_migration_keeps_unknown_setup_and_sync_bytes() {
    let mut f = Fixture::new();
    f.sync();
    let records: Vec<String> = {
        let mut q = f
            .journal
            .db
            .prepare(
                "SELECT record FROM setup_effects UNION ALL SELECT record FROM clock_sync_effects",
            )
            .unwrap();
        let rows = q.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    downgrade_to_seven(&f.journal.db);
    let next = DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("next-boot").unwrap())
        .unwrap();
    assert_eq!(
        next.db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    let after: Vec<String> = {
        let mut q = next
            .db
            .prepare(
                "SELECT record FROM setup_effects UNION ALL SELECT record FROM clock_sync_effects",
            )
            .unwrap();
        let rows = q.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(after, records);
    assert!(next.setup_route_reserved(&f.setup.route_sha256).unwrap());
    assert_eq!(census(&next.db).unwrap(), (0, 0));
}

#[test]
fn reconciliation_rejected_stop_registration_preserves_prepared_and_terminal_history_without_receipts(
) {
    let mut f = Fixture::new();
    let stop = f.stop_intent();
    f.journal.try_register_rejected_stop(&stop).unwrap();
    let history = f
        .journal
        .history(&f.setup.registration.device_id, 1)
        .unwrap();
    assert_eq!(history[0].outcome, None);
    assert!(!f.journal.emergency_stop_recorded(&history[0].id).unwrap());
    let receipt = f.journal.prepare_intent(&stop).unwrap();
    f.journal
        .complete(&receipt, DispatchOutcome::Acknowledged)
        .unwrap();
    f.journal.try_register_rejected_stop(&stop).unwrap();
    assert_eq!(
        f.journal
            .history(&f.setup.registration.device_id, 1)
            .unwrap()[0]
            .outcome,
        Some(DispatchOutcome::Acknowledged)
    );
    let basis = f.snapshot();
    let record = f.record(basis.clone(), basis);
    f.journal.record_resolution(&record).unwrap();
    assert!(f.journal.try_register_rejected_stop(&stop).is_err());
}

#[test]
fn reconciliation_resolves_unknown_setup_without_rewriting_it_or_releasing_new_custody() {
    let mut f = Fixture::with_setup(false);
    let basis = f.snapshot();
    let record = f.record(basis.clone(), basis);
    f.journal.record_resolution(&record).unwrap();
    assert_eq!(
        f.journal.setup_history(f.setup_receipt.id()).unwrap().state,
        SetupJournalState::Unknown
    );
    f.journal
        .complete_setup(&f.setup_claim, SetupCompletion::Unknown)
        .unwrap();
    assert!(f
        .journal
        .complete_setup(
            &f.setup_claim,
            SetupCompletion::Acknowledged(f.correlation.acknowledgment.clone())
        )
        .is_err());
    let mut next = f.setup.clone();
    next.request_id = RequestId::new("next-after-unknown").unwrap();
    let receipt = f.journal.prepare_setup(next).unwrap();
    assert!(f
        .journal
        .setup_route_reserved(&f.setup.route_sha256)
        .unwrap());
    assert!(!f
        .journal
        .is_resolved(CustodyKind::Setup, receipt.id())
        .unwrap());
}

#[test]
fn reconciliation_covers_play_parent_and_effect_and_rejects_old_prepared_claim() {
    let mut f = Fixture::new();
    let (intent, original) = f.play_intent();
    let prepared = f.journal.prepare_play(&intent).unwrap();
    let basis = f.snapshot();
    assert_eq!(
        basis.counts(),
        HandyCustodyCounts {
            hsp_streams: 0,
            hsp_preparations: 0,
            setup: 1,
            clock_sync: 0,
            play: 1,
            effects: 1
        }
    );
    let record = f.record(basis.clone(), basis);
    f.journal.record_resolution(&record).unwrap();
    assert!(f.journal.claim_play(prepared).is_err());
    assert!(f.journal.prepare_play(&intent).is_err());
    assert!(f
        .journal
        .cancel_unclaimed_play(
            &original.actor_session,
            &original.request_id,
            &original.id().unwrap()
        )
        .is_err());
    assert_eq!(
        f.journal
            .play_request_history(&original.actor_session, &original.request_id)
            .unwrap()
            .unwrap()
            .state,
        PlayOperationState::Prepared
    );
    assert!(!f
        .journal
        .has_device_effects(&f.setup.registration.device_id)
        .unwrap());
}

#[test]
fn reconciliation_excludes_only_consistently_cancelled_unclaimed_play_effects() {
    let mut f = Fixture::new();
    let (intent, record) = f.play_intent();
    f.journal.prepare_play(&intent).unwrap();
    f.journal
        .cancel_unclaimed_play(
            &record.actor_session,
            &record.request_id,
            &record.id().unwrap(),
        )
        .unwrap();
    let snapshot = f.snapshot();
    assert_eq!(
        snapshot.counts(),
        HandyCustodyCounts {
            hsp_streams: 0,
            hsp_preparations: 0,
            setup: 1,
            clock_sync: 0,
            play: 0,
            effects: 0
        }
    );
    assert!(!f
        .journal
        .has_device_effects(&f.setup.registration.device_id)
        .unwrap());
    f.journal
        .db
        .execute_batch("UPDATE effects SET outcome='unknown'")
        .unwrap();
    assert!(f
        .journal
        .custody_snapshot(
            &f.setup.registration.device_id,
            &f.setup.route_sha256,
            f.setup.route_namespace_sha256.as_ref().unwrap()
        )
        .is_err());
    assert!(f
        .journal
        .has_device_effects(&f.setup.registration.device_id)
        .is_err());
}

pub(super) fn downgrade_to_seven(db: &Connection) {
    db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members;DROP TABLE custody_resolution_census;DROP TABLE custody_resolutions;DROP TABLE setup_custody_routes;DROP TABLE clock_sync_custody_routes;DROP INDEX setup_active_route;CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled';DROP INDEX clock_sync_active_route;CREATE UNIQUE INDEX clock_sync_active_route ON clock_sync_effects(route) WHERE state IN ('prepared','claimed','unknown');PRAGMA user_version=7;").unwrap();
}
