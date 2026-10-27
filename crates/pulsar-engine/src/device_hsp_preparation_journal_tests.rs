//! Authored journal scenarios only; no hardware, accepted evidence or live clocks.
use super::*;
use crate::handy_hsp_records::HspBindingRecord;
use crate::handy_hsp_runtime::{
    HandyHspRuntime, HspAuthorityWindow, HspBinding, HspDispatchFacts, HspPlayReference,
    HspSettledTransport,
};
use pulsar_core::{HspPhaseWindow, HspPolicy, HspStreamIdentity, LiveIdentity};

fn original(index: u64) -> HspPreparationRecord {
    let c: HandyProgramCorrelation = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
    ))
    .unwrap();
    HspPreparationRecord {
        boot: DeviceSessionId::new("authored-hsp-preparation-boot").unwrap(),
        actor_session: SessionId::new("authored-hsp-preparation-actor").unwrap(),
        request_id: RequestId::new(format!("preparation-{index}")).unwrap(),
        request_fingerprint: format!("{index:064x}"),
        project_id: ProjectId::new("authored-project").unwrap(),
        revision: RevisionId::new(1),
        device_session: DeviceSessionId::new(format!("device-session-{index}")).unwrap(),
        registration: HandyRegistration {
            device_id: DeviceId::new(format!("device-{index}")).unwrap(),
            revision: 1,
            settings: HandyRegistrationSettings {
                label: "Authored metadata; no physical acceptance".into(),
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
            session_id: format!("remote-session-{index}"),
            firmware_status: 0,
        },
        mode: HandyDeviceMode {
            mode: 4,
            mode_session_id: 7,
        },
        operative_settings_json: serde_json::to_string(&c.inspection.device.operative_settings)
            .unwrap(),
        qualification: HandyQualificationBinding {
            dossier_id: ArtifactId::new("d".repeat(64)).unwrap(),
            revision: 2,
        },
        subject_sha256: "6".repeat(64),
        profile_sha256: "e".repeat(64),
        route_sha256: format!("{index:064x}"),
        route_namespace_sha256: "c".repeat(64),
        user_offset_before: HandyUserTimeOffset { offset_ms: 0 },
        started_boottime_ns: 100,
        deadline_boottime_ns: 10_000_000_000,
    }
}
fn ack() -> HandyClockSyncAcknowledgment {
    HandyClockSyncAcknowledgment {
        info: HandyDeviceTimeInfo {
            device_uptime_ms: 100,
            reported_clock_offset_ms: 0,
            last_sync_round_trip_ms: 1,
        },
        sent_boottime_ns: 200,
        received_boottime_ns: 300,
    }
}
fn child(
    record: &HspPreparationRecord,
    index: u64,
) -> (
    HspStreamRecord,
    HandyHspRuntime,
    HspEffect,
    HspQualificationLinkRecord,
    HspDispatchFacts,
) {
    let binding = HspBinding::new(
        HspStreamIdentity {
            generation: index,
            stream_id: 42,
            live: LiveIdentity {
                epoch: AttemptId::new(format!("epoch-{index}")).unwrap(),
                project: record.project_id.clone(),
                revision: record.revision,
                source: SourceVersionId::new(format!("source-{index}")).unwrap(),
            },
        },
        ClientId::new("authored-owner").unwrap(),
        record.device_session.clone(),
        ArtifactId::new(format!("authored-clock-{index}")).unwrap(),
    );
    let facts = HspDispatchFacts::new(
        binding.clone(),
        HspPlayReference::new(
            1000,
            HspPhaseWindow {
                earliest_ms: -100,
                latest_ms: -100,
            },
            -100,
            10000,
        )
        .unwrap(),
        HspAuthorityWindow::new(2_000_000_000, 2_000_000_000, 2_000_000_000),
    )
    .unwrap();
    let mut runtime = HandyHspRuntime::new(
        binding.clone(),
        HspPolicy {
            max_buffered_points: 8,
            device_capacity_points: 8,
            max_chunk_points: 2,
            min_lead_ms: 10,
            max_future_ms: 1000,
            max_sample_age_ns: 1_000_000,
            max_gap_ms: 200,
        },
    )
    .unwrap();
    let effect = runtime.next_effect(1000, Some(&facts)).unwrap().unwrap();
    let stream = HspStreamRecord {
        boot: record.boot.clone(),
        actor_session: record.actor_session.clone(),
        request_id: RequestId::new(format!("stream-{index}")).unwrap(),
        registration: record.registration.clone(),
        device: record.device.clone(),
        route_sha256: record.route_sha256.clone(),
        route_namespace_sha256: record.route_namespace_sha256.clone(),
        binding: HspBindingRecord::capture(&binding),
        qualification_sha256: record.qualification.dossier_id.to_string(),
        profile_sha256: record.profile_sha256.clone(),
    };
    let link = HspQualificationLinkRecord {
        stream_id: stream.id().unwrap(),
        qualification: record.qualification.clone(),
        subject_sha256: record.subject_sha256.clone(),
        profile_sha256: record.profile_sha256.clone(),
        admission_sha256: "7".repeat(64),
    };
    (stream, runtime, effect, link, facts)
}
fn journal() -> (tempfile::TempDir, DeviceEffectJournal) {
    let dir = tempfile::tempdir().unwrap();
    let journal = DeviceEffectJournal::open(dir.path(), original(1).boot).unwrap();
    (dir, journal)
}
fn accepted_sync(
    journal: &mut DeviceEffectJournal,
    record: &HspPreparationRecord,
) -> PreparedHspPreparationReceipt {
    let prepared = journal.prepare_hsp_preparation(record).unwrap();
    let claimed = journal.claim_hsp_preparation(&prepared).unwrap();
    journal
        .complete_hsp_preparation(&claimed, ClockSyncCompletion::Acknowledged(ack()))
        .unwrap();
    prepared
}
fn snapshot(
    journal: &DeviceEffectJournal,
    record: &HspPreparationRecord,
) -> super::super::super::reconciliation::CustodySnapshot {
    journal
        .custody_snapshot(
            &record.registration.device_id,
            &record.route_sha256,
            &record.route_namespace_sha256,
        )
        .unwrap()
}
fn resolution(
    journal: &DeviceEffectJournal,
    record: &HspPreparationRecord,
    basis: super::super::super::reconciliation::CustodySnapshot,
    current: super::super::super::reconciliation::CustodySnapshot,
) -> super::super::super::reconciliation::CustodyResolutionRecord {
    let mut plan = HandyRecoveryPlan {
        plan_id: ArtifactId::new("a".repeat(64)).unwrap(),
        snapshot_id: ArtifactId::new(basis.id().unwrap()).unwrap(),
        project_id: record.project_id.clone(),
        selection: HandyRecoverySelection {
            device_id: record.registration.device_id.clone(),
            registration_revision: 1,
            engine_epoch: journal.boot.clone(),
            device_session: DeviceSessionId::new("authored-recovery-session").unwrap(),
            qualification: record.qualification.clone(),
        },
        registration: record.registration.clone(),
        route_sha256: record.route_sha256.clone(),
        route_namespace_sha256: record.route_namespace_sha256.clone(),
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
    let observation = HandySetupDeviceObservation {
        identity: record.device.clone(),
        mode: record.mode.clone(),
        operative_settings: record.settings().unwrap(),
        remote_hssp: None,
        remote_hsp: Some(HandyHspObservation {
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
        }),
        observed_at_unix_ms: 1,
    };
    super::super::super::reconciliation::CustodyResolutionRecord {
        actor_session: record.actor_session.clone(),
        request_id: RequestId::new("authored-preparation-resolution").unwrap(),
        request_fingerprint: "b".repeat(64),
        plan,
        basis,
        snapshot: current,
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
fn hsp_preparation_original_replay_and_unclaimed_cancellation_are_terminal() {
    let (_dir, mut journal) = journal();
    let record = original(1);
    let prepared = journal.prepare_hsp_preparation(&record).unwrap();
    assert_eq!(journal.prepare_hsp_preparation(&record).unwrap(), prepared);
    let mut changed = record.clone();
    changed.deadline_boottime_ns += 1;
    assert!(journal.prepare_hsp_preparation(&changed).is_err());
    assert!(journal
        .mark_hsp_preparation_unknown_original(
            prepared.id(),
            &record.actor_session,
            &record.request_id
        )
        .is_err());
    let cancelled = journal.cancel_hsp_preparation(&prepared).unwrap();
    assert_eq!(cancelled.state, HandyClockSyncState::Cancelled);
    assert!(cancelled.acknowledgment.is_none());
    assert_eq!(
        journal
            .cancel_hsp_preparation_original(
                prepared.id(),
                &record.actor_session,
                &record.request_id
            )
            .unwrap()
            .state,
        HandyClockSyncState::Cancelled
    );
    assert!(journal.claim_hsp_preparation(&prepared).is_err());
    assert!(!journal
        .has_device_effects(&record.registration.device_id)
        .unwrap());
}
#[test]
fn hsp_preparation_claim_is_single_use_unknown_is_immutable_and_restart_is_not_ready() {
    let (dir, mut journal) = journal();
    let record = original(1);
    let prepared = journal.prepare_hsp_preparation(&record).unwrap();
    let claim = journal.claim_hsp_preparation(&prepared).unwrap();
    assert!(journal.claim_hsp_preparation(&prepared).is_err());
    assert!(journal.cancel_hsp_preparation(&prepared).is_err());
    let unknown = journal
        .mark_hsp_preparation_unknown_original(
            prepared.id(),
            &record.actor_session,
            &record.request_id,
        )
        .unwrap();
    assert_eq!(unknown.state, HandyClockSyncState::Unknown);
    assert!(journal
        .complete_hsp_preparation(&claim, ClockSyncCompletion::Acknowledged(ack()))
        .is_err());
    let before = journal.hsp_preparation_membership(prepared.id()).unwrap();
    drop(journal);
    let reopened =
        DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("new-boot").unwrap()).unwrap();
    assert_eq!(
        reopened.hsp_preparation_membership(prepared.id()).unwrap(),
        before
    );
    assert_eq!(
        reopened
            .hsp_preparation_history(&record.actor_session, &record.request_id)
            .unwrap()
            .unwrap()
            .history()
            .unwrap()
            .state,
        HandyClockSyncState::Unknown
    );
}
#[test]
fn hsp_preparation_crash_claim_becomes_unknown_without_record_rewrite() {
    let (dir, mut journal) = journal();
    let record = original(1);
    let prepared = journal.prepare_hsp_preparation(&record).unwrap();
    journal.claim_hsp_preparation(&prepared).unwrap();
    let before: String = journal
        .db
        .query_row(
            "SELECT record FROM hsp_preparations WHERE id=?1",
            [prepared.id()],
            |r| r.get(0),
        )
        .unwrap();
    drop(journal);
    let mut reopened =
        DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("new-boot").unwrap()).unwrap();
    let row = reopened.hsp_preparation_observation(prepared.id()).unwrap();
    assert_eq!(row.state, HandyClockSyncState::Unknown);
    assert_eq!(row.record.to_json().unwrap(), before.as_bytes());
    assert!(row.acknowledgment.is_none());
    assert!(reopened.claim_hsp_preparation(&prepared).is_err());
    assert!(reopened.cancel_hsp_preparation(&prepared).is_err());
}
#[test]
fn hsp_preparation_acknowledgment_keeps_original_deadline() {
    let (_dir, mut journal) = journal();
    let record = original(1);
    let prepared = journal.prepare_hsp_preparation(&record).unwrap();
    let claim = journal.claim_hsp_preparation(&prepared).unwrap();
    let mut bad = ack();
    bad.received_boottime_ns = record.deadline_boottime_ns;
    assert!(journal
        .complete_hsp_preparation(&claim, ClockSyncCompletion::Acknowledged(bad))
        .is_err());
    let mut bad = ack();
    bad.sent_boottime_ns = record.started_boottime_ns - 1;
    assert!(journal
        .complete_hsp_preparation(&claim, ClockSyncCompletion::Acknowledged(bad))
        .is_err());
    let row = journal
        .complete_hsp_preparation(&claim, ClockSyncCompletion::Acknowledged(ack()))
        .unwrap();
    assert_eq!(
        row.history().unwrap().deadline_boottime_ns,
        record.deadline_boottime_ns
    );
    assert_eq!(
        journal
            .mark_hsp_preparation_unknown_original(
                prepared.id(),
                &record.actor_session,
                &record.request_id
            )
            .unwrap()
            .state,
        HandyClockSyncState::Acknowledged
    );
}
#[test]
fn hsp_preparation_excludes_legacy_aliases_namespaces_and_unlinked_hsp_streams() {
    let (_dir, mut journal) = journal();
    let record = original(1);
    let _prepared = journal.prepare_hsp_preparation(&record).unwrap();
    let mut alias = original(2);
    alias.route_sha256 = record.route_sha256.clone();
    assert!(journal.prepare_hsp_preparation(&alias).is_err());
    let mut namespace = original(3);
    namespace.route_namespace_sha256 = "f".repeat(64);
    assert!(journal.prepare_hsp_preparation(&namespace).is_err());
    assert!(super::super::ensure_legacy_route_available(
        &journal.db,
        &record.route_sha256,
        &record.route_namespace_sha256,
        &record.registration.device_id
    )
    .is_err());
    let (stream, _, _, _, _) = child(&record, 1);
    assert!(journal.prepare_hsp_stream(&stream).is_err());
    assert!(journal
        .has_device_effects(&record.registration.device_id)
        .unwrap());
    journal.prepare_hsp_preparation(&original(4)).unwrap();
}
#[test]
fn hsp_preparation_handoff_transfers_only_with_original_child_setup_claim() {
    let (_dir, mut journal) = journal();
    let record = original(1);
    let parent = accepted_sync(&mut journal, &record);
    let (stream, _, effect, link, _) = child(&record, 1);
    let (receipt, prepared) = journal
        .prepare_hsp_stream_from_preparation(&parent, &stream, &effect, &link)
        .unwrap();
    assert!(
        !journal
            .hsp_preparation_observation(parent.id())
            .unwrap()
            .custody_transferred
    );
    let replay = journal
        .prepare_hsp_stream_from_preparation(&parent, &stream, &effect, &link)
        .unwrap();
    assert_eq!(replay.1, prepared);
    let mut other = stream.clone();
    other.request_id = RequestId::new("other-child").unwrap();
    let mut other_link = link.clone();
    other_link.stream_id = other.id().unwrap();
    assert!(journal
        .prepare_hsp_stream_from_preparation(&parent, &other, &effect, &other_link)
        .is_err());
    assert!(journal.cancel_hsp_preparation(&parent).is_err());
    journal.claim_hsp(&prepared, &effect).unwrap();
    let observed = journal.hsp_preparation_observation(parent.id()).unwrap();
    assert!(observed.custody_transferred);
    assert_eq!(observed.linked_stream_id.as_deref(), Some(receipt.id()));
    assert!(journal
        .prepare_hsp_stream_from_preparation(&parent, &stream, &effect, &link)
        .is_err());
    assert!(journal.claim_hsp(&prepared, &effect).is_err());
}
#[test]
fn hsp_cancelled_child_keeps_parent_custody_and_exact_recovery_membership() {
    let (_dir, mut journal) = journal();
    let record = original(1);
    let parent = accepted_sync(&mut journal, &record);
    let (stream, _, effect, link, _) = child(&record, 1);
    let (_, prepared) = journal
        .prepare_hsp_stream_from_preparation(&parent, &stream, &effect, &link)
        .unwrap();
    let basis = snapshot(&journal, &record);
    journal.cancel_hsp_effect(&prepared).unwrap();
    assert!(
        !journal
            .hsp_preparation_observation(parent.id())
            .unwrap()
            .custody_transferred
    );
    assert!(journal
        .has_device_effects(&record.registration.device_id)
        .unwrap());
    let captured = snapshot(&journal, &record);
    assert_eq!(captured.hsp_preparations.len(), 1);
    assert_eq!(captured.hsp_streams.len(), 1);
    assert_eq!(captured.hsp_streams[0].state, "cancelled");
    let resolution = resolution(&journal, &record, basis, captured.clone());
    let id = journal.record_resolution(&resolution).unwrap();
    let proof = journal
        .hsp_resolution_receipt(&id, &captured.hsp_streams)
        .unwrap();
    assert!(proof.contains_exact_preparations(&captured.hsp_preparations));
    assert!(!journal
        .has_device_effects(&record.registration.device_id)
        .unwrap());
    assert!(journal
        .prepare_hsp_stream_from_preparation(&parent, &stream, &effect, &link)
        .is_err());
}
#[test]
fn hsp_preparation_only_recovery_resolves_original_unknown_and_releases_route() {
    let (_dir, mut journal) = journal();
    let record = original(1);
    let prepared = journal.prepare_hsp_preparation(&record).unwrap();
    let claim = journal.claim_hsp_preparation(&prepared).unwrap();
    journal
        .complete_hsp_preparation(&claim, ClockSyncCompletion::Unknown)
        .unwrap();
    let captured = snapshot(&journal, &record);
    assert!(captured.hsp_streams.is_empty());
    assert_eq!(captured.counts().hsp_preparations, 1);
    let resolution = resolution(&journal, &record, captured.clone(), captured.clone());
    let id = journal.record_resolution(&resolution).unwrap();
    assert_eq!(journal.record_resolution(&resolution).unwrap(), id);
    let proof = journal.hsp_resolution_receipt(&id, &[]).unwrap();
    assert!(proof.matches_target(
        &record.registration.device_id,
        &record.route_sha256,
        &record.route_namespace_sha256
    ));
    assert!(proof.contains_exact_preparations(&captured.hsp_preparations));
    assert!(!proof.contains_exact_preparations(&[
        captured.hsp_preparations[0].clone(),
        captured.hsp_preparations[0].clone()
    ]));
    assert!(snapshot(&journal, &record).hsp_preparations.is_empty());
    assert!(journal.claim_hsp_preparation(&prepared).is_err());
    assert!(journal.cancel_hsp_preparation(&prepared).is_err());
    assert_eq!(
        journal
            .hsp_preparation_observation(prepared.id())
            .unwrap()
            .state,
        HandyClockSyncState::Unknown
    );
    let mut next = record.clone();
    next.request_id = RequestId::new("fresh-after-resolution").unwrap();
    next.request_fingerprint = "9".repeat(64);
    journal.prepare_hsp_preparation(&next).unwrap();
}
#[test]
fn hsp_transferred_parent_and_child_resolve_as_one_exact_aggregate() {
    let (_dir, mut journal) = journal();
    let record = original(1);
    let parent = accepted_sync(&mut journal, &record);
    let (stream, mut runtime, effect, link, facts) = child(&record, 1);
    let (_, prepared) = journal
        .prepare_hsp_stream_from_preparation(&parent, &stream, &effect, &link)
        .unwrap();
    let claim = journal.claim_hsp(&prepared, &effect).unwrap();
    let completion = runtime
        .complete_settled(&effect, HspSettledTransport::Unknown, 1000, Some(&facts))
        .unwrap();
    journal.complete_hsp(&claim, &completion).unwrap();
    let captured = snapshot(&journal, &record);
    assert!(captured.hsp_preparations[0].custody_transferred);
    let resolution = resolution(&journal, &record, captured.clone(), captured.clone());
    let id = journal.record_resolution(&resolution).unwrap();
    journal
        .hsp_resolution_receipt(&id, &captured.hsp_streams)
        .unwrap();
    assert!(
        journal
            .hsp_preparation_observation(parent.id())
            .unwrap()
            .resolved
    );
    assert!(!journal
        .has_device_effects(&record.registration.device_id)
        .unwrap());
    journal
        .db
        .execute(
            "DELETE FROM hsp_preparation_resolutions WHERE preparation=?1",
            [parent.id()],
        )
        .unwrap();
    assert!(DeviceEffectJournal::validate_existing(&journal.db).is_err());
}
#[test]
fn hsp_preparation_record_integrity_capacity_and_witnesses_are_bounded() {
    let (dir, mut journal) = journal();
    let record = original(1);
    let prepared = journal.prepare_hsp_preparation(&record).unwrap();
    DeviceEffectJournal::verify_hsp_preparation_records(
        dir.path(),
        &[(
            record.actor_session.clone(),
            record.request_id.clone(),
            prepared.id().into(),
        )],
    )
    .unwrap();
    assert!(DeviceEffectJournal::verify_hsp_preparation_records(
        dir.path(),
        &[(
            record.actor_session.clone(),
            RequestId::new("wrong").unwrap(),
            prepared.id().into()
        )]
    )
    .is_err());
    assert!(journal
        .db
        .execute_batch("UPDATE hsp_preparation_census SET preparations=65")
        .is_err());
    journal
        .db
        .execute_batch("UPDATE hsp_preparations SET lifecycle_sha256=printf('%064d',0)")
        .unwrap();
    assert!(DeviceEffectJournal::validate_existing(&journal.db).is_err());
}
#[test]
fn hsp_v11_migration_adds_empty_preparation_history_without_rewriting_records() {
    let (dir, journal) = journal();
    let before:Vec<String>=journal.db.prepare("SELECT sql FROM sqlite_master WHERE name IN ('hsp_streams','hsp_effects','hsp_qualification_links','hsp_resolution_members') ORDER BY name").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions;DROP TABLE hsp_preparation_links;DROP TABLE hsp_preparations;DROP TABLE hsp_preparation_census;PRAGMA user_version=11;").unwrap();
    drop(journal);
    let migrated = DeviceEffectJournal::open(dir.path(), original(1).boot).unwrap();
    assert_eq!(
        DeviceEffectJournal::validate_existing(&migrated.db).unwrap(),
        12
    );
    let after:Vec<String>=migrated.db.prepare("SELECT sql FROM sqlite_master WHERE name IN ('hsp_streams','hsp_effects','hsp_qualification_links','hsp_resolution_members') ORDER BY name").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(before, after);
    assert!(migrated
        .hsp_preparation_history(&original(1).actor_session, &original(1).request_id)
        .unwrap()
        .is_none());
}

fn native_history(db: &Connection) -> serde_json::Value {
    let tables:Vec<String>=db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name!='owner' ORDER BY name").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    let mut result = serde_json::Map::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let mut q = db
            .prepare(&format!("SELECT * FROM \"{quoted}\" ORDER BY 1,2"))
            .unwrap();
        let columns = q
            .column_names()
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>();
        let width = columns.len();
        let rows = q
            .query_map([], |r| {
                (0..width)
                    .map(|i| {
                        Ok(match r.get::<_, rusqlite::types::Value>(i)? {
                            rusqlite::types::Value::Null => serde_json::Value::Null,
                            rusqlite::types::Value::Integer(v) => serde_json::json!(v),
                            rusqlite::types::Value::Real(v) => serde_json::json!(v),
                            rusqlite::types::Value::Text(v) => serde_json::json!(v),
                            rusqlite::types::Value::Blob(_) => {
                                panic!("journal fixture contains unexpected binary data")
                            }
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        result.insert(table, serde_json::json!({"columns":columns,"rows":rows}));
    }
    serde_json::Value::Object(result)
}
fn native_hash(path: &Path) -> String {
    use std::io::Read;
    let mut file = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let count = file.read(&mut bytes).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    format!("{:x}", hash.finalize())
}
fn native_write(output: &Path, name: &str, bytes: &[u8]) -> serde_json::Value {
    use std::io::Write;
    let path = output.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    serde_json::json!({"path":name,"sha256":native_hash(&path),"bytes":bytes.len()})
}
#[test]
#[ignore = "explicit synthetic v12 native-reader fixture export; requires fresh PULSAR_HSP_PREPARATION_NATIVE_OUTPUT"]
fn export_populated_hsp_preparation_journal_for_native_reader_campaign() {
    let output = std::path::PathBuf::from(
        std::env::var_os("PULSAR_HSP_PREPARATION_NATIVE_OUTPUT")
            .expect("set an explicit fresh fixture path"),
    );
    assert!(
        output.is_absolute()
            && output.starts_with("/mnt/4tb/artifacts")
            && !output.exists()
            && output.parent().is_some_and(|p| p.is_dir())
    );
    assert!(!output
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir)));
    std::fs::create_dir(&output).unwrap();
    let (_scratch, mut journal) = journal();
    let mut preparation_ids = Vec::new();
    let mut stream_ids = Vec::new();
    let r = original(1);
    let p = journal.prepare_hsp_preparation(&r).unwrap();
    let claim = journal.claim_hsp_preparation(&p).unwrap();
    journal
        .complete_hsp_preparation(&claim, ClockSyncCompletion::Unknown)
        .unwrap();
    preparation_ids.push(p.id().to_owned());
    for (index, transfer) in [(2, false), (3, true)] {
        let r = original(index);
        let parent = accepted_sync(&mut journal, &r);
        preparation_ids.push(parent.id().to_owned());
        let (stream, mut runtime, effect, link, facts) = child(&r, index);
        let (receipt, prepared) = journal
            .prepare_hsp_stream_from_preparation(&parent, &stream, &effect, &link)
            .unwrap();
        stream_ids.push(receipt.id().to_owned());
        if transfer {
            let claim = journal.claim_hsp(&prepared, &effect).unwrap();
            let completion = runtime
                .complete_settled(&effect, HspSettledTransport::Unknown, 1000, Some(&facts))
                .unwrap();
            journal.complete_hsp(&claim, &completion).unwrap();
        } else {
            journal.cancel_hsp_effect(&prepared).unwrap();
        }
    }
    assert_eq!(
        DeviceEffectJournal::validate_existing(&journal.db).unwrap(),
        12
    );
    let history = native_history(&journal.db);
    let target = output.join("journal.sqlite3");
    journal
        .db
        .execute("VACUUM INTO ?1", [target.to_str().unwrap()])
        .unwrap();
    std::fs::File::open(&target).unwrap().sync_all().unwrap();
    let copy =
        Connection::open_with_flags(&target, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(DeviceEffectJournal::validate_existing(&copy).unwrap(), 12);
    assert_eq!(native_history(&copy), history);
    drop(copy);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o400)).unwrap();
    }
    let mut files = vec![
        serde_json::json!({"path":"journal.sqlite3","sha256":native_hash(&target),"bytes":std::fs::metadata(&target).unwrap().len()}),
    ];
    files.push(native_write(
        &output,
        "history.json",
        &serde_json::to_vec_pretty(&history).unwrap(),
    ));
    let image = std::env::current_exe().unwrap();
    let fixture = serde_json::json!({"schema_version":1,"journal_schema_version":12,"synthetic_fixture":true,"qualified":false,"production_acceptance_created":false,"real_device_contacted":false,"physical_effect_observed":false,"dispatch_authority_exported":false,
        "original_boot":original(1).boot,"preparation_ids":preparation_ids,"stream_ids":stream_ids,
        "preparation_states":["unknown","acknowledged","acknowledged"],"child_states":[null,"cancelled","unknown"],"custody_transferred":[false,false,true],
        "creation_path":"prepare_hsp_preparation -> claim/complete original synchronization -> exact child link -> cancel or first Setup claim -> VACUUM INTO",
        "limitation":"Authored journal metadata only. No credentials, network, physical measurements, human acceptance or restored native clock authority. First synchronization remains Unknown; transfer occurs only with the third preparation's exact child Setup claim.",
        "source_test_image":image,"source_test_image_sha256":native_hash(&image),"files":files});
    native_write(
        &output,
        "fixture.json",
        &serde_json::to_vec_pretty(&fixture).unwrap(),
    );
    crate::artifacts::sync_dir(&output).unwrap();
    println!(
        "{}",
        serde_json::json!({"fixture":output.join("fixture.json"),"fixture_sha256":native_hash(&output.join("fixture.json")),"preparations":3,"streams":2,"qualified":false})
    );
}

#[test]
fn hsp_preparation_capacity_refuses_new_original_but_retains_existing_metadata() {
    let (_dir, mut journal) = journal();
    for index in 1..=64 {
        journal.prepare_hsp_preparation(&original(index)).unwrap();
    }
    assert!(journal.prepare_hsp_preparation(&original(65)).is_err());
    let replay = journal.prepare_hsp_preparation(&original(1)).unwrap();
    assert_eq!(replay.id(), original(1).id().unwrap());
    assert_eq!(
        journal
            .db
            .query_row("SELECT count(*) FROM hsp_preparations", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        64
    );
    assert_eq!(
        DeviceEffectJournal::validate_existing(&journal.db).unwrap(),
        12
    );
}
