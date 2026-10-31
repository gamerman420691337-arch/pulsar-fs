// Public Engine package operations over explicitly authored archive metadata.
// No native inference, measurement, reviewer acceptance or device is exercised.
fn browser_unpack(bytes: &[u8]) -> (PortableProjectManifest, BTreeMap<String, Vec<u8>>) {
    let decoded = crate::project_package_format::read_package(
        &mut std::io::Cursor::new(bytes),
        |_| Ok(std::io::sink()),
        PackageCodecLimits::default(),
    )
    .unwrap();
    let size = u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;
    let mut cursor = 52 + size;
    let mut objects = BTreeMap::new();
    for object in &decoded.manifest.objects {
        let end = cursor + object.byte_len as usize;
        objects.insert(object.sha256.clone(), bytes[cursor..end].to_vec());
        cursor = end;
    }
    assert_eq!(cursor, bytes.len());
    (decoded.manifest, objects)
}
fn browser_pack(
    manifest: &PortableProjectManifest,
    objects: &BTreeMap<String, Vec<u8>>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_package(
        manifest,
        &mut bytes,
        |o| Ok(std::io::Cursor::new(objects[&o.sha256].clone())),
        PackageCodecLimits::default(),
    )
    .unwrap();
    bytes
}
fn browser_no_authority(f: &Fixture) {
    let db = f.engine.db.lock().unwrap();
    for table in [
        "jobs",
        "attempts",
        "inference_equivalence_records",
        "inference_equivalence_decisions",
        "inference_fallback_jobs",
        "inference_fallback_pending",
        "cpu_runtime_attempts",
        "live_cpu_admissions",
        "browser_fallback_admissions",
        "live_cuda_capture_evidence",
    ] {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        if exists {
            assert_eq!(
                db.query_row(&format!("SELECT COUNT(*) FROM main.{table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0,
                "restored authority in {table}"
            );
        }
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sessions WHERE id IN ('actor','unit-only-authored-actor')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        43
    );
    drop(db);
    #[cfg(target_arch = "x86_64")]
    assert_eq!(
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .retained_count_for_test(),
        0
    );
}
fn browser_approve(f: &Fixture, project: &ProjectId) {
    f.scoped(
        Command::AllowProjectPackaging {
            owner_proof: f.engine.package_owner_token.clone(),
        },
        project,
        None,
    )
    .unwrap();
}
fn browser_export_status(
    f: &Fixture,
    session: &SessionId,
    token: &str,
    project: &ProjectId,
    revision: RevisionId,
) -> PResult<PackageExportStatus> {
    let request = |command, revision| {
        Request::new(id!(RequestId), command)
            .with_session(session.clone())
            .with_auth_token(token.to_owned())
            .in_project(project.clone(), revision)
    };
    let ResponseBody::PackageExport(started) = f
        .engine
        .handle(request(Command::StartProjectExport, Some(revision)))
        .result?
    else {
        panic!("public export")
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let ResponseBody::PackageExport(status) = f
            .engine
            .handle(request(
                Command::ProjectPackageStatus {
                    operation_id: started.operation_id.clone(),
                },
                None,
            ))
            .result?
        else {
            panic!("public export status")
        };
        if matches!(
            status.state,
            PackageExportOperationState::Ready
                | PackageExportOperationState::Failed
                | PackageExportOperationState::Cancelled
                | PackageExportOperationState::Interrupted
        ) {
            return Ok(*status);
        }
        assert!(
            Instant::now() < deadline,
            "public browser history export timed out: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn browser_export(f: &Fixture, project: &ProjectId, revision: RevisionId) -> Vec<u8> {
    let ready = browser_export_status(f, &f.session, &f.token, project, revision).unwrap();
    assert_eq!(
        ready.state,
        PackageExportOperationState::Ready,
        "{:?}",
        ready.error
    );
    let descriptor = ready.artifact.unwrap();
    assert_eq!(descriptor.format_version, 17);
    let bytes = fs::read(
        f.engine
            .config
            .state_dir
            .join("project-packages")
            .join(&descriptor.sha256),
    )
    .unwrap();
    assert_eq!(bytes.len() as u64, descriptor.byte_len);
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), descriptor.sha256);
    bytes
}
fn browser_reopen(f: Fixture) -> Fixture {
    let Fixture {
        engine,
        session,
        token,
        project,
        dir,
    } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    Fixture {
        engine,
        session,
        token,
        project,
        dir,
    }
}
fn browser_drop(f: Fixture) {
    let Fixture { engine, dir, .. } = f;
    Engine::retire_for_test(engine);
    drop(dir);
}
fn browser_rejected(bytes: &[u8]) {
    let f = fixture();
    let before = f.count("projects");
    let (_, status) = f.start(bytes);
    let lease = f.upload(&status, bytes);
    let done = f.finish(&status, &lease);
    f.drain();
    assert_eq!(
        done.state,
        PackageImportOperationState::Failed,
        "{:?}",
        done.error
    );
    assert!(done.receipt.is_none());
    assert!(
        done.error
            .as_ref()
            .is_some_and(|e| e.message.contains("browser") || e.message.contains("source origin")),
        "{:?}",
        done.error
    );
    assert_eq!(f.count("projects"), before);
    assert_eq!(f.count("jobs"), 0);
    assert_eq!(f.count("attempts"), 0);
    browser_drop(f);
}
pub(super) fn exercise_browser_portable_lifecycle(bytes: &[u8], payload: &PackageObjectRef) {
    let (original, original_objects) = browser_unpack(bytes);
    assert_eq!(original.format_version, 17);
    let recording = original
        .sources
        .iter()
        .find(|s| s.browser_fallback_history.as_ref() == Some(payload))
        .unwrap()
        .identity
        .clone();
    let body = &original_objects[&payload.sha256];
    let header_len = u32::from_le_bytes(body[8..12].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&body[12..12 + header_len]).unwrap();
    let original_actors: std::collections::BTreeSet<String> = header["admissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["actor"].as_str().unwrap().to_owned())
        .chain(
            header["reports"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["decision"]["actor_session"].as_str().unwrap().to_owned()),
        )
        .collect();
    let check = |f: &Fixture| {
        browser_no_authority(f);
        for actor in &original_actors {
            assert_eq!(
                f.engine
                    .db
                    .lock()
                    .unwrap()
                    .query_row("SELECT COUNT(*) FROM sessions WHERE id=?1", [actor], |r| {
                        r.get::<_, i64>(0)
                    })
                    .unwrap(),
                0,
                "archive restored original session"
            );
        }

        assert_eq!(
            fs::read(
                f.engine
                    .config
                    .state_dir
                    .join("imported-project-evidence")
                    .join(&payload.sha256)
            )
            .unwrap(),
            original_objects[&payload.sha256]
        );
        assert_eq!(
            fs::read(
                f.engine
                    .config
                    .state_dir
                    .join("snapshots")
                    .join(&recording.sha256)
            )
            .unwrap(),
            original_objects[&recording.sha256]
        );
    };
    // Recomputed hashes do not make substituted evidence a valid original.
    let mut bad = original.clone();
    let mut objects = original_objects.clone();
    let mut body = objects.remove(&payload.sha256).unwrap();
    *body.last_mut().unwrap() ^= 1;
    let new_sha = format!("{:x}", Sha256::digest(&body));
    objects.insert(new_sha.clone(), body);
    for source in &mut bad.sources {
        if let Some(reference) = &mut source.browser_fallback_history {
            reference.sha256 = new_sha.clone();
        }
    }
    bad.objects
        .iter_mut()
        .find(|o| o.sha256 == payload.sha256)
        .unwrap()
        .sha256 = new_sha;
    bad.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    eprintln!("browser-archive phase: reject substituted evidence");
    browser_rejected(&browser_pack(&bad, &objects));
    let mut stripped = original.clone();
    for source in &mut stripped.sources {
        source.browser_fallback_history = None;
    }
    stripped.objects.retain(|o| o.sha256 != payload.sha256);
    stripped.format_version = stripped.required_format_version();
    stripped.counts = stripped.record_counts().unwrap();
    eprintln!("browser-archive phase: reject stripped root");
    browser_rejected(&browser_pack(&stripped, &original_objects));

    eprintln!("browser-archive phase: first public import");
    let first = fixture();
    let clone = first.import(bytes);
    check(&first);
    let first = browser_reopen(first);
    check(&first);
    browser_forbidden(browser_export_status(
        &first,
        &first.session,
        &first.token,
        &clone.project_id,
        clone.revision,
    ));
    browser_approve(&first, &clone.project_id);
    let (delegate, delegate_token) = match first
        .call(Command::Pair {
            client_name: "Authored archive delegated package scope".into(),
            pairing_token: first.engine.token.clone(),
        })
        .unwrap()
    {
        ResponseBody::Paired {
            session,
            auth_token,
        } => (session, auth_token),
        _ => panic!("pair"),
    };
    first
        .call(Command::Grant {
            session: delegate.clone(),
            project_id: clone.project_id.clone(),
            scopes: vec![Scope::PackageProject],
        })
        .unwrap();
    browser_forbidden(browser_export_status(
        &first,
        &delegate,
        &delegate_token,
        &clone.project_id,
        clone.revision,
    ));
    eprintln!("browser-archive phase: first public re-export");
    let reexport = browser_export(&first, &clone.project_id, clone.revision);
    let (outer, mut bodies) = browser_unpack(&reexport);
    assert_eq!(bodies[&payload.sha256], original_objects[&payload.sha256]);
    assert_eq!(
        bodies[&recording.sha256],
        original_objects[&recording.sha256]
    );
    let edge = &outer.imported_origins[0];
    assert_eq!(
        bodies[&edge.manifest.sha256],
        original.canonical_bytes().unwrap()
    );
    // Strip an ancestor declaration and rebuild both its container and edge.
    let old_sha = edge.manifest.sha256.clone();
    let mut ancestor: PortableProjectManifest = serde_json::from_slice(&bodies[&old_sha]).unwrap();
    for source in &mut ancestor.sources {
        source.browser_fallback_history = None;
    }
    ancestor.objects.retain(|o| o.sha256 != payload.sha256);
    ancestor.format_version = ancestor.required_format_version();
    ancestor.counts = ancestor.record_counts().unwrap();
    let ancestor_bytes = ancestor.canonical_bytes().unwrap();
    let ancestor_container = browser_pack(&ancestor, &bodies);
    let new_sha = format!("{:x}", Sha256::digest(&ancestor_bytes));
    bodies.remove(&old_sha);
    bodies.insert(new_sha.clone(), ancestor_bytes.clone());
    let mut changed = outer.clone();
    let edge = &mut changed.imported_origins[0];
    edge.manifest = PackageObjectRef {
        sha256: new_sha.clone(),
        byte_len: ancestor_bytes.len() as u64,
    };
    edge.objects = ancestor.objects.clone();
    edge.container.format_version = ancestor.format_version;
    edge.container.object_count = ancestor.objects.len() as u32;
    edge.container.manifest_sha256 = new_sha.clone();
    edge.container.sha256 = format!("{:x}", Sha256::digest(&ancestor_container));
    edge.container.byte_len = ancestor_container.len() as u64;
    let object = changed
        .objects
        .iter_mut()
        .find(|o| o.sha256 == old_sha)
        .unwrap();
    object.sha256 = new_sha;
    object.byte_len = ancestor_bytes.len() as u64;
    changed.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    eprintln!("browser-archive phase: reject stripped ancestor");
    browser_rejected(&browser_pack(&changed, &bodies));

    eprintln!("browser-archive phase: second public import");
    let second = fixture();
    let clone2 = second.import(&reexport);
    check(&second);
    let second = browser_reopen(second);
    check(&second);
    assert_ne!(clone.project_id, clone2.project_id);
    browser_approve(&second, &clone2.project_id);
    let again = browser_export(&second, &clone2.project_id, clone2.revision);
    let (_, last) = browser_unpack(&again);
    assert_eq!(last[&payload.sha256], original_objects[&payload.sha256]);
    assert_eq!(last[&recording.sha256], original_objects[&recording.sha256]);
    assert_eq!(last[&old_sha], original.canonical_bytes().unwrap());
    browser_drop(first);
    browser_drop(second);
}

fn browser_forbidden(result: PResult<PackageExportStatus>) {
    match result {
        Err(error) => assert_eq!(error.code, ErrorCode::Forbidden),
        Ok(status) => {
            assert_eq!(status.state, PackageExportOperationState::Failed);
            assert!(status.artifact.is_none());
            assert_eq!(status.error.unwrap().code, ErrorCode::Forbidden);
        }
    }
}
#[test]
fn browser_archive_local_source_export_import_restart_and_two_clones() {
    eprintln!("browser-archive phase: local setup");
    let f = fixture();
    let media_path = f.dir.path().join("authored-browser-media");
    fs::write(&media_path, b"AUTHORED MEDIA ONLY - NO DECODE OR INFERENCE").unwrap();
    let ResponseBody::Source { source_version } = f
        .scoped(
            Command::ImportSource { path: media_path },
            &f.project,
            Some(0),
        )
        .unwrap()
    else {
        panic!("import source")
    };
    f.scoped(
        Command::SetSourcePlacement {
            spec: SourcePlacementSpec {
                source_version: source_version.clone(),
                source_start: SourceTimestamp::new(0, 1).unwrap(),
                source_end: SourceTimestamp::new(10, 1).unwrap(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                rate_numerator: 1,
                rate_denominator: 1,
            },
            replace: None,
        },
        &f.project,
        Some(0),
    )
    .unwrap();
    let (revision, media, placement) = {
        let db = f.engine.db.lock().unwrap();
        let snapshot = project_snapshot(&db, &f.project).unwrap();
        let media = f.engine.source(&db, &f.project, &source_version).unwrap();
        (snapshot.revision, media, snapshot.placements[0].clone())
    };
    eprintln!("browser-archive phase: seed historical acknowledgement");
    let raw = super::super::portable_browser_history::authored_local_browser_recording(
        &f.engine, &f.session, &f.token, &f.project, revision, &media, &placement,
    );
    let path = f.dir.path().join("authored-recording-snapshot");
    fs::write(&path, &raw).unwrap();
    let ResponseBody::Source { source_version } = f
        .scoped(
            Command::ImportSource { path },
            &f.project,
            Some(revision.value()),
        )
        .unwrap()
    else {
        panic!("import recording source")
    };
    // The private test installs the engine-owned purpose for authored bytes.
    // No native capture, measurement or public acceptance is represented.
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE sources SET kind='live_recording' WHERE project=?1 AND version=?2",
            params![f.project.as_str(), source_version.as_str()],
        )
        .unwrap();
    browser_forbidden(browser_export_status(
        &f, &f.session, &f.token, &f.project, revision,
    ));
    browser_approve(&f, &f.project);
    eprintln!("browser-archive phase: original public export");
    let bytes = browser_export(&f, &f.project, revision);
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT clock_floor_unix_ms FROM inference_equivalence_schema",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        3000,
        "archival export advanced current acceptance clock"
    );

    let (manifest, objects) = browser_unpack(&bytes);
    let source = manifest
        .sources
        .iter()
        .find(|s| s.source_version == source_version)
        .unwrap();
    assert_eq!(objects[&source.identity.sha256], raw);
    let reference = source.browser_fallback_history.as_ref().unwrap().clone();
    assert!(objects.contains_key(&reference.sha256));
    browser_drop(f);
    exercise_browser_portable_lifecycle(&bytes, &reference);
}
