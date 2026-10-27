//! Synthetic retained history for native reader campaigns; never dispatch authority.
use super::hsp::{HspJournalState, HspStreamJournalState, HspStreamReceipt};
use super::{Connection, DeviceEffectJournal};
use crate::handy_hsp_records::{HspBindingRecord, HspStreamRecord};
use crate::handy_hsp_runtime::{
    HandyHspRuntime, HspAuthorityWindow, HspBinding, HspDispatchFacts, HspEffect, HspEffectKind,
    HspObservedOutcome, HspPlayReference, HspRuntimeState, HspSettledTransport,
};
use pulsar_core::{
    ArtifactId, AttemptId, ClientId, DeviceId, DeviceSessionId, HspPhaseWindow, HspPoint,
    HspPolicy, HspSample, HspStreamIdentity, LiveIdentity, ProjectId, RequestId, RevisionId,
    SessionId, SourceVersionId,
};
use pulsar_protocol::{HandyIdentity, HandyRegistration, HandyRegistrationSettings};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn binding() -> HspBinding {
    HspBinding::new(
        HspStreamIdentity {
            generation: 1,
            stream_id: 42,
            live: LiveIdentity {
                epoch: AttemptId::new("synthetic-hsp-native-epoch").unwrap(),
                project: ProjectId::new("synthetic-hsp-native-project").unwrap(),
                revision: RevisionId::new(1),
                source: SourceVersionId::new("synthetic-hsp-native-source").unwrap(),
            },
        },
        ClientId::new("synthetic-hsp-native-owner").unwrap(),
        DeviceSessionId::new("synthetic-hsp-native-session").unwrap(),
        ArtifactId::new("synthetic-hsp-native-clock").unwrap(),
    )
}

fn record() -> HspStreamRecord {
    HspStreamRecord {
        boot: DeviceSessionId::new("synthetic-hsp-native-boot").unwrap(),
        actor_session: SessionId::new("synthetic-hsp-native-actor").unwrap(),
        request_id: RequestId::new("synthetic-hsp-native-request").unwrap(),
        registration: HandyRegistration {
            device_id: DeviceId::new("synthetic-hsp-native-device").unwrap(),
            revision: 1,
            settings: HandyRegistrationSettings {
                label: "Synthetic history only; no physical evidence".into(),
                api_key_env: "SYNTHETIC_HSP_API".into(),
                connection_key_env: "SYNTHETIC_HSP_ROUTE".into(),
            },
            disabled: false,
            binding_sha256: "a".repeat(64),
        },
        device: HandyIdentity {
            firmware: "4.0".into(),
            model_number: 2,
            model_name: "Handy".into(),
            model_variant: 0,
            session_id: "synthetic-hsp-native-device-session".into(),
            firmware_status: 0,
        },
        route_sha256: "b".repeat(64),
        route_namespace_sha256: "c".repeat(64),
        binding: HspBindingRecord::capture(&binding()),
        qualification_sha256: "d".repeat(64),
        profile_sha256: "e".repeat(64),
    }
}

