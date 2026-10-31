//! Authored acknowledgement/evidence shapes only. No native execution, actual
//! measurements, reviewer acceptance or device qualification occurs here.
use super::*;
use std::io::Cursor;

fn authored_sample(page: &mut LiveRecordingPage) {
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
            source_epoch: page.identity.epoch.clone(),
            previous_frame: FrameId::new(0),
            current_frame: FrameId::new(1),
            previous_time: SourceTimestamp::new(0, 1).unwrap(),
            current_time: SourceTimestamp::new(1, 10).unwrap(),
            presentation_clock: Some(page.clock.clone()),
        },
    };
    page.total_samples = 1;
    page.samples.clear();
    page.samples.push(LiveCaptureEstimate {
        age_basis: LiveCaptureAgeBasis::ProducerDeclared,
        evidence: EvidenceKind::Inferred,
        qualified: false,
        result: LiveInferenceReply::CapturedCpuEstimated {
            attempt_id: page.inference_attempt.clone(),
            identity: page.identity.clone(),
            profile_sha256: digest(encode(&page.profile).unwrap().as_bytes()),
            manifest_sha256: page
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
    page.validate().unwrap();
}

const MEDIA: &[u8] = b"AUTHORED MEDIA BYTES - NO DECODE OR MEASUREMENT";
fn fixture() -> (Connection, PackageSource, Recording, Vec<u8>) {
    let mut report = crate::browser_fallback_history::authored_capture_for_archive_test();
    let media_hash = digest(MEDIA);
    report
        .options
        .browser_media
        .as_mut()
        .unwrap()
        .selection
        .source = ArtifactIdentity {
        artifact_id: ArtifactId::new(media_hash.clone()).unwrap(),
        sha256: media_hash,
        byte_len: MEDIA.len() as u64,
    };
    report
        .inference
        .as_mut()
        .unwrap()
        .fallback
        .as_mut()
        .unwrap()
        .semantic_identity = browser_inference_semantic_identity(&report.options).unwrap();
    let (mut db, equivalence, _) =
        super::super::inference_equivalence_store::resolver_store_tests::browser_archive_record(
            &report,
        );
    let reference = InferenceFallbackReference {
        dossier_id: equivalence.dossier_id.clone(),
        expected_revision: equivalence.revision,
        expected_report_sha256: digest(encode(&equivalence).unwrap().as_bytes()),
    };
    report
        .options
        .inference_fallback
        .as_mut()
        .unwrap()
        .equivalents = vec![reference.clone()];
    let selection = report
        .options
        .inference_fallback
        .as_ref()
        .unwrap()
        .identity()
        .unwrap();
    let info = report.inference.as_mut().unwrap();
    let admission = info.fallback.as_mut().unwrap();
    admission.observed_environment = equivalence.manifest.subject.from_environment.clone();
    admission.selection = selection;
    admission.reference = reference;
    admission.admitted_at_unix_ms = 4000;
    admission.expires_at_unix_ms = 10000;
    report.validate().unwrap();
    db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,revision INTEGER NOT NULL);CREATE TABLE sessions(id TEXT PRIMARY KEY);INSERT INTO sessions VALUES('actor');").unwrap();
    db.execute(
        "INSERT INTO projects VALUES(?1,?2)",
        params![
            report.identity.project.as_str(),
            report.identity.revision.value()
        ],
    )
    .unwrap();
    let actor = SessionId::new("actor").unwrap();
    let request = Request::new(
        RequestId::new("archive-original-start").unwrap(),
        Command::StartLiveCapture {
            engine_epoch: report.engine_epoch.clone(),
            options: report.options.clone(),
        },
    )
    .with_session(actor.clone())
    .in_project(
        report.identity.project.clone(),
        Some(report.identity.revision),
    );
    let body = ResponseBody::LiveCapture(Box::new(report.clone()));
    let fp = digest(&request_fingerprint(&request).unwrap());
    let tx = db.transaction().unwrap();
    tx.execute(
        "INSERT INTO request_identities VALUES(?1,?2,?3)",
        params![actor.as_str(), request.request_id.as_str(), fp],
    )
    .unwrap();
    crate::browser_fallback_history::remember(&tx, actor.as_str(), &request, &fp, &body).unwrap();
    tx.execute(
        "INSERT INTO requests VALUES(?1,?2,?3,?4)",
        params![
            actor.as_str(),
            request.request_id.as_str(),
            fp,
            encode(&body).unwrap()
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    let mut page = crate::live_cpu_history::authored_recording_page_for_test(&report);
    page.fallback = BrowserInferenceRecordingProvenance::from_capture(&report)
        .unwrap()
        .map(Box::new);
    authored_sample(&mut page);
    let options = LiveRecordingStageOptions {
        engine_epoch: report.engine_epoch.clone(),
        recordings: vec![LiveRecordingSelection {
            epoch: page.identity.epoch.clone(),
            recording_request_id: page.recording_request_id.clone(),
        }],
        range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000_000)).unwrap(),
        maximum_interpolation_ns: 100_000_000,
    };
    let raw=serde_json::to_vec(&serde_json::json!({"schema":1,"kind":"engine-owned-live-recording-source","qualified":false,"stage_request_id":"archive-stage","actor":actor,"options":options,"source_time_basis":page.source_time_basis,"contains_cuda_provisional":false,"browser_media":page.browser_media,"pages":[page]})).unwrap();
    let hash = digest(&raw);
    let source = PackageSource {
        browser_fallback_history: None,
        source_version: SourceVersionId::new("recording-snapshot").unwrap(),
        kind: SourceKind::LiveRecording,
        label: "Authored recording provenance".into(),
        identity: ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash,
            byte_len: raw.len() as u64,
        },
        pinned: true,
        evicted: false,
        stash_associations: vec![],
    };
    let recording = Recording::parse(&raw).unwrap();
    (db, source, recording, raw)
}
fn manifest(
    source: PackageSource,
    recording: &Recording,
    payload: &[u8],
) -> PortableProjectManifest {
    let page = &recording.pages[0];
    let revision = page.identity.revision;
    let media = page.browser_media.as_ref().unwrap();
    let placement = media.selection.placement.clone();
    let placement_id = placement.placement_id.clone();
    let sha = digest(b"{\"tracks\":[]}");
    let motion = ProgramDescriptor {
        artifact_id: ArtifactId::new(format!("motion.{sha}")).unwrap(),
        sha256: sha.clone(),
        byte_len: 13,
        codec: MotionCodec::MotionProgramJsonV1,
        axes: vec![],
    };
    let mut m = PortableProjectManifest {
        format_version: 17,
        origin_project_id: recording.project().clone(),
        captured_revision: revision,
        captured_event_cursor: 0,
        completeness: PackageCompleteness::default(),
        counts: PackageRecordCounts::default(),
        head: PackageProjectHead {
            name: "Authored archive fixture".into(),
            revision,
            history_cursor: 0,
            motion: motion.clone(),
            protected: vec![],
        },
        revisions: (0..=revision.value())
            .map(|r| PackageRevision {
                revision: RevisionId::new(r),
                actor: "actor".into(),
                kind: "authored".into(),
                label: String::new(),
                motion: motion.clone(),
                protected: vec![],
            })
            .collect(),
        edit_states: vec![PackageEditState {
            position: 0,
            motion,
            protected: vec![],
            source_revision: None,
        }],
        revision_lineage: vec![],
        candidates: vec![],
        authored_lineage: vec![],
        sources: vec![
            source.clone(),
            PackageSource {
                browser_fallback_history: None,
                source_version: media.selection.source_version.clone(),
                kind: SourceKind::Media,
                label: "Authored undecoded media".into(),
                identity: media.selection.source.clone(),
                pinned: true,
                evicted: false,
                stash_associations: vec![],
            },
        ],
        legacy_cells: vec![],
        generated_origins: vec![],
        export_receipts: vec![],
        objects: vec![
            PackageObjectDescriptor {
                sha256: sha,
                byte_len: 13,
                roles: vec![PackageObjectRole::MotionProgram],
            },
            PackageObjectDescriptor {
                sha256: source.identity.sha256.clone(),
                byte_len: source.identity.byte_len,
                roles: vec![PackageObjectRole::SourceSnapshot],
            },
            PackageObjectDescriptor {
                sha256: digest(payload),
                byte_len: payload.len() as u64,
                roles: vec![PackageObjectRole::BrowserInferenceFallbackHistory],
            },
        ],
        imported_origins: vec![],
        placements: Some(PackagePlacementClosure {
            definitions: vec![placement],
            active: vec![placement_id.clone()],
            revisions: (0..=revision.value())
                .map(|r| PackageRevisionPlacements {
                    revision: RevisionId::new(r),
                    placements: vec![placement_id.clone()],
                })
                .collect(),
            history: vec![PackageHistoryPlacements {
                position: 0,
                placements: vec![placement_id],
            }],
        }),
        retiming_receipts: vec![],
    };
    m.objects.push(PackageObjectDescriptor {
        sha256: media.selection.source.sha256.clone(),
        byte_len: media.selection.source.byte_len,
        roles: vec![PackageObjectRole::SourceSnapshot],
    });
    m.sources
        .sort_by(|a, b| a.source_version.cmp(&b.source_version));
    m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    m.counts = m.record_counts().unwrap();
    m.validate().unwrap();
    m
}
fn package(
    m: &PortableProjectManifest,
    objects: &BTreeMap<String, Vec<u8>>,
) -> PackageArtifactDescriptor {
    let mut raw = Vec::new();
    let limits = crate::project_package_format::PackageCodecLimits::default();
    crate::project_package_format::write_package(
        m,
        &mut raw,
        |o| Ok(Cursor::new(objects[&o.sha256].clone())),
        limits,
    )
    .unwrap();
    let read = crate::project_package_format::read_package(
        &mut Cursor::new(&raw),
        |_| Ok(std::io::sink()),
        limits,
    )
    .unwrap();
    assert_eq!(
        read.manifest.canonical_bytes().unwrap(),
        m.canonical_bytes().unwrap()
    );
    PackageArtifactDescriptor {
        format_version: m.format_version,
        sha256: digest(&raw),
        byte_len: raw.len() as u64,
        manifest_sha256: digest(&m.canonical_bytes().unwrap()),
        project_id: m.origin_project_id.clone(),
        captured_revision: m.captured_revision,
        captured_event_cursor: m.captured_event_cursor,
        object_count: m.objects.len() as u32,
        verification: PackageReadyVerification::AllDeclaredObjectsVerified,
    }
}
#[test]
fn browser_archive_codec_and_two_clone_projections_preserve_inert_evidence() {
    let (mut db, mut source, recording, raw) = fixture();
    let tx = db.transaction().unwrap();
    let bytes = capture(&tx, &source, &recording, 256 * 1024 * 1024)
        .unwrap()
        .unwrap();
    tx.rollback().unwrap();
    let p = Payload::decode(&bytes).unwrap();
    p.validate(&recording).unwrap();
    assert_eq!(p.documents.len(), 8);
    assert_eq!(p.header.admissions.len(), 1);
    source.browser_fallback_history = Some(PackageObjectRef {
        sha256: digest(&bytes),
        byte_len: bytes.len() as u64,
    });
    let mut node = manifest(source.clone(), &recording, &bytes);
    let mut objects = BTreeMap::from([
        (digest(&raw), raw),
        (digest(&bytes), bytes.clone()),
        (digest(b"{\"tracks\":[]}"), b"{\"tracks\":[]}".to_vec()),
    ]);
    objects.insert(digest(MEDIA), MEDIA.to_vec());
    let mut origins = BTreeMap::new();
    for name in ["clone-one", "clone-two"] {
        validate_graph(&node, &origins, |r| Ok(objects[&r.sha256].clone())).unwrap();
        let container = package(&node, &objects);
        let plan = crate::project_package_import_plan::plan_clone_import(
            &node,
            &container,
            &origins,
            &crate::project_package_import_plan::CloneIdentityAssignment {
                project_id: ProjectId::new(name).unwrap(),
                candidates: BTreeMap::new(),
                placements: node
                    .placements
                    .iter()
                    .flat_map(|p| p.definitions.iter())
                    .enumerate()
                    .map(|(i, p)| {
                        (
                            p.placement_id.clone(),
                            SourcePlacementId::new(format!("{name}-placement-{i}")).unwrap(),
                        )
                    })
                    .collect(),
            },
        )
        .unwrap();
        objects.insert(
            plan.origin_manifest_ref.sha256.clone(),
            plan.origin_manifest.clone(),
        );
        origins.insert(plan.origin_manifest_ref.sha256, node);
        node = plan.manifest;
        assert_eq!(
            node.sources[0].browser_fallback_history,
            source.browser_fallback_history
        );
        assert!(node.generated_origins.is_empty());
    }
    validate_graph(&node, &origins, |r| Ok(objects[&r.sha256].clone())).unwrap();
    package(&node, &objects);
    assert_eq!(
        &objects[&source.browser_fallback_history.as_ref().unwrap().sha256],
        &bytes
    );
    let root = node.clone();
    node.sources[0].browser_fallback_history = None;
    node.format_version = node.required_format_version();
    node.counts = node.record_counts().unwrap();
    assert!(validate_graph(&node, &origins, |r| Ok(objects[&r.sha256].clone())).is_err());
    let mut bad = origins.clone();
    bad.values_mut()
        .find(|m| m.origin_project_id == *recording.project())
        .unwrap()
        .sources[0]
        .browser_fallback_history = None;
    assert!(validate_graph(&root, &bad, |r| Ok(objects[&r.sha256].clone())).is_err());
}
#[test]
fn browser_archive_refuses_changed_evidence_admissions_and_frames() {
    let (mut db, source, recording, original_raw) = fixture();
    let tx = db.transaction().unwrap();
    let bytes = capture(&tx, &source, &recording, 256 * 1024 * 1024)
        .unwrap()
        .unwrap();
    for fault in [
        "document",
        "original",
        "actor",
        "reference",
        "clock",
        "detached",
    ] {
        let mut p = Payload::decode(&bytes).unwrap();
        match fault {
            "document" => p.documents.values_mut().next().unwrap()[0] ^= 1,
            "original" => p.header.admissions.clear(),
            "actor" => p.header.admissions[0].actor = SessionId::new("foreign").unwrap(),
            "reference" => p.header.reports[0].revision += 1,
            "clock" => {
                let mut body: ResponseBody =
                    serde_json::from_str(&p.header.admissions[0].response).unwrap();
                let ResponseBody::LiveCapture(c) = &mut body else {
                    panic!("fixture capture")
                };
                c.inference
                    .as_mut()
                    .unwrap()
                    .fallback
                    .as_mut()
                    .unwrap()
                    .admitted_at_unix_ms = 5000;
                p.header.admissions[0].response = encode(&body).unwrap();
            }
            _ => {
                p.documents.insert("f".repeat(64), vec![1]);
            }
        }
        assert!(p.validate(&recording).is_err(), "{fault}");
    }
    let mut changed = Recording::parse(&original_raw).unwrap();
    let original = changed.pages[0].cpu_manifest.as_ref().unwrap();
    let mut image = original.image.clone();
    image.owner_policy.sha256 = "e".repeat(64);
    image.owner_policy.artifact_id = ArtifactId::new("e".repeat(64)).unwrap();
    changed.pages[0].cpu_manifest = Some(Box::new(
        LiveCpuInferenceManifest::new(original.stream.clone(), image).unwrap(),
    ));
    authored_sample(&mut changed.pages[0]);
    changed.pages[0].validate().unwrap();
    assert!(Payload::decode(&bytes).unwrap().validate(&changed).is_err());
    for fault in ["magic", "length", "truncated", "trailing"] {
        let mut raw = bytes.clone();
        match fault {
            "magic" => raw[0] ^= 1,
            "length" => raw[8..12].copy_from_slice(&u32::MAX.to_le_bytes()),
            "truncated" => {
                raw.pop();
            }
            _ => raw.push(0),
        };
        assert!(Payload::decode(&raw).is_err(), "{fault}");
    }
    assert!(capture(&tx, &source, &recording, 1).is_err());
    tx.rollback().unwrap();
    db.execute(
        "DELETE FROM request_identities WHERE request='archive-original-start'",
        [],
    )
    .unwrap();
    let tx = db.transaction().unwrap();
    assert!(capture(&tx, &source, &recording, 256 * 1024 * 1024).is_err());
}
#[test]
fn browser_archive_graph_refuses_stripped_declarations_and_bounds_before_read() {
    let (mut db, mut source, recording, raw) = fixture();
    let tx = db.transaction().unwrap();
    let bytes = capture(&tx, &source, &recording, 256 * 1024 * 1024)
        .unwrap()
        .unwrap();
    source.browser_fallback_history = Some(PackageObjectRef {
        sha256: digest(&bytes),
        byte_len: bytes.len() as u64,
    });
    let mut node = manifest(source, &recording, &bytes);
    let objects = BTreeMap::from([(digest(&raw), raw), (digest(&bytes), bytes)]);
    let mut wrong_placement = node.clone();
    wrong_placement.placements.as_mut().unwrap().definitions[0]
        .spec
        .rate_numerator = 2;
    wrong_placement.validate().unwrap();
    assert!(
        validate_graph(&wrong_placement, &BTreeMap::new(), |r| Ok(objects
            [&r.sha256]
            .clone()))
        .is_err()
    );
    let mut wrong_media = node.clone();
    let media = wrong_media
        .sources
        .iter_mut()
        .find(|s| s.kind == SourceKind::Media)
        .unwrap();
    let old = media.identity.sha256.clone();
    media.identity.sha256 = digest(b"DIFFERENT AUTHORED MEDIA");
    media.identity.artifact_id = ArtifactId::new(media.identity.sha256.clone()).unwrap();
    media.identity.byte_len = 24;
    let object = wrong_media
        .objects
        .iter_mut()
        .find(|o| o.sha256 == old)
        .unwrap();
    object.sha256 = media.identity.sha256.clone();
    object.byte_len = 24;
    wrong_media.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    wrong_media.validate().unwrap();
    assert!(validate_graph(&wrong_media, &BTreeMap::new(), |r| Ok(
        objects[&r.sha256].clone()
    ))
    .is_err());
    node.sources[0].browser_fallback_history = None;
    for o in &mut node.objects {
        if o.roles
            .contains(&PackageObjectRole::BrowserInferenceFallbackHistory)
        {
            o.roles = vec![PackageObjectRole::DependencyEvidence];
        }
    }
    node.format_version = node.required_format_version();
    assert!(validate_graph(&node, &BTreeMap::new(), |r| Ok(objects[&r.sha256].clone())).is_err());
    let source_hash = node.sources[0].identity.sha256.clone();
    node.sources[0].identity.byte_len = RECORDING_LIMIT + 1;
    node.objects
        .iter_mut()
        .find(|o| o.sha256 == source_hash)
        .unwrap()
        .byte_len = RECORDING_LIMIT + 1;
    let mut reads = 0;
    assert!(validate_graph(&node, &BTreeMap::new(), |_| {
        reads += 1;
        Ok(vec![])
    })
    .is_err());
    assert_eq!(reads, 0);
}

