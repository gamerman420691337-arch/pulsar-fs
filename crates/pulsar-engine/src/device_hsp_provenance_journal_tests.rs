use crate::handy_playback_clock::{
    HspClockPin, NativeDeviceClock, NativePlaybackClock, QualifiedDeviceClock,
};
use crate::handy_server_clock::{NativeServerClock, QualifiedServerClock, Sample as ServerSample};
use pulsar_core::{HspSourceCursor, HspSourceTransitionSpec};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn provenance_pin(
    binding: &HspBinding,
    generation: u64,
    sent: u64,
    retired: Arc<AtomicBool>,
) -> HspClockPin {
    let device = NativeDeviceClock::new(
        HandyClockSyncAcknowledgment {
            info: HandyDeviceTimeInfo {
                device_uptime_ms: 0,
                reported_clock_offset_ms: 0,
                last_sync_round_trip_ms: 0,
            },
            sent_boottime_ns: 0,
            received_boottime_ns: 0,
        },
        QualifiedDeviceClock {
            contract: HandyDeviceClockQualification {
                model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
                user_offset_ms: 0,
                device_error_ns: 1,
                maximum_sync_age_ns: 20_000,
                maximum_sync_round_trip_ms: 10,
                drift_ppm: 0,
            },
            maximum_total_error_ns: 1_000_000,
        },
    )
    .unwrap();
    let server = NativeServerClock::new(
        binding.clock_domain().clone(),
        ServerSample {
            sent_ns: sent,
            received_ns: sent,
            server_ms: 10_000,
        },
        QualifiedServerClock {
            contract: HandyServerClockQualification {
                model: HandyServerClockModel::RequestBracketMonotonic,
                timestamp_error_ns: 1,
            },
            max_age_ns: 20_000,
            max_uncertainty_ns: 1_000_000,
            drift_ppm: 0,
        },
    )
    .unwrap();
    HspClockPin::new(
        ArtifactId::new("synthetic-clock-owner").unwrap(),
        generation,
        Arc::new(NativePlaybackClock { server, device }),
        10_000,
        10_000,
        retired,
    )
    .unwrap()
}

fn provenance_facts(binding: &HspBinding, pin: &HspClockPin, at: u64) -> HspDispatchFacts {
    let clock = pin.at(at).unwrap();
    HspDispatchFacts::new(
        binding.clone(),
        HspPlayReference::new(
            at,
            HspPhaseWindow {
                earliest_ms: -102,
                latest_ms: -98,
            },
            -100,
            clock.estimated_server_ms,
        )
        .unwrap(),
        HspAuthorityWindow::new(10_000, 10_000, 10_000),
    )
    .unwrap()
    .with_clock_pin(pin.clone())
    .unwrap()
}