fn facts() -> HspDispatchFacts {
    HspDispatchFacts::new(
        binding(),
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

fn offer(runtime: &mut HandyHspRuntime, start: u32, sequence: u64) {
    for offset in 0..2 {
        runtime
            .offer(
                &binding().identity().live,
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

fn complete(
    journal: &mut DeviceEffectJournal,
    stream: &HspStreamReceipt,
    runtime: &mut HandyHspRuntime,
    effect: &HspEffect,
    acknowledgment: Option<(i32, u8)>,
) -> String {
    let prepared = journal.prepare_hsp_effect(stream, effect).unwrap();
    let claim = journal.claim_hsp(&prepared, effect).unwrap();
    let body = acknowledgment.map(|(tail, state)| {
        serde_json::to_vec(&json!({"result": {
            "play_state":state,"points":(tail+1).max(0),"max_points":8,
            "current_point":if tail<0 {-1} else {0},"current_time":-100,
            "loop":false,"playback_rate":1,"first_point_time":0,
            "last_point_time":tail.max(0) as u32*100,"stream_id":42,
            "tail_point_stream_index":tail,"tail_point_stream_index_threshold":1,
            "pause_on_starving":false
        }}))
        .unwrap()
    });
    let settled = match &body {
        Some(body) => HspSettledTransport::Response { status: 200, body },
        None => HspSettledTransport::Unknown,
    };
    let completion = runtime
        .complete_settled(effect, settled, 100, Some(&facts()))
        .unwrap();
    assert_eq!(
        matches!(completion.outcome, HspObservedOutcome::Acknowledged(_)),
        acknowledgment.is_some(),
    );
    let observed = journal.complete_hsp(&claim, &completion).unwrap();
    assert_eq!(
        observed.state,
        if acknowledgment.is_some() {
            HspJournalState::Acknowledged
        } else {
            HspJournalState::Unknown
        }
    );
    assert!(journal.claim_hsp(&prepared, effect).is_err());
    prepared.id().to_owned()
}

fn rows(db: &Connection, query: &str) -> Value {
    let mut statement = db.prepare(query).unwrap();
    let columns = statement.column_count();
    let values = statement
        .query_map([], |row| {
            Ok((0..columns)
                .map(|column| match row.get_ref(column).unwrap() {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(value) => json!(value),
                    rusqlite::types::ValueRef::Text(value) => {
                        json!(std::str::from_utf8(value).unwrap())
                    }
                    _ => panic!("unexpected HSP snapshot column type"),
                })
                .collect::<Vec<_>>())
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    json!(values)
}

fn history(db: &Connection) -> Value {
    json!({
        "streams": rows(db, "SELECT id,intent,route,namespace,device,record,state FROM hsp_streams ORDER BY id"),
        "effects": rows(db, "SELECT id,stream,sequence,kind,record,state,outcome,outcome_sha256,late,late_sha256,lifecycle_sha256 FROM hsp_effects ORDER BY sequence,id"),
        "census": rows(db, "SELECT singleton,streams,ordinary,stops,stream_bytes,ordinary_bytes,stop_bytes FROM hsp_census ORDER BY singleton"),
    })
}

fn fresh_output() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("PULSAR_HSP_NATIVE_OUTPUT")
            .expect("set PULSAR_HSP_NATIVE_OUTPUT to a fresh absolute export directory"),
    );
    assert!(path.is_absolute());
    assert!(path.components().all(|part| matches!(
        part,
        std::path::Component::RootDir | std::path::Component::Normal(_)
    )));
    assert!(path.starts_with("/mnt/4tb/tmp") || path.starts_with("/mnt/4tb/artifacts"));
    let parent = path.parent().unwrap();
    assert_eq!(parent.canonicalize().unwrap(), parent);
    std::fs::create_dir(&path).expect("HSP export directory must not already exist");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    path
}

fn hash_file(path: &Path) -> String {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    format!("{:x}", hash.finalize())
}

fn file_metadata(path: &Path) -> Value {
    json!({"name":path.file_name().unwrap().to_str().unwrap(),
        "sha256":hash_file(path),"bytes":path.metadata().unwrap().len()})
}

fn write_snapshot(output: &Path, name: &str, bytes: &[u8]) -> Value {
    let path = output.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o400).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    file_metadata(&path)
}

#[test]
#[ignore = "exports synthetic HSP history for a separately supervised native preceding-reader/v12 campaign"]
fn export_populated_hsp_journal_for_native_reader_campaign() {
    let output = fresh_output();
    let temporary = tempfile::Builder::new()
        .prefix("hsp-export-")
        .tempdir_in("/mnt/4tb/tmp")
        .unwrap();
    let original = record();
    let mut journal = DeviceEffectJournal::open(temporary.path(), original.boot.clone()).unwrap();
    let stream = journal.prepare_hsp_stream(&original).unwrap();
    let mut runtime = HandyHspRuntime::new(
        binding(),
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
    let mut ids = Vec::new();
    let setup = runtime.next_effect(100, Some(&facts())).unwrap().unwrap();
    assert_eq!(setup.kind(), HspEffectKind::Setup);
    ids.push(complete(
        &mut journal,
        &stream,
        &mut runtime,
        &setup,
        Some((-1, 2)),
    ));
    offer(&mut runtime, 0, 1);
    let initial = runtime.next_effect(100, Some(&facts())).unwrap().unwrap();
    assert_eq!(initial.kind(), HspEffectKind::Append);
    ids.push(complete(
        &mut journal,
        &stream,
        &mut runtime,
        &initial,
        Some((1, 2)),
    ));
    runtime.request_play(100, &facts()).unwrap();
    let play = runtime.next_effect(100, Some(&facts())).unwrap().unwrap();
    assert_eq!(play.kind(), HspEffectKind::Play);
    ids.push(complete(
        &mut journal,
        &stream,
        &mut runtime,
        &play,
        Some((1, 1)),
    ));
    offer(&mut runtime, 200, 3);
    let ambiguous = runtime.next_effect(100, Some(&facts())).unwrap().unwrap();
    assert_eq!(ambiguous.kind(), HspEffectKind::Append);
    ids.push(complete(
        &mut journal,
        &stream,
        &mut runtime,
        &ambiguous,
        None,
    ));
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    let stop = runtime.next_effect(100, None).unwrap().unwrap();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    ids.push(complete(
        &mut journal,
        &stream,
        &mut runtime,
        &stop,
        Some((1, 2)),
    ));
    assert_eq!(runtime.state(), HspRuntimeState::StopAcknowledged);
    assert_eq!(
        journal.hsp_effect(&ids[3]).unwrap().state,
        HspJournalState::Unknown
    );
    let retained = journal.hsp_stream(stream.id()).unwrap();
    assert_eq!(retained.state, HspStreamJournalState::Custody);
    assert_eq!(retained.effect_ids.len(), 5);
    assert!(journal.cancel_hsp_stream(&stream).is_err());
    assert_eq!(
        DeviceEffectJournal::validate_existing(&journal.db).unwrap(),
        12
    );
    let expected = history(&journal.db);
    let target = output.join("journal.sqlite3");
    journal
        .db
        .execute("VACUUM INTO ?1", [target.to_str().unwrap()])
        .unwrap();
    std::fs::File::open(&target).unwrap().sync_all().unwrap();
    let copy =
        Connection::open_with_flags(&target, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(DeviceEffectJournal::validate_existing(&copy).unwrap(), 12);
    assert_eq!(history(&copy), expected);
    drop(copy);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o400)).unwrap();
    }
    let mut files = vec![file_metadata(&target)];
    files.push(write_snapshot(
        &output,
        "history.json",
        &serde_json::to_vec_pretty(&expected).unwrap(),
    ));
    files.push(write_snapshot(
        &output,
        "stream-record.json",
        &retained.record.to_json().unwrap(),
    ));
    for (index, id) in ids.iter().enumerate() {
        let observed = journal.hsp_effect(id).unwrap();
        let (stored, outcome): (String, String) = journal
            .db
            .query_row(
                "SELECT record,outcome FROM hsp_effects WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(observed.record.to_json().unwrap(), stored.as_bytes());
        assert_eq!(
            serde_json::to_vec(observed.outcome.as_ref().unwrap()).unwrap(),
            outcome.as_bytes()
        );
        files.push(write_snapshot(
            &output,
            &format!("effect-{}.json", index + 1),
            stored.as_bytes(),
        ));
        files.push(write_snapshot(
            &output,
            &format!("outcome-{}.json", index + 1),
            outcome.as_bytes(),
        ));
    }
    let image = std::env::current_exe().unwrap();
    let metadata = json!({
        "schema_version":1,"journal_schema_version":12,"synthetic_fixture":true,
        "qualified":false,"production_acceptance_created":false,"real_device_contacted":false,
        "physical_effect_observed":false,"dispatch_authority_exported":false,
        "original_boot":original.boot,"stream_id":stream.id(),"effect_ids":ids,
        "stream_state":"custody","effect_kinds":["setup","append","play","append","stop"],
        "effect_states":["acknowledged","acknowledged","acknowledged","unknown","acknowledged"],
        "creation_path":"HandyHspRuntime -> prepare_hsp_stream -> prepare_hsp_effect -> claim_hsp -> complete_hsp -> VACUUM INTO",
        "limitation":"All bindings, reference digests, timing facts and outcomes are synthetic fixtures. No credentials, network, device, physical measurement, qualification or human acceptance was used. Stop acknowledgment retains unresolved stream custody and the earlier append remains unknown. History is not restored authority.",
        "source_test_image":image,"source_test_image_sha256":hash_file(&image),"files":files,
    });
    write_snapshot(
        &output,
        "fixture.json",
        &serde_json::to_vec_pretty(&metadata).unwrap(),
    );
    drop(journal);
    temporary.close().unwrap();
    println!(
        "synthetic HSP native fixture exported to {}",
        output.display()
    );
}