/// Seed acknowledged-shaped historical data only. Native source custody, model
/// execution and acceptance are deliberately absent from this software fixture.
pub(in crate::authority) fn authored_local_browser_recording(
    engine: &Engine,
    actor: &SessionId,
    token: &str,
    project: &ProjectId,
    revision: RevisionId,
    media: &SourceArtifact,
    placement: &SourcePlacement,
) -> Vec<u8> {
    let mut report = crate::browser_fallback_history::authored_capture_for_archive_test();
    report.engine_epoch = AttemptId::new(engine.engine_epoch.clone()).unwrap();
    report.identity.project = project.clone();
    report.identity.revision = revision;
    report.identity.source = media.source_version.clone();
    let binding = report.options.browser_media.as_mut().unwrap();
    binding.engine_epoch = report.engine_epoch.clone();
    binding.project_id = project.clone();
    binding.revision = revision;
    binding.selection = BrowserMediaSelection {
        source_version: media.source_version.clone(),
        source: media.identity.clone(),
        placement: placement.clone(),
    };
    report.clock = report.options.clock(report.identity.epoch.clone());
    let info = report.inference.as_mut().unwrap();
    let old = info.cpu_manifest.as_ref().unwrap();
    let mut stream = old.stream.clone();
    stream.identity = report.identity.clone();
    stream.clock = report.clock.clone();
    info.cpu_manifest = Some(Box::new(
        LiveCpuInferenceManifest::new(stream, old.image.clone()).unwrap(),
    ));
    info.fallback.as_mut().unwrap().semantic_identity =
        browser_inference_semantic_identity(&report.options).unwrap();
    super::super::inference_equivalence_store::resolver_store_tests::seed_browser_archive_record(
        &mut engine.db.lock().unwrap(),
        actor,
        &mut report,
    );
    let request = Request::new(
        RequestId::new(format!("authored-browser-start-{}", uuid::Uuid::new_v4())).unwrap(),
        Command::StartLiveCapture {
            engine_epoch: report.engine_epoch.clone(),
            options: report.options.clone(),
        },
    )
    .with_session(actor.clone())
    .with_auth_token(token.to_owned())
    .in_project(project.clone(), Some(revision));
    let body = ResponseBody::LiveCapture(Box::new(report.clone()));
    let fp = digest(&request_fingerprint(&request).unwrap());
    let mut db = engine.db.lock().unwrap();
    let tx = db.transaction().unwrap();
    super::super::remember(&tx, actor.as_str(), &request, &fp, &body).unwrap();
    tx.commit().unwrap();
    drop(db);
    let mut page = crate::live_cpu_history::authored_recording_page_for_test(&report);
    page.fallback = BrowserInferenceRecordingProvenance::from_capture(&report)
        .unwrap()
        .map(Box::new);
    authored_sample(&mut page);
    let options = LiveRecordingStageOptions {
        engine_epoch: report.engine_epoch.clone(),
        recordings: vec![LiveRecordingSelection {
            epoch: page.identity.epoch.clone(),
            recording_request_id: page.recording_request_id.clone(),
        }],
        range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000_000)).unwrap(),
        maximum_interpolation_ns: 100_000_000,
    };
    serde_json::to_vec(&serde_json::json!({"schema":1,"kind":"engine-owned-live-recording-source","qualified":false,"stage_request_id":"AUTHORED-STAGE-NOT-NATIVE","actor":actor,"options":options,"source_time_basis":page.source_time_basis,"contains_cuda_provisional":false,"browser_media":page.browser_media,"pages":[page]})).unwrap()
}