struct ProvenanceFixture {
    journal: DeviceEffectJournal,
    runtime: HandyHspRuntime,
    stream: HspStreamReceipt,
    record: HspStreamRecord,
    binding: HspBinding,
    last: HspSample,
}
impl ProvenanceFixture {
    fn new(mut journal: DeviceEffectJournal, record: HspStreamRecord, binding: HspBinding) -> Self {
        let source = HspSourceCursor {
            identity: binding.identity().live.clone(),
            attempt_id: AttemptId::new("synthetic-worker-a").unwrap(),
        };
        let runtime = HandyHspRuntime::new_with_source(
            binding.clone(),
            HspPolicy {
                max_sample_age_ns: 10_000,
                ..runtime().policy()
            },
            source,
        )
        .unwrap();
        let stream = journal.prepare_hsp_stream(&record).unwrap();
        Self {
            journal,
            runtime,
            stream,
            record,
            binding,
            last: HspSample {
                point: HspPoint::new(100, 50).unwrap(),
                source_sequence: 2,
                captured_at_ns: 100,
                accepted_at_ns: 100,
                continuous_from_previous: true,
            },
        }
    }
    fn next(&mut self, pin: &HspClockPin, at: u64) -> (HspEffect, PreparedHspReceipt) {
        let facts = provenance_facts(&self.binding, pin, at);
        let effect = self.runtime.next_effect(at, Some(&facts)).unwrap().unwrap();
        let prepared = self
            .journal
            .prepare_hsp_effect(&self.stream, &effect)
            .unwrap();
        (effect, prepared)
    }
    fn acknowledge(
        &mut self,
        pin: &HspClockPin,
        at: u64,
        effect: &HspEffect,
        prepared: &PreparedHspReceipt,
        tail: i32,
        playing: bool,
    ) {
        let claim = self.journal.claim_hsp(prepared, effect).unwrap();
        let facts = provenance_facts(&self.binding, pin, at);
        let completion = self
            .runtime
            .complete_settled(
                effect,
                HspSettledTransport::Response {
                    status: 200,
                    body: &response(tail, playing),
                },
                at,
                Some(&facts),
            )
            .unwrap();
        self.journal.complete_hsp(&claim, &completion).unwrap();
    }
    fn start(&mut self, pin: &HspClockPin) {
        let (setup, prepared) = self.next(pin, 100);
        self.acknowledge(pin, 100, &setup, &prepared, -1, false);
        let facts = provenance_facts(&self.binding, pin, 100);
        for index in 0..2 {
            let sample = HspSample {
                point: HspPoint::new(index * 100, 50).unwrap(),
                source_sequence: u64::from(index) + 1,
                captured_at_ns: 100,
                accepted_at_ns: 100,
                continuous_from_previous: index != 0,
            };
            self.runtime
                .offer(&self.binding.identity().live, sample, 100, &facts)
                .unwrap();
        }
        let (append, prepared) = self.next(pin, 100);
        self.acknowledge(pin, 100, &append, &prepared, 1, false);
        self.runtime.request_play(100, &facts).unwrap();
        let (play, prepared) = self.next(pin, 100);
        self.acknowledge(pin, 100, &play, &prepared, 1, true);
    }
    fn transfer(
        &mut self,
        pin: &HspClockPin,
        at: u64,
        name: &str,
    ) -> (HspEffect, PreparedHspReceipt) {
        let predecessor = self.runtime.active_source().unwrap().clone();
        let mut successor = predecessor.clone();
        successor.identity.epoch = AttemptId::new(format!("synthetic-epoch-{name}")).unwrap();
        successor.attempt_id = AttemptId::new(format!("synthetic-worker-{name}")).unwrap();
        let first = HspSample {
            point: HspPoint::new(self.last.point.time_ms + 100, 50).unwrap(),
            source_sequence: 1,
            captured_at_ns: at - 10,
            accepted_at_ns: at - 5,
            continuous_from_previous: false,
        };
        let facts = provenance_facts(&self.binding, pin, at);
        let token = self
            .runtime
            .prepare_source_transition(
                HspSourceTransitionSpec {
                    predecessor,
                    successor,
                    predecessor_tail: self.last,
                    first_successor: first,
                    expires_at_ns: 9000,
                },
                at,
                &facts,
            )
            .unwrap();
        self.runtime
            .commit_source_transition(token, at, &facts)
            .unwrap();
        let (append, prepared) = self.next(pin, at);
        let tail = append.chunk().unwrap().tail_point_stream_index() as i32;
        self.acknowledge(pin, at, &append, &prepared, tail, true);
        self.last = first;
        (append, prepared)
    }
}

