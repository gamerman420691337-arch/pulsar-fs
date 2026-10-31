//! Authored metadata only; no accepted equivalence, native work or device calls.
use super::*;

fn artifact(n: u8) -> ArtifactIdentity {
    let sha256 = format!("{n:064x}");
    ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
        sha256,
        byte_len: 100,
    }
}
pub(super) fn report() -> LiveCaptureReport {
    let mut report = crate::live_cpu_history::authored_capture_for_test();
    let identity = report.identity.clone();
    let media = BrowserMediaBinding {
        preparation_request_id: RequestId::new("prepare-media").unwrap(),
        lease_id: ArtifactId::new("media-lease").unwrap(),
        engine_epoch: report.engine_epoch.clone(),
        project_id: identity.project.clone(),
        revision: identity.revision,
        selection: BrowserMediaSelection {
            source_version: identity.source.clone(),
            source: artifact(171),
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("placement").unwrap(),
                created_revision: RevisionId::new(0),
                spec: SourcePlacementSpec {
                    source_version: identity.source.clone(),
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
        lease_ms: 30000,
    };
    report.options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
    report.options.source_anchor = SourceTimestamp::new(0, 1).unwrap();
    report.options.project_anchor = ProjectTime::ZERO;
    report.options.exact_project_anchor = Some(ExactSeconds::from_project(ProjectTime::ZERO));
    report.options.presentation_anchor = LiveClockTick(0);
    report.options.browser_media = Some(media);
    report.options.browser_analysis = Some(BrowserAnalysisCaptureDeclaration {
        feed_id: ArtifactId::new("feed").unwrap(),
        timing_binding_id: ArtifactId::new("timing").unwrap(),
        player: HandyPlayerIdentity {
            launch_id: AttemptId::new("launch").unwrap(),
            page_id: AttemptId::new("page").unwrap(),
            player_id: AttemptId::new("player").unwrap(),
            source_id: AttemptId::new("dom-source").unwrap(),
            generation: 1,
            time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        },
        policy: BrowserAnalysisLookaheadPolicy {
            minimum_lead_us: 1,
            target_lead_us: 1000,
            maximum_lead_us: 2000,
        },
    });
    let selection = BrowserInferenceFallbackSelection {
        schema_version: 1,
        tier: InferenceEquivalenceTier::Default,
        axis: Axis::Stroke,
        maximum_fallbacks: 1,
        equivalents: vec![InferenceFallbackReference {
            dossier_id: ArtifactId::new("1".repeat(64)).unwrap(),
            expected_revision: 1,
            expected_report_sha256: "2".repeat(64),
        }],
        source_policy: BrowserInferenceFallbackSourcePolicy::any_bound_media(),
        view_policy: BrowserInferenceFallbackViewPolicy {
            schema_version: 1,
            namespace: BrowserInferenceFallbackSourceNamespace::BrowserMediaCapture,
            applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
        },
    };
    report.options.inference_fallback = Some(selection.clone());
    report.clock = report.options.clock(identity.epoch);
    report.queue = None;
    let semantics = browser_inference_semantic_identity(&report.options).unwrap();
    let info = report.inference.as_mut().unwrap();
    let manifest = info.cpu_manifest.as_mut().unwrap();
    manifest.stream.clock = report.clock.clone();
    info.fallback = Some(BrowserInferenceFallbackAdmission {
        schema_version: 1,
        selection: selection.identity().unwrap(),
        reference: selection.equivalents[0].clone(),
        semantic_identity: semantics,
        observed_environment: InferenceEquivalenceEnvironment {
            engine: artifact(120),
            worker: manifest.stream.worker.clone(),
            os: "linux".into(),
            architecture: "x86_64".into(),
            platform_build: artifact(121),
            hardware: artifact(122),
            dependencies: vec![manifest.stream.profile.runtime.clone()],
        },
        admitted_at_unix_ms: 100,
        expires_at_unix_ms: 200,
        ordinal: 0,
        predecessor: None,
        history: Vec::new(),
    });
    report.validate().unwrap();
    report
}
fn fixture() -> (Connection, Request, LiveCaptureReport) {
    let report = report();
    let request = Request::new(
        RequestId::new("start").unwrap(),
        Command::StartLiveCapture {
            engine_epoch: report.engine_epoch.clone(),
            options: report.options.clone(),
        },
    )
    .with_session(SessionId::new("actor").unwrap())
    .in_project(
        report.identity.project.clone(),
        Some(report.identity.revision),
    );
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=20;CREATE TABLE projects(id TEXT PRIMARY KEY,revision INTEGER NOT NULL);CREATE TABLE sessions(id TEXT PRIMARY KEY);CREATE TABLE requests(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(session,request));CREATE TABLE request_identities(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(session,request));INSERT INTO sessions VALUES('actor');").unwrap();
    db.execute(
        "INSERT INTO projects VALUES(?1,?2)",
        params![
            report.identity.project.as_str(),
            i64::try_from(report.identity.revision.value()).unwrap()
        ],
    )
    .unwrap();
    (db, request, report)
}
fn install(db: &mut Connection, request: &Request, report: &LiveCaptureReport) {
    let tx = db.transaction().unwrap();
    let body = ResponseBody::LiveCapture(Box::new(report.clone()));
    let fingerprint = hash(&request_fingerprint(request).unwrap());
    tx.execute(
        "INSERT INTO request_identities VALUES('actor',?1,?2)",
        params![request.request_id.as_str(), fingerprint],
    )
    .unwrap();
    remember(&tx, "actor", request, &fingerprint, &body).unwrap();
    tx.execute(
        "INSERT INTO requests VALUES('actor',?1,?2,?3)",
        params![
            request.request_id.as_str(),
            fingerprint,
            serde_json::to_string(&body).unwrap()
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    validate(db).unwrap();
}
fn page(report: &LiveCaptureReport) -> LiveRecordingPage {
    let mut page = crate::live_cpu_history::authored_recording_page_for_test(report);
    page.retirement = report
        .retirement
        .clone()
        .or(Some(LiveCaptureRetirement::Stopped));
    page.fallback = BrowserInferenceRecordingProvenance::from_capture(report)
        .unwrap()
        .map(Box::new);
    page.validate().unwrap();
    page
}

#[test]
fn recording_requires_exact_original_fallback_options_admission_and_actor() {
    let (mut db, request, report) = fixture();
    install(&mut db, &request, &report);
    let actor = request.session.as_ref().unwrap();
    let good = page(&report);
    validate_recording_page(&db, actor, &good).unwrap();
    for fault in [
        "strip",
        "clock",
        "policy",
        "reference",
        "attempt",
        "environment",
    ] {
        let mut changed = good.clone();
        match fault {
            "strip" => changed.fallback = None,
            "clock" => changed.clock.presentation_anchor = LiveClockTick(1),
            "policy" => {
                changed
                    .fallback
                    .as_mut()
                    .unwrap()
                    .options
                    .policy
                    .maximum_analysis_age_ns += 1
            }
            "reference" => {
                changed
                    .fallback
                    .as_mut()
                    .unwrap()
                    .admission
                    .reference
                    .expected_revision += 1
            }
            "attempt" => changed.inference_attempt = AttemptId::new("foreign-attempt").unwrap(),
            _ => {
                changed
                    .fallback
                    .as_mut()
                    .unwrap()
                    .admission
                    .observed_environment
                    .hardware = artifact(123)
            }
        }
        assert!(
            validate_recording_page(&db, actor, &changed).is_err(),
            "{fault}"
        );
    }
    assert!(validate_recording_page(&db, &SessionId::new("foreign").unwrap(), &good).is_err());
}

#[test]
fn admission_index_refuses_stripped_tampered_shadowed_or_unbound_receipts() {
    for sql in [
        "DELETE FROM browser_fallback_admissions",
        "DROP TABLE browser_fallback_admissions",
        "UPDATE browser_fallback_admissions SET response_sha256=printf('%064d',0)",
        "UPDATE browser_fallback_admissions SET revision=revision+1",
        "UPDATE request_identities SET fingerprint=printf('%064d',0)",
        "UPDATE requests SET response='{}'",
        "UPDATE requests SET response=zeroblob(1048577)",
        "CREATE TEMP TABLE browser_fallback_admissions(x)",
        "CREATE TRIGGER injected AFTER INSERT ON browser_fallback_admissions BEGIN SELECT 1; END",
        "PRAGMA user_version=20",
    ] {
        let (mut db, request, report) = fixture();
        install(&mut db, &request, &report);
        db.execute_batch(sql).unwrap();
        assert!(validate(&db).is_err(), "{sql}");
        assert!(
            validate_recording_page(&db, request.session.as_ref().unwrap(), &page(&report))
                .is_err(),
            "{sql}"
        );
    }
}

#[test]
fn deferred_receipt_failure_rolls_back_reader_and_admission_together() {
    let (mut db, request, report) = fixture();
    let body = ResponseBody::LiveCapture(Box::new(report));
    let tx = db.transaction().unwrap();
    let fingerprint = hash(&request_fingerprint(&request).unwrap());
    tx.execute(
        "INSERT INTO request_identities VALUES('actor','start',?1)",
        [&fingerprint],
    )
    .unwrap();
    remember(&tx, "actor", &request, &fingerprint, &body).unwrap();
    assert!(tx.commit().is_err()); // Exact original request receipt was never inserted.
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        20
    );
    assert!(!exists(&db, TABLE).unwrap());
    validate(&db).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn recording_assembly_preserves_failed_epoch_and_accepts_only_its_declared_target_chain() {
    let mut target = report();
    let mut manifest = crate::cuda_mounts::live_test_witness().manifest;
    let old_model = manifest.stream.model.identity.clone();
    let model = target
        .options
        .inference
        .as_ref()
        .unwrap()
        .profile
        .model
        .clone();
    manifest.stream.model.identity = model.clone();
    manifest.stream.profile.model = model.clone();
    manifest.dependencies.retain(|id| *id != old_model);
    if !manifest.dependencies.contains(&model) {
        manifest.dependencies.push(model);
    }
    manifest.stream.identity = target.identity.clone();
    manifest.stream.identity.epoch = AttemptId::new("failed-epoch").unwrap();
    manifest.stream.attempt_id = AttemptId::new("failed-attempt").unwrap();
    manifest.stream.clock = target.clock.clone();
    manifest.stream.clock.epoch = manifest.stream.identity.epoch.clone();
    manifest.validate().unwrap();
    let failure = BrowserInferenceFallbackFailure {
        observation: InferenceProviderUnavailable {
            schema_version: 1,
            provider: InferenceProvider::Cuda,
            profile_sha256: crate::inference_profiles::profile_identity(&manifest.stream.profile)
                .unwrap()
                .sha256,
            check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
        },
        manifest: Box::new(manifest),
    };
    let mut previous = target.clone();
    previous.identity = failure.manifest.stream.identity.clone();
    previous.clock = failure.manifest.stream.clock.clone();
    previous.options.inference.as_mut().unwrap().profile = failure.manifest.stream.profile.clone();
    previous.options.lease_ms = u32::try_from(failure.manifest.stream.budget.wall_time_ms).unwrap();
    previous.options.browser_analysis.as_mut().unwrap().feed_id =
        ArtifactId::new("failed-feed").unwrap();
    previous.active = false;
    previous.admitted_pixel_bytes = 0;
    previous.retirement = Some(LiveCaptureRetirement::WorkerFailed);
    let info = previous.inference.as_mut().unwrap();
    info.cpu_manifest = None;
    info.attempt_id = failure.manifest.stream.attempt_id.clone();
    info.profile_sha256 = failure.observation.profile_sha256.clone();
    info.state = LiveCaptureInferenceState::Failed;
    info.error = Some(ProtocolError::new(
        ErrorCode::Unavailable,
        "authored original failure",
    ));
    let admission = info.fallback.as_mut().unwrap();
    admission.observed_environment.worker = failure.manifest.stream.worker.clone();
    admission.observed_environment.dependencies =
        vec![failure.manifest.stream.profile.runtime.clone()];
    info.fallback_transition = Some(BrowserInferenceFallbackTransition {
        failure: failure.clone(),
        phase: BrowserInferenceFallbackPhase::Started,
        next_epoch: Some(target.identity.epoch.clone()),
        error: None,
    });
    previous.validate().unwrap();
    let target_hash = target.inference.as_ref().unwrap().profile_sha256.clone();
    let admission = target
        .inference
        .as_mut()
        .unwrap()
        .fallback
        .as_mut()
        .unwrap();
    admission.ordinal = 1;
    admission.predecessor = Some(previous.identity.clone());
    admission.history.push(BrowserInferenceFallbackStep {
        failure,
        reference: admission.reference.clone(),
        next_profile_sha256: target_hash.clone(),
    });
    target.active = false;
    target.admitted_pixel_bytes = 0;
    target.retirement = Some(LiveCaptureRetirement::Stopped);
    target.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Retired;
    target.validate().unwrap();
    let before = page(&previous);
    let mut after = page(&target);
    after.recording_request_id = RequestId::new("target-recording").unwrap();
    after.total_samples = 1;
    let pixels = ArtifactIdentity {
        artifact_id: ArtifactId::new("b".repeat(64)).unwrap(),
        sha256: "b".repeat(64),
        byte_len: LIVE_FRAME_BYTES as u64,
    };
    let input = LiveInferenceInput {
        sequence: 1,
        previous: pixels.clone(),
        current: pixels,
        pair: RawFramePair {
            source_epoch: target.identity.epoch.clone(),
            previous_frame: FrameId::new(0),
            current_frame: FrameId::new(1),
            previous_time: SourceTimestamp::new(0, 1).unwrap(),
            current_time: SourceTimestamp::new(1, 10).unwrap(),
            presentation_clock: Some(target.clock.clone()),
        },
    };
    after.samples.push(LiveCaptureEstimate {
        age_basis: LiveCaptureAgeBasis::ProducerDeclared,
        evidence: EvidenceKind::Inferred,
        qualified: false,
        result: LiveInferenceReply::CapturedCpuEstimated {
            attempt_id: after.inference_attempt.clone(),
            identity: after.identity.clone(),
            profile_sha256: target_hash,
            manifest_sha256: after
                .cpu_manifest
                .as_ref()
                .unwrap()
                .identity_sha256()
                .unwrap(),
            input,
            position: NormalizedPosition::new(0.5).unwrap(),
            pair_analysis_elapsed_ns: 1,
        },
        capture_tick: LiveClockTick(1),
        accepted_at: LiveClockTick(2),
        capture_to_analysis_ns: 1,
        capture_age_ns: 1,
        fresh: true,
    });
    after.validate().unwrap();
    let range = TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000_000)).unwrap();
    let assembled =
        assemble_live_recordings(&[before.clone(), after.clone()], range, 100_000_000).unwrap();
    assert!(!assembled.contains_cuda_provisional);
    assert_eq!(assembled.recording_requests.len(), 2);
    let bytes = serde_json::to_vec(&after).unwrap();
    let restored: LiveRecordingPage = serde_json::from_slice(&bytes).unwrap();
    assert!(same(&after.fallback, &restored.fallback).unwrap());
    for fault in ["stripped", "wrong-link", "unfinished", "changed-admission"] {
        let mut first = before.clone();
        let mut next = after.clone();
        match fault {
            "stripped" => next.fallback = None,
            "wrong-link" => {
                first
                    .fallback
                    .as_mut()
                    .unwrap()
                    .transition
                    .as_mut()
                    .unwrap()
                    .next_epoch = Some(AttemptId::new("unrelated").unwrap())
            }
            "unfinished" => {
                let t = first
                    .fallback
                    .as_mut()
                    .unwrap()
                    .transition
                    .as_mut()
                    .unwrap();
                t.phase = BrowserInferenceFallbackPhase::Preparing;
                t.next_epoch = None;
            }
            _ => {
                next.fallback.as_mut().unwrap().admission.history[0].next_profile_sha256 =
                    "0".repeat(64)
            }
        }
        assert!(
            assemble_live_recordings(&[first, next], range, 100_000_000).is_err(),
            "{fault}"
        );
    }
}
