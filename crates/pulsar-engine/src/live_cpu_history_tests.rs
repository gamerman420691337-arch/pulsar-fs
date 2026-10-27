//! Authored metadata tests; no native worker or execution qualification.
use super::*;
fn artifact(n: u8) -> ArtifactIdentity {
    let sha256 = format!("{n:064x}");
    ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
        sha256,
        byte_len: 10,
    }
}
/// Authored metadata fixture only: no measured equivalence or native proof.
pub(crate) fn manifest(
    attempt: &AttemptId,
    identity: &LiveIdentity,
    clock: &LivePresentationClock,
    profile: &DeclaredInferenceProfile,
    lease_ms: u64,
) -> LiveCpuInferenceManifest {
    let mut libraries: Vec<_> = CPU_RUNTIME_REQUIRED_ALIASES
        .iter()
        .enumerate()
        .map(|(i, name)| CapturedNativeLibrary {
            identity: if *name == "libonnxruntime.so.1" {
                profile.runtime.clone()
            } else {
                artifact(i as u8 + 20)
            },
            names: vec![name.to_string()],
        })
        .collect();
    libraries.sort_by(|a, b| a.identity.sha256.cmp(&b.identity.sha256));
    LiveCpuInferenceManifest::new(
        LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: attempt.clone(),
            identity: identity.clone(),
            clock: clock.clone(),
            model: ArtifactReference {
                path: LIVE_CPU_MODEL_PATH.into(),
                identity: profile.model.clone(),
            },
            runtime: ArtifactReference {
                path: LIVE_CPU_RUNTIME_PATH.into(),
                identity: profile.runtime.clone(),
            },
            worker: artifact(251),
            profile: profile.clone(),
            budget: ResourceBudget {
                memory_bytes: profile.minimum_memory_bytes.max(256 << 20),
                output_bytes: profile.minimum_output_bytes.max(4096),
                wall_time_ms: lease_ms,
                cpu_threads: profile.cpu_threads,
            },
        },
        CpuRuntimeImage {
            schema_version: 1,
            owner_policy: artifact(250),
            libraries,
        },
    )
    .unwrap()
}
fn report() -> LiveCaptureReport {
    let mut value: LiveCaptureReport = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/live-cpu-capture-report.json"
    ))
    .unwrap();
    value.received_frames = 0;
    value.last_frame = None;
    value.server_elapsed = Some(LiveClockTick(0));
    let info = value.inference.as_mut().unwrap();
    info.state = LiveCaptureInferenceState::Starting;
    info.completed = 0;
    info.accepted = 0;
    info.stale = 0;
    info.latest = None;
    info.cpu_manifest = Some(Box::new(manifest(
        &info.attempt_id,
        &value.identity,
        &value.clock,
        &value.options.inference.as_ref().unwrap().profile,
        u64::from(value.options.lease_ms),
    )));
    value.validate().unwrap();
    value
}
fn actor() -> SessionId {
    SessionId::new("actor").unwrap()
}
fn request(report: &LiveCaptureReport) -> Request {
    Request::new(
        RequestId::new("start").unwrap(),
        Command::StartLiveCapture {
            engine_epoch: report.engine_epoch.clone(),
            options: report.options.clone(),
        },
    )
    .with_session(actor())
    .in_project(
        report.identity.project.clone(),
        Some(report.identity.revision),
    )
}
fn schema(db: &Connection) {
    db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=20;CREATE TABLE projects(id TEXT PRIMARY KEY,revision INTEGER NOT NULL);CREATE TABLE sessions(id TEXT PRIMARY KEY);CREATE TABLE requests(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(session,request));CREATE TABLE request_identities(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(session,request));INSERT INTO projects VALUES('project',3);INSERT INTO sessions VALUES('actor');").unwrap();
}
fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    schema(&db);
    db
}
fn remember_fixture(tx: &Transaction<'_>, request: &Request, body: &ResponseBody) -> Result<()> {
    let session = request.session.as_ref().unwrap().as_str();
    let fingerprint = digest(&request_fingerprint(request)?);
    tx.execute(
        "INSERT INTO request_identities VALUES(?1,?2,?3)",
        params![session, request.request_id.as_str(), fingerprint],
    )?;
    remember(tx, session, request, &fingerprint, body)?;
    tx.execute(
        "INSERT INTO requests VALUES(?1,?2,?3,?4)",
        params![
            session,
            request.request_id.as_str(),
            fingerprint,
            serde_json::to_string(body)?
        ],
    )?;
    Ok(())
}
fn install(db: &mut Connection) -> (Request, LiveCaptureReport) {
    let report = report();
    let request = request(&report);
    let tx = db.transaction().unwrap();
    remember_fixture(
        &tx,
        &request,
        &ResponseBody::LiveCapture(Box::new(report.clone())),
    )
    .unwrap();
    tx.commit().unwrap();
    validate(db).unwrap();
    (request, report)
}
fn page(report: &LiveCaptureReport) -> LiveRecordingPage {
    LiveRecordingPage {
        cpu_manifest: report.inference.as_ref().unwrap().cpu_manifest.clone(),
        browser_media: report.options.browser_media.clone(),
        model_pack: report.options.model_pack.clone(),
        source_time_basis: report.options.source_time_basis,
        schema: 1,
        engine_epoch: report.engine_epoch.clone(),
        identity: report.identity.clone(),
        clock: report.clock.clone(),
        profile: report.options.inference.as_ref().unwrap().profile.clone(),
        inference_attempt: report.inference.as_ref().unwrap().attempt_id.clone(),
        recording_request_id: RequestId::new("record").unwrap(),
        maximum_samples: 1,
        total_samples: 0,
        discarded: false,
        exhausted: false,
        active: false,
        retirement: Some(LiveCaptureRetirement::Stopped),
        after: 0,
        next: None,
        samples: vec![],
        qualified: false,
    }
}
fn raw(db: &Connection) -> i64 {
    db.query_row("PRAGMA main.user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn origin_roundtrip_keeps_mode_and_strips_credential_before_serialization() {
    let mut db = db();
    let report = report();
    let request = request(&report).with_auth_token("authored-sensitive-sentinel".into());
    let tx = db.transaction().unwrap();
    remember_fixture(
        &tx,
        &request,
        &ResponseBody::LiveCapture(Box::new(report.clone())),
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(raw(&db), 38);
    validate(&db).unwrap();
    let bytes: Vec<u8> = db
        .query_row("SELECT origin FROM live_cpu_admissions", [], |r| r.get(0))
        .unwrap();
    assert!(!String::from_utf8(bytes.clone())
        .unwrap()
        .contains("authored-sensitive-sentinel"));
    let origin: Origin = serde_json::from_slice(&bytes).unwrap();
    assert!(origin.request.auth_token.is_none());
    assert_ne!(
        origin.fingerprint,
        digest(&request_fingerprint(&origin.request).unwrap())
    );
    assert_eq!(
        origin.fingerprint,
        digest(&request_fingerprint(&request).unwrap())
    );
    assert!(same(&origin.manifest, cpu(&report).unwrap()).unwrap());
}
#[test]
fn original_request_and_admission_rollback_with_the_reader_barrier() {
    let mut db = db();
    let report = report();
    let request = request(&report);
    let tx = db.transaction().unwrap();
    remember_fixture(&tx, &request, &ResponseBody::LiveCapture(Box::new(report))).unwrap();
    assert_eq!(raw(&tx), 38);
    tx.rollback().unwrap();
    assert_eq!(raw(&db), 20);
    assert!(!layout(&db).unwrap());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM requests", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    validate(&db).unwrap();
}
#[test]
fn database_reopen_validates_metadata_without_reconstructing_live_capture() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    schema(&db);
    let (_, mut report) = install(&mut db);
    drop(db);
    let db =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    db.execute_batch("PRAGMA query_only=ON").unwrap();
    validate(&db).unwrap();
    report.active = false;
    report.retirement = Some(LiveCaptureRetirement::EngineRestarted);
    report.server_elapsed = None;
    report.admitted_pixel_bytes = 0;
    let info = report.inference.as_mut().unwrap();
    info.state = LiveCaptureInferenceState::Retired;
    info.latest = None;
    require_capture(&db, &actor(), &report).unwrap();
    assert!(cpu(&report).is_some());
    assert_eq!(
        db.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn record_pages_require_original_actor_boot_and_exact_admitted_lease() {
    let mut db = db();
    let (_, report) = install(&mut db);
    let page = page(&report);
    page.validate().unwrap();
    validate_recording_page(&db, &actor(), &page).unwrap();
    assert!(
        validate_recording_page(&db, &SessionId::new("different-actor").unwrap(), &page).is_err()
    );
    for case in 0..5 {
        let mut altered = page.clone();
        match case {
            0 => altered.engine_epoch = AttemptId::new("different-boot").unwrap(),
            1 => altered.cpu_manifest = None,
            2 => {
                let m = altered.cpu_manifest.take().unwrap();
                let mut stream = m.stream;
                stream.budget.wall_time_ms -= 1;
                altered.cpu_manifest = Some(Box::new(
                    LiveCpuInferenceManifest::new(stream, m.image).unwrap(),
                ));
            }
            3 => {
                let m = altered.cpu_manifest.take().unwrap();
                let mut image = m.image;
                image.owner_policy = artifact(249);
                altered.cpu_manifest = Some(Box::new(
                    LiveCpuInferenceManifest::new(m.stream, image).unwrap(),
                ));
            }
            _ => altered.recording_request_id = RequestId::new("another-recording-intent").unwrap(),
        }
        if case == 4 {
            // Recording intent is boot-local; this store does not fabricate a
            // durable recording-intent authorization that does not exist.
            validate_recording_page(&db, &actor(), &altered).unwrap();
        } else {
            assert!(
                validate_recording_page(&db, &actor(), &altered).is_err(),
                "case {case}"
            );
        }
    }
}
#[test]
fn later_report_and_page_cannot_create_a_new_admission_or_strip_existing_mode() {
    let mut db = db();
    let (initial, report) = install(&mut db);
    let mut stopped = report.clone();
    stopped.active = false;
    stopped.retirement = Some(LiveCaptureRetirement::Stopped);
    stopped.admitted_pixel_bytes = 0;
    stopped.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Retired;
    let mut request = initial.clone();
    request.request_id = RequestId::new("stop").unwrap();
    request.command = Command::StopLiveCapture {
        epoch: report.identity.epoch.clone(),
    };
    let tx = db.transaction().unwrap();
    remember_fixture(
        &tx,
        &request,
        &ResponseBody::LiveCapture(Box::new(stopped.clone())),
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM live_cpu_admissions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    stopped.inference.as_mut().unwrap().cpu_manifest = None;
    assert!(require_capture(&db, &actor(), &stopped).is_err());
    let mut orphan = report.clone();
    orphan.identity.epoch = AttemptId::new("orphan").unwrap();
    assert!(require_capture(&db, &actor(), &orphan).is_err());
    validate(&db).unwrap();
}
#[test]
fn altered_index_origin_receipt_namespace_or_reader_refuses_read_only() {
    for sql in [
        "UPDATE live_cpu_admissions SET owner='other'",
        "UPDATE live_cpu_admissions SET request='other'",
        "UPDATE live_cpu_admissions SET project='other'",
        "UPDATE live_cpu_admissions SET revision=revision+1",
        "UPDATE live_cpu_admissions SET attempt='other'",
        "UPDATE live_cpu_admissions SET origin=CAST(origin AS TEXT)",
        "UPDATE live_cpu_admissions SET origin=CAST(origin || ' ' AS BLOB)",
        "UPDATE live_cpu_admissions SET origin=zeroblob(2097153)",
        "UPDATE requests SET fingerprint=replace(fingerprint,substr(fingerprint,1,1),'z')",
        "UPDATE requests SET response=response || ' '",
        "UPDATE request_identities SET fingerprint='absent'",
        "DELETE FROM sessions",
        "DELETE FROM requests",
        "DELETE FROM live_cpu_admissions",
        "DROP TABLE live_cpu_admissions",
        "PRAGMA user_version=20",
        "DROP TABLE live_cpu_admissions;DROP TABLE live_cpu_reader;PRAGMA user_version=20",
        "CREATE TRIGGER bad AFTER INSERT ON live_cpu_admissions BEGIN SELECT RAISE(IGNORE);END",
        "CREATE TEMP TABLE live_cpu_admissions(epoch TEXT)",
    ] {
        let mut db = db();
        install(&mut db);
        db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        db.execute_batch(sql).unwrap();
        let changes: i64 = db
            .query_row("SELECT total_changes()", [], |r| r.get(0))
            .unwrap();
        let before = raw(&db);
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        assert!(validate(&db).is_err(), "mutation accepted: {sql}");
        assert_eq!(raw(&db), before);
        assert_eq!(
            db.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            changes
        );
    }
}
#[test]
fn escaped_json_keys_cannot_hide_orphan_mode_from_startup() {
    let mut db = db();
    install(&mut db);
    db.execute_batch("PRAGMA foreign_keys=OFF;DROP TABLE live_cpu_admissions;DROP TABLE live_cpu_reader;PRAGMA user_version=20").unwrap();
    let response: String = db
        .query_row("SELECT response FROM requests", [], |r| r.get(0))
        .unwrap();
    db.execute(
        "UPDATE requests SET response=?1",
        [response
            .replace("cpu_manifest", "cpu_\\u006danifest")
            .replace("\"kind\"", "\"k\\u0069nd\"")],
    )
    .unwrap();
    assert!(validate(&db).is_err());
}
#[test]
fn pack_resolution_only_fills_absent_inference_for_original_selected_choice() {
    let mut resolved = report().options;
    let choice: ModelPackChoice = serde_json::from_value(serde_json::json!({
        "manifest_sha256":"a".repeat(64), "selection_revision":1
    }))
    .unwrap();
    resolved.model_pack = Some(choice);
    let mut original = resolved.clone();
    original.inference = None;
    request_options(&original, &resolved).unwrap();
    original.model_pack = None;
    assert!(request_options(&original, &resolved).is_err());
    original = resolved.clone();
    original.lease_ms -= 1;
    assert!(request_options(&original, &resolved).is_err());
    original = resolved.clone();
    original.inference.as_mut().unwrap().model_path = "/different/model".into();
    assert!(request_options(&original, &resolved).is_err());
}

fn continuation(prior: &LiveCaptureReport, at: bool) -> (Request, LiveCaptureReport) {
    let anchor = SourceTimestamp::new(1, 1).unwrap();
    let options = prior.continuation_options(anchor, 20_000).unwrap();
    let mut next = prior.clone();
    next.identity.epoch = AttemptId::new(if at {
        "continue-at-epoch"
    } else {
        "continue-epoch"
    })
    .unwrap();
    next.options = options.clone();
    next.clock = options.clock(next.identity.epoch.clone());
    next.continuation = Some(LiveCaptureContinuation {
        predecessor: prior.identity.clone(),
        predecessor_clock: prior.clock.clone(),
        predecessor_elapsed: LiveClockTick(1),
        predecessor_last_frame: None,
    });
    let info = next.inference.as_mut().unwrap();
    info.attempt_id = AttemptId::new(if at {
        "continue-at-attempt"
    } else {
        "continue-attempt"
    })
    .unwrap();
    info.cpu_manifest = Some(Box::new(manifest(
        &info.attempt_id,
        &next.identity,
        &next.clock,
        &next.options.inference.as_ref().unwrap().profile,
        20_000,
    )));
    next.validate().unwrap();
    let command = if at {
        Command::ContinueLiveCaptureAt {
            engine_epoch: next.engine_epoch.clone(),
            predecessor: prior.identity.epoch.clone(),
            source_anchor: anchor,
            lease_ms: 20_000,
        }
    } else {
        Command::ContinueLiveCapture {
            engine_epoch: next.engine_epoch.clone(),
            predecessor: prior.identity.epoch.clone(),
            options,
        }
    };
    let request = Request::new(
        RequestId::new(if at { "continue-at" } else { "continue" }).unwrap(),
        command,
    )
    .with_session(actor())
    .in_project(next.identity.project.clone(), Some(next.identity.revision));
    (request, next)
}
#[test]
fn original_continue_and_continue_at_kinds_bind_the_retained_predecessor() {
    for at in [false, true] {
        let mut db = db();
        let (_, prior) = install(&mut db);
        let (request, next) = continuation(&prior, at);
        let tx = db.transaction().unwrap();
        remember_fixture(
            &tx,
            &request,
            &ResponseBody::LiveCapture(Box::new(next.clone())),
        )
        .unwrap();
        tx.commit().unwrap();
        validate(&db).unwrap();
        let (origin, _) = load_base(&db, &next.identity.epoch).unwrap().unwrap();
        assert_eq!(
            std::mem::discriminant(&origin.request.command),
            std::mem::discriminant(&request.command)
        );
        let mut page = page(&next);
        page.cpu_manifest
            .as_mut()
            .unwrap()
            .stream
            .budget
            .wall_time_ms = 19_999;
        assert!(validate_recording_page(&db, &actor(), &page).is_err());
        db.execute_batch(
            "PRAGMA foreign_keys=OFF;DELETE FROM live_cpu_admissions WHERE epoch='epoch'",
        )
        .unwrap();
        assert!(validate(&db).is_err());
    }
}
fn browser(mut report: LiveCaptureReport) -> LiveCaptureReport {
    let source = report.identity.source.clone();
    let binding = BrowserMediaBinding {
        preparation_request_id: RequestId::new("media-prepare").unwrap(),
        lease_id: ArtifactId::new("media-lease").unwrap(),
        engine_epoch: report.engine_epoch.clone(),
        project_id: report.identity.project.clone(),
        revision: report.identity.revision,
        selection: BrowserMediaSelection {
            source_version: source.clone(),
            source: artifact(200),
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("placement").unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: source,
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(10, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: ProjectTime::ZERO,
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            },
        },
        clock: DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
        },
        lease_ms: 30_000,
    };
    report.options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
    report.options.exact_project_anchor = Some(ExactSeconds::new(0, 1).unwrap());
    report.options.browser_media = Some(binding);
    report.options.browser_analysis=Some(serde_json::from_value(serde_json::json!({
        "feed_id":"feed", "timing_binding_id":"timing", "player":{"launch_id":"launch","page_id":"page","player_id":"visible", "source_id":"source","generation":1,"time_basis":"source_media_presentation"},
        "policy":{"minimum_lead_us":100,"target_lead_us":1000,"maximum_lead_us":2000}
    })).unwrap());
    report.clock = report.options.clock(report.identity.epoch.clone());
    let info = report.inference.as_mut().unwrap();
    info.cpu_manifest = Some(Box::new(manifest(
        &info.attempt_id,
        &report.identity,
        &report.clock,
        &report.options.inference.as_ref().unwrap().profile,
        30_000,
    )));
    report.validate().unwrap();
    report
}
#[test]
fn browser_successor_nested_receipt_binds_original_kind_and_derived_options() {
    let mut db = db();
    let prior = browser(report());
    let start = request(&prior);
    let tx = db.transaction().unwrap();
    remember_fixture(
        &tx,
        &start,
        &ResponseBody::LiveCapture(Box::new(prior.clone())),
    )
    .unwrap();
    tx.commit().unwrap();
    let mut options = BrowserAnalysisSuccessorOptions {
        browser_media: prior.options.browser_media.clone().unwrap(),
        browser_analysis: prior.options.browser_analysis.clone().unwrap(),
        lease_ms: 20_000,
    };
    options.browser_media.preparation_request_id = RequestId::new("next-media-prepare").unwrap();
    options.browser_media.lease_id = ArtifactId::new("next-media-lease").unwrap();
    options.browser_analysis.feed_id = ArtifactId::new("next-feed").unwrap();
    options.browser_analysis.timing_binding_id = ArtifactId::new("next-timing").unwrap();
    let mut next = prior.clone();
    next.identity.epoch = AttemptId::new("successor").unwrap();
    next.options.browser_media = Some(options.browser_media.clone());
    next.options.browser_analysis = Some(options.browser_analysis.clone());
    next.options.lease_ms = options.lease_ms;
    next.clock = next.options.clock(next.identity.epoch.clone());
    let info = next.inference.as_mut().unwrap();
    info.attempt_id = AttemptId::new("successor-attempt").unwrap();
    info.cpu_manifest = Some(Box::new(manifest(
        &info.attempt_id,
        &next.identity,
        &next.clock,
        &next.options.inference.as_ref().unwrap().profile,
        20_000,
    )));
    next.validate().unwrap();
    let request = Request::new(
        RequestId::new("successor-prepare").unwrap(),
        Command::PrepareBrowserAnalysisSuccessor {
            engine_epoch: next.engine_epoch.clone(),
            predecessor: prior.identity.epoch.clone(),
            options,
        },
    )
    .with_session(actor())
    .in_project(next.identity.project.clone(), Some(next.identity.revision));
    let successor = BrowserAnalysisSuccessorReport {
        original_request_id: request.request_id.clone(),
        predecessor: prior.identity.clone(),
        successor: next.clone(),
        phase: BrowserAnalysisSuccessorPhase::Preparing,
        transfer_request_id: None,
    };
    let body = ResponseBody::BrowserAnalysisSuccessor(Box::new(successor));
    let tx = db.transaction().unwrap();
    remember_fixture(&tx, &request, &body).unwrap();
    tx.commit().unwrap();
    validate(&db).unwrap();
    validate_recording_page(&db, &actor(), &page(&next)).unwrap();
    let (mut origin, _) = load_base(&db, &next.identity.epoch).unwrap().unwrap();
    assert!(initial_report(&origin, &ResponseBody::LiveCapture(Box::new(next.clone()))).is_err());
    origin.request.command = Command::StartLiveCapture {
        engine_epoch: next.engine_epoch.clone(),
        options: next.options.clone(),
    };
    assert!(initial_report(&origin, &body).is_err());
    let mut altered = next;
    altered.options.browser_analysis.as_mut().unwrap().feed_id =
        ArtifactId::new("retagged-feed").unwrap();
    assert!(require_capture(&db, &actor(), &altered).is_err());
}
#[test]
fn origin_command_scope_and_token_tampering_refuse_even_with_canonical_json() {
    for case in 0..5 {
        let mut db = db();
        let (_, report) = install(&mut db);
        let bytes: Vec<u8> = db
            .query_row("SELECT origin FROM live_cpu_admissions", [], |r| r.get(0))
            .unwrap();
        let mut origin: Origin = serde_json::from_slice(&bytes).unwrap();
        match case {
            0 => origin.request.auth_token = Some("forbidden-retained-token".into()),
            1 => origin.request.session = Some(SessionId::new("foreign-actor").unwrap()),
            2 => {
                origin.request.command = Command::ContinueLiveCaptureAt {
                    engine_epoch: report.engine_epoch.clone(),
                    predecessor: AttemptId::new("unbound-prior").unwrap(),
                    source_anchor: report.options.source_anchor,
                    lease_ms: report.options.lease_ms,
                }
            }
            3 => origin.request.expected_revision = Some(RevisionId::new(2)),
            _ => {
                origin.manifest = manifest(
                    &origin.manifest.stream.attempt_id,
                    &origin.manifest.stream.identity,
                    &origin.manifest.stream.clock,
                    &origin.manifest.stream.profile,
                    29_999,
                )
            }
        }
        db.execute(
            "UPDATE live_cpu_admissions SET origin=?1",
            [serde_json::to_vec(&origin).unwrap()],
        )
        .unwrap();
        assert!(validate(&db).is_err(), "origin mutation {case}");
    }
}