#[test]
fn hsp_provenance_two_cursors_and_clock_generations_roundtrip_without_relabeling() {
    let dir = tempfile::tempdir().unwrap();
    let journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    assert_eq!(
        journal
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    let retired = Arc::new(AtomicBool::new(false));
    let first = provenance_pin(&binding(1), 1, 100, retired.clone());
    let mut f = ProvenanceFixture::new(journal, record(), binding(1));
    f.start(&first);
    let (b, pb) = f.transfer(&first, 200, "b");
    let second = provenance_pin(&binding(1), 2, 300, retired.clone());
    let (c, pc) = f.transfer(&second, 350, "c");
    assert_eq!(
        f.journal
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        13
    );
    for (effect, prepared, name, generation) in [(&b, &pb, "b", 1), (&c, &pc, "c", 2)] {
        let record = f.journal.hsp_effect(prepared.id()).unwrap().record;
        let chunk = record.chunk.as_ref().unwrap();
        assert_eq!(chunk.identity, f.record.binding.identity);
        assert_eq!(
            chunk
                .source_cursor
                .as_ref()
                .unwrap()
                .identity
                .epoch
                .as_str(),
            format!("synthetic-epoch-{name}")
        );
        assert_eq!(
            chunk.source_cursor.as_ref().unwrap().attempt_id.as_str(),
            format!("synthetic-worker-{name}")
        );
        assert_eq!(chunk.samples[0].source_sequence, 1);
        assert!(!chunk.samples[0].continuous_from_previous);
        assert_eq!(
            record
                .original_facts
                .as_ref()
                .unwrap()
                .clock_observation
                .as_ref()
                .unwrap()
                .generation,
            generation
        );
        assert_eq!(record.request.body.as_deref(), effect.request().body());
        assert_eq!(
            HspEffectRecord::from_json(&record.to_json().unwrap(), &f.record).unwrap(),
            record
        );
    }
    let before = f.journal.hsp_effect(pc.id()).unwrap().record;
    retired.store(true, Ordering::Release);
    let after = HspEffectRecord::capture(f.stream.id(), &f.record, &c).unwrap();
    assert_eq!(
        after, before,
        "retiring native authority does not erase original history"
    );
    assert!(f.journal.claim_hsp(&pc, &c).is_err());
}

#[test]
fn hsp_provenance_same_cursor_false_and_noninitial_successor_sequence_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    let pin = provenance_pin(&binding(1), 1, 100, Arc::new(AtomicBool::new(false)));
    let mut f = ProvenanceFixture::new(journal, record(), binding(1));
    f.start(&pin);
    let (_, pb) = f.transfer(&pin, 200, "b");
    let mut all = catalog(&f.journal.db).unwrap();
    let original_b = all.effects[pb.id()].record.clone();
    all.effects
        .get_mut(pb.id())
        .unwrap()
        .record
        .chunk
        .as_mut()
        .unwrap()
        .samples[0]
        .source_sequence = 2;
    assert!(validate_history(&all.streams[f.stream.id()], &all.effects).is_err());
    all.effects.get_mut(pb.id()).unwrap().record = original_b;
    let next = HspSample {
        point: HspPoint::new(300, 50).unwrap(),
        source_sequence: 2,
        captured_at_ns: 290,
        accepted_at_ns: 295,
        continuous_from_previous: true,
    };
    let source = f.runtime.active_source().unwrap().identity.clone();
    f.runtime
        .offer(&source, next, 300, &provenance_facts(&f.binding, &pin, 300))
        .unwrap();
    let (_, next_receipt) = f.next(&pin, 300);
    let mut all = catalog(&f.journal.db).unwrap();
    let sample = &mut all
        .effects
        .get_mut(next_receipt.id())
        .unwrap()
        .record
        .chunk
        .as_mut()
        .unwrap()
        .samples[0];
    sample.continuous_from_previous = false;
    sample.source_sequence = 1;
    assert!(validate_history(&all.streams[f.stream.id()], &all.effects).is_err());
}

