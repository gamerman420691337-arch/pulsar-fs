//! Authored pending synthetic metadata, not qualification or acceptance.
use super::*;
fn subject() -> HandyHspSourceSubject {
    let artifact = |c: char, byte_len| {
        let sha256 = c.to_string().repeat(64);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len,
        }
    };
    let media = binding();
    HandyHspSourceSubject {
        schema_version: 1,
        origin: HandyEvidenceOrigin::Synthetic,
        context: HandyQualificationContext {
            schema_version: 1,
            control_image: artifact('a', 1),
            platform: "linux/authored-fixture".into(),
            transport: ArtifactId::new(HANDY_HSP_QUALIFICATION_TRANSPORT).unwrap(),
        },
        device_qualification: HandyQualificationBinding {
            dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
            revision: 2,
        },
        browser_environment: artifact('c', 16),
        project_id: media.project_id,
        revision: media.revision,
        selection: media.selection,
        clock: media.clock,
        inference_profile: DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact('d', 32),
            runtime: artifact('e', 64),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::NearestStretch,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0f32 / 255.0; 6],
                bias: vec![0.; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        },
        cpu_runtime_image: None,
        source_time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        recipe: HandyHspSourceRecipe::RvfcRgb64x64NearestStretchV1,
        age_basis: LiveCaptureAgeBasis::ProducerDeclared,
        age_observation: HandyHspSourceAgeObservation::HttpReceiptMonotonic,
        lookahead: BrowserAnalysisLookaheadPolicy {
            target_lead_us: 500_000,
            minimum_lead_us: 100_000,
            maximum_lead_us: 1_000_000,
        },
        maximum_analysis_age_ns: 500_000_000,
        maximum_capture_to_engine_receipt_ns: 100_000_000,
        finite_drain_policy_sha256: None,
        timing: HandyBrowserStartTiming {
            clock: BrowserClockAssumptions {
                timestamp_error_ns: 1000,
                relative_rate_error_ppm: 100,
                maximum_age_ns: 1_000_000_000,
                maximum_interval_ns: 10_000_000,
            },
            reference: HandyBrowserStartReference::ExpectedDisplay,
            display_early_ns: 1_000_000,
            display_late_ns: 2_000_000,
            source_correspondence_error_ns: 1000,
            discrete_media_phase_error_ns: 1_000_000,
            media_rate_error_ppm: 100,
            physical_execution_error_ns: 2_000_000,
            maximum_start_age_ns: 500_000_000,
            maximum_alignment_error_ns: 20_000_000,
        },
    }
}
fn binding() -> BrowserMediaBinding {
    BrowserMediaBinding {
        preparation_request_id: RequestId::new("media-prepare").unwrap(),
        lease_id: ArtifactId::new("media-lease").unwrap(),
        engine_epoch: AttemptId::new("boot").unwrap(),
        project_id: ProjectId::new("project").unwrap(),
        revision: RevisionId::new(2),
        selection: BrowserMediaSelection {
            source_version: SourceVersionId::new("source").unwrap(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("source-bytes").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("placement").unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: SourceVersionId::new("source").unwrap(),
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
            presentation_anchor: SourceTimestamp::new(5, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
        },
        lease_ms: 1000,
    }
}

fn image() -> ArtifactIdentity {
    ArtifactIdentity {
        artifact_id: ArtifactId::new("f".repeat(64)).unwrap(),
        sha256: "f".repeat(64),
        byte_len: 16,
    }
}
fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA user_version=20;PRAGMA foreign_keys=ON;CREATE TABLE projects(id TEXT PRIMARY KEY,revision INTEGER);CREATE TABLE sessions(id TEXT PRIMARY KEY);CREATE TABLE requests(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(session,request));CREATE TABLE request_identities(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(session,request));INSERT INTO projects VALUES('project',2);INSERT INTO sessions VALUES('actor');").unwrap();
    db
}
fn raw(db: &Connection) -> i64 {
    db.query_row("PRAGMA main.user_version", [], |r| r.get(0))
        .unwrap()
}
fn empty_admissions(db: &Connection) {
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM live_cpu_admissions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
fn pending(captured: bool) -> HandyHspSourceQualificationReceipt {
    let mut subject = subject();
    if captured {
        subject.cpu_runtime_image = Some(image());
    }
    let evidence = HandyHspSourceEvidenceKind::ALL
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            let bytes = format!("authored evidence {i}");
            let sha256 = digest(bytes.as_bytes());
            let identity = ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len: bytes.len() as u64,
            };
            if kind == HandyHspSourceEvidenceKind::EnvironmentConfiguration {
                subject.browser_environment = identity.clone();
            }
            HandyHspSourceQualificationEvidence { kind, identity }
        })
        .collect();
    let manifest = HandyHspSourceQualificationManifest { subject, evidence };
    let report = HandyHspSourceQualificationReport {
        dossier_id: manifest.identity().unwrap(),
        revision: 1,
        state: HandyQualificationState::Pending,
        manifest,
        decision: None,
    };
    let receipt = HandyHspSourceQualificationReceipt {
        input_sha256: Some("a".repeat(64)),
        request_id: RequestId::new("source-import").unwrap(),
        report,
    };
    receipt.validate().unwrap();
    receipt
}
fn store(db: &mut Connection, captured: bool) -> HandyHspSourceQualificationReceipt {
    let receipt = pending(captured);
    let request = Request::new(
        receipt.request_id.clone(),
        Command::ImportHandyHspSourceQualification {
            path: "/authored/input.json".into(),
            manifest_sha256: "a".repeat(64),
            owner_proof: "authored-owner-fixture".into(),
        },
    )
    .with_session(SessionId::new("actor").unwrap())
    .in_project(ProjectId::new("project").unwrap(), Some(RevisionId::new(2)));
    let body = ResponseBody::HandyHspSourceQualificationSaved(Box::new(receipt.clone()));
    let tx = db.transaction().unwrap();
    // Deliberately no capture admission exists; this guards the common writer.
    remember(&tx, "actor", &request, &"a".repeat(64), &body).unwrap();
    tx.execute(
        "INSERT INTO requests VALUES('actor',?1,?2,?3)",
        params![
            request.request_id.as_str(),
            "a".repeat(64),
            serde_json::to_string(&body).unwrap()
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    receipt
}
#[test]
fn pending_captured_subject_alone_arms_reader_and_creates_no_capture_origin() {
    let mut db = db();
    let receipt = store(&mut db, true);
    assert_eq!(raw(&db), 38);
    empty_admissions(&db);
    assert_eq!(receipt.report.state, HandyQualificationState::Pending);
    assert_eq!(
        receipt.report.manifest.subject.origin,
        HandyEvidenceOrigin::Synthetic
    );
    assert!(receipt.report.decision.is_none());
    require_hsp_source_reader(&db, &receipt.report.manifest.subject).unwrap();
    db.execute_batch("PRAGMA query_only=ON").unwrap();
    let changes: i64 = db
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    validate(&db).unwrap();
    empty_admissions(&db);
    assert_eq!(
        db.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        changes
    );
}
#[test]
fn host_subject_leaves_original_reader_and_history_layout_unchanged() {
    let mut db = db();
    let receipt = store(&mut db, false);
    assert_eq!(raw(&db), 20);
    assert!(!layout(&db).unwrap());
    require_hsp_source_reader(&db, &receipt.report.manifest.subject).unwrap();
    validate(&db).unwrap();
    assert!(!serde_json::to_string(&receipt)
        .unwrap()
        .contains("cpu_runtime_image"));
}
#[test]
fn qualification_metadata_barrier_rolls_back_with_its_original_transaction() {
    let mut db = db();
    let receipt = pending(true);
    let tx = db.transaction().unwrap();
    arm_hsp_source_subject(&tx, &receipt.report.manifest.subject).unwrap();
    assert_eq!(raw(&tx), 38);
    empty_admissions(&tx);
    tx.rollback().unwrap();
    assert_eq!(raw(&db), 20);
    assert!(!layout(&db).unwrap());
    assert!(require_hsp_source_reader(&db, &receipt.report.manifest.subject).is_err());
    validate(&db).unwrap();
}
#[test]
fn qualification_without_capture_cannot_hide_stripped_reader_marker() {
    for sql in [
        "DROP TABLE live_cpu_admissions",
        "DROP TABLE live_cpu_reader",
        "PRAGMA user_version=20",
        "DROP TABLE live_cpu_admissions;DROP TABLE live_cpu_reader;PRAGMA user_version=20",
    ] {
        let mut db = db();
        let receipt = store(&mut db, true);
        db.execute_batch(sql).unwrap();
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        let before = raw(&db);
        assert!(
            require_hsp_source_reader(&db, &receipt.report.manifest.subject).is_err(),
            "{sql}"
        );
        assert!(validate(&db).is_err(), "{sql}");
        assert_eq!(raw(&db), before);
    }
}
#[test]
fn invalid_captured_image_does_not_arm_reader() {
    let mut db = db();
    let mut subject = subject();
    let mut bad = image();
    bad.byte_len = 0;
    subject.cpu_runtime_image = Some(bad);
    let tx = db.transaction().unwrap();
    assert!(arm_hsp_source_subject(&tx, &subject).is_err());
    tx.rollback().unwrap();
    assert_eq!(raw(&db), 20);
    assert!(!layout(&db).unwrap());
}