#[test]
fn hsp_provenance_clock_descriptors_are_validated_and_bound_into_effect_digest() {
    let dir = tempfile::tempdir().unwrap();
    let journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    let pin = provenance_pin(&binding(1), 1, 100, Arc::new(AtomicBool::new(false)));
    let mut f = ProvenanceFixture::new(journal, record(), binding(1));
    let (effect, prepared) = f.next(&pin, 100);
    let original = f.journal.hsp_effect(prepared.id()).unwrap().record;
    for case in 0..4 {
        let mut changed = original.clone();
        let clock = changed
            .original_facts
            .as_mut()
            .unwrap()
            .clock_observation
            .as_mut()
            .unwrap();
        match case {
            0 => clock.generation = 0,
            1 => clock.reference_mapping.estimated_server_ms += 1,
            2 => clock.session_until_ns = 20_001,
            _ => clock.original_device_sync.sent_boottime_ns = 1,
        }
        assert!(changed.validate(&f.record).is_err());
    }
    let mut changed = original.clone();
    changed
        .original_facts
        .as_mut()
        .unwrap()
        .clock_observation
        .as_mut()
        .unwrap()
        .owner_id = ArtifactId::new("other-native-owner").unwrap();
    assert_ne!(changed.id().unwrap(), original.id().unwrap());
    assert!(changed.matches_effect(&f.record, &effect).is_err());
}

#[test]
fn hsp_provenance_upgrade_is_conditional_atomic_and_marker_is_strict() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    let legacy = journal.prepare_hsp_stream(&record()).unwrap();
    let legacy_bytes = journal
        .hsp_stream(legacy.id())
        .unwrap()
        .record
        .to_json()
        .unwrap();
    assert_eq!(
        journal
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    {
        let tx = journal
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        require_provenance_schema(&tx).unwrap();
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            13
        );
        tx.rollback().unwrap();
    }
    assert_eq!(
        journal
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        journal
            .hsp_stream(legacy.id())
            .unwrap()
            .record
            .to_json()
            .unwrap(),
        legacy_bytes
    );
    journal.db.execute_batch("PRAGMA user_version=13").unwrap();
    assert!(
        journal.hsp_stream(legacy.id()).is_err(),
        "a version number alone cannot enable extended metadata"
    );
    journal.db.execute_batch("PRAGMA user_version=12").unwrap();
    let tx = journal
        .db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    require_provenance_schema(&tx).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        journal
            .hsp_stream(legacy.id())
            .unwrap()
            .record
            .to_json()
            .unwrap(),
        legacy_bytes
    );
    journal
        .db
        .execute_batch("DELETE FROM hsp_provenance_format")
        .unwrap();
    assert!(journal.hsp_stream(legacy.id()).is_err());
}

#[test]
fn hsp_provenance_v12_rows_keep_exact_legacy_bytes_and_domain_hashes() {
    #[derive(serde::Serialize)]
    struct LegacyFacts<'a> {
        binding: &'a crate::handy_hsp_records::HspBindingRecord,
        at_ns: u64,
        earliest_ms: i64,
        latest_ms: i64,
        start_time_ms: i32,
        server_time_ms: u64,
        lease_until_ns: u64,
        source_until_ns: u64,
        clock_until_ns: u64,
    }
    #[derive(serde::Serialize)]
    struct LegacyChunk<'a> {
        identity: &'a crate::handy_hsp_records::HspStreamIdentityRecord,
        sequence: u64,
        tail_point_stream_index: u32,
        samples: &'a [crate::handy_hsp_records::HspSampleRecord],
    }
    #[derive(serde::Serialize)]
    struct LegacyEffect<'a> {
        stream_id: &'a str,
        stream_sha256: &'a str,
        sequence: u64,
        kind: HspEffectRecordKind,
        binding: &'a crate::handy_hsp_records::HspBindingRecord,
        request: &'a crate::handy_hsp_records::HspRequestRecord,
        original_facts: Option<LegacyFacts<'a>>,
        chunk: Option<LegacyChunk<'a>>,
        expected_running: bool,
    }
    let mut f = Fixture::new();
    f.setup();
    offer(&mut f.runtime, 0, 1);
    let (_, prepared) = f.next();
    let record = f.journal.hsp_effect(prepared.id()).unwrap().record;
    let original_facts = record.original_facts.as_ref().map(|v| LegacyFacts {
        binding: &v.binding,
        at_ns: v.at_ns,
        earliest_ms: v.earliest_ms,
        latest_ms: v.latest_ms,
        start_time_ms: v.start_time_ms,
        server_time_ms: v.server_time_ms,
        lease_until_ns: v.lease_until_ns,
        source_until_ns: v.source_until_ns,
        clock_until_ns: v.clock_until_ns,
    });
    let chunk = record.chunk.as_ref().map(|v| LegacyChunk {
        identity: &v.identity,
        sequence: v.sequence,
        tail_point_stream_index: v.tail_point_stream_index,
        samples: &v.samples,
    });
    let bytes = serde_json::to_vec(&LegacyEffect {
        stream_id: &record.stream_id,
        stream_sha256: &record.stream_sha256,
        sequence: record.sequence,
        kind: record.kind,
        binding: &record.binding,
        request: &record.request,
        original_facts,
        chunk,
        expected_running: record.expected_running,
    })
    .unwrap();
    assert_eq!(record.to_json().unwrap(), bytes);
    let mut hash = Sha256::new();
    hash.update(b"pulsar.handy.hsp.effect.v1\0");
    hash.update(&bytes);
    assert_eq!(record.id().unwrap(), format!("{:x}", hash.finalize()));
    assert_eq!(
        f.journal
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    drop(f.journal);
    let reopened = DeviceEffectJournal::open(
        f.dir.path(),
        DeviceSessionId::new("synthetic-boot").unwrap(),
    )
    .unwrap();
    assert_eq!(
        reopened
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        reopened
            .hsp_effect(prepared.id())
            .unwrap()
            .record
            .to_json()
            .unwrap(),
        bytes
    );
}

#[test]
#[ignore = "requires an explicitly copied sealed v12 journal and fresh evidence output; metadata-only authored fixture"]
fn hsp_provenance_native_upgrade_v12_fixture_for_old_reader_and_writer_campaign() {
    let state = std::path::PathBuf::from(
        std::env::var_os("PULSAR_HSP_V13_STATE").expect("PULSAR_HSP_V13_STATE"),
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("PULSAR_HSP_V13_OUTPUT").expect("PULSAR_HSP_V13_OUTPUT"),
    );
    std::fs::create_dir(&output).expect("fresh evidence directory");
    let probe = Connection::open_with_flags(
        state.join("device-effects/journal.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(DeviceEffectJournal::validate_existing(&probe).unwrap(), 12);
    let boot: String = probe
        .query_row("SELECT boot FROM owner WHERE singleton=1", [], |r| r.get(0))
        .unwrap();
    let legacy_rows: Vec<(String, String)> = probe
        .prepare("SELECT id,record FROM hsp_effects ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let legacy_streams: Vec<(String, String)> = probe
        .prepare("SELECT id,record FROM hsp_streams ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let prior_catalog = catalog(&probe).unwrap();
    let namespace = prior_catalog
        .streams
        .values()
        .find(|stream| {
            stream.state != HspStreamJournalState::Cancelled
                && !prior_catalog.resolved.contains(&stream.id)
        })
        .map(|stream| stream.record.route_namespace_sha256.clone())
        .unwrap_or_else(|| "c".repeat(64));
    drop(probe);
    // Same boot is deliberate in this isolated compatibility fixture: a held
    // preceding writer must refuse v13 itself, not merely a changed owner boot.
    let journal =
        DeviceEffectJournal::open(&state, DeviceSessionId::new(boot.clone()).unwrap()).unwrap();
    let native_binding = binding(9001);
    let mut native_record = record();
    native_record.boot = DeviceSessionId::new(boot).unwrap();
    native_record.request_id = RequestId::new("v13-provenance-native-fixture").unwrap();
    native_record.registration.device_id = DeviceId::new("v13-provenance-native-device").unwrap();
    native_record.route_sha256 = "1".repeat(64);
    native_record.route_namespace_sha256 = namespace;
    native_record.binding = crate::handy_hsp_records::HspBindingRecord::capture(&native_binding);
    let mut f = ProvenanceFixture::new(journal, native_record, native_binding.clone());
    let retired = Arc::new(AtomicBool::new(false));
    let first = provenance_pin(&native_binding, 1, 100, retired.clone());
    f.start(&first);
    let (_, b) = f.transfer(&first, 200, "b");
    let second = provenance_pin(&native_binding, 2, 300, retired);
    let (_, c) = f.transfer(&second, 350, "c");
    for (id, original) in &legacy_rows {
        let retained: String = f
            .journal
            .db
            .query_row("SELECT record FROM hsp_effects WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(&retained, original);
    }
    for (id, original) in &legacy_streams {
        let retained: String = f
            .journal
            .db
            .query_row("SELECT record FROM hsp_streams WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(&retained, original);
    }
    assert_eq!(
        DeviceEffectJournal::validate_existing(&f.journal.db).unwrap(),
        13
    );
    let report = serde_json::json!({"schema_version":1,"journal_before":12,"journal_after":13,"same_boot":true,"legacy_effect_rows_preserved":legacy_rows.len(),"legacy_stream_rows_preserved":legacy_streams.len(),"source_b":f.journal.hsp_effect(b.id()).unwrap().record,"source_c":f.journal.hsp_effect(c.id()).unwrap().record,"synthetic_metadata_only":true,"physical_acceptance":false,"native_authority_restored":false});
    std::fs::write(
        output.join("v13-upgrade.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[test]
fn hsp_provenance_failed_extended_prepare_rolls_back_schema_and_legacy_rows() {
    let mut f = Fixture::new();
    f.setup();
    let before = f.journal.hsp_stream(f.stream.id()).unwrap();
    let original = f.journal.hsp_effect(&before.effect_ids[0]).unwrap().record;
    let source = HspSourceCursor {
        identity: binding(1).identity().live.clone(),
        attempt_id: AttemptId::new("synthetic-worker-a").unwrap(),
    };
    let mut conflicting =
        HandyHspRuntime::new_with_source(binding(1), runtime().policy(), source).unwrap();
    let pin = provenance_pin(&binding(1), 1, 100, Arc::new(AtomicBool::new(false)));
    let facts = provenance_facts(&binding(1), &pin, 100);
    let effect = conflicting.next_effect(100, Some(&facts)).unwrap().unwrap();
    assert!(
        f.journal.prepare_hsp_effect(&f.stream, &effect).is_err(),
        "same stream cannot gain another setup sequence"
    );
    assert_eq!(
        f.journal
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        f.journal
            .db
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='hsp_provenance_format'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        f.journal.hsp_effect(&before.effect_ids[0]).unwrap().record,
        original
    );
}

#[test]
fn hsp_provenance_clock_generation_cannot_describe_two_different_observations() {
    let dir = tempfile::tempdir().unwrap();
    let journal = DeviceEffectJournal::open(dir.path(), record().boot).unwrap();
    let first = provenance_pin(&binding(1), 1, 100, Arc::new(AtomicBool::new(false)));
    let mut f = ProvenanceFixture::new(journal, record(), binding(1));
    f.start(&first);
    let second = provenance_pin(&binding(1), 2, 300, Arc::new(AtomicBool::new(false)));
    let (_, prepared) = f.transfer(&second, 350, "b");
    let mut all = catalog(&f.journal.db).unwrap();
    all.effects
        .get_mut(prepared.id())
        .unwrap()
        .record
        .original_facts
        .as_mut()
        .unwrap()
        .clock_observation
        .as_mut()
        .unwrap()
        .generation = 1;
    assert!(validate_history(&all.streams[f.stream.id()], &all.effects).is_err());
}
