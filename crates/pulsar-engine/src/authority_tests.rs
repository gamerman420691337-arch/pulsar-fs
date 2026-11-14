mod profile_ram {
    include!("profile_ram_engine_tests.rs");
}
include!("model_pack_withdrawal_tests.rs");
use super::*;
include!("live_recording_candidate_tests.rs");
include!("engine_live_capture_clock_tests.rs");
include!("engine_live_fallback_target_tests.rs");
include!("engine_live_capture_delivery_tests.rs");
#[cfg(target_os = "linux")]
include!("annotated_engine_generation_tests.rs");
#[cfg(target_os = "linux")]
include!("admitted_engine_crash_tests.rs");
#[cfg(target_os = "linux")]
include!("native_capture_engine_tests.rs");
#[cfg(all(target_os = "linux", feature = "cuda"))]
include!("engine_cuda_generation_tests.rs");
include!("engine_cpu_generation_tests.rs");
include!("engine_inference_fallback_tests.rs");
include!("engine_inference_fallback_native_tests.rs");
include!("engine_live_cpu_native_tests.rs");
include!("engine_live_cpu_browser_native_tests.rs");
#[cfg(target_os = "linux")]
include!("engine_native_preparation_db_tests.rs");
#[cfg(target_os = "linux")]
include!("engine_native_preparation_control_tests.rs");
include!("engine_media_probe_tests.rs");
include!("engine_handy_registration_tests.rs");
include!("engine_handy_session_tests.rs");
include!("engine_handy_script_tests.rs");
include!("engine_device_archive_tests.rs");
include!("engine_handy_hosting_tests.rs");
include!("engine_handy_setup_tests.rs");
include!("engine_handy_program_tests.rs");
include!("engine_handy_preparation_tests.rs");
include!("engine_handy_controller_tests.rs");
include!("engine_handy_server_clock_tests.rs");
include!("engine_handy_device_timing_tests.rs");
include!("engine_handy_clock_sync_tests.rs");
include!("engine_handy_playback_clock_tests.rs");
include!("engine_handy_continuous_timing_tests.rs");
include!("engine_handy_player_guard_tests.rs");
include!("engine_browser_media_tests.rs");
include!("engine_browser_media_capture_tests.rs");
include!("engine_browser_timing_tests.rs");
include!("engine_handy_execution_tests.rs");
include!("engine_handy_hsp_execution_tests.rs");
include!("engine_handy_hsp_continuity_control_tests.rs");
include!("engine_handy_hsp_public_preparation_io_tests.rs");
include!("engine_handy_emergency_tests.rs");
include!("engine_handy_playback_tests.rs");
include!("engine_shutdown_tests.rs");
include!("engine_inference_attempt_native_tests.rs");
include!("engine_inference_equivalence_tests.rs");
include!("engine_handy_setup_status_tests.rs");
include!("engine_handy_setup_send_tests.rs");
include!("engine_handy_setup_cancel_tests.rs");
include!("engine_handy_qualification_tests.rs");
include!("engine_handy_browser_start_qualification_tests.rs");
include!("engine_handy_hsp_source_qualification_tests.rs");
include!("engine_handy_browser_playback_tests.rs");
include!("engine_handy_reconciliation_tests.rs");
struct Fixture {
    engine: Arc<Engine>,
    session: SessionId,
    project: ProjectId,
    dir: tempfile::TempDir,
}
thread_local! {static TOKENS:std::cell::RefCell<HashMap<String,String>>=std::cell::RefCell::new(HashMap::new());}
fn request(
    session: &SessionId,
    project: Option<&ProjectId>,
    revision: Option<u64>,
    command: Command,
) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        request_id: id!(RequestId),
        session: Some(session.clone()),
        auth_token: TOKENS.with(|tokens| tokens.borrow().get(session.as_str()).cloned()),
        project: project.cloned(),
        expected_revision: revision.map(RevisionId::new),
        command,
    }
}
fn pair(engine: &Arc<Engine>, name: &str) -> SessionId {
    let r = Request::new(
        id!(RequestId),
        Command::Pair {
            client_name: name.into(),
            pairing_token: engine.token.clone(),
        },
    );
    match engine.handle(r).result.unwrap() {
        ResponseBody::Paired {
            session,
            auth_token,
        } => {
            TOKENS.with(|tokens| tokens.borrow_mut().insert(session.to_string(), auth_token));
            session
        }
        _ => panic!("pairing response"),
    }
}
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("nonexecuted-worker-fixture");
    fs::write(&binary, b"authority-test executable identity").unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path().join("private-state"), binary)).unwrap();
    let session = pair(&engine, "test client");
    let r = request(
        &session,
        None,
        None,
        Command::CreateProject {
            name: "Test project".into(),
        },
    );
    let project = match engine.handle(r).result.unwrap() {
        ResponseBody::Project(p) => p.project_id,
        _ => panic!("project response"),
    };
    Fixture {
        engine,
        session,
        project,
        dir,
    }
}

#[test]
fn captured_model_path_cannot_be_rebound_to_different_bytes() {
    let f = fixture();
    let source = f.dir.path().join("model.onnx");
    fs::write(&source, b"original model bytes").unwrap();
    let model = f.engine.snapshot_model(&source).unwrap();
    let repeated = f.engine.snapshot_model(&model.path).unwrap();
    assert_eq!(repeated.identity.sha256, model.identity.sha256);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&model.path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(&model.path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&model.path, permissions).unwrap();
    }
    fs::write(&model.path, b"changed captured model bytes").unwrap();
    assert_eq!(
        f.engine.snapshot_model(&model.path).unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
}

#[test]
fn unsupported_visual_tiers_refuse_before_job_or_attempt_creation() {
    let f = fixture();
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT OR IGNORE INTO grants VALUES(?1,?2,?3)",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Generate)
            ],
        )
        .unwrap();
    for (preset, settings) in [
        ("maximum", GenerationSettings::default()),
        (
            "default",
            GenerationSettings {
                axis: Axis::Yaw,
                ..Default::default()
            },
        ),
    ] {
        let result = f.engine.handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::Generate {
                source_version: SourceVersionId::new("not-needed-for-tier-refusal").unwrap(),
                preset: preset.into(),
                settings: Some(settings),
                model_path: None,
            },
        ));
        assert_eq!(result.result.unwrap_err().code, ErrorCode::Unsupported);
    }
    let db = f.engine.db.lock().unwrap();
    for table in ["jobs", "attempts"] {
        let count: i64 = db
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
    assert_eq!(
        fs::read_dir(f.engine.config.state_dir.join("attempts"))
            .unwrap()
            .count(),
        0
    );
    #[cfg(target_os = "linux")]
    assert!(!f.engine.worker_fence.has_pending_worker_custody().unwrap());
}

#[test]
fn auxiliary_schema_failure_rolls_back_every_member() {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE sources(project TEXT,version TEXT,PRIMARY KEY(project,version));CREATE TABLE candidates(id TEXT PRIMARY KEY);PRAGMA user_version=8;").unwrap();
    assert!(initialize_auxiliary_schema(&mut db).is_err());
    let created:i64=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT IN ('sources','candidates')",[],|r|r.get(0)).unwrap();
    assert_eq!(
        created, 0,
        "a failed auxiliary migration must not publish earlier tables"
    );
    let columns: Vec<String> = db
        .prepare("PRAGMA table_info(candidates)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(columns, vec!["id"]);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert!(db.is_autocommit());
}

#[test]
fn auxiliary_schema_preserves_rows_defaults_and_reopens_idempotently() {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE sources(project TEXT,version TEXT,PRIMARY KEY(project,version));CREATE TABLE candidates(id TEXT PRIMARY KEY);CREATE TABLE exports(session TEXT,request TEXT,PRIMARY KEY(session,request));INSERT INTO candidates VALUES('retained');INSERT INTO exports VALUES('session','request');PRAGMA user_version=8;").unwrap();
    initialize_auxiliary_schema(&mut db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT review FROM candidates WHERE id='retained'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "[]"
    );
    assert_eq!(
        db.query_row(
            "SELECT axis FROM exports WHERE session='session' AND request='request'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "stroke"
    );
    db.execute(
        "UPDATE candidates SET review='retained review' WHERE id='retained'",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE exports SET axis='roll' WHERE session='session' AND request='request'",
        [],
    )
    .unwrap();
    initialize_auxiliary_schema(&mut db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT review FROM candidates WHERE id='retained'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "retained review"
    );
    assert_eq!(
        db.query_row(
            "SELECT axis FROM exports WHERE session='session' AND request='request'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "roll"
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert!(db.is_autocommit());
}

#[cfg(unix)]
fn verify_mixed_archive_clone(
    f: &Fixture,
    source: &SourceArtifact,
    master: &ProjectSnapshot,
    candidate: &CandidateId,
    job: &JobId,
    retimed: bool,
) {
    use crate::project_package_format::{write_package, PackageCodecLimits};
    {
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "INSERT OR IGNORE INTO grants VALUES(?1,?2,?3)",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::PackageProject)
            ],
        )
        .unwrap();
    }
    let token = TOKENS
        .with(|tokens| tokens.borrow().get(f.session.as_str()).cloned())
        .unwrap();
    let master = if retimed {
        let spec = SourcePlacementSpec {
            source_version: source.source_version.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(750_000_001, 1_000_000_000).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            rate_numerator: 3,
            rate_denominator: 1,
        };
        let ResponseBody::Project(placed) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(master.revision.value()),
                Command::SetSourcePlacement {
                    spec,
                    replace: None,
                },
            ))
            .result
            .unwrap()
        else {
            panic!("retime base placement");
        };
        retime_mixed_fixture(f, &f.project, *placed, 2)
    } else {
        master.clone()
    };
    let plan =
        project_packages::capture(&f.engine, &f.session, &token, &f.project, master.revision)
            .unwrap();
    assert_eq!(plan.manifest.retiming_receipts.len(), usize::from(retimed));
    let mut bytes = Vec::new();
    write_package(
        &plan.manifest,
        &mut bytes,
        |object| plan.open_object(object),
        PackageCodecLimits {
            max_package_bytes: DEFAULT_MAX_PROJECT_PACKAGE_BYTES,
        },
    )
    .unwrap();
    drop(plan);
    fs::rename(
        f.engine.config.state_dir.join("attempts"),
        f.dir.path().join("parked-attempts"),
    )
    .unwrap();
    let package = PackageUploadDeclaration {
        format_version: package_format_version(&bytes[..52]).unwrap(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        byte_len: bytes.len() as u64,
    };
    let ResponseBody::PackageImport(initial) = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::StartProjectImport { package },
        ))
        .result
        .unwrap()
    else {
        panic!("import admission");
    };
    let ResponseBody::PackageUpload(lease) = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::BeginProjectPackageUpload {
                operation_id: initial.operation_id.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("upload lease");
    };
    let handshake = PackageUploadHandshake {
        version: PROTOCOL_VERSION,
        channel: PackageUploadChannel::ProjectPackageUpload,
        session: f.session.clone(),
        auth_token: token,
        lease_id: lease.lease_id.clone(),
        engine_epoch: lease.engine_epoch.clone(),
        operation_id: lease.operation_id.clone(),
        generation: lease.generation,
        package_sha256: lease.package.sha256.clone(),
    };
    let (connection, _) = f.engine.import_bulk_open(&handshake).unwrap();
    let mut offset = 0;
    for chunk in bytes.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
        offset = f
            .engine
            .import_bulk_append(&connection, &handshake, offset, chunk)
            .unwrap();
    }
    assert_eq!(offset, bytes.len() as u64);
    drop(connection);
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::SealProjectImport {
                operation_id: initial.operation_id.clone(),
                upload_generation: lease.generation,
            },
        ))
        .result
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let cloned = loop {
        let ResponseBody::PackageImport(status) = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::ProjectImportStatus {
                    operation_id: initial.operation_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("import status");
        };
        if status.state.is_terminal() {
            assert_eq!(
                status.state,
                PackageImportOperationState::Completed,
                "{:?}",
                status.error
            );
            break status.receipt.unwrap();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "authored archive import observation deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let alias = {
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM jobs WHERE project=?1",
                [cloned.project_id.as_str()],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        let raw: String = db
            .query_row(
                "SELECT manifest FROM project_import_archives WHERE project=?1",
                [cloned.project_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let m: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let edge = m["imported_origins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["container"]["project_id"] == f.project.as_str())
            .unwrap();
        let id = edge["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["origin"] == candidate.as_str())
            .unwrap()["local"]
            .as_str()
            .unwrap();
        CandidateId::new(id).unwrap()
    };
    let report = {
        let db = f.engine.db.lock().unwrap();
        archived_generation::load(
            &db,
            &cloned.project_id,
            &alias,
            &f.engine.pool,
            &f.engine.import_store,
        )
        .unwrap()
    };
    assert_eq!(report.origins.len(), 1);
    let origin = &report.origins[0];
    assert_eq!(&origin.job_id, job);
    {
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            stash_lineage::verified_archived_master(
                &db,
                &cloned.project_id,
                cloned.revision,
                &report,
                &f.engine.pool,
                &f.engine.import_store
            )
            .unwrap_err()
            .code,
            ErrorCode::DependencyMismatch
        );
    }

    let media = f.dir.path().join("fresh-archive-media.mkv");
    fs::copy(&source.path, &media).unwrap();
    let ResponseBody::Source { source_version } = f
        .engine
        .handle(request(
            &f.session,
            Some(&cloned.project_id),
            Some(cloned.revision.value()),
            Command::ImportSource {
                path: media.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("fresh clone binding");
    };
    assert_eq!(source_version, origin.source_version);
    let current = if retimed {
        let base = project_snapshot(&f.engine.db.lock().unwrap(), &cloned.project_id).unwrap();
        retime_mixed_fixture(f, &cloned.project_id, base, 4)
    } else {
        project_snapshot(&f.engine.db.lock().unwrap(), &cloned.project_id).unwrap()
    };
    let capture = StashSourceCapture {
        endpoint: Some("http://127.0.0.1:1/graphql".into()),
        source_version,
        identity: source.identity.clone(),
        scene_id: "archive-scene".into(),
        file_id: "archive-file".into(),
        server_path: "/authored/fresh-archive-media.mkv".into(),
        local_source: media.clone(),
        script_destination: media.with_extension("funscript"),
    };
    {
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "INSERT INTO stash_sources(project,version,scene,file,receipt) VALUES(?1,?2,?3,?4,?5)",
            params![
                cloned.project_id.as_str(),
                capture.source_version.as_str(),
                capture.scene_id,
                capture.file_id,
                encode(&capture).unwrap()
            ],
        )
        .unwrap();
    }
    let export = request(
        &f.session,
        Some(&cloned.project_id),
        Some(current.revision.value()),
        Command::StashExportScript {
            connection: StashConnection {
                endpoint: capture.endpoint.clone().unwrap(),
                credential_env: None,
            },
            source_version: capture.source_version.clone(),
            scene_id: capture.scene_id.clone(),
            file_id: capture.file_id.clone(),
            candidate_id: alias.clone(),
            job_id: job.clone(),
            archived_origin: Some(origin.origin_manifest.clone()),
            source_time: true,
            owner_proof: f.engine.stash_owner_token.clone(),
        },
    );
    let publisher = crate::stash_files::Publication::prepare(&capture).unwrap();
    let ResponseBody::StashScriptExported(receipt) = f
        .engine
        .export(
            &export,
            &capture.script_destination,
            "authored-archived-mixed-clock-export",
            ExportKind::Stash {
                capture: &capture,
                candidate: &alias,
                job,
                archived_origin: Some(&origin.origin_manifest),
                publisher: &publisher,
            },
        )
        .unwrap()
    else {
        panic!("archival publication");
    };
    let proof = receipt.timing_proof.as_ref().unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&fs::read(&proof.path).unwrap()).unwrap();
    assert_eq!(raw["schema_version"], 7);
    assert_eq!(
        raw["context"]["action_ancestry"]["carriers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    if retimed {
        assert_eq!(
            raw["context"]["native_retimings"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            raw["context"]["archived_retimings"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            raw["context"]["archived_context"]["master"]["retiming_revisions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        for carrier in raw["context"]["action_ancestry"]["carriers"]
            .as_array()
            .unwrap()
        {
            assert_eq!(carrier["retimings"].as_array().unwrap().len(), 2);
        }
    }
    let project_times = if retimed {
        [124_999_999, 156_249_999]
    } else {
        [166_666_666, 208_333_333]
    };
    for (project_time, frame, ns) in [
        (project_times[0], 4, 500_000_000),
        (project_times[1], 5, 625_000_000),
    ] {
        let m = raw["mappings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["project_time"] == project_time)
            .unwrap();
        assert_eq!(m["original_source_frame"], frame);
        assert_eq!(m["quantization"]["time"], ns);
    }
    let observe = request(
        &f.session,
        Some(&cloned.project_id),
        None,
        Command::StashPublicationStatus {
            publication_request_id: export.request_id.clone(),
            owner_proof: f.engine.stash_owner_token.clone(),
        },
    );
    let status = || {
        let ResponseBody::StashPublication(v) = f
            .engine
            .stash_publication_observation(&observe, &export.request_id)
            .unwrap()
        else {
            panic!("archival observation");
        };
        v.proof_observation
    };
    assert_eq!(status(), StashArtifactObservation::Verified);
    let saved_response = encode(&ResponseBody::StashScriptExported(receipt.clone())).unwrap();
    let faults = if retimed {
        vec![
            "mapping",
            "selection",
            "catalogue",
            "profile",
            "downgrade",
            "native_retime",
            "archived_retime",
            "carrier_chain",
        ]
    } else {
        vec!["mapping", "selection", "catalogue", "profile", "downgrade"]
    };
    for fault in faults {
        let mut forged = raw.clone();
        match fault {
            "mapping" => forged["mappings"][0]["original_source_frame"] = serde_json::json!(777),
            "selection" => {
                forged["context"]["action_ancestry"]["selections"][2]["carrier"] =
                    serde_json::json!(0)
            }
            "catalogue" => {
                forged["context"]["archived_context"]["master"]["retiming_revisions"] =
                    serde_json::json!([0])
            }
            "profile" => {
                forged["context"]["archived_context"]["origin"]["preset"] =
                    serde_json::json!("forged-preset")
            }
            "native_retime" => {
                forged["context"]["native_retimings"][0]["to"]["spec"]["rate_numerator"] =
                    serde_json::json!(777)
            }
            "archived_retime" => {
                forged["context"]["archived_retimings"][0]["origin_receipt"]["to"]["spec"]
                    ["rate_numerator"] = serde_json::json!(777)
            }
            "carrier_chain" => {
                forged["context"]["action_ancestry"]["carriers"][0]["retimings"] =
                    serde_json::json!([])
            }
            _ => forged["schema_version"] = serde_json::json!(5),
        }
        let bytes = serde_json::to_vec(&forged).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let mut reference = proof.clone();
        reference.identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash.clone(),
            byte_len: bytes.len() as u64,
        };
        reference.path = proof.path.parent().unwrap().join(format!("{hash}.json"));
        reference.archived_context =
            Some(serde_json::from_value(forged["context"]["archived_context"].clone()).unwrap());
        fs::write(&reference.path, &bytes).unwrap();
        let mut changed = receipt.clone();
        changed.timing_proof = Some(reference.clone());
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(&reference).unwrap(),
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
            db.execute(
                "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(&ResponseBody::StashScriptExported(changed)).unwrap(),
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
        }
        assert_eq!(status(), StashArtifactObservation::Unverified, "{fault}");
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(proof).unwrap(),
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
            db.execute(
                "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                params![
                    saved_response,
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
        }
        fs::remove_file(&reference.path).unwrap();
        assert_eq!(status(), StashArtifactObservation::Verified);
    }
    assert!(!receipt.release_qualified && !proof.archived_context.as_ref().unwrap().active_job);
    if let Some(directory) = std::env::var_os("PULSAR_TEST_CLOCK_EVIDENCE_DIR") {
        let directory = PathBuf::from(directory);
        for (name, path) in [
            ("proof.json", &proof.path),
            ("script.funscript", &capture.script_destination),
        ] {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap();
            std::io::copy(&mut fs::File::open(path).unwrap(), &mut output).unwrap();
        }
    }
}

#[cfg(unix)]
fn retime_mixed_fixture(
    f: &Fixture,
    project: &ProjectId,
    base: ProjectSnapshot,
    rate: i32,
) -> ProjectSnapshot {
    let old = base.placements[0].clone();
    let mut spec = old.spec.clone();
    spec.rate_numerator = rate;
    let ResponseBody::Project(placed) = f
        .engine
        .handle(request(
            &f.session,
            Some(project),
            Some(base.revision.value()),
            Command::SetSourcePlacement {
                spec,
                replace: Some(old.placement_id.clone()),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("replacement placement");
    };
    let ResponseBody::Project(retimed) = f
        .engine
        .handle(request(
            &f.session,
            Some(project),
            Some(placed.revision.value()),
            Command::RetimeMotion {
                from_placement: old.placement_id,
                to_placement: placed.placements[0].placement_id.clone(),
                axes: vec![Axis::Stroke],
            },
        ))
        .result
        .unwrap()
    else {
        panic!("retimed master");
    };
    *retimed
}
#[cfg(unix)]
pub(crate) fn verify_archived_retimed_mixed_export_fixture(
    tools: WorkerTools,
    source: SourceArtifact,
) {
    verify_mixed_export_fixture(tools, source, true, true);
}
#[cfg(unix)]
pub(crate) fn verify_native_mixed_export_fixture(tools: WorkerTools, source: SourceArtifact) {
    verify_mixed_export_fixture(tools, source, false, false);
}
#[cfg(unix)]
pub(crate) fn verify_archived_mixed_export_fixture(tools: WorkerTools, source: SourceArtifact) {
    verify_mixed_export_fixture(tools, source, true, false);
}
#[cfg(unix)]
fn verify_mixed_export_fixture(
    tools: WorkerTools,
    mut source: SourceArtifact,
    archived: bool,
    retimed: bool,
) {
    source.source_version = SourceVersionId::new(source.identity.sha256.clone()).unwrap();
    let f = fixture();
    // This numerical/export fixture is not a crash-durability gate. Keep
    // its authored SQL setup independent of host journal-sync stalls.
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("PRAGMA synchronous=OFF;")
        .unwrap();
    let placement = SourcePlacement {
        placement_id: SourcePlacementId::new("mixed-clock-placement").unwrap(),
        created_revision: RevisionId::new(0),
        spec: SourcePlacementSpec {
            source_version: source.source_version.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(750_000_001, 1_000_000_000).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(0),
            rate_numerator: 3,
            rate_denominator: 1,
        },
    };
    let mut candidates = Vec::new();
    let mut jobs = Vec::new();
    if archived {
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::ImportSource {
                    path: source.path.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("captured archive source");
        };
        assert_eq!(source_version, source.source_version);
        let db = f.engine.db.lock().unwrap();
        source.path = PathBuf::from(
            db.query_row(
                "SELECT path FROM sources WHERE project=?1 AND version=?2",
                params![f.project.as_str(), source_version.as_str()],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
        );
        let tx = db.unchecked_transaction().unwrap();
        f.engine.arm_worker_fence(&tx).unwrap();
        tx.commit().unwrap();
    } else {
        let db = f.engine.db.lock().unwrap();
        db.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes) VALUES(?1,?2,?3,?3,'authored mixed clock',?4,?5)",params![f.project.as_str(),source.source_version.as_str(),source.path.to_string_lossy(),source.identity.sha256,source.identity.byte_len]).unwrap();
    }
    for stride in [3, 1] {
        let attempt = id!(AttemptId);
        let output = if archived {
            f.engine
                .config
                .state_dir
                .join("attempts")
                .join(attempt.as_str())
        } else {
            f.dir.path().join(format!("producer-{stride}"))
        };
        fs::create_dir_all(&output).unwrap();
        let manifest = WorkerRequest {
            version: PROTOCOL_VERSION,
            attempt_id: attempt,
            job_id: id!(JobId),
            project_id: f.project.clone(),
            base_revision: RevisionId::new(if stride == 3 { 0 } else { 1 }),
            source: source.clone(),
            output_dir: output.canonicalize().unwrap(),
            operation: WorkerOperation::Generate {
                preset: "fast".into(),
                settings: (GenerationSettings {
                    source_placement: Some(placement.clone()),
                    source_sample_stride: stride,
                    analysis_width: 64,
                    analysis_height: 64,
                    ..Default::default()
                })
                .into(),
                model: None,
            },
            budget: ResourceBudget {
                memory_bytes: 512 * 1024 * 1024,
                output_bytes: 16 * 1024 * 1024,
                wall_time_ms: 30_000,
                cpu_threads: 1,
            },
            tools: tools.clone(),
            dependencies: vec![],
            cuda_execution: None,
            independent_audio: None,
        };
        let WorkerOutput::Generated {
            mut program,
            mut receipt,
            mut lineage,
            review,
        } = crate::worker::execute_manifest(&manifest).result.unwrap()
        else {
            panic!("authored generation");
        };
        let broker_program = output.join("program.json");
        let broker_receipt = output.join("receipt.json");
        fs::rename(&program.path, &broker_program).unwrap();
        program.path = broker_program;
        fs::rename(&receipt.path, &broker_receipt).unwrap();
        receipt.path = broker_receipt;
        let motion: MotionProgram =
            serde_json::from_slice(&fs::read(&program.path).unwrap()).unwrap();
        assert!(
            motion.track(Axis::Stroke).unwrap().gaps().is_empty(),
            "authored stride {stride} must have decoded support through the placement boundary"
        );
        if archived {
            lineage.push(receipt.identity.clone());
        } else {
            lineage = vec![receipt.identity.clone()];
        }
        let candidate = id!(CandidateId);
        let raw = encode(&manifest).unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,?7,?4,?5,'complete',?6)",params![manifest.job_id.as_str(),manifest.attempt_id.as_str(),f.project.as_str(),f.session.as_str(),source.source_version.as_str(),raw,rev_sql(manifest.base_revision).unwrap()]).unwrap();
            db.execute(
                "INSERT INTO attempts VALUES(?1,?2,?3,?4)",
                params![
                    manifest.attempt_id.as_str(),
                    manifest.job_id.as_str(),
                    raw,
                    if archived { "completed" } else { "complete" }
                ],
            )
            .unwrap();
            let reference = motion_state::save(&db, &motion).unwrap();
            db.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?7,?3,?4,?5,?6)",params![candidate.as_str(),f.project.as_str(),reference,manifest.job_id.as_str(),encode(&lineage).unwrap(),encode(&review).unwrap(),rev_sql(manifest.base_revision).unwrap()]).unwrap();
        }
        if stride == 3 {
            assert!(matches!(
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::CommitCandidate {
                            candidate_id: candidate.clone()
                        }
                    ))
                    .result
                    .unwrap(),
                ResponseBody::Project(_)
            ));
        }
        candidates.push(candidate);
        jobs.push(manifest.job_id);
    }
    let merged = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::MergeCandidate {
                candidate_id: candidates[1].clone(),
                axes: vec![Axis::Stroke],
                range: Some(
                    TimeRange::new(
                        ProjectTime::from_nanos(125_000_000),
                        ProjectTime::from_nanos(250_000_000),
                    )
                    .unwrap(),
                ),
            },
        ))
        .result
        .unwrap();
    let ResponseBody::Project(master) = merged else {
        panic!("merged project");
    };
    if archived {
        verify_mixed_archive_clone(&f, &source, &master, &candidates[0], &jobs[0], retimed);
        return;
    }
    let capture = StashSourceCapture {
        endpoint: Some("http://127.0.0.1:1/graphql".into()),
        source_version: source.source_version.clone(),
        identity: source.identity.clone(),
        scene_id: "authored-scene".into(),
        file_id: "authored-file".into(),
        server_path: "/authored/moving.mkv".into(),
        local_source: source.path.clone(),
        script_destination: source.path.with_extension("funscript"),
    };
    {
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "INSERT INTO stash_sources(project,version,scene,file,receipt) VALUES(?1,?2,?3,?4,?5)",
            params![
                f.project.as_str(),
                capture.source_version.as_str(),
                capture.scene_id,
                capture.file_id,
                encode(&capture).unwrap()
            ],
        )
        .unwrap();
    }
    let command = Command::StashExportScript {
        connection: StashConnection {
            endpoint: capture.endpoint.clone().unwrap(),
            credential_env: None,
        },
        source_version: source.source_version.clone(),
        scene_id: capture.scene_id.clone(),
        file_id: capture.file_id.clone(),
        candidate_id: candidates[0].clone(),
        job_id: jobs[0].clone(),
        archived_origin: None,
        source_time: true,
        owner_proof: f.engine.stash_owner_token.clone(),
    };
    let export = request(
        &f.session,
        Some(&f.project),
        Some(master.revision.value()),
        command,
    );
    let publisher = crate::stash_files::Publication::prepare(&capture).unwrap();
    // Exercise the production export seam after the independently tested
    // Stash discovery/capture preflight; this fixture contacts no server.
    let ResponseBody::StashScriptExported(receipt) = f
        .engine
        .export(
            &export,
            &capture.script_destination,
            "authored-mixed-clock-export",
            ExportKind::Stash {
                capture: &capture,
                candidate: &candidates[0],
                job: &jobs[0],
                archived_origin: None,
                publisher: &publisher,
            },
        )
        .unwrap()
    else {
        panic!("published script");
    };
    let proof = receipt.timing_proof.as_ref().unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&fs::read(&proof.path).unwrap()).unwrap();
    assert_eq!(raw["schema_version"], 6);
    assert_eq!(
        raw["context"]["action_ancestry"]["carriers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for (project_time, frame, source_ns) in
        [(166_666_666, 4, 500_000_000), (208_333_333, 5, 625_000_000)]
    {
        let mapping = raw["mappings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["project_time"] == project_time)
            .unwrap();
        assert_eq!(mapping["original_source_frame"], frame);
        assert_eq!(mapping["quantization"]["time"], source_ns);
    }
    assert!(!receipt.release_qualified);
    let observe = request(
        &f.session,
        Some(&f.project),
        None,
        Command::StashPublicationStatus {
            publication_request_id: export.request_id.clone(),
            owner_proof: f.engine.stash_owner_token.clone(),
        },
    );
    let ResponseBody::StashPublication(observation) = f
        .engine
        .stash_publication_observation(&observe, &export.request_id)
        .unwrap()
    else {
        panic!("publication observation");
    };
    assert_eq!(
        observation.proof_observation,
        StashArtifactObservation::Verified
    );
    let saved_response = encode(&ResponseBody::StashScriptExported(receipt.clone())).unwrap();
    for fault in ["mapping", "selection", "anchor", "legacy_mapping"] {
        let mut forged = raw.clone();
        match fault {
            "mapping" => forged["mappings"][0]["original_source_frame"] = serde_json::json!(777),
            "selection" => {
                forged["context"]["action_ancestry"]["selections"][2]["carrier"] =
                    serde_json::json!(0)
            }
            "anchor" => {
                forged["context"]["original_candidate"] = serde_json::json!("wrong-native-anchor")
            }
            _ => {
                forged["schema_version"] = serde_json::json!(2);
                forged["context"]
                    .as_object_mut()
                    .unwrap()
                    .remove("action_ancestry");
                forged["mappings"][0]["original_source_frame"] = serde_json::json!(777);
            }
        }
        let bytes = serde_json::to_vec(&forged).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let mut reference = proof.clone();
        reference.identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash.clone(),
            byte_len: bytes.len() as u64,
        };
        reference.path = proof.path.parent().unwrap().join(format!("{hash}.json"));
        fs::write(&reference.path, &bytes).unwrap();
        let mut forged_receipt = receipt.clone();
        forged_receipt.timing_proof = Some(reference.clone());
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(&reference).unwrap(),
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
            db.execute(
                "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(&ResponseBody::StashScriptExported(forged_receipt)).unwrap(),
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
        }
        let ResponseBody::StashPublication(observed) = f
            .engine
            .stash_publication_observation(&observe, &export.request_id)
            .unwrap()
        else {
            panic!("forged observation");
        };
        assert_eq!(
            observed.proof_observation,
            StashArtifactObservation::Unverified,
            "{fault}"
        );
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE stash_clock_proofs SET proof=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(proof).unwrap(),
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
            db.execute(
                "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                params![
                    saved_response,
                    f.session.as_str(),
                    export.request_id.as_str()
                ],
            )
            .unwrap();
        }
        fs::remove_file(&reference.path).unwrap();
        let ResponseBody::StashPublication(restored) = f
            .engine
            .stash_publication_observation(&observe, &export.request_id)
            .unwrap()
        else {
            panic!("restored observation");
        };
        assert_eq!(
            restored.proof_observation,
            StashArtifactObservation::Verified
        );
    }
}

#[test]
fn annotated_submission_requires_imported_source_before_model_snapshot_or_job_publication() {
    let f = fixture();
    let manifest: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap();
    let WorkerOperation::Generate { mut settings, .. } = manifest.operation else {
        panic!("generate fixture");
    };
    let profile = settings.inference_profile.as_mut().unwrap();
    profile.provider = InferenceProvider::Cpu;
    profile.schema_version = 1;
    profile.cuda_resources = None;
    profile.preprocessing.resize = InferenceResize::AnnotatedTemporalView;
    settings.annotated_view=Some(serde_json::from_value(serde_json::json!({
        "source_sha256":"a".repeat(64),"declaration":{"schema_version":1,
            "source_dimensions":[128,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"authored",
            "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}}}
    })).unwrap());
    settings.validate().unwrap();
    let version: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let snapshots = f.engine.config.state_dir.join("snapshots");
    let before = fs::read_dir(&snapshots).unwrap().count();
    let error = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::Generate {
                source_version: SourceVersionId::new("absent-source").unwrap(),
                preset: "fast".into(),
                settings: Some(*settings),
                model_path: Some(f.dir.path().join("absent-model.onnx")),
            },
        ))
        .result
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound, "{error}");
    let db = f.engine.db.lock().unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        version
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(fs::read_dir(snapshots).unwrap().count(), before);
}

#[cfg(target_os = "linux")]
#[test]
fn annotated_storage_reopens_and_bad_membership_refuses_before_recovery_or_wal_write() {
    for corrupt in [
        None,
        Some("UPDATE attempts SET manifest=manifest || ' '"),
        Some("DELETE FROM annotated_generation_attempts"),
        Some("PRAGMA user_version=20"),
    ] {
        let f = fixture();
        let mut manifest = crate::annotated_generation_db::tests::request();
        manifest.project_id = f.project.clone();
        manifest.base_revision = RevisionId::new(0);
        let raw = serde_json::to_string(&manifest).unwrap();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            f.engine.arm_worker_fence(&tx).unwrap();
            tx.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes) VALUES(?1,?2,?3,?3,'authored metadata',?4,?5)",params![f.project.as_str(),manifest.source.source_version.as_str(),manifest.source.path.to_string_lossy(),manifest.source.identity.sha256,manifest.source.identity.byte_len]).unwrap();
            tx.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,'failed',?6)",params![manifest.job_id.as_str(),manifest.attempt_id.as_str(),f.project.as_str(),f.session.as_str(),manifest.source.source_version.as_str(),raw]).unwrap();
            tx.execute(
                "INSERT INTO attempts VALUES(?1,?2,?3,'failed')",
                params![manifest.attempt_id.as_str(), manifest.job_id.as_str(), raw],
            )
            .unwrap();
            crate::annotated_generation_db::retain(&tx, &manifest, &f.engine.pool).unwrap();
            tx.commit().unwrap();
        }
        let config = Engine::retire_for_test(f.engine);
        if let Some(sql) = corrupt {
            let observer = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
            observer.execute_batch(sql).unwrap();
            let identities = || -> Vec<(String, String)> {
                [
                    "projects.sqlite3",
                    "projects.sqlite3-wal",
                    "worker-fence.json",
                ]
                .iter()
                .filter_map(|name| {
                    let path = config.state_dir.join(name);
                    path.is_file().then(|| {
                        (
                            name.to_string(),
                            format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                        )
                    })
                })
                .collect()
            };
            let before = identities();
            assert!(
                reopen_after_transient_leases(config.clone()).is_err(),
                "{sql}"
            );
            assert_eq!(identities(), before, "{sql}");
            drop(observer);
        } else {
            let engine = reopen_after_transient_leases(config).unwrap();
            let db = engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                23
            );
            assert_eq!(
                db.query_row(
                    "SELECT manifest FROM attempts WHERE id=?1",
                    [manifest.attempt_id.as_str()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                raw
            );
            engine.require_worker_fence(&db).unwrap();
            crate::annotated_generation_db::validate(&db, &engine.pool).unwrap();
        }
    }
}

#[cfg(target_os = "linux")]
fn reopen_after_transient_leases(config: EngineConfig) -> Result<Arc<Engine>> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match Engine::open(config.clone()) {
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5))
            }
            result => return result,
        }
    }
}
#[cfg(target_os = "linux")]
fn arm_fixture(f: &Fixture) {
    let mut db = f.engine.db.lock().unwrap();
    let tx = db.transaction().unwrap();
    // Historical-reader fixtures intentionally retain the preceding format.
    crate::worker_fence_db::arm(&tx, &f.engine.worker_fence.scope().unwrap()).unwrap();
    f.engine.require_worker_fence(&tx).unwrap();
    tx.commit().unwrap();
}
#[cfg(target_os = "linux")]
fn live_cuda_storage_fixture() -> Fixture {
    let f = fixture();
    let source_path = f.dir.path().join("storage-source.bin");
    fs::write(&source_path, b"authored storage fixture").unwrap();
    let ResponseBody::Source { source_version } = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::ImportSource { path: source_path },
        ))
        .result
        .unwrap()
    else {
        panic!("source")
    };
    let original = crate::live_cuda_evidence_db::tests::evidence(&f.engine.pool);
    let admission = crate::live_cuda_evidence::EvidenceAdmission::reserve(&f.engine.pool).unwrap();
    // Authored rebinding tests durable consistency only. It does not prove
    // new native execution or an authorized live finalization.
    let mut manifest = original.manifest().clone();
    manifest.stream.identity.project = f.project.clone();
    manifest.stream.identity.source = source_version;
    let mut summary = original.summary().clone();
    summary.manifest_sha256 = manifest.identity_sha256().unwrap();
    let mut receipt: serde_json::Value = serde_json::from_slice(original.receipt()).unwrap();
    receipt["live_binding"] = serde_json::to_value(&summary).unwrap();
    let mut evidence = admission
        .finished(
            &manifest,
            &summary,
            serde_json::to_vec(&receipt).unwrap(),
            original.trace().to_vec(),
        )
        .unwrap();
    drop(original);
    evidence
        .reserve_storage(fs2::available_space(&f.engine.config.state_dir).unwrap())
        .unwrap();
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        f.engine.arm_worker_fence(&tx).unwrap();
        crate::live_cuda_evidence_db::retain(&tx, &evidence).unwrap();
        tx.commit().unwrap();
    }
    f
}
#[cfg(target_os = "linux")]
#[test]
fn live_cuda_storage_reopens_and_refuses_corruption_before_recovery_or_wal() {
    for bad in [
        "PRAGMA user_version=20",
        "DROP TABLE live_cuda_evidence",
        "UPDATE live_cuda_evidence SET trace=CAST(trace || ' ' AS BLOB)",
    ] {
        let f = live_cuda_storage_fixture();
        let config = f.engine.config.clone();
        drop(f.engine);
        let reopened = reopen_after_transient_leases(config.clone()).unwrap();
        assert_eq!(
            reopened
                .db
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            21
        );
        drop(reopened);
        let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        keeper
            .execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        keeper.execute_batch(bad).unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
        assert!(Engine::open(config).is_err(), "{bad}");
        assert_eq!(
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before,
            "{bad}"
        );
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires explicitly preserved schema-20 application reader; run serially"]
fn live_cuda_storage_refuses_preceding_reader_without_db_wal_or_ledger_mutation() {
    let reader = std::env::var_os("PULSAR_LIVE_CUDA_PRECEDING_READER")
        .expect("pin preceding schema-20 application");
    let f = live_cuda_storage_fixture();
    let config = f.engine.config.clone();
    drop(f.engine);
    let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
    keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' schema21';").unwrap();
    let paths = [
        config.state_dir.join("projects.sqlite3"),
        config.state_dir.join("projects.sqlite3-wal"),
        config.state_dir.join("worker-fence.json"),
    ];
    assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
    let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let result = std::process::Command::new("/usr/bin/timeout")
        .args(["--signal=TERM", "5"])
        .arg(&reader)
        .arg("engine")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PULSAR_STATE_DIR", &config.state_dir)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration"));
    assert!(!config.state_dir.join("engine.sock").exists());
    assert_eq!(
        paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    let reopened = reopen_after_transient_leases(config.clone()).unwrap();
    assert_eq!(
        reopened
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM live_cuda_evidence", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    if let Some(base) = std::env::var_os("PULSAR_LIVE_CUDA_READER_EVIDENCE") {
        let base = PathBuf::from(base);
        assert!(!base.exists());
        artifacts::private_directory(&base).unwrap();
        fs::write(base.join("preceding.stderr"), &result.stderr).unwrap();
        for (path, bytes) in paths.iter().zip(before) {
            fs::write(base.join(path.file_name().unwrap()), bytes).unwrap();
        }
    }
    eprintln!("schema-20 application refused schema21 with unchanged DB/WAL/ledger; current Engine readback reopened one retained row");
}
#[cfg(target_os = "linux")]
#[test]
fn staging_requirement_refuses_live_and_restart_admission_before_mutating_state() {
    let f = fixture();
    arm_fixture(&f);
    let config = f.engine.config.clone();
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        crate::worker_fence_db::arm_staging(&tx, &f.engine.worker_fence.scope().unwrap()).unwrap();
        assert!(f.engine.require_worker_fence(&tx).is_err());
        assert!(f.engine.arm_worker_fence(&tx).is_err());
        tx.commit().unwrap();
    }
    drop(f.engine);
    let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
    keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' staged-reader-test';").unwrap();
    let paths = [
        config.state_dir.join("projects.sqlite3"),
        config.state_dir.join("projects.sqlite3-wal"),
        config.state_dir.join("worker-fence.json"),
    ];
    assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
    let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let error = Engine::open(config)
        .err()
        .expect("format-2 ledger admitted below staging requirement");
    assert!(
        error.to_string().contains("below its database requirement"),
        "{error:#}"
    );
    assert_eq!(
        paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>(),
        before
    );
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a writable delegated cgroup namespace; run explicitly serially"]
fn native_creation_requirement_reopens_schema20_with_format4_and_preserves_scope() {
    let f = fixture();
    let config = f.engine.config.clone();
    let scope = f.engine.worker_fence.scope().unwrap();
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        f.engine.arm_worker_fence(&tx).unwrap();
        f.engine.require_worker_fence(&tx).unwrap();
        f.engine.arm_staging_fence(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }
    assert_eq!(f.engine.worker_fence.version().unwrap(), 4);
    let observer = Connection::open_with_flags(
        config.state_dir.join("projects.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        observer
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        20
    );
    assert_eq!(
        crate::worker_fence_db::binding(&observer, 20)
            .unwrap()
            .unwrap()
            .minimum_ledger_version,
        4
    );
    let claim = Arc::new(f.engine.pool.reserve(32 * 1024 * 1024).unwrap());
    let admitted = f
        .engine
        .worker_fence
        .admit_group(claim, 32 * 1024 * 1024, 1)
        .unwrap();
    let mut custody = admitted.into_worker();
    let group = custody.group_handle();
    let path = group.path().to_owned();
    custody
        .retire(std::time::Instant::now() + std::time::Duration::from_secs(3))
        .unwrap();
    drop(group);
    drop(custody);
    assert!(!path.exists());
    drop(observer);
    drop(f.engine);
    let reopened = reopen_after_transient_leases(config).unwrap();
    assert_eq!(reopened.worker_fence.scope().unwrap(), scope);
    assert_eq!(reopened.worker_fence.version().unwrap(), 4);
    let db = reopened.db.lock().unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        20
    );
    reopened.require_worker_fence(&db).unwrap();
}
#[cfg(target_os = "linux")]
#[test]
fn creation_requirement_refuses_live_and_restart_admission_before_mutating_state() {
    let f = fixture();
    let config = f.engine.config.clone();
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        f.engine.arm_staging_fence(&tx).unwrap();
        crate::worker_fence_db::arm_creation(&tx, &f.engine.worker_fence.scope().unwrap()).unwrap();
        assert!(f.engine.require_worker_fence(&tx).is_err());
        assert!(f.engine.arm_worker_fence(&tx).is_err());
        assert!(f.engine.arm_staging_fence(&tx).is_err());
        assert_eq!(
            crate::worker_fence_db::binding(&tx, 20)
                .unwrap()
                .unwrap()
                .minimum_ledger_version,
            4
        );
        tx.commit().unwrap();
        source_placements::initialize(&mut db, 20).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }
    drop(f.engine);
    let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
    keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' creation-reader-test';").unwrap();
    let paths = [
        config.state_dir.join("projects.sqlite3"),
        config.state_dir.join("projects.sqlite3-wal"),
        config.state_dir.join("worker-fence.json"),
    ];
    assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
    let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let error = Engine::open(config)
        .err()
        .expect("format-3 ledger admitted below creation requirement");
    assert!(
        error.to_string().contains("below its database requirement"),
        "{error:#}"
    );
    assert_eq!(
        paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>(),
        before
    );
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires explicitly preserved format-2 application reader; run serially"]
fn native_staging_requirement_refuses_preceding_reader_without_db_wal_mutation() {
    exercise_creation_reader_requirement(false);
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires explicitly preserved schema-19 application reader; run serially"]
fn native_creation_requirement_refuses_schema19_reader_without_db_wal_mutation() {
    exercise_creation_reader_requirement(true);
}
#[cfg(target_os = "linux")]
fn exercise_creation_reader_requirement(creation: bool) {
    use std::os::unix::fs::FileTypeExt;
    struct Process(std::process::Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let reader = std::env::var_os(if creation {
        "PULSAR_CREATION_PRECEDING_READER"
    } else {
        "PULSAR_STAGING_PRECEDING_READER"
    })
    .expect("preceding reader required");
    let f = fixture();
    arm_fixture(&f);
    let config = f.engine.config.clone();
    if creation {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        f.engine.arm_staging_fence(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            19
        );
    }
    let scope = f.engine.worker_fence.scope().unwrap();
    drop(f.engine);
    let socket = config.state_dir.join("engine.sock");
    assert!(!socket.exists());
    let mut positive = Process(
        std::process::Command::new(&reader)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PULSAR_STATE_DIR", &config.state_dir)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if fs::symlink_metadata(&socket).is_ok_and(|m| m.file_type().is_socket()) {
            break;
        }
        assert!(
            positive.0.try_wait().unwrap().is_none(),
            "preceding reader did not open old binding"
        );
        assert!(
            std::time::Instant::now() < end,
            "preceding reader listener deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    positive.0.kill().unwrap();
    positive.0.wait().unwrap();
    drop(positive);
    fs::remove_file(&socket).unwrap();
    eprintln!(
        "preceding application reader opened schema-{} format-{} state and reached its listener",
        if creation { 19 } else { 17 },
        if creation { 3 } else { 2 }
    );
    let mut keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
    keeper
        .execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let tx = keeper.transaction().unwrap();
    if creation {
        crate::worker_fence_db::arm_creation(&tx, &scope).unwrap();
    } else {
        crate::worker_fence_db::arm_staging(&tx, &scope).unwrap();
    }
    tx.commit().unwrap();
    keeper
        .execute_batch("UPDATE projects SET name=name || ' native staging-reader-test';")
        .unwrap();
    let paths = [
        config.state_dir.join("projects.sqlite3"),
        config.state_dir.join("projects.sqlite3-wal"),
        config.state_dir.join("worker-fence.json"),
    ];
    assert!(fs::metadata(&paths[1]).unwrap().len() > 32);
    let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let result = std::process::Command::new("/usr/bin/timeout")
        .args(["--signal=TERM", "5"])
        .arg(&reader)
        .arg("engine")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PULSAR_STATE_DIR", &config.state_dir)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration"));
    assert!(!socket.exists());
    assert_eq!(
        paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    let error = Engine::open(config)
        .err()
        .expect("downgraded ledger admitted");
    assert!(
        error.to_string().contains("below its database requirement"),
        "{error:#}"
    );
    assert_eq!(
        paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    eprintln!("preceding reader refused new binding; current engine refused insufficient ledger; committed DB/WAL and fence bytes unchanged");
}
#[cfg(target_os = "linux")]
#[test]
fn engine_bound_fence_reopens_with_same_scope_and_barrier() {
    let f = fixture();
    assert!(
        crate::worker_fence_db::scope(&f.engine.db.lock().unwrap(), 8)
            .unwrap()
            .is_none()
    );
    arm_fixture(&f);
    let scope = f.engine.worker_fence.scope().unwrap();
    let config = f.engine.config.clone();
    drop(f.engine);
    let reopened = reopen_after_transient_leases(config).unwrap();
    let db = reopened.db.lock().unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        17
    );
    assert_eq!(
        crate::worker_fence_db::scope(&db, 17).unwrap().as_deref(),
        Some(scope.as_str())
    );
    assert_eq!(reopened.worker_fence.scope().unwrap(), scope);
}
#[cfg(target_os = "linux")]
#[test]
fn engine_bound_fence_refuses_a_new_scope_or_missing_files() {
    for case in ["scope", "both-files"] {
        let f = fixture();
        arm_fixture(&f);
        let config = f.engine.config.clone();
        drop(f.engine);
        // Observe old-owner release before changing this test's own files.
        let owner = reopen_after_transient_leases(config.clone()).unwrap();
        drop(owner);
        if case == "scope" {
            let path = config.state_dir.join("worker-fence.json");
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["scope"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
            // Preserve the primitive's canonical struct field order.
            let old = fs::read_to_string(&path).unwrap();
            let old_scope = serde_json::from_str::<serde_json::Value>(&old).unwrap()["scope"]
                .as_str()
                .unwrap()
                .to_string();
            fs::write(
                path,
                old.replace(&old_scope, value["scope"].as_str().unwrap()),
            )
            .unwrap();
        } else {
            fs::remove_file(config.state_dir.join("worker-fence.json")).unwrap();
            fs::remove_file(config.state_dir.join("worker-fence.lock")).unwrap();
        }
        assert!(
            reopen_after_transient_leases(config.clone()).is_err(),
            "{case}"
        );
        if case == "both-files" {
            assert!(!config.state_dir.join("worker-fence.lock").exists());
        }
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an owned writable delegated cgroup; run explicitly serially"]
fn native_engine_restart_refuses_populated_roster_without_signal_then_recovers() {
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    use std::process::{Command, Stdio};
    let f = fixture();
    arm_fixture(&f);
    let config = f.engine.config.clone();
    let group = crate::cgroup::AttemptCgroup::create(
        &format!("engine-restart-test-{}", uuid::Uuid::new_v4()),
        32 * 1024 * 1024,
        1,
    )
    .unwrap();
    f.engine.worker_fence.register(&group).unwrap();
    let fd = group.procs.as_raw_fd();
    let mut command = Command::new("/bin/sleep");
    command
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(move || {
            if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    drop(f.engine);
    let ledger = fs::read(config.state_dir.join("worker-fence.json")).unwrap();
    let error = reopen_after_transient_leases(config.clone())
        .err()
        .expect("populated roster admitted");
    assert!(error.to_string().contains("populated"), "{error:#}");
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(
        fs::read(config.state_dir.join("worker-fence.json")).unwrap(),
        ledger
    );
    group.kill();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() || !group.is_empty().unwrap() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let reopened = reopen_after_transient_leases(config.clone()).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(config.state_dir.join("worker-fence.json")).unwrap())
            .unwrap();
    assert!(value["groups"].as_object().unwrap().is_empty());
    assert_eq!(
        reopened
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        17
    );
}
fn capture_options() -> LiveCaptureOptions {
    LiveCaptureOptions {
        inference_fallback: None,
        browser_analysis: None,
        browser_media: None,
        model_pack: None,
        source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
        continuation_mode: LiveContinuationMode::Immediate,
        encoded_source: None,
        inference: None,
        policy: LivePolicy {
            maximum_analysis_age_ns: 200_000_000,
            added_buffer_ns: 0,
        },
        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
        project_anchor: ProjectTime::ZERO,
        exact_project_anchor: None,
        presentation_anchor: LiveClockTick(0),
        lease_ms: 30_000,
    }
}
fn capture_start(f: &Fixture) -> Request {
    request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::StartLiveCapture {
            engine_epoch: AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
            options: capture_options(),
        },
    )
}
fn capture_report(body: ResponseBody) -> LiveCaptureReport {
    let ResponseBody::LiveCapture(report) = body else {
        panic!("capture response")
    };
    *report
}
#[cfg(target_os = "linux")]
#[test]
fn capture_restart_replays_retirement_without_admitting_old_boot() {
    let f = fixture();
    let start = capture_start(&f);
    let initial = capture_report(f.engine.handle(start.clone()).result.unwrap());
    let continuation = capture_continue(&f, &initial);
    let next = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
    let handshake = live_handshake(&f, &next);
    let config = f.engine.config.clone();
    let Fixture {
        engine,
        session,
        project,
        dir: _dir,
    } = f;
    drop(engine);
    let engine = reopen_after_transient_leases(config).unwrap();
    assert_ne!(engine.engine_epoch, initial.engine_epoch.as_str());
    for intent in [&start, &continuation] {
        let retired = capture_report(engine.handle(intent.clone()).result.unwrap());
        retired.validate().unwrap();
        assert!(!retired.active);
        assert_eq!(
            retired.retirement,
            Some(LiveCaptureRetirement::EngineRestarted)
        );
        assert_eq!(retired.admitted_pixel_bytes, 0);
        assert!(retired.server_elapsed.is_none());
    }
    assert!(!engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&initial.identity.epoch));
    assert!(!engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&next.identity.epoch));
    assert!(engine.live_bulk_ready(&handshake, None).is_err());
    let mut stale = start.clone();
    stale.request_id = id!(RequestId);
    assert_eq!(
        engine.handle(stale).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    let mut conflict = start.clone();
    if let Command::StartLiveCapture { options, .. } = &mut conflict.command {
        options.lease_ms = 20_000;
    }
    assert_eq!(
        engine.handle(conflict).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    let mut revision = start.clone();
    revision.request_id = id!(RequestId);
    revision.expected_revision = Some(RevisionId::new(1));
    assert_eq!(
        engine.handle(revision).result.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let fresh = request(
        &session,
        Some(&project),
        Some(0),
        Command::StartLiveCapture {
            engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
            options: capture_options(),
        },
    );
    let active = capture_report(engine.handle(fresh).result.unwrap());
    assert!(active.active);
    assert_ne!(active.identity.epoch, next.identity.epoch);
    engine
        .handle(request(
            &session,
            Some(&project),
            Some(0),
            Command::StopLiveCapture {
                epoch: active.identity.epoch,
            },
        ))
        .result
        .unwrap();
    let admin = pair(&engine, "restart replay administrator");
    engine
        .handle(request(
            &session,
            None,
            None,
            Command::Grant {
                session: admin.clone(),
                project_id: project.clone(),
                scopes: vec![Scope::Read, Scope::ManageGrants],
            },
        ))
        .result
        .unwrap();
    engine
        .handle(request(
            &admin,
            None,
            None,
            Command::Revoke {
                session: session.clone(),
                project_id: project.clone(),
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        engine.handle(start).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
}
#[test]
fn capture_lifecycle_replays_without_rearming_and_preserves_project_revision() {
    let f = fixture();
    let start = capture_start(&f);
    let first = capture_report(f.engine.handle(start.clone()).result.unwrap());
    assert!(first.active);
    assert_eq!(
        first.admitted_pixel_bytes,
        crate::live_frames::LIVE_RGB_ADMISSION_BYTES
    );
    assert_eq!(first.received_frames, 0);
    assert_eq!(
        capture_report(f.engine.handle(start.clone()).result.unwrap()).identity,
        first.identity
    );
    let mut changed = start.clone();
    if let Command::StartLiveCapture { options, .. } = &mut changed.command {
        options.lease_ms = 20_000;
    }
    assert_eq!(
        f.engine.handle(changed).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    assert_eq!(
        f.engine.handle(capture_start(&f)).result.unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    let stop = request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::StopLiveCapture {
            epoch: first.identity.epoch.clone(),
        },
    );
    let stopped = capture_report(f.engine.handle(stop.clone()).result.unwrap());
    assert!(!stopped.active);
    assert_eq!(stopped.retirement, Some(LiveCaptureRetirement::Stopped));
    assert_eq!(stopped.admitted_pixel_bytes, 0);
    assert!(!capture_report(f.engine.handle(stop).result.unwrap()).active);
    assert!(!capture_report(f.engine.handle(start).result.unwrap()).active);
    assert_eq!(
        project_snapshot(&f.engine.db.lock().unwrap(), &f.project)
            .unwrap()
            .revision,
        RevisionId::new(0)
    );
    assert!(capture_report(f.engine.handle(capture_start(&f)).result.unwrap()).active);
}
fn capture_continue(f: &Fixture, prior: &LiveCaptureReport) -> Request {
    let mut options = prior.options.clone();
    options.source_anchor = SourceTimestamp::new(1, 1).unwrap();
    options.project_anchor = ProjectTime::from_nanos(1_000_000_000);
    options.presentation_anchor = LiveClockTick(0);
    request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::ContinueLiveCapture {
            engine_epoch: prior.engine_epoch.clone(),
            predecessor: prior.identity.epoch.clone(),
            options,
        },
    )
}
#[test]
fn capture_continuation_fences_predecessor_and_replay_never_rearms() {
    let f = fixture();
    let start = capture_start(&f);
    let prior = capture_report(f.engine.handle(start.clone()).result.unwrap());
    let old_channel = live_handshake(&f, &prior);
    f.engine
        .live_bulk_upload(&old_channel, &live_header(0, 0), &vec![0; LIVE_FRAME_BYTES])
        .unwrap();
    let continuation = capture_continue(&f, &prior);
    let next = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
    assert_ne!(prior.identity.epoch, next.identity.epoch);
    assert_eq!(prior.identity.source, next.identity.source);
    assert_eq!(
        next.continuation.as_ref().unwrap().predecessor,
        prior.identity
    );
    assert_eq!(
        next.continuation
            .as_ref()
            .unwrap()
            .predecessor_last_frame
            .as_ref()
            .unwrap()
            .frame,
        FrameId::new(0)
    );
    assert_eq!(next.received_frames, 0);
    assert!(next.last_frame.is_none());
    assert!(f
        .engine
        .live_bulk_upload(&old_channel, &live_header(1, 1), &vec![1; LIVE_FRAME_BYTES])
        .is_err());
    let retired = capture_report(f.engine.handle(start).result.unwrap());
    assert_eq!(retired.retirement, Some(LiveCaptureRetirement::Superseded));
    assert!(retired.queue.is_none());
    let new_channel = live_handshake(&f, &next);
    assert!(f
        .engine
        .live_bulk_upload(&new_channel, &live_header(0, 0), &vec![0; LIVE_FRAME_BYTES])
        .is_err());
    let mut header = live_header(0, 0);
    header.source_time = SourceTimestamp::new(1, 1).unwrap();
    f.engine
        .live_bulk_upload(&new_channel, &header, &vec![0; LIVE_FRAME_BYTES])
        .unwrap();
    let replayed = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
    assert_eq!(replayed.identity, next.identity);
    assert_eq!(replayed.received_frames, 1);
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StopLiveCapture {
                epoch: next.identity.epoch.clone(),
            },
        ))
        .result
        .unwrap();
    assert!(!capture_report(f.engine.handle(continuation).result.unwrap()).active);
    assert!(f
        .engine
        .handle(capture_continue(&f, &prior))
        .result
        .is_err());
    assert_eq!(
        project_snapshot(&f.engine.db.lock().unwrap(), &f.project)
            .unwrap()
            .revision,
        RevisionId::new(0)
    );
}
#[test]
fn capture_continuation_admission_and_receipt_failure_preserve_predecessor() {
    let f = fixture();
    let prior = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    for mode in 0..5 {
        let mut continuation = capture_continue(&f, &prior);
        if let Command::ContinueLiveCapture {
            engine_epoch,
            options,
            ..
        } = &mut continuation.command
        {
            match mode {
                0 => options.source_anchor = SourceTimestamp::new(0, 1).unwrap(),
                1 => options.project_anchor = ProjectTime::ZERO,
                2 => options.presentation_anchor = LiveClockTick(1),
                3 => options.policy.maximum_analysis_age_ns += 1,
                _ => *engine_epoch = AttemptId::new("retired-boot").unwrap(),
            }
        }
        assert!(f.engine.handle(continuation).result.is_err());
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&prior.identity.epoch));
    }
    let continuation = capture_continue(&f, &prior);
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER deny_capture_continuation BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'authored continuation receipt failure'); END;").unwrap();
    assert!(f.engine.handle(continuation.clone()).result.is_err());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&prior.identity.epoch));
    let db = f.engine.db.lock().unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM request_identities WHERE request=?1",
            [continuation.request_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    db.execute_batch("DROP TRIGGER deny_capture_continuation")
        .unwrap();
    drop(db);
    assert!(capture_report(f.engine.handle(continuation).result.unwrap()).active);
}
#[test]
fn fractional_capture_continuation_derives_exact_phase_and_rejects_equal_floor_substitution() {
    let f = fixture();
    let start = capture_start(&f);
    let prior = capture_report(f.engine.handle(start).result.unwrap());
    let anchor = SourceTimestamp::new(1, 30).unwrap();
    let options = prior.continuation_options(anchor, 30_000).unwrap();
    assert_eq!(
        options.exact_project_anchor,
        Some(ExactSeconds::new(1, 30).unwrap())
    );
    assert_eq!(options.project_anchor, ProjectTime::from_nanos(33_333_333));
    let operation = |options| {
        request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::ContinueLiveCapture {
                engine_epoch: prior.engine_epoch.clone(),
                predecessor: prior.identity.epoch.clone(),
                options,
            },
        )
    };
    let mut changed = options.clone();
    changed.exact_project_anchor = Some(ExactSeconds::new(33_333_333, 1_000_000_000).unwrap());
    changed.validate().unwrap();
    assert!(f.engine.handle(operation(changed)).result.is_err());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&prior.identity.epoch));
    let next = capture_report(f.engine.handle(operation(options)).result.unwrap());
    let channel = live_handshake(&f, &next);
    for frame in 0..3 {
        let mut header = live_header(frame, frame as u8);
        header.source_time = SourceTimestamp::new(frame as i64 + 1, 30).unwrap();
        let (report, _) = f
            .engine
            .live_bulk_upload(&channel, &header, &vec![frame as u8; LIVE_FRAME_BYTES])
            .unwrap();
        assert_eq!(
            report.clock.map(header.source_time).unwrap().project,
            prior.clock.map(header.source_time).unwrap().project
        );
    }
    assert_eq!(
        next.clock
            .map(SourceTimestamp::new(3, 30).unwrap())
            .unwrap()
            .project
            .time
            .as_nanos(),
        100_000_000
    );
    let twice = next
        .continuation_options(SourceTimestamp::new(4, 30).unwrap(), 30_000)
        .unwrap();
    assert_eq!(
        twice.exact_project_anchor,
        Some(ExactSeconds::new(2, 15).unwrap())
    );
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StopLiveCapture {
                epoch: next.identity.epoch.clone(),
            },
        ))
        .result
        .unwrap();
    let retired = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: next.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert!(retired
        .continuation_options(SourceTimestamp::new(4, 30).unwrap(), 30_000)
        .is_err());
}
#[test]
fn derived_capture_continuation_replays_after_retirement_and_conflicts_without_rearm() {
    let f = fixture();
    let prior = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let command = Command::ContinueLiveCaptureAt {
        engine_epoch: prior.engine_epoch.clone(),
        predecessor: prior.identity.epoch.clone(),
        source_anchor: SourceTimestamp::new(4, 30).unwrap(),
        lease_ms: 30_000,
    };
    let continuation = request(&f.session, Some(&f.project), Some(0), command.clone());
    let next = capture_report(f.engine.handle(continuation.clone()).result.unwrap());
    assert_eq!(
        next.options.exact_project_anchor,
        Some(ExactSeconds::new(2, 15).unwrap())
    );
    assert_eq!(
        next.options.project_anchor,
        ProjectTime::from_nanos(133_333_333)
    );
    assert_eq!(
        next.clock
            .map(SourceTimestamp::new(6, 30).unwrap())
            .unwrap()
            .project
            .time
            .as_nanos(),
        200_000_000
    );
    assert_eq!(
        capture_report(f.engine.handle(continuation.clone()).result.unwrap()).identity,
        next.identity
    );
    let mut changed = continuation.clone();
    if let Command::ContinueLiveCaptureAt { source_anchor, .. } = &mut changed.command {
        *source_anchor = SourceTimestamp::new(5, 30).unwrap();
    }
    assert_eq!(
        f.engine.handle(changed).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    let bad = request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::ContinueLiveCaptureAt {
            engine_epoch: next.engine_epoch.clone(),
            predecessor: next.identity.epoch.clone(),
            source_anchor: SourceTimestamp::new(4, 30).unwrap(),
            lease_ms: 0,
        },
    );
    assert!(f.engine.handle(bad).result.is_err());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&next.identity.epoch));
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StopLiveCapture {
                epoch: next.identity.epoch.clone(),
            },
        ))
        .result
        .unwrap();
    assert!(!capture_report(f.engine.handle(continuation).result.unwrap()).active);
    assert!(f
        .engine
        .handle(request(&f.session, Some(&f.project), Some(0), command))
        .result
        .is_err());
}
#[test]
fn capture_continuation_requires_owner_current_revision_and_active_authority() {
    let f = fixture();
    let prior = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let other = pair(&f.engine, "foreign continuation actor");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: other.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
            },
        ))
        .result
        .unwrap();
    let command = capture_continue(&f, &prior).command;
    assert_eq!(
        f.engine
            .handle(request(&other, Some(&f.project), Some(0), command.clone()))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.engine
            .handle(request(&f.session, Some(&f.project), Some(1), command))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .expire_for_test(&prior.identity.epoch);
    assert_eq!(
        f.engine
            .handle(capture_continue(&f, &prior))
            .result
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    assert!(!f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&prior.identity.epoch));
}
#[test]
fn capture_owner_revision_and_expiration_are_fenced() {
    let f = fixture();
    let first = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let other = pair(&f.engine, "foreign capture reader");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: other.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(
                &other,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: first.identity.epoch.clone()
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .expire_for_test(&first.identity.epoch);
    let expired = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: first.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert_eq!(expired.retirement, Some(LiveCaptureRetirement::Expired));
    let next = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::SetProtectedRegions {
                regions: vec![ProtectedRegion {
                    axis: None,
                    range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1)).unwrap(),
                }],
            },
        ))
        .result
        .unwrap();
    let retired = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: next.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap(),
    );
    assert_eq!(
        retired.retirement,
        Some(LiveCaptureRetirement::RevisionChanged)
    );
    assert!(!retired.active);
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::LiveCaptureStatus {
                    epoch: next.identity.epoch
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}
#[test]
fn capture_revocation_retires_state_before_restored_read_grant() {
    let f = fixture();
    let first = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let admin = pair(&f.engine, "capture grant administrator");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: admin.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::ManageGrants],
            },
        ))
        .result
        .unwrap();
    f.engine
        .handle(request(
            &admin,
            None,
            None,
            Command::Revoke {
                session: f.session.clone(),
                project_id: f.project.clone(),
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: first.identity.epoch.clone()
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    f.engine
        .handle(request(
            &admin,
            None,
            None,
            Command::Grant {
                session: f.session.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    let retired = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: first.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert_eq!(
        retired.retirement,
        Some(LiveCaptureRetirement::AuthorityLost)
    );
    assert!(!retired.active);
    assert_eq!(
        f.engine.handle(capture_start(&f)).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
}
#[test]
fn capture_timer_expires_without_another_client_request() {
    let f = fixture();
    let mut start = capture_start(&f);
    if let Command::StartLiveCapture { options, .. } = &mut start.command {
        options.lease_ms = 20;
    }
    let live = capture_report(f.engine.handle(start).result.unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&live.identity.epoch)
    {
        assert!(
            std::time::Instant::now() < deadline,
            "capture timer did not retire expired lease"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn capture_receipt_failure_publishes_no_lease_and_rolls_back_request_identity() {
    let f = fixture();
    let start = capture_start(&f);
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER deny_capture_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'authored capture receipt failure'); END;").unwrap();
    assert!(f.engine.handle(start.clone()).result.is_err());
    let db = f.engine.db.lock().unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM request_identities WHERE request=?1",
            [start.request_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    db.execute_batch("DROP TRIGGER deny_capture_receipt")
        .unwrap();
    drop(db);
    let live = capture_report(f.engine.handle(start).result.unwrap());
    assert!(live.active);
}
#[test]
fn capture_restart_refuses_unknown_old_boot_start_and_restores_no_live_state() {
    let f = fixture();
    let start = capture_start(&f);
    let first = capture_report(f.engine.handle(start.clone()).result.unwrap());
    let config = f.engine.config.clone();
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = f;
    drop(engine);
    let reopened = Engine::open(config).unwrap();
    let mut unknown = start;
    unknown.request_id = id!(RequestId);
    assert_eq!(
        reopened.handle(unknown).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    assert_eq!(
        reopened
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: first.identity.epoch
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    drop(dir);
}
fn live_handshake(f: &Fixture, report: &LiveCaptureReport) -> LiveBulkHandshake {
    LiveBulkHandshake {
        version: PROTOCOL_VERSION,
        channel: LiveBulkChannel::LiveCapture,
        session: f.session.clone(),
        auth_token: TOKENS.with(|tokens| tokens.borrow()[f.session.as_str()].clone()),
        engine_epoch: report.engine_epoch.clone(),
        identity: report.identity.clone(),
    }
}
fn live_header(frame: u64, byte: u8) -> LiveFrameHeader {
    LiveFrameHeader {
        frame: FrameId::new(frame),
        source_time: SourceTimestamp::new(frame as i64, 1000).unwrap(),
        capture_tick: LiveClockTick(frame),
        sha256: format!("{:x}", Sha256::digest(vec![byte; LIVE_FRAME_BYTES])),
    }
}
#[test]
fn live_ingress_preserves_latest_pair_and_exact_retry_without_durable_frame_rows() {
    let f = fixture();
    let report = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let handshake = live_handshake(&f, &report);
    let before: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    for i in 0..100 {
        let header = live_header(i, i as u8);
        f.engine.live_bulk_ready(&handshake, Some(&header)).unwrap();
        let (received, replayed) = f
            .engine
            .live_bulk_upload(&handshake, &header, &vec![i as u8; LIVE_FRAME_BYTES])
            .unwrap();
        assert!(!replayed);
        assert_eq!(received.received_frames, i + 1);
        assert_eq!(received.last_frame, Some(header));
    }
    let (retry, replayed) = f
        .engine
        .live_bulk_upload(
            &handshake,
            &live_header(99, 99),
            &vec![99; LIVE_FRAME_BYTES],
        )
        .unwrap();
    assert!(replayed);
    assert_eq!(retry.received_frames, 100);
    assert!(f
        .engine
        .live_bulk_ready(&handshake, Some(&live_header(98, 98)))
        .is_err());
    assert_eq!(
        f.engine
            .live_bulk_ready(&handshake, Some(&live_header(99, 0)))
            .unwrap_err()
            .code,
        ErrorCode::RequestConflict
    );
    let work = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .take_for_test(&report.identity.epoch);
    assert_eq!(work.previous(), vec![98; LIVE_FRAME_BYTES]);
    assert_eq!(work.current(), vec![99; LIVE_FRAME_BYTES]);
    assert_eq!(work.declaration().previous_frame, FrameId::new(98));
    let after: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(before, after);
}
#[test]
fn live_ingress_rejects_bad_identity_hash_and_clocks_without_mutating_capture() {
    let f = fixture();
    let report = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let handshake = live_handshake(&f, &report);
    let mut wrong = handshake.clone();
    wrong.auth_token = "wrong".into();
    assert_eq!(
        f.engine.live_bulk_ready(&wrong, None).unwrap_err().code,
        ErrorCode::Unauthorized
    );
    wrong = handshake.clone();
    wrong.identity.source = SourceVersionId::new("wrong-source").unwrap();
    assert!(f.engine.live_bulk_ready(&wrong, None).is_err());
    wrong = handshake.clone();
    wrong.engine_epoch = AttemptId::new("wrong-boot").unwrap();
    assert!(f.engine.live_bulk_ready(&wrong, None).is_err());
    let mut header = live_header(0, 0);
    assert!(f
        .engine
        .live_bulk_upload(&handshake, &header, &vec![1; LIVE_FRAME_BYTES])
        .is_err());
    assert!(f.engine.live_bulk_upload(&handshake, &header, &[]).is_err());
    header.capture_tick = LiveClockTick(u64::MAX);
    assert!(f.engine.live_bulk_ready(&handshake, Some(&header)).is_err());
    assert_eq!(
        f.engine
            .live_bulk_ready(&handshake, None)
            .unwrap()
            .received_frames,
        0
    );
    f.engine
        .live_bulk_upload(&handshake, &live_header(1, 1), &vec![1; LIVE_FRAME_BYTES])
        .unwrap();
    header = live_header(2, 2);
    header.source_time = SourceTimestamp::new(0, 1).unwrap();
    assert!(f
        .engine
        .live_bulk_upload(&handshake, &header, &vec![2; LIVE_FRAME_BYTES])
        .is_err());
    assert_eq!(
        f.engine
            .live_bulk_ready(&handshake, None)
            .unwrap()
            .last_frame,
        Some(live_header(1, 1))
    );
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StopLiveCapture {
                epoch: report.identity.epoch,
            },
        ))
        .result
        .unwrap();
    assert!(f
        .engine
        .live_bulk_upload(&handshake, &live_header(2, 2), &vec![2; LIVE_FRAME_BYTES])
        .is_err());
}
#[test]
fn live_ingress_rechecks_revocation_and_expiry_between_header_and_pixels() {
    let f = fixture();
    let report = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let handshake = live_handshake(&f, &report);
    let header = live_header(0, 0);
    f.engine.live_bulk_ready(&handshake, Some(&header)).unwrap();
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .expire_for_test(&report.identity.epoch);
    assert!(f
        .engine
        .live_bulk_upload(&handshake, &header, &vec![0; LIVE_FRAME_BYTES])
        .is_err());
    let next = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
    let handshake = live_handshake(&f, &next);
    f.engine.live_bulk_ready(&handshake, Some(&header)).unwrap();
    let admin = pair(&f.engine, "ingress admin");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: admin.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::ManageGrants],
            },
        ))
        .result
        .unwrap();
    f.engine
        .handle(request(
            &admin,
            None,
            None,
            Command::Revoke {
                session: f.session.clone(),
                project_id: f.project.clone(),
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .live_bulk_upload(&handshake, &header, &vec![0; LIVE_FRAME_BYTES])
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert!(!f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&next.identity.epoch));
}
fn analysis_work(
    f: &Fixture,
) -> (
    LiveCaptureReport,
    AttemptId,
    crate::live_frames::LiveRgbWork,
    LiveInferenceReply,
) {
    let capture = capture_report(f.engine.handle(capture_start(f)).result.unwrap());
    let artifact = ArtifactIdentity {
        artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
        sha256: "a".repeat(64),
        byte_len: 10,
    };
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: artifact.clone(),
        runtime: artifact.clone(),
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
            scale: vec![1.0 / 255.0; 6],
            bias: vec![0.0; 6],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes: 134217728,
        minimum_output_bytes: 1048576,
        cpu_threads: 2,
        cuda_resources: None,
    };
    let manifest = LiveInferenceManifest {
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new("authored-analysis").unwrap(),
        identity: capture.identity.clone(),
        clock: capture.clock.clone(),
        model: ArtifactReference {
            path: "/authored/model".into(),
            identity: artifact.clone(),
        },
        runtime: ArtifactReference {
            path: "/authored/runtime".into(),
            identity: artifact,
        },
        worker: f.engine.binary_identity.clone(),
        profile,
        budget: ResourceBudget {
            memory_bytes: WORKER_MEMORY,
            output_bytes: 1048576,
            wall_time_ms: 30000,
            cpu_threads: 2,
        },
    };
    manifest.validate().unwrap();
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .arm_for_test(&capture.identity.epoch, &manifest);
    let handshake = live_handshake(f, &capture);
    for frame in 0..2 {
        f.engine
            .live_bulk_upload(
                &handshake,
                &live_header(frame, frame as u8),
                &vec![frame as u8; LIVE_FRAME_BYTES],
            )
            .unwrap();
    }
    let work = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .take_for_test(&capture.identity.epoch);
    let pixels = |bytes: &[u8]| {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    };
    let result = LiveInferenceReply::Estimated {
        attempt_id: manifest.attempt_id.clone(),
        identity: manifest.identity.clone(),
        profile_sha256: crate::inference_profiles::profile_identity(&manifest.profile)
            .unwrap()
            .sha256,
        input: LiveInferenceInput {
            sequence: 1,
            pair: work.declaration(),
            previous: pixels(work.previous()),
            current: pixels(work.current()),
        },
        position: NormalizedPosition::new(0.25).unwrap(),
        pair_analysis_elapsed_ns: 1,
    };
    (capture, manifest.attempt_id, work, result)
}
#[test]
fn prepared_ready_preserves_prior_output_then_switches_atomically() {
    let f = fixture();
    let (prior, attempt, work, result) = analysis_work(&f);
    let (next, next_attempt) = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .prepare_for_test(&f.engine, &prior.identity.epoch);
    assert!(f
        .engine
        .live_analysis_complete(&prior.identity.epoch, &attempt, work, result)
        .unwrap());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&prior.identity.epoch));
    assert!(!f
        .engine
        .live_analysis_ready_for_test(&next, &AttemptId::new("foreign-ready").unwrap())
        .unwrap());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&prior.identity.epoch));
    assert!(f
        .engine
        .live_analysis_ready_for_test(&next, &next_attempt)
        .unwrap());
    let old = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: prior.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert!(!old.active);
    assert_eq!(old.retirement, Some(LiveCaptureRetirement::Superseded));
    let new = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus { epoch: next },
            ))
            .result
            .unwrap(),
    );
    assert!(new.active);
    assert_eq!(
        new.inference.unwrap().state,
        LiveCaptureInferenceState::Running
    );
}
#[test]
fn prepared_admission_refuses_second_pending_successor_without_retiring_prior() {
    let f = fixture();
    let (prior, _, _, _) = analysis_work(&f);
    let (next, _) = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .prepare_for_test(&f.engine, &prior.identity.epoch);
    let status = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: next.clone(),
                },
            ))
            .result
            .unwrap(),
    );
    let start = request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::StartLiveCapture {
            engine_epoch: prior.engine_epoch.clone(),
            options: status.options.clone(),
        },
    );
    assert!(f
        .engine
        .handle(start)
        .result
        .unwrap_err()
        .message
        .contains("cannot start without a predecessor"));
    let intent = request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::ContinueLiveCapture {
            engine_epoch: prior.engine_epoch.clone(),
            predecessor: prior.identity.epoch.clone(),
            options: status.options,
        },
    );
    assert_eq!(
        f.engine.handle(intent).result.unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    let registry = f.engine.live_captures.lock().unwrap();
    assert!(registry.active_for_test(&prior.identity.epoch));
    assert!(registry.active_for_test(&next));
}
#[test]
fn prepared_control_remains_actor_owned_despite_full_project_grants() {
    let f = fixture();
    let (prior, _, _, _) = analysis_work(&f);
    let foreign = pair(&f.engine, "prepared foreign client");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: foreign.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
            },
        ))
        .result
        .unwrap();
    f.engine
        .handle(request(
            &foreign,
            Some(&f.project),
            Some(0),
            Command::LiveCaptureLimits,
        ))
        .result
        .unwrap();
    let current = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: prior.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap(),
    );
    let mut options = current
        .continuation_options(SourceTimestamp::new(1, 1).unwrap(), 30_000)
        .unwrap();
    options.continuation_mode = LiveContinuationMode::WhenReady;
    let ResponseBody::Project(other_project) = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::CreateProject {
                name: "prepared other project".into(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("project response")
    };
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&other_project.project_id),
                Some(0),
                Command::ContinueLiveCapture {
                    engine_epoch: prior.engine_epoch.clone(),
                    predecessor: prior.identity.epoch.clone(),
                    options: options.clone()
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.engine
            .handle(request(
                &foreign,
                Some(&f.project),
                Some(0),
                Command::ContinueLiveCapture {
                    engine_epoch: prior.engine_epoch.clone(),
                    predecessor: prior.identity.epoch.clone(),
                    options
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let (next, attempt) = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .prepare_for_test(&f.engine, &prior.identity.epoch);
    for epoch in [&prior.identity.epoch, &next] {
        for command in [
            Command::LiveCaptureStatus {
                epoch: epoch.clone(),
            },
            Command::StopLiveCapture {
                epoch: epoch.clone(),
            },
            Command::FinishLiveCapture {
                epoch: epoch.clone(),
            },
            Command::ContinueLiveCaptureAt {
                engine_epoch: prior.engine_epoch.clone(),
                predecessor: epoch.clone(),
                source_anchor: SourceTimestamp::new(2, 1).unwrap(),
                lease_ms: 30_000,
            },
        ] {
            assert_eq!(
                f.engine
                    .handle(request(&foreign, Some(&f.project), Some(0), command))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        let owned = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        let mut handshake = live_handshake(&f, &owned);
        handshake.session = foreign.clone();
        handshake.auth_token = TOKENS.with(|tokens| tokens.borrow()[foreign.as_str()].clone());
        assert_eq!(
            f.engine.live_bulk_ready(&handshake, None).unwrap_err().code,
            ErrorCode::Forbidden
        );
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(epoch));
    }
    assert!(f
        .engine
        .live_analysis_ready_for_test(&next, &attempt)
        .unwrap());
    let new = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus { epoch: next },
            ))
            .result
            .unwrap(),
    );
    assert!(new.active);
    assert_eq!(
        new.inference.unwrap().state,
        LiveCaptureInferenceState::Running
    );
    assert!(!f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&prior.identity.epoch));
}
#[test]
fn public_grant_revocation_fences_prepared_ready_and_restoring_grants_cannot_rearm() {
    let f = fixture();
    let (prior, _, _, _) = analysis_work(&f);
    let (next, attempt) = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .prepare_for_test(&f.engine, &prior.identity.epoch);
    let admin = pair(&f.engine, "prepared grant administrator");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: admin.clone(),
                project_id: f.project.clone(),
                scopes: vec![
                    Scope::Read,
                    Scope::ManageGrants,
                    Scope::Generate,
                    Scope::ImportSource,
                ],
            },
        ))
        .result
        .unwrap();
    f.engine
        .handle(request(
            &admin,
            None,
            None,
            Command::Revoke {
                session: f.session.clone(),
                project_id: f.project.clone(),
            },
        ))
        .result
        .unwrap();
    assert!(!f
        .engine
        .live_analysis_ready_for_test(&next, &attempt)
        .unwrap());
    f.engine
        .handle(request(
            &admin,
            None,
            None,
            Command::Grant {
                session: f.session.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
            },
        ))
        .result
        .unwrap();
    assert!(!f
        .engine
        .live_analysis_ready_for_test(&next, &attempt)
        .unwrap());
    for epoch in [&prior.identity.epoch, &next] {
        let report = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        assert!(!report.active);
        report.validate().unwrap();
    }
}
#[test]
fn prepared_failure_preserves_prior_and_prior_termination_fences_ready() {
    for case in ["failure", "stop", "expiry", "revision", "revoke"] {
        let f = fixture();
        let (prior, _, _, _) = analysis_work(&f);
        let (next, attempt) = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .prepare_for_test(&f.engine, &prior.identity.epoch);
        match case {
            "failure" => f
                .engine
                .live_analysis_failure_for_test(&next, &attempt)
                .unwrap(),
            "stop" => {
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::StopLiveCapture {
                            epoch: prior.identity.epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap();
            }
            "expiry" => f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .expire_for_test(&prior.identity.epoch),
            "revision" => {
                f.engine
                    .db
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE projects SET revision=1 WHERE id=?1",
                        [f.project.as_str()],
                    )
                    .unwrap();
            }
            "revoke" => {
                f.engine
                    .db
                    .lock()
                    .unwrap()
                    .execute(
                        "DELETE FROM grants WHERE session=?1 AND project=?2",
                        [f.session.as_str(), f.project.as_str()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            !f.engine
                .live_analysis_ready_for_test(&next, &attempt)
                .unwrap(),
            "{case}"
        );
        let registry = f.engine.live_captures.lock().unwrap();
        assert!(!registry.active_for_test(&next), "{case}");
        registry.validate_report_for_test(&next);
        registry.validate_report_for_test(&prior.identity.epoch);
        assert_eq!(
            registry.active_for_test(&prior.identity.epoch),
            case == "failure",
            "{case}"
        );
    }
}
#[test]
fn live_analysis_acceptance_and_status_age_are_bound_to_original_capture() {
    let f = fixture();
    let (capture, attempt, work, result) = analysis_work(&f);
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    let status = || {
        capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: capture.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    };
    let current = status();
    let info = current.inference.unwrap();
    assert_eq!(info.accepted, 1);
    assert!(info.latest.unwrap().fresh);
    assert_eq!(current.queue.unwrap().accepted_completions, 1);
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .age_for_test(&capture.identity.epoch);
    let aged = status();
    assert!(!aged.inference.unwrap().latest.unwrap().fresh);
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StopLiveCapture {
                epoch: capture.identity.epoch.clone(),
            },
        ))
        .result
        .unwrap();
    let stopped = status();
    assert!(stopped.inference.unwrap().latest.is_none());
    assert!(stopped.queue.is_none());
}
#[test]
fn live_analysis_late_completion_is_counted_stale_without_acceptance() {
    let f = fixture();
    let (capture, attempt, work, result) = analysis_work(&f);
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .age_for_test(&capture.identity.epoch);
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    let report = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: capture.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    let info = report.inference.unwrap();
    assert_eq!(info.completed, 1);
    assert_eq!(info.stale, 1);
    assert_eq!(info.accepted, 0);
    assert!(info.latest.is_none());
}
#[test]
fn live_analysis_wrong_attempt_retires_without_publishing_or_rearming() {
    let f = fixture();
    let (capture, attempt, work, mut result) = analysis_work(&f);
    if let LiveInferenceReply::Estimated { attempt_id, .. } = &mut result {
        *attempt_id = AttemptId::new("foreign-attempt").unwrap();
    }
    assert_eq!(
        f.engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    let report = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: capture.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert!(!report.active);
    assert_eq!(report.retirement, Some(LiveCaptureRetirement::WorkerFailed));
    let info = report.inference.unwrap();
    assert_eq!(info.state, LiveCaptureInferenceState::Failed);
    assert_eq!(info.accepted, 0);
    assert!(info.latest.is_none());
    assert!(info.error.is_some());
    assert_eq!(report.admitted_pixel_bytes, 0);
}
#[test]
fn live_analysis_revision_and_revocation_discard_issued_results() {
    for revoke in [false, true] {
        let f = fixture();
        let (capture, attempt, work, result) = analysis_work(&f);
        if revoke {
            let admin = pair(&f.engine, "analysis administrator");
            f.engine
                .handle(request(
                    &f.session,
                    None,
                    None,
                    Command::Grant {
                        session: admin.clone(),
                        project_id: f.project.clone(),
                        scopes: vec![Scope::Read, Scope::ManageGrants],
                    },
                ))
                .result
                .unwrap();
            f.engine
                .handle(request(
                    &admin,
                    None,
                    None,
                    Command::Revoke {
                        session: f.session.clone(),
                        project_id: f.project.clone(),
                    },
                ))
                .result
                .unwrap();
        } else {
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::SetProtectedRegions {
                        regions: vec![ProtectedRegion {
                            axis: None,
                            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1))
                                .unwrap(),
                        }],
                    },
                ))
                .result
                .unwrap();
        }
        assert!(!f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        assert!(!f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&capture.identity.epoch));
    }
}
fn cuda_analysis_work(
    f: &Fixture,
) -> (
    LiveCaptureReport,
    AttemptId,
    crate::live_frames::LiveRgbWork,
    LiveInferenceReply,
) {
    let (capture, attempt, work, result) = analysis_work(f);
    let profile_sha256 = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .cuda_for_test(&capture.identity.epoch);
    let LiveInferenceReply::Estimated {
        attempt_id,
        identity,
        input,
        position,
        pair_analysis_elapsed_ns,
        ..
    } = result
    else {
        unreachable!()
    };
    (
        capture,
        attempt,
        work,
        LiveInferenceReply::CudaProvisional {
            attempt_id,
            identity,
            profile_sha256,
            manifest_sha256: "b".repeat(64),
            input,
            position,
            pair_analysis_elapsed_ns,
        },
    )
}
#[test]
fn live_analysis_cuda_provisional_completion_uses_capture_clock_and_stop_clears_latest() {
    for aged in [false, true] {
        let f = fixture();
        let (capture, attempt, work, result) = cuda_analysis_work(&f);
        if aged {
            f.engine
                .live_captures
                .lock()
                .unwrap()
                .age_for_test(&capture.identity.epoch);
        }
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        let status = || {
            capture_report(
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::LiveCaptureStatus {
                            epoch: capture.identity.epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            )
        };
        let report = status();
        report.validate().unwrap();
        let info = report.inference.unwrap();
        assert_eq!(info.completed, 1);
        assert_eq!(info.stale, u64::from(aged));
        assert_eq!(info.accepted, u64::from(!aged));
        assert_eq!(info.cuda_manifest_sha256, Some("b".repeat(64)));
        if let Some(latest) = info.latest {
            assert!(!latest.qualified);
            assert!(matches!(
                latest.result,
                LiveInferenceReply::CudaProvisional { .. }
            ));
        }
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        let stopped = status();
        assert!(!stopped.active);
        assert!(stopped.inference.unwrap().latest.is_none());
    }
}
#[test]
fn live_analysis_cuda_rejects_foreign_manifest_and_cpu_disguise() {
    for refusal in ["manifest", "cpu-disguise", "sequence"] {
        let f = fixture();
        let (capture, attempt, work, result) = cuda_analysis_work(&f);
        let LiveInferenceReply::CudaProvisional {
            attempt_id,
            identity,
            profile_sha256,
            mut input,
            position,
            pair_analysis_elapsed_ns,
            ..
        } = result
        else {
            unreachable!()
        };
        if refusal == "sequence" {
            input.sequence = 2;
        }
        let result = if refusal == "cpu-disguise" {
            LiveInferenceReply::Estimated {
                attempt_id,
                identity,
                profile_sha256,
                input,
                position,
                pair_analysis_elapsed_ns,
            }
        } else {
            LiveInferenceReply::CudaProvisional {
                attempt_id,
                identity,
                profile_sha256,
                manifest_sha256: if refusal == "manifest" { "c" } else { "b" }.repeat(64),
                input,
                position,
                pair_analysis_elapsed_ns,
            }
        };
        assert_eq!(
            f.engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        let report = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: capture.identity.epoch,
                    },
                ))
                .result
                .unwrap(),
        );
        assert!(!report.active);
        assert_eq!(report.retirement, Some(LiveCaptureRetirement::WorkerFailed));
        assert!(report.inference.unwrap().latest.is_none());
    }
}
#[test]
fn live_analysis_cuda_revision_revocation_and_expiry_discard_issued_results() {
    for reason in ["revision", "revoke", "expiry"] {
        let f = fixture();
        let (capture, attempt, work, result) = cuda_analysis_work(&f);
        match reason {
            "revision" => {
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::SetProtectedRegions {
                            regions: vec![ProtectedRegion {
                                axis: None,
                                range: TimeRange::new(
                                    ProjectTime::ZERO,
                                    ProjectTime::from_nanos(1),
                                )
                                .unwrap(),
                            }],
                        },
                    ))
                    .result
                    .unwrap();
            }
            "revoke" => {
                let admin = pair(&f.engine, "CUDA reporting administrator");
                f.engine
                    .handle(request(
                        &f.session,
                        None,
                        None,
                        Command::Grant {
                            session: admin.clone(),
                            project_id: f.project.clone(),
                            scopes: vec![Scope::Read, Scope::ManageGrants],
                        },
                    ))
                    .result
                    .unwrap();
                f.engine
                    .handle(request(
                        &admin,
                        None,
                        None,
                        Command::Revoke {
                            session: f.session.clone(),
                            project_id: f.project.clone(),
                        },
                    ))
                    .result
                    .unwrap();
            }
            "expiry" => f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .expire_for_test(&capture.identity.epoch),
            _ => unreachable!(),
        }
        assert!(!f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        assert!(!f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .active_for_test(&capture.identity.epoch));
    }
}
#[test]
fn live_cuda_finish_request_drains_issued_work_and_replays_current_terminal_state() {
    let f = fixture();
    let (capture, attempt, work, result) = cuda_analysis_work(&f);
    let finish = request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::FinishLiveCapture {
            epoch: capture.identity.epoch.clone(),
        },
    );
    let requested = capture_report(f.engine.handle(finish.clone()).result.unwrap());
    requested.validate().unwrap();
    assert_eq!(
        requested.inference.as_ref().unwrap().state,
        LiveCaptureInferenceState::FinishRequested
    );
    assert!(requested.inference.as_ref().unwrap().latest.is_none());
    let handshake = live_handshake(&f, &capture);
    assert!(f.engine.live_bulk_ready(&handshake, None).is_err());
    assert!(f
        .engine
        .live_analysis_take(&capture.identity.epoch, &attempt)
        .unwrap()
        .is_none());
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    let replayed = capture_report(f.engine.handle(finish.clone()).result.unwrap());
    replayed.validate().unwrap();
    assert_eq!(replayed.inference.as_ref().unwrap().completed, 1);
    assert!(replayed.inference.as_ref().unwrap().latest.is_none());
    let mut conflict = finish.clone();
    conflict.command = Command::StopLiveCapture {
        epoch: capture.identity.epoch.clone(),
    };
    assert_eq!(
        f.engine.handle(conflict).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StopLiveCapture {
                epoch: capture.identity.epoch.clone(),
            },
        ))
        .result
        .unwrap();
    let replayed = capture_report(f.engine.handle(finish).result.unwrap());
    assert_eq!(replayed.retirement, Some(LiveCaptureRetirement::Stopped));
    assert!(replayed.inference.unwrap().cuda_evidence.is_none());
}
#[test]
fn live_cuda_finish_requires_cuda_owner_revision_and_generation_authority() {
    let f = fixture();
    let (capture, _, _, _) = analysis_work(&f);
    let finish = || {
        request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::FinishLiveCapture {
                epoch: capture.identity.epoch.clone(),
            },
        )
    };
    assert_eq!(
        f.engine.handle(finish()).result.unwrap_err().code,
        ErrorCode::Unsupported
    );
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .cuda_for_test(&capture.identity.epoch);
    let reader = pair(&f.engine, "Finish reader");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: reader.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    let foreign = || {
        request(
            &reader,
            Some(&f.project),
            Some(0),
            Command::FinishLiveCapture {
                epoch: capture.identity.epoch.clone(),
            },
        )
    };
    assert_eq!(
        f.engine.handle(foreign()).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: reader.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::Generate, Scope::ImportSource],
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine.handle(foreign()).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let mut wrong = finish();
    wrong.expected_revision = Some(RevisionId::new(1));
    assert_eq!(
        f.engine.handle(wrong).result.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let mut unauth = finish();
    unauth.auth_token = None;
    assert_eq!(
        f.engine.handle(unauth).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .active_for_test(&capture.identity.epoch));
}
#[test]
fn live_analysis_cuda_finishing_freezes_intake_take_and_continuation() {
    let f = fixture();
    let (capture, attempt, work, result) = cuda_analysis_work(&f);
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    let cancel = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .finishing_for_test(&capture.identity.epoch);
    assert!(!cancel.load(Ordering::Acquire));
    let handshake = live_handshake(&f, &capture);
    assert!(f.engine.live_bulk_ready(&handshake, None).is_err());
    assert!(f
        .engine
        .live_bulk_upload(&handshake, &live_header(2, 2), &vec![2; LIVE_FRAME_BYTES])
        .is_err());
    assert!(f
        .engine
        .live_analysis_take(&capture.identity.epoch, &attempt)
        .unwrap()
        .is_none());
    assert!(f
        .engine
        .handle(capture_continue(&f, &capture))
        .result
        .is_err());
    let status = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap(),
    );
    status.validate().unwrap();
    assert!(status.active);
    assert_eq!(
        status.inference.as_ref().unwrap().state,
        LiveCaptureInferenceState::Finishing
    );
    assert!(status.inference.as_ref().unwrap().latest.is_none());
    assert_eq!(status.received_frames, 2);
    assert!(!cancel.load(Ordering::Acquire));
    let stopped = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: capture.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert_eq!(stopped.retirement, Some(LiveCaptureRetirement::Stopped));
    assert!(cancel.load(Ordering::Acquire));
    assert!(stopped.inference.unwrap().cuda_evidence.is_none());
}
#[test]
fn live_analysis_cuda_finishing_failure_retires_without_database_lock() {
    let f = fixture();
    let (capture, attempt, work, result) = cuda_analysis_work(&f);
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    let cancel = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .finishing_for_test(&capture.identity.epoch);
    let db = f.engine.db.lock().unwrap();
    f.engine
        .live_analysis_failure_for_test(&capture.identity.epoch, &attempt)
        .unwrap();
    drop(db);
    assert!(cancel.load(Ordering::Acquire));
    let report = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: capture.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert_eq!(report.retirement, Some(LiveCaptureRetirement::WorkerFailed));
    let info = report.inference.unwrap();
    assert_eq!(info.state, LiveCaptureInferenceState::Failed);
    assert!(info.cuda_evidence.is_none());
    assert!(info.latest.is_none());
    assert!(info.error.is_some());
}
#[test]
fn live_analysis_cuda_finishing_remains_under_revision_revocation_and_expiry() {
    for cause in ["revision", "revoke", "expiry"] {
        let f = fixture();
        let (capture, attempt, work, result) = cuda_analysis_work(&f);
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap());
        let cancel = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .finishing_for_test(&capture.identity.epoch);
        match cause {
            "revision" => {
                f.engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        Some(0),
                        Command::SetProtectedRegions { regions: vec![] },
                    ))
                    .result
                    .unwrap();
            }
            "revoke" => {
                let admin = pair(&f.engine, "finishing administrator");
                f.engine
                    .handle(request(
                        &f.session,
                        None,
                        None,
                        Command::Grant {
                            session: admin.clone(),
                            project_id: f.project.clone(),
                            scopes: vec![Scope::Read, Scope::ManageGrants],
                        },
                    ))
                    .result
                    .unwrap();
                f.engine
                    .handle(request(
                        &admin,
                        None,
                        None,
                        Command::Revoke {
                            session: f.session.clone(),
                            project_id: f.project.clone(),
                        },
                    ))
                    .result
                    .unwrap();
            }
            "expiry" => f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .expire_for_test(&capture.identity.epoch),
            _ => unreachable!(),
        }
        let _ = f
            .engine
            .live_analysis_take(&capture.identity.epoch, &attempt);
        assert!(
            !f.engine
                .live_captures
                .lock()
                .unwrap()
                .active_for_test(&capture.identity.epoch),
            "{cause}"
        );
        assert!(cancel.load(Ordering::Acquire), "{cause}");
    }
}
#[test]
fn annotated_preview_requires_import_authority_before_any_source_or_model_access() {
    let f = fixture();
    let reader = pair(&f.engine, "annotated-preview reader");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: reader.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    let view:AnnotatedGenerationView=serde_json::from_value(serde_json::json!({"source_sha256":"a".repeat(64),"declaration":{"schema_version":1,"source_dimensions":[64,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"authored","video_view":{"layout":"mono","eye":"left","projection":{"kind":"flat"}}}})).unwrap();
    let context = FrameContext {
        source_version: SourceVersionId::new("missing").unwrap(),
        source_placement: SourcePlacementId::new("missing").unwrap(),
        frame: FrameId::new(0),
        transform: annotated_preview_transform(&view).unwrap(),
        seek_generation: 0,
        request_generation: 0,
    };
    let result = f
        .engine
        .handle(request(
            &reader,
            Some(&f.project),
            None,
            Command::PreviewAnnotatedFrame {
                context,
                time: ModelPackPreviewTime::Source {
                    source_time: SourceTimestamp::new(0, 1).unwrap(),
                    time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                },
                spec: AnnotatedPreviewSpec {
                    view,
                    source_sample_stride: 1,
                },
                model: AnnotatedPreviewModel::Pack {
                    choice: ModelPackChoice {
                        manifest_sha256: "a".repeat(64),
                        selection_revision: 1,
                    },
                },
            },
        ))
        .result;
    assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
    assert!(!f.engine.config.state_dir.join("model-packs").exists());
    assert!(f.engine.workers.lock().unwrap().is_empty());
}
#[test]
fn pack_preview_requires_import_authority_before_any_source_or_pack_access() {
    let f = fixture();
    let reader = pair(&f.engine, "pack-preview reader");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: reader.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    let context = FrameContext {
        source_version: SourceVersionId::new("missing-source").unwrap(),
        source_placement: SourcePlacementId::new("missing-placement").unwrap(),
        frame: FrameId::new(0),
        transform: video_view_transform(None).unwrap(),
        seek_generation: 0,
        request_generation: 0,
    };
    let result = f
        .engine
        .handle(request(
            &reader,
            Some(&f.project),
            None,
            Command::PreviewModelPackFrame {
                include_source_end: false,
                context,
                time: ModelPackPreviewTime::Source {
                    source_time: SourceTimestamp::new(0, 1).unwrap(),
                    time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                },
                video_view: None,
                choice: ModelPackChoice {
                    manifest_sha256: "a".repeat(64),
                    selection_revision: 1,
                },
            },
        ))
        .result;
    assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
    assert!(!f.engine.config.state_dir.join("model-packs").exists());
    assert!(f.engine.workers.lock().unwrap().is_empty());
}
#[test]
fn pack_preview_completion_fence_refuses_deactivation_even_for_retained_inputs() {
    let f = fixture();
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
        sha256: "a".repeat(64),
        byte_len: 100,
    };
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: identity.clone(),
        runtime: identity,
        preprocessing: InferencePreprocessing {
            input: ModelInputContract {
                width: 64,
                height: 64,
                channels: 3,
                class_count: 1,
                layout: TensorLayout::Nchw,
                decoder: DetectorDecoder::YoloChannelMajor,
            },
            resize: InferenceResize::Stretch,
            color: InferenceColor::Rgb,
            scale: vec![1.0 / 255.0; 3],
            bias: vec![0.0; 3],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes: 134217728,
        minimum_output_bytes: 1048576,
        cpu_threads: 2,
        cuda_resources: None,
    };
    let mut db = f.engine.db.lock().unwrap();
    let tx = db.transaction().unwrap();
    let selected = crate::model_pack_selection::select(
        &tx,
        0,
        Some(("c".repeat(64), profile)),
        &f.session,
        &RequestId::new("preview-selection").unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    let choice = selected.choice.unwrap();
    f.engine.require_current_pack_choice(&db, &choice).unwrap();
    let tx = db.transaction().unwrap();
    crate::model_pack_selection::select(
        &tx,
        1,
        None,
        &f.session,
        &RequestId::new("preview-deactivation").unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(
        f.engine
            .require_current_pack_choice(&db, &choice)
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
}
#[test]
fn catalog_commands_require_fresh_session_and_separate_owner_before_storage() {
    let f = fixture();
    for owner_proof in [
        "wrong-catalog-owner".into(),
        f.engine.stash_owner_token.clone(),
        f.engine.handy_owner_token.clone(),
    ] {
        let command = Command::RefreshModelCatalog {
            source: "/nonexistent-catalog-source".into(),
            bootstrap: "/nonexistent-catalog-root".into(),
            bootstrap_sha256: "a".repeat(64),
            expected_catalog_revision: 0,
            approve_bootstrap: true,
            owner_proof,
        };
        assert_eq!(
            f.engine
                .handle(request(&f.session, None, None, command))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    let mut status = request(
        &f.session,
        None,
        None,
        Command::ModelCatalogStatus {
            bootstrap_sha256: "a".repeat(64),
            owner_proof: f.engine.model_pack_owner_token.clone(),
        },
    );
    status.auth_token = None;
    assert_eq!(
        f.engine.handle(status).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
    assert!(!f.engine.config.state_dir.join("model-catalogs").exists());
}
#[test]
fn model_pack_commands_require_separate_owner_capability_and_session() {
    let f = fixture();
    for owner_proof in [
        "incorrect-owner-capability".into(),
        f.engine.handy_owner_token.clone(),
        f.engine.stash_owner_token.clone(),
        f.engine.package_owner_token.clone(),
    ] {
        let result = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::ImportModelPack {
                    source: "/nonexistent-pack-fixture".into(),
                    owner_proof,
                },
            ))
            .result;
        assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
    }
    assert!(!f.engine.config.state_dir.join("model-packs").exists());
    let mut command = request(
        &f.session,
        None,
        None,
        Command::ModelPackStatus {
            manifest_sha256: "a".repeat(64),
            owner_proof: f.engine.model_pack_owner_token.clone(),
        },
    );
    command.auth_token = None;
    assert_eq!(
        f.engine.handle(command).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
    let wrong = request(
        &f.session,
        None,
        None,
        Command::ModelPackStagingStatus {
            owner_proof: f.engine.stash_owner_token.clone(),
        },
    );
    assert_eq!(
        f.engine.handle(wrong).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let forbidden = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::DiscardModelPackStage {
                basename: ".pack-stage-authored".into(),
                manifest_sha256: "a".repeat(64),
                owner_proof: f.engine.stash_owner_token.clone(),
            },
        ))
        .result;
    assert_eq!(forbidden.unwrap_err().code, ErrorCode::Forbidden);
    assert!(!f
        .engine
        .config
        .state_dir
        .join("model-pack-discards")
        .exists());
    let body = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::ModelPackStagingStatus {
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        ))
        .result
        .unwrap();
    let ResponseBody::ModelPackStaging(report) = body else {
        panic!("missing staging report")
    };
    assert!(report.stages.is_empty());
    assert!(!f.engine.config.state_dir.join("model-packs").exists());
}
#[test]
fn model_pack_selection_authority_replay_and_conflict_are_independent_of_project_grants() {
    let f = fixture();
    let wrong = request(
        &f.session,
        None,
        None,
        Command::SelectModelPack {
            manifest_sha256: None,
            expected_selection_revision: 0,
            approve_unqualified_native_execution: false,
            owner_proof: f.engine.stash_owner_token.clone(),
        },
    );
    assert_eq!(
        f.engine.handle(wrong).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let mut command = request(
        &f.session,
        None,
        None,
        Command::SelectModelPack {
            manifest_sha256: None,
            expected_selection_revision: 0,
            approve_unqualified_native_execution: false,
            owner_proof: f.engine.model_pack_owner_token.clone(),
        },
    );
    let first = f.engine.handle(command.clone()).result.unwrap();
    assert_eq!(
        encode(&first).unwrap(),
        encode(&f.engine.handle(command.clone()).result.unwrap()).unwrap()
    );
    if let Command::SelectModelPack {
        expected_selection_revision,
        ..
    } = &mut command.command
    {
        *expected_selection_revision = 1;
    }
    assert_eq!(
        f.engine.handle(command).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    let report = crate::model_pack_selection::status(&f.engine.db.lock().unwrap()).unwrap();
    assert_eq!(report.selection_revision, 1);
    let mut read = request(
        &f.session,
        None,
        None,
        Command::ModelPackSelectionStatus {
            owner_proof: f.engine.model_pack_owner_token.clone(),
        },
    );
    read.auth_token = None;
    assert_eq!(
        f.engine.handle(read).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
}
fn program(position: f64) -> MotionProgram {
    MotionProgram::new(vec![MotionTrack::new(
        Axis::Stroke,
        vec![
            MotionAction::new(
                ProjectTime::ZERO,
                NormalizedPosition::new(position).unwrap(),
                EvidenceKind::Observed,
            )
            .unwrap(),
            MotionAction::new(
                ProjectTime::from_nanos(1_000_000_000),
                NormalizedPosition::new(position).unwrap(),
                EvidenceKind::Observed,
            )
            .unwrap(),
        ],
    )
    .unwrap()])
    .unwrap()
}
#[test]
fn handy_inspection_requires_its_own_local_capability_before_credentials() {
    let f = fixture();
    let inspection = HandyInspectionRequest {
        api_key_env: "HANDY_ABSENT_TEST_API".into(),
        connection_key_env: "HANDY_ABSENT_TEST_DEVICE".into(),
        allow_online_inspection: true,
    };
    for proof in [
        String::new(),
        "incorrect-owner-capability".into(),
        f.engine.stash_owner_token.clone(),
        f.engine.package_owner_token.clone(),
    ] {
        let result = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::HandyInspect {
                    inspection: inspection.clone(),
                    owner_proof: proof,
                },
            ))
            .result;
        assert!(
            matches!(result,Err(error) if matches!(error.code,ErrorCode::Forbidden|ErrorCode::InvalidRequest))
        );
    }
    let result = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::HandyInspect {
                inspection,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        ))
        .result;
    assert!(
        matches!(result,Err(error) if error.code==ErrorCode::Unavailable && error.message.contains("credential is absent"))
    );
    assert!(f
        .dir
        .path()
        .join("private-state/handy-owner.token")
        .is_file());
}
#[test]
fn handy_inspection_requires_session_authentication_and_consent() {
    let f = fixture();
    let command = Command::HandyInspect {
        inspection: HandyInspectionRequest {
            api_key_env: "HANDY_ABSENT_TEST_API".into(),
            connection_key_env: "HANDY_ABSENT_TEST_DEVICE".into(),
            allow_online_inspection: true,
        },
        owner_proof: f.engine.handy_owner_token.clone(),
    };
    let mut r = request(&f.session, None, None, command.clone());
    r.auth_token = None;
    assert_eq!(
        f.engine.handle(r).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
    let Command::HandyInspect {
        mut inspection,
        owner_proof,
    } = command
    else {
        unreachable!()
    };
    inspection.allow_online_inspection = false;
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::HandyInspect {
                    inspection,
                    owner_proof
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}
// Stage only a proposal; the public command still checks all authority and revision rules.
fn commit_program(f: &Fixture, revision: u64, program: MotionProgram) -> Command {
    let candidate_id = id!(CandidateId);
    let db = f.engine.db.lock().unwrap();
    let reference = crate::authority::motion_state::save(&db, &program).unwrap();
    db.execute(
        "INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,NULL,'[]','[]')",
        params![candidate_id.as_str(), f.project.as_str(), revision as i64, reference],
    ).unwrap();
    Command::CommitCandidate { candidate_id }
}
#[test]
fn duplicate_edit_is_durable_replay_not_second_revision() {
    let f = fixture();
    let r = request(
        &f.session,
        Some(&f.project),
        Some(0),
        commit_program(&f, 0, program(0.3)),
    );
    let first = f.engine.handle(r.clone()).result.unwrap();
    let second = f.engine.handle(r.clone()).result.unwrap();
    assert_eq!(encode(&first).unwrap(), encode(&second).unwrap());
    let config = f.engine.config.clone();
    drop(f.engine);
    let reopened = Engine::open(config).unwrap();
    assert_eq!(
        encode(&first).unwrap(),
        encode(&reopened.handle(r).result.unwrap()).unwrap()
    );
    let db = reopened.db.lock().unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM revisions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}
#[test]
fn stale_revision_and_reused_request_identity_reject() {
    let f = fixture();
    let r = request(
        &f.session,
        Some(&f.project),
        Some(0),
        commit_program(&f, 0, program(0.3)),
    );
    assert!(f.engine.handle(r.clone()).result.is_ok());
    let mut changed = r.clone();
    changed.command = commit_program(&f, 0, program(0.9));
    assert_eq!(
        f.engine.handle(changed).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    let mut stale = r;
    stale.request_id = id!(RequestId);
    assert_eq!(
        f.engine.handle(stale).result.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
}
#[test]
fn independently_paired_client_has_no_existing_project_authority() {
    let f = fixture();
    let other = pair(&f.engine, "PulsarDesktop");
    let r = request(
        &other,
        None,
        None,
        Command::OpenProject {
            project_id: f.project.clone(),
        },
    );
    assert_eq!(
        f.engine.handle(r).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let grant = request(
        &f.session,
        None,
        None,
        Command::Grant {
            session: other.clone(),
            project_id: f.project.clone(),
            scopes: vec![Scope::Read],
        },
    );
    assert!(f.engine.handle(grant).result.is_ok());
    assert!(f
        .engine
        .handle(request(
            &other,
            Some(&f.project),
            None,
            Command::GetSnapshot
        ))
        .result
        .is_ok());
    assert_eq!(
        f.engine
            .handle(request(
                &other,
                Some(&f.project),
                Some(0),
                commit_program(&f, 0, program(0.2))
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}
#[test]
fn revocation_denies_cached_queries_and_undo_preserves_fresh_revisions() {
    let f = fixture();
    let other = pair(&f.engine, "reviewer");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: other.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    let query = request(&other, Some(&f.project), None, Command::GetSnapshot);
    f.engine.handle(query.clone()).result.unwrap();
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Revoke {
                session: other,
                project_id: f.project.clone(),
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine.handle(query).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.2)),
        ))
        .result
        .unwrap();
    let undo = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::Undo,
        ))
        .result
        .unwrap();
    match undo {
        ResponseBody::Project(p) => {
            assert_eq!(p.revision.value(), 2);
            assert_eq!(
                f.engine.motion_store.read(&p.motion.program).unwrap(),
                MotionProgram::default()
            );
        }
        _ => panic!(),
    }
    let redo = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::Redo,
        ))
        .result
        .unwrap();
    match redo {
        ResponseBody::Project(p) => {
            assert_eq!(p.revision.value(), 3);
            assert_eq!(
                f.engine.motion_store.read(&p.motion.program).unwrap(),
                program(0.2)
            );
        }
        _ => panic!(),
    }
}
#[test]
fn protected_regions_cover_replacement_and_undo() {
    let f = fixture();
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.2)),
        ))
        .result
        .unwrap();
    let regions = vec![ProtectedRegion {
        axis: Some(Axis::Stroke),
        range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000_001)).unwrap(),
    }];
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::SetProtectedRegions { regions },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(2),
                commit_program(&f, 2, program(0.8))
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let editor = pair(&f.engine, "ordinary editor");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: editor.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::Edit],
            },
        ))
        .result
        .unwrap();
    for command in [
        Command::Undo,
        Command::SetProtectedRegions { regions: vec![] },
    ] {
        assert_eq!(
            f.engine
                .handle(request(&editor, Some(&f.project), Some(2), command))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::Undo,
        ))
        .result
        .unwrap();
    {
        let db = f.engine.db.lock().unwrap();
        let protected: String = db
            .query_row("SELECT protected FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(protected, "[]");
    }
    let before = f
        .engine
        .handle(request(
            &editor,
            Some(&f.project),
            None,
            Command::GetSnapshot,
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(&editor, Some(&f.project), Some(3), Command::Redo))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let after = f
        .engine
        .handle(request(
            &editor,
            Some(&f.project),
            None,
            Command::GetSnapshot,
        ))
        .result
        .unwrap();
    assert_eq!(encode(&before).unwrap(), encode(&after).unwrap());
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(3),
            Command::Redo,
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(4),
                commit_program(&f, 4, program(0.8))
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}
#[test]
fn physical_effects_fail_closed() {
    let f = fixture();
    let result = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::Arm {
                device_id: id!(DeviceId),
                revision: RevisionId::new(0),
            },
        ))
        .result;
    assert_eq!(result.unwrap_err().code, ErrorCode::Unsupported);
    assert!(serde_json::from_str::<Request>(r#"{"version":1,"request_id":"unknown","session":null,"project":null,"expected_revision":null,"command":{"operation":"shell","arguments":{"command":"rm"}}}"#).is_err());
}

#[test]
fn inference_catalog_reports_implemented_coverage_without_qualification() {
    let f = fixture();
    let query = request(&f.session, None, None, Command::InferenceCatalog);
    assert!(!query.command.is_mutating());
    let ResponseBody::InferenceCatalog(catalog) = f.engine.handle(query).result.unwrap() else {
        panic!("catalog response required");
    };
    assert_eq!(catalog.providers, [InferenceProvider::Cpu]);
    assert!(catalog.qualified_profiles.is_empty());
    assert!(!catalog.qualified_fallback_available);
    let capabilities = f.engine.capabilities();
    assert_eq!(
        capabilities
            .operations
            .iter()
            .any(|operation| operation == "generate_with_inference_fallback"),
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
    );
    assert!(!capabilities.physical_playback);
    assert_eq!(catalog.memory_bytes, 4 * 1024 * 1024 * 1024);
    assert_eq!(catalog.cpu_threads, 2);
    assert_eq!(catalog.output_bytes, OUTPUT_LIMIT);
    assert!(f
        .engine
        .handle(Request::new(id!(RequestId), Command::InferenceCatalog))
        .result
        .is_err());
}
#[test]
fn export_has_no_provenance_and_replays_without_overwrite() {
    let f = fixture();
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.2)),
        ))
        .result
        .unwrap();
    let path = f.dir.path().join("neutral.funscript");
    let r = request(
        &f.session,
        Some(&f.project),
        None,
        Command::Export { path: path.clone() },
    );
    f.engine.handle(r.clone()).result.unwrap();
    f.engine.handle(r).result.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(value["actions"][0]["pos"], 20);
    assert!(value.get("source").is_none());
    assert!(value.get("lineage").is_none());
    let db = f.engine.db.lock().unwrap();
    let state: String = db
        .query_row("SELECT state FROM exports", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "complete");
}
#[cfg(unix)]
#[test]
fn existing_export_destination_is_a_stable_refusal() {
    let f = fixture();
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.2)),
        ))
        .result
        .unwrap();
    let path = f.dir.path().join("existing.funscript");
    fs::write(&path, b"original").unwrap();
    let export = request(
        &f.session,
        Some(&f.project),
        None,
        Command::Export { path: path.clone() },
    );
    let refused = f.engine.handle(export.clone()).result.unwrap_err();
    assert_eq!(refused.code, ErrorCode::RequestConflict);
    assert_eq!(fs::read(&path).unwrap(), b"original");
    fs::remove_file(&path).unwrap();
    assert_eq!(f.engine.handle(export).result.unwrap_err(), refused);
    assert!(!path.exists());
}

#[test]
fn explicit_gaps_block_standard_export() {
    let gap = TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1000)).unwrap();
    let p = MotionProgram::new(vec![MotionTrack::with_gaps(
        Axis::Stroke,
        vec![],
        vec![gap],
    )
    .unwrap()])
    .unwrap();
    assert_eq!(neutral_export(&p).unwrap_err().code, ErrorCode::Unsupported);
}
#[test]
fn imported_snapshot_pin_and_evict_are_scoped() {
    let f = fixture();
    let original = f.dir.path().join("original.mp4");
    fs::write(&original, b"immutable test").unwrap();
    let source = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::ImportSource {
                path: original.clone(),
            },
        ))
        .result
        .unwrap()
    {
        ResponseBody::Source { source_version } => source_version,
        _ => panic!(),
    };
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::PinSource {
                source_version: source.clone(),
                pinned: true,
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::EvictSource {
                    source_version: source.clone()
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::PinSource {
                source_version: source.clone(),
                pinned: false,
            },
        ))
        .result
        .unwrap();
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::EvictSource {
                source_version: source,
            },
        ))
        .result
        .unwrap();
    assert_eq!(fs::read(original).unwrap(), b"immutable test");
}
#[test]
fn invalid_credentials_never_create_session() {
    let f = fixture();
    let r = Request::new(
        id!(RequestId),
        Command::Pair {
            client_name: "test".into(),
            pairing_token: "incorrect".into(),
        },
    );
    assert_eq!(
        f.engine.handle(r).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
    let db = f.engine.db.lock().unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
#[test]
fn full_durability_is_selected() {
    let f = fixture();
    let db = f.engine.db.lock().unwrap();
    let synchronous: i64 = db
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .unwrap();
    assert_eq!(synchronous, 2);
}
#[test]
fn public_session_identity_is_not_a_bearer_credential() {
    let f = fixture();
    let other = pair(&f.engine, "other client");
    let mut stolen = request(&other, Some(&f.project), None, Command::GetSnapshot);
    stolen.session = Some(f.session.clone());
    assert_eq!(
        f.engine.handle(stolen).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
    let mut absent = request(&f.session, Some(&f.project), None, Command::GetSnapshot);
    absent.auth_token = None;
    assert_eq!(
        f.engine.handle(absent).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
}
#[test]
fn queries_do_not_accumulate_durable_request_rows() {
    let f = fixture();
    let count = || {
        let db = f.engine.db.lock().unwrap();
        db.query_row("SELECT COUNT(*) FROM requests", [], |r| r.get::<_, i64>(0))
            .unwrap()
    };
    let before = count();
    let query = request(&f.session, Some(&f.project), None, Command::GetSnapshot);
    for _ in 0..20 {
        f.engine.handle(query.clone()).result.unwrap();
    }
    assert_eq!(count(), before);
}
#[test]
fn ordinary_edit_grant_does_not_authorize_filesystem_import() {
    let f = fixture();
    let editor = pair(&f.engine, "editor");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: editor.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Edit],
            },
        ))
        .result
        .unwrap();
    let result = f
        .engine
        .handle(request(
            &editor,
            Some(&f.project),
            None,
            Command::ImportSource {
                path: f.dir.path().join("not-read"),
            },
        ))
        .result;
    assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
}
fn staged_manifest(f: &Fixture) -> (WorkerRequest, SourceVersionId) {
    let import = |name: &str, bytes: &[u8]| {
        let path = f.dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        match f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Source { source_version } => source_version,
            _ => panic!(),
        }
    };
    let source_version = import("media.fixture", b"media");
    let model_version = import("model.fixture", b"model");
    let db = f.engine.db.lock().unwrap();
    let source = f.engine.source(&db, &f.project, &source_version).unwrap();
    let model = f.engine.source(&db, &f.project, &model_version).unwrap();
    let identity = source.identity.clone();
    let reference = ArtifactReference {
        identity: identity.clone(),
        path: source.path.clone(),
    };
    let tools = WorkerTools {
        ffmpeg: reference.clone(),
        ffprobe: reference,
        onnx_runtime: None,
    };
    let job = id!(JobId);
    let attempt = id!(AttemptId);
    let output_dir = f
        .engine
        .config
        .state_dir
        .join("attempts")
        .join(attempt.as_str());
    artifacts::private_directory(&output_dir).unwrap();
    let manifest = WorkerRequest {
        cuda_execution: None,
        independent_audio: None,
        version: PROTOCOL_VERSION,
        attempt_id: attempt.clone(),
        job_id: job.clone(),
        project_id: f.project.clone(),
        base_revision: RevisionId::new(0),
        source,
        output_dir,
        operation: WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                inference_profile: None,
                model_input: Some(ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                }),
                ..Default::default()
            })
            .into(),
            model: Some(model.clone()),
        },
        budget: ResourceBudget {
            memory_bytes: 1024,
            output_bytes: 1024,
            wall_time_ms: 1000,
            cpu_threads: 1,
        },
        dependencies: vec![model.identity, identity],
        tools,
    };
    db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,'queued',?6)",params![job.as_str(),attempt.as_str(),f.project.as_str(),f.session.as_str(),source_version.as_str(),encode(&manifest).unwrap()]).unwrap();
    db.execute(
        "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
        params![attempt.as_str(), job.as_str(), encode(&manifest).unwrap()],
    )
    .unwrap();
    (manifest, model_version)
}
#[test]
fn active_model_bytes_cannot_be_evicted_through_source_alias() {
    let f = fixture();
    let (manifest, model) = staged_manifest(&f);
    for state in ["queued", "running", "interrupted"] {
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE jobs SET state=?2 WHERE id=?1",
                params![manifest.job_id.as_str(), state],
            )
            .unwrap();
        }
        let response = f.engine.handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::EvictSource {
                source_version: model.clone(),
            },
        ));
        assert_eq!(response.result.unwrap_err().code, ErrorCode::Forbidden);
    }
}
#[test]
fn attempt_history_records_late_cancelled_failure_without_publication_or_query_receipts() {
    let f = fixture();
    let (manifest, _) = staged_manifest(&f);
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::CancelJob {
                job_id: manifest.job_id.clone(),
            },
        ))
        .result
        .unwrap();
    let error = ProtocolError::new(
        ErrorCode::Unavailable,
        "authored settled worker failure; no native execution",
    );
    f.engine
        .finish_job_observed(
            &manifest,
            Err(error.clone()),
            Some(WorkerRetirementObservation {
                worker_error: Some(JobAttemptDiagnostic::from_error(&error)),
                provider_unavailable: None,
            }),
        )
        .unwrap();
    let count = || {
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM requests", [], |r| r.get::<_, i64>(0))
            .unwrap()
    };
    let before = count();
    for _ in 0..3 {
        let response = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::JobAttempts {
                    job_id: manifest.job_id.clone(),
                    after_attempt: None,
                    limit: 8,
                },
            ))
            .result
            .unwrap();
        let ResponseBody::JobAttempts(page) = response else {
            panic!("history response")
        };
        page.validate().unwrap();
        assert_eq!(page.attempts.len(), 1);
        assert_eq!(page.attempts[0].state, "cancelled");
        let observation = page.attempts[0].outcome.as_ref().unwrap();
        assert_eq!(
            observation.validation_error.as_ref().unwrap().code,
            ErrorCode::Unavailable
        );
        assert!(observation.worker_retirement.is_some());
    }
    assert_eq!(count(), before);
    let db = f.engine.db.lock().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM candidates WHERE job=?1",
            [manifest.job_id.as_str()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn attempt_history_requires_project_read_grant_and_never_infers_retirement() {
    let f = fixture();
    let (manifest, _) = staged_manifest(&f);
    let stranger = pair(&f.engine, "history-stranger");
    let command = Command::JobAttempts {
        job_id: manifest.job_id.clone(),
        after_attempt: None,
        limit: 8,
    };
    let error = f
        .engine
        .handle(request(&stranger, Some(&f.project), None, command.clone()))
        .result
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    let ResponseBody::JobAttempts(page) = f
        .engine
        .handle(request(&f.session, Some(&f.project), None, command))
        .result
        .unwrap()
    else {
        panic!()
    };
    assert!(page.attempts[0].outcome.is_none());
    assert!(page.attempts[0].declared_profile.is_none());
    assert!(page.history_only);
}
#[test]
fn attempt_history_queued_host_failure_reaches_terminal_state() {
    let f = fixture();
    let (manifest, _) = staged_manifest(&f);
    f.engine
        .finish_job(
            &manifest,
            Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "authored dispatch failure before worker launch",
            )),
        )
        .unwrap();
    let db = f.engine.db.lock().unwrap();
    let state = job_snapshot(&db, &manifest.job_id, &f.project).unwrap();
    assert_eq!(state.state, "failed");
    let page =
        crate::job_attempt_history::page(&db, &f.project, &manifest.job_id, None, 8).unwrap();
    let outcome = page.attempts[0].outcome.as_ref().unwrap();
    assert!(outcome.worker_retirement.is_none());
    assert_eq!(
        outcome.validation_error.as_ref().unwrap().code,
        ErrorCode::Unavailable
    );
}
#[test]
fn completion_retains_cuda_membership_and_refuses_missing_changed_or_other_epoch_evidence() {
    // Authored consistency fixtures exercise the real completion transaction;
    // they do not establish launch authorization or genuine native provenance.
    for case in ["accepted", "missing", "changed", "other-epoch"] {
        let f = fixture();
        let (staged, _) = staged_manifest(&f);
        let mut manifest: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        manifest.job_id = staged.job_id;
        manifest.attempt_id = staged.attempt_id;
        manifest.project_id = f.project.clone();
        manifest.source = staged.source;
        manifest.dependencies.push(manifest.source.identity.clone());
        let (manifest, evidence) =
            crate::cuda_acceptance::authored_evidence_for(manifest, &staged.output_dir);
        let p = staged.output_dir.join("program.json");
        fs::write(&p, serde_json::to_vec(&program(0.3)).unwrap()).unwrap();
        let program = crate::worker::pin_artifact(&p).unwrap();
        let r = staged.output_dir.join("receipt.json");
        fs::write(&r, b"{\"scope\":\"authored consistency fixture\"}").unwrap();
        let receipt = crate::worker::pin_artifact(&r).unwrap();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            let raw = encode(&manifest).unwrap();
            tx.execute(
                "UPDATE jobs SET manifest=?1 WHERE id=?2",
                params![raw, manifest.job_id.as_str()],
            )
            .unwrap();
            tx.execute(
                "UPDATE attempts SET manifest=?1 WHERE id=?2",
                params![raw, manifest.attempt_id.as_str()],
            )
            .unwrap();
            crate::cuda_attempts::declare(&tx, &manifest, &f.engine.engine_epoch).unwrap();
            tx.execute(
                "UPDATE jobs SET state='running' WHERE id=?1",
                [manifest.job_id.as_str()],
            )
            .unwrap();
            tx.execute(
                "UPDATE attempts SET state='running' WHERE id=?1",
                [manifest.attempt_id.as_str()],
            )
            .unwrap();
            if case == "other-epoch" {
                tx.execute(
                    "UPDATE cuda_attempt_intents SET engine_epoch=?1",
                    [uuid::Uuid::new_v4().to_string()],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        if case == "changed" {
            fs::write(&evidence.trace.path, b"[]").unwrap();
        }
        f.engine
            .finish_job(
                &manifest,
                Ok(launch::LaunchExecution {
                    output: WorkerOutput::Generated {
                        program,
                        receipt,
                        lineage: vec![],
                        review: vec![],
                    },
                    cuda_artifacts: if case == "missing" {
                        None
                    } else {
                        Some(evidence.clone())
                    },
                    worker_peak_memory_bytes: None,
                }),
            )
            .unwrap();
        let db = f.engine.db.lock().unwrap();
        let state = job_snapshot(&db, &manifest.job_id, &f.project).unwrap();
        assert_eq!(
            state.state,
            if case == "accepted" {
                "completed"
            } else {
                "failed"
            },
            "{case}"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM candidates", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            i64::from(case == "accepted")
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            if case == "accepted" { 16 } else { 15 }
        );
        if case == "accepted" {
            crate::cuda_acceptance::validate(&db, 16).unwrap();
            assert_eq!(
                crate::cuda_acceptance::portable_members(&db, &f.project)
                    .unwrap()
                    .1
                    .len(),
                1
            );
            let lineage: String = db
                .query_row("SELECT lineage FROM candidates", [], |r| r.get(0))
                .unwrap();
            let lineage: Vec<ArtifactIdentity> = decode(&lineage).unwrap();
            assert!(
                lineage.contains(&evidence.receipt.identity)
                    && lineage.contains(&evidence.trace.identity)
            );
        }
    }
}
#[test]
fn accepted_cuda_capture_preserves_dedicated_roles_and_general_receipt() {
    authored_cuda_portable_fixture(None, false);
}
#[test]
#[ignore = "requires an explicitly preserved preceding schema-17 application reader"]
fn native_portable_cuda_export_refuses_preceding_reader() {
    let old = std::env::var_os("PULSAR_PORTABLE_CUDA_PRECEDING_READER")
        .expect("preceding reader required");
    authored_cuda_portable_fixture(Some(std::path::Path::new(&old)), false);
}
fn historical_reader_subject(f: &Fixture) -> tempfile::TempDir {
    fn copy_directory(source: &std::path::Path, destination: &std::path::Path, root: bool) {
        artifacts::private_directory(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if root
                && (name.to_string_lossy().starts_with("projects.sqlite3")
                    || name == "worker-fence.json"
                    || name == "worker-fence.lock")
            {
                continue;
            }
            let destination = destination.join(&name);
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                copy_directory(&entry.path(), &destination, false);
            } else {
                assert!(
                    kind.is_file(),
                    "private fixture contains a nonregular entry"
                );
                fs::copy(entry.path(), destination).unwrap();
            }
        }
    }
    let snapshot = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(snapshot.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    copy_directory(&f.engine.config.state_dir, snapshot.path(), true);
    // Snapshot SQLite's committed view, including WAL, rather than copying
    // live database files. The old reader gets its own unbound host lease.
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "VACUUM INTO ?1",
            [snapshot.path().join("projects.sqlite3").to_str().unwrap()],
        )
        .unwrap();
    snapshot
}
#[cfg(target_os = "linux")]
#[test]
fn accepted_cuda_capture_from_format3_host_preserves_portable_roles() {
    authored_cuda_portable_fixture(None, true);
}
fn authored_cuda_portable_fixture(old_reader: Option<&std::path::Path>, staging_host: bool) {
    // Authored consistency evidence, never a GPU admission or provenance claim.
    let f = fixture();
    let (staged, _) = staged_manifest(&f);
    let mut manifest: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap();
    manifest.job_id = staged.job_id;
    manifest.attempt_id = staged.attempt_id;
    manifest.project_id = f.project.clone();
    manifest.source = staged.source;
    manifest.dependencies.extend(staged.dependencies);
    if let (
        WorkerOperation::Generate {
            model, settings, ..
        },
        WorkerOperation::Generate {
            model: Some(staged_model),
            ..
        },
    ) = (&mut manifest.operation, staged.operation)
    {
        let original = settings.inference_profile.as_ref().unwrap().model.clone();
        manifest
            .dependencies
            .retain(|identity| identity != &original);
        settings.inference_profile.as_mut().unwrap().model = staged_model.identity.clone();
        *model = Some(staged_model);
    } else {
        panic!("generation fixture");
    }
    let (manifest, evidence) =
        crate::cuda_acceptance::authored_evidence_for(manifest, &staged.output_dir);
    let path = staged.output_dir.join("program.json");
    fs::write(&path, serde_json::to_vec(&program(0.3)).unwrap()).unwrap();
    let program = crate::worker::pin_artifact(&path).unwrap();
    let path = staged.output_dir.join("receipt.json");
    fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schema_version":2,"recipe":"authored-portable-cuda-consistency",
            "project_id":manifest.project_id,"base_revision":manifest.base_revision,
            "job_id":manifest.job_id,"attempt_id":manifest.attempt_id,
            "source_version":manifest.source.source_version,"program":program.identity,
            "dependencies":manifest.dependencies
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = crate::worker::pin_artifact(&path).unwrap();
    let general = receipt.identity.clone();
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        let raw = encode(&manifest).unwrap();
        tx.execute(
            "UPDATE jobs SET manifest=?1 WHERE id=?2",
            params![raw, manifest.job_id.as_str()],
        )
        .unwrap();
        tx.execute(
            "UPDATE attempts SET manifest=?1 WHERE id=?2",
            params![raw, manifest.attempt_id.as_str()],
        )
        .unwrap();
        crate::cuda_attempts::declare(&tx, &manifest, &f.engine.engine_epoch).unwrap();
        tx.execute(
            "UPDATE jobs SET state='running' WHERE id=?1",
            [manifest.job_id.as_str()],
        )
        .unwrap();
        tx.execute(
            "UPDATE attempts SET state='running' WHERE id=?1",
            [manifest.attempt_id.as_str()],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    f.engine
        .finish_job(
            &manifest,
            Ok(launch::LaunchExecution {
                output: WorkerOutput::Generated {
                    program,
                    receipt,
                    lineage: manifest.dependencies.clone(),
                    review: vec![],
                },
                cuda_artifacts: Some(evidence.clone()),
                worker_peak_memory_bytes: None,
            }),
        )
        .unwrap();
    if let Some(old) = old_reader {
        let subject = historical_reader_subject(&f);
        let result = std::process::Command::new("/usr/bin/timeout")
            .args(["--signal=TERM", "2"])
            .arg(old)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PULSAR_STATE_DIR", subject.path())
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(124),
            "preceding reader must open pre-barrier subject: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            assert!(
                fs::symlink_metadata(subject.path().join("engine.sock"))
                    .unwrap()
                    .file_type()
                    .is_socket(),
                "preceding reader must reach its listener after opening schema 16"
            );
        }
    }
    let ResponseBody::PackageExport(initial) = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StartProjectExport,
        ))
        .result
        .unwrap()
    else {
        panic!("export response")
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let exported = loop {
        let ResponseBody::PackageExport(status) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ProjectPackageStatus {
                    operation_id: initial.operation_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("export status")
        };
        if status.state.is_terminal() {
            assert_eq!(
                status.state,
                PackageExportOperationState::Ready,
                "{:?}",
                status.error
            );
            break status;
        }
        assert!(std::time::Instant::now() < deadline, "CUDA export deadline");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(exported.artifact.as_ref().unwrap().format_version, 10);
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        18
    );
    if let Some(old) = old_reader {
        let subject = historical_reader_subject(&f);
        let database = subject.path().join("projects.sqlite3");
        let wal = subject.path().join("projects.sqlite3-wal");
        let keeper = Connection::open(&database).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' native reader fixture';").unwrap();
        assert!(fs::metadata(&wal).unwrap().len() > 32);
        let identity = |path: &std::path::Path| {
            fs::read(path)
                .ok()
                .map(|bytes| (bytes.len(), format!("{:x}", Sha256::digest(&bytes))))
        };
        let before = [identity(&database), identity(&wal)];
        let result = std::process::Command::new("/usr/bin/timeout")
            .args(["--signal=TERM", "5"])
            .arg(old)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PULSAR_STATE_DIR", subject.path())
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(1),
            "preceding reader refusal: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration"));
        assert_eq!(
            [identity(&database), identity(&wal)],
            before,
            "preceding reader must preserve committed DB/WAL"
        );
        drop(keeper);
    }
    #[cfg(target_os = "linux")]
    if staging_host {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        f.engine.arm_staging_fence(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            19
        );
        drop(db);
        let claim = f
            .engine
            .pool
            .reserve(1024)
            .unwrap()
            .with_storage(4096, 32 * 1024 * 1024)
            .unwrap();
        let staging = f.engine.worker_fence.create_staging(claim).unwrap();
        let path = staging.path();
        fs::write(path.join("authored"), b"no byte users escaped this fixture").unwrap();
        staging
            .close_after_use_retired()
            .wait(std::time::Instant::now() + std::time::Duration::from_secs(3))
            .unwrap();
        assert!(!path.exists());
        assert_eq!(f.engine.worker_fence.version().unwrap(), 3);
    }
    let token = TOKENS.with(|tokens| tokens.borrow().get(f.session.as_str()).unwrap().clone());
    let plan = project_packages::capture(
        &f.engine,
        &f.session,
        &token,
        &f.project,
        RevisionId::new(0),
    )
    .unwrap();
    assert_eq!(plan.manifest.format_version, 10);
    let origin = &plan.manifest.generated_origins[0];
    let cuda = origin.cuda_evidence.as_ref().unwrap();
    assert_eq!(origin.receipt.as_ref(), Some(&general));
    assert_eq!(cuda.receipt, evidence.receipt.identity);
    assert_eq!(cuda.trace, evidence.trace.identity);
    let candidate = plan
        .manifest
        .candidates
        .iter()
        .find(|c| c.candidate_id == cuda.accepted_candidate)
        .unwrap();
    assert_eq!(cuda.general_receipt(&candidate.lineage).unwrap(), &general);
    for (identity, role) in [
        (&cuda.receipt, PackageObjectRole::CudaExecutionReceipt),
        (&cuda.trace, PackageObjectRole::CudaNodeTrace),
    ] {
        let object = plan
            .manifest
            .objects
            .iter()
            .find(|o| o.sha256 == identity.sha256)
            .unwrap();
        assert!(object.roles.contains(&role));
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut plan.open_object(object).unwrap(), &mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, identity.byte_len);
    }
    let mut bytes = Vec::new();
    crate::project_package_format::write_package(
        &plan.manifest,
        &mut bytes,
        |object| plan.open_object(object),
        Default::default(),
    )
    .unwrap();
    #[cfg(target_os = "linux")]
    if staging_host {
        let scope = f.engine.worker_fence.scope().unwrap();
        for host_only in [
            scope.as_bytes(),
            b"worker_fence_binding",
            b"minimum_ledger_version",
            b"pulsar-stage-",
        ] {
            assert!(
                !bytes
                    .windows(host_only.len())
                    .any(|window| window == host_only),
                "host staging ownership leaked into logical CUDA archive"
            );
        }
    }
    project_package_imports::exercise_authored_cuda_portable_lifecycle(
        &bytes,
        &cuda.trace,
        old_reader,
    );
    fs::write(&evidence.trace.path, b"[]").unwrap();
    assert!(project_packages::capture(
        &f.engine,
        &f.session,
        &token,
        &f.project,
        RevisionId::new(0)
    )
    .is_err());
}
#[test]
fn cpu_completion_refuses_cuda_evidence_before_reading_program_or_changing_barrier() {
    let f = fixture();
    let (manifest, _) = staged_manifest(&f);
    let (_, evidence) = crate::cuda_acceptance::authored_evidence(&manifest.output_dir);
    let original: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    {
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET state='running' WHERE id=?1",
            [manifest.job_id.as_str()],
        )
        .unwrap();
        db.execute(
            "UPDATE attempts SET state='running' WHERE id=?1",
            [manifest.attempt_id.as_str()],
        )
        .unwrap();
    }
    f.engine
        .finish_job(
            &manifest,
            Ok(launch::LaunchExecution {
                output: WorkerOutput::Generated {
                    program: evidence.trace.clone(),
                    receipt: evidence.receipt.clone(),
                    lineage: vec![],
                    review: vec![],
                },
                cuda_artifacts: Some(evidence),
                worker_peak_memory_bytes: None,
            }),
        )
        .unwrap();
    let db = f.engine.db.lock().unwrap();
    let (state, error): (String, String) = db
        .query_row(
            "SELECT state,error FROM jobs WHERE id=?1",
            [manifest.job_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "failed");
    assert!(error.contains("completion CUDA evidence differs from declared provider"));
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        original
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM candidates", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn independent_audio_dependencies_hold_snapshots_and_require_exact_declaration() {
    let f = fixture();
    let (mut manifest, audio_version) = staged_manifest(&f);
    let audio = f
        .engine
        .source(&f.engine.db.lock().unwrap(), &f.project, &audio_version)
        .unwrap();
    manifest.independent_audio = Some(audio.clone());
    if let WorkerOperation::Generate {
        model, settings, ..
    } = &mut manifest.operation
    {
        *model = None;
        settings.model_input = None;
        settings.mixed_audio = Some(MixedAudioSettings {
            intent: MultimodalIntent::Faithful,
            mapping: AudioMapping::new(Axis::Stroke, 0.2, NormalizedPosition::new(0.4).unwrap())
                .unwrap(),
            independent: Some(IndependentAudio {
                source_version: audio_version.clone(),
                alignment: AudioAlignment {
                    rate: None,
                    video_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    audio_anchor: SourceTimestamp::new(1, 8).unwrap(),
                },
            }),
        });
    }
    manifest.validate().unwrap();
    let mut mismatched = manifest.clone();
    mismatched.independent_audio = None;
    assert!(mismatched.validate().is_err());
    let mut mismatched = manifest.clone();
    mismatched
        .dependencies
        .retain(|identity| identity != &audio.identity);
    assert!(mismatched.validate().is_err());
    for state in ["queued", "running", "interrupted"] {
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE jobs SET state=?2,manifest=?3 WHERE id=?1",
                params![manifest.job_id.as_str(), state, encode(&manifest).unwrap()],
            )
            .unwrap();
        let result = f.engine.handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::EvictSource {
                source_version: audio_version.clone(),
            },
        ));
        assert_eq!(result.result.unwrap_err().code, ErrorCode::Forbidden);
        assert!(audio.path.is_file());
    }
}
#[test]
fn cancellation_receipt_failure_does_not_signal_worker_and_exact_retry_commits() {
    for fail_at_commit in [false, true] {
        let f = fixture();
        let (manifest, _) = staged_manifest(&f);
        let flag = Arc::new(AtomicBool::new(false));
        f.engine
            .workers
            .lock()
            .unwrap()
            .insert(manifest.job_id.to_string(), flag.clone());
        let cancel = request(
            &f.session,
            Some(&f.project),
            None,
            Command::CancelJob {
                job_id: manifest.job_id.clone(),
            },
        );
        let before = {
            let db = f.engine.db.lock().unwrap();
            let states: (String, String) = db.query_row(
            "SELECT jobs.state,attempts.state FROM jobs JOIN attempts ON attempts.id=jobs.attempt WHERE jobs.id=?1",
            [manifest.job_id.as_str()], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
            if fail_at_commit {
                // Deferred foreign-key refusal occurs in COMMIT, after remember succeeds.
                assert_eq!(
                    db.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                db.execute_batch("CREATE TABLE cancel_commit_parent(id INTEGER PRIMARY KEY); CREATE TABLE cancel_commit_child(id INTEGER REFERENCES cancel_commit_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER reject_cancel_receipt AFTER INSERT ON requests BEGIN INSERT INTO cancel_commit_child VALUES(1); END;").unwrap();
            } else {
                db.execute_batch("CREATE TEMP TRIGGER reject_cancel_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'injected cancellation receipt failure'); END;").unwrap();
            }
            states
        };
        let error = f.engine.handle(cancel.clone()).result.unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains(if fail_at_commit {
            "FOREIGN KEY constraint failed"
        } else {
            "injected cancellation receipt failure"
        }));
        assert!(
            !flag.load(Ordering::Acquire),
            "rolled-back cancellation signalled the worker"
        );
        {
            let db = f.engine.db.lock().unwrap();
            let after: (String, String) = db.query_row(
            "SELECT jobs.state,attempts.state FROM jobs JOIN attempts ON attempts.id=jobs.attempt WHERE jobs.id=?1",
            [manifest.job_id.as_str()], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
            assert_eq!(after, before);
            let receipts: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM requests WHERE session=?1 AND request=?2",
                    params![f.session.as_str(), cancel.request_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(receipts, 0);
            db.execute_batch("DROP TRIGGER reject_cancel_receipt;")
                .unwrap();
        }
        let reply = f.engine.handle(cancel.clone()).result.unwrap();
        assert!(matches!(reply,ResponseBody::Job(ref job) if job.state=="cancelled"));
        assert!(flag.load(Ordering::Acquire));
        // Exact replay returns the durable result without signalling a replacement handle.
        let replacement = Arc::new(AtomicBool::new(false));
        f.engine
            .workers
            .lock()
            .unwrap()
            .insert(manifest.job_id.to_string(), replacement.clone());
        assert!(
            matches!(f.engine.handle(cancel).result.unwrap(),ResponseBody::Job(ref job) if job.state=="cancelled")
        );
        assert!(!replacement.load(Ordering::Acquire));
    }
}

#[test]
fn revocation_rollback_preserves_all_worker_classes_and_exact_retry_signals_only_scope() {
    for fail_at_commit in [false, true] {
        let f = fixture();
        let target = pair(&f.engine, "revoked test session");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: target.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate],
                },
            ))
            .result
            .unwrap();
        let (manifest, _) = staged_manifest(&f);
        let keys = [
            manifest.job_id.to_string(),
            format!("preview:{}:{}:fixture", f.project, target),
            format!("wasm-edit:{}:{}:fixture", f.project, target),
            format!("probe:{}:{}:source:fixture", f.project, target),
            format!("preview:{}:{}:unrelated", f.project, f.session),
            format!("probe:{}:{}:source:unrelated", f.project, f.session),
        ];
        let flags: Vec<_> = keys
            .iter()
            .map(|key| {
                let flag = Arc::new(AtomicBool::new(false));
                f.engine
                    .workers
                    .lock()
                    .unwrap()
                    .insert(key.clone(), flag.clone());
                flag
            })
            .collect();
        let revoke = request(
            &f.session,
            None,
            None,
            Command::Revoke {
                session: target.clone(),
                project_id: f.project.clone(),
            },
        );
        let before = {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE jobs SET session=?2 WHERE id=?1",
                params![manifest.job_id.as_str(), target.as_str()],
            )
            .unwrap();
            // Inert stored invocation exercises rollback of the revocation UPDATE;
            // no guest, process termination or WASM qualification is claimed.
            db.execute("INSERT INTO wasm_edits(id,project,session,base_revision,engine_epoch,request_id,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,state) VALUES('revoke-fixture',?1,?2,0,'fixture','fixture','fixture','{}',X'',X'','{}','{}',1,'fixture','running')",params![f.project.as_str(),target.as_str()]).unwrap();
            let counts:(i64,i64)=db.query_row("SELECT (SELECT COUNT(*) FROM grants WHERE session=?1 AND project=?2),(SELECT COUNT(*) FROM events WHERE project=?2)",params![target.as_str(),f.project.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
            if fail_at_commit {
                db.execute_batch("CREATE TABLE revoke_commit_parent(id INTEGER PRIMARY KEY); CREATE TABLE revoke_commit_child(id INTEGER REFERENCES revoke_commit_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER reject_revoke_receipt AFTER INSERT ON requests BEGIN INSERT INTO revoke_commit_child VALUES(1); END;").unwrap();
            } else {
                db.execute_batch("CREATE TEMP TRIGGER reject_revoke_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'injected revocation receipt failure'); END;").unwrap();
            }
            counts
        };
        let error = f.engine.handle(revoke.clone()).result.unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains(if fail_at_commit {
            "FOREIGN KEY constraint failed"
        } else {
            "injected revocation receipt failure"
        }));
        assert!(
            flags.iter().all(|flag| !flag.load(Ordering::Acquire)),
            "rolled-back revocation signalled a worker"
        );
        {
            let db = f.engine.db.lock().unwrap();
            let after:(i64,i64)=db.query_row("SELECT (SELECT COUNT(*) FROM grants WHERE session=?1 AND project=?2),(SELECT COUNT(*) FROM events WHERE project=?2)",params![target.as_str(),f.project.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
            assert_eq!(before, after);
            assert_eq!(
                job_snapshot(&db, &manifest.job_id, &f.project)
                    .unwrap()
                    .state,
                "queued"
            );
            assert_eq!(
                db.query_row(
                    "SELECT state FROM attempts WHERE id=?1",
                    [manifest.attempt_id.as_str()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "queued"
            );
            assert_eq!(
                db.query_row(
                    "SELECT state FROM wasm_edits WHERE id='revoke-fixture'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "running"
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM requests WHERE session=?1 AND request=?2",
                    params![f.session.as_str(), revoke.request_id.as_str()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            db.execute_batch("DROP TRIGGER reject_revoke_receipt;")
                .unwrap();
        }
        assert!(matches!(
            f.engine.handle(revoke.clone()).result.unwrap(),
            ResponseBody::Ack
        ));
        assert!(flags[..4].iter().all(|flag| flag.load(Ordering::Acquire)));
        assert!(flags[4..].iter().all(|flag| !flag.load(Ordering::Acquire)));
        {
            let db = f.engine.db.lock().unwrap();
            assert_eq!(
                job_snapshot(&db, &manifest.job_id, &f.project)
                    .unwrap()
                    .state,
                "cancelled"
            );
            assert_eq!(
                db.query_row(
                    "SELECT state FROM wasm_edits WHERE id='revoke-fixture'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "cancelled"
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM grants WHERE session=?1 AND project=?2",
                    params![target.as_str(), f.project.as_str()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
        }
        let replacements: Vec<_> = keys[..3]
            .iter()
            .map(|key| {
                let flag = Arc::new(AtomicBool::new(false));
                f.engine
                    .workers
                    .lock()
                    .unwrap()
                    .insert(key.clone(), flag.clone());
                flag
            })
            .collect();
        assert!(matches!(
            f.engine.handle(revoke).result.unwrap(),
            ResponseBody::Ack
        ));
        assert!(replacements
            .iter()
            .all(|flag| !flag.load(Ordering::Acquire)));
    }
}

#[test]
fn wasm_cancellation_rollback_preserves_invocation_and_worker_until_exact_retry() {
    for original_state in ["running", "interrupted"] {
        for fail_at_commit in [false, true] {
            let f = fixture();
            let attempt = id!(AttemptId);
            let start_id = id!(RequestId);
            // Typed, validated retained invocation; no guest/process is launched.
            let module=wat::parse_str("(module (memory (export \"memory\") 1) (func (export \"pulsar_alloc\") (param i32) (result i32) i32.const 0) (func (export \"pulsar_process\") (param i32 i32) (result i64) i64.const 0))").unwrap();
            let spec = WasmEditSpec {
                manifest: ExtensionManifest {
                    schema_version: PROTOCOL_VERSION,
                    name: "Cancellation fixture".into(),
                    role: ExtensionRole::Editor,
                    artifact: ArtifactIdentity {
                        artifact_id: ArtifactId::new("cancel-module").unwrap(),
                        sha256: format!("{:x}", Sha256::digest(&module)),
                        byte_len: module.len() as u64,
                    },
                    abi: WASM_PROCESSING_ABI.into(),
                    requested_scopes: vec![],
                    budget: ResourceBudget {
                        memory_bytes: 256 * 1024 * 1024,
                        output_bytes: WASM_BUFFER_MAX_BYTES as u64,
                        wall_time_ms: 5000,
                        cpu_threads: 2,
                    },
                    confinement: ConfinementRequirement::Wasm,
                },
                module,
                guest_memory_bytes: 1024 * 1024,
                expected_worker: f.engine.binary_identity.clone(),
                approve_execution: true,
                label: "Cancellation fixture".into(),
            };
            spec.validate().unwrap();
            let input =
                serde_json::to_vec(&pulsar_core::EditValuesProgram::new(vec![]).unwrap()).unwrap();
            let original_error = if original_state == "interrupted" {
                Some(
                    encode(&ProtocolError::new(
                        ErrorCode::Unavailable,
                        "authored interrupted fixture",
                    ))
                    .unwrap(),
                )
            } else {
                None
            };
            let worker_key = format!("wasm-edit:{}:{}:{}", f.project, f.session, attempt);
            let flag = Arc::new(AtomicBool::new(false));
            f.engine
                .workers
                .lock()
                .unwrap()
                .insert(worker_key.clone(), flag.clone());
            let cancel = request(
                &f.session,
                Some(&f.project),
                None,
                Command::CancelWasmEdit {
                    attempt_id: attempt.clone(),
                },
            );
            {
                let db = f.engine.db.lock().unwrap();
                let base =
                    motion_state::revision_descriptor(&db, &f.project, RevisionId::new(0)).unwrap();
                db.execute("INSERT INTO wasm_edits(id,project,session,base_revision,engine_epoch,request_id,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,state,error) VALUES(?1,?2,?3,0,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",params![attempt.as_str(),f.project.as_str(),f.session.as_str(),f.engine.engine_epoch,start_id.as_str(),"a".repeat(64),encode(&spec.manifest).unwrap(),spec.module,input,encode(&base).unwrap(),encode(&spec.expected_worker).unwrap(),spec.guest_memory_bytes as i64,spec.label,original_state,original_error]).unwrap();
                wasm_edits::validate(&db).unwrap();
                if fail_at_commit {
                    db.execute_batch("CREATE TABLE wasm_cancel_parent(id INTEGER PRIMARY KEY); CREATE TABLE wasm_cancel_child(id INTEGER REFERENCES wasm_cancel_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER reject_wasm_cancel AFTER INSERT ON requests BEGIN INSERT INTO wasm_cancel_child VALUES(1); END;").unwrap();
                } else {
                    db.execute_batch("CREATE TEMP TRIGGER reject_wasm_cancel BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'injected WASM cancellation receipt failure'); END;").unwrap();
                }
            }
            let error = f.engine.handle(cancel.clone()).result.unwrap_err();
            assert_eq!(error.code, ErrorCode::Internal);
            assert!(error.message.contains(if fail_at_commit {
                "FOREIGN KEY constraint failed"
            } else {
                "injected WASM cancellation receipt failure"
            }));
            assert!(
                !flag.load(Ordering::Acquire),
                "rolled-back WASM cancellation signalled the worker"
            );
            {
                let db = f.engine.db.lock().unwrap();
                let after: (String, Option<String>) = db
                    .query_row(
                        "SELECT state,error FROM wasm_edits WHERE id=?1",
                        [attempt.as_str()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(after, (original_state.into(), original_error.clone()));
                assert_eq!(
                    db.query_row(
                        "SELECT COUNT(*) FROM requests WHERE session=?1 AND request=?2",
                        params![f.session.as_str(), cancel.request_id.as_str()],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    0
                );
                wasm_edits::validate(&db).unwrap();
                db.execute_batch("DROP TRIGGER reject_wasm_cancel;")
                    .unwrap();
            }
            assert!(
                matches!(f.engine.handle(cancel.clone()).result.unwrap(),ResponseBody::WasmEdit(ref report) if report.state==WasmEditState::Cancelled)
            );
            assert!(flag.load(Ordering::Acquire));
            wasm_edits::validate(&f.engine.db.lock().unwrap()).unwrap();
            let replacement = Arc::new(AtomicBool::new(false));
            f.engine
                .workers
                .lock()
                .unwrap()
                .insert(worker_key, replacement.clone());
            assert!(
                matches!(f.engine.handle(cancel).result.unwrap(),ResponseBody::WasmEdit(ref report) if report.state==WasmEditState::Cancelled)
            );
            assert!(!replacement.load(Ordering::Acquire));
            let terminal_cancel = request(
                &f.session,
                Some(&f.project),
                None,
                Command::CancelWasmEdit {
                    attempt_id: attempt,
                },
            );
            assert!(
                matches!(f.engine.handle(terminal_cancel).result.unwrap(),ResponseBody::WasmEdit(ref report) if report.state==WasmEditState::Cancelled)
            );
            assert!(!replacement.load(Ordering::Acquire));
            assert_eq!(
                f.engine
                    .db
                    .lock()
                    .unwrap()
                    .query_row("SELECT COUNT(*) FROM candidates", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
}

#[test]
fn cancelled_and_revoked_work_cannot_publish_late_candidates() {
    let f = fixture();
    let (manifest, _) = staged_manifest(&f);
    let flag = Arc::new(AtomicBool::new(false));
    f.engine
        .workers
        .lock()
        .unwrap()
        .insert(manifest.job_id.to_string(), flag.clone());
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::CancelJob {
                job_id: manifest.job_id.clone(),
            },
        ))
        .result
        .unwrap();
    assert!(flag.load(Ordering::Acquire));
    f.engine
        .finish_job(
            &manifest,
            Err(ProtocolError::invalid("late malformed result")),
        )
        .unwrap();
    {
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            job_snapshot(&db, &manifest.job_id, &f.project)
                .unwrap()
                .state,
            "cancelled"
        );
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    // A terminal attempt's observation is immutable, including a different
    // late response. Exercise revocation with a distinct original job.
    assert_eq!(
        f.engine
            .finish_job(
                &manifest,
                Err(ProtocolError::invalid("conflicting late result"))
            )
            .unwrap_err()
            .code,
        ErrorCode::RequestConflict
    );
    let f = fixture();
    let (manifest, _) = staged_manifest(&f);
    {
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET state='running' WHERE id=?1",
            [manifest.job_id.as_str()],
        )
        .unwrap();
    }
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Revoke {
                session: f.session.clone(),
                project_id: f.project.clone(),
            },
        ))
        .result
        .unwrap();
    f.engine
        .finish_job(
            &manifest,
            Err(ProtocolError::invalid("another late result")),
        )
        .unwrap();
    let db = f.engine.db.lock().unwrap();
    assert_eq!(
        job_snapshot(&db, &manifest.job_id, &f.project)
            .unwrap()
            .state,
        "cancelled"
    );
}
#[test]
fn explicit_motion_retime_is_revision_bound_replayable_protected_and_restorable() {
    let f = fixture();
    let file = f.dir.path().join("retime-source");
    fs::write(&file, b"inert media fixture").unwrap();
    let source = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::ImportSource { path: file },
        ))
        .result
        .unwrap()
    {
        ResponseBody::Source { source_version } => source_version,
        _ => panic!("source"),
    };
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.3)),
        ))
        .result
        .unwrap();
    let spec = SourcePlacementSpec {
        source_version: source,
        source_start: SourceTimestamp::new(0, 1).unwrap(),
        source_end: SourceTimestamp::new(10, 1).unwrap(),
        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
        project_anchor: ProjectTime::ZERO,
        rate_numerator: 1,
        rate_denominator: 1,
    };
    let old = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::SetSourcePlacement {
                spec: spec.clone(),
                replace: None,
            },
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p.placements[0].clone(),
        _ => panic!("placement"),
    };
    let mut changed = spec;
    changed.rate_numerator = 2;
    let current = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::SetSourcePlacement {
                spec: changed,
                replace: Some(old.placement_id.clone()),
            },
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p,
        _ => panic!("placement"),
    };
    let command = Command::RetimeMotion {
        from_placement: old.placement_id,
        to_placement: current.placements[0].placement_id.clone(),
        axes: vec![Axis::Stroke],
    };
    let other = pair(&f.engine, "read-only retime fixture");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: other.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(&other, Some(&f.project), Some(3), command.clone()))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(2),
                command.clone()
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    {
        let db = f.engine.db.lock().unwrap();
        db.execute_batch("CREATE TRIGGER receipt_failure BEFORE INSERT ON motion_retime_receipts BEGIN SELECT RAISE(ABORT,'synthetic receipt failure');END;").unwrap();
    }
    let failed = request(&f.session, Some(&f.project), Some(3), command.clone());
    assert!(f.engine.handle(failed.clone()).result.is_err());
    {
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            project_state(&db, &f.project).unwrap().revision,
            RevisionId::new(3)
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM motion_retime_receipts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.execute_batch("DROP TRIGGER receipt_failure;").unwrap();
    }
    let mutation = request(&f.session, Some(&f.project), Some(3), command.clone());
    let output = f.engine.handle(mutation.clone()).result.unwrap();
    assert_eq!(
        encode(&output).unwrap(),
        encode(&f.engine.handle(mutation).result.unwrap()).unwrap()
    );
    let ResponseBody::Project(out) = output else {
        panic!("project")
    };
    assert_eq!(
        project_state(&f.engine.db.lock().unwrap(), &f.project)
            .unwrap()
            .program
            .tracks()[0]
            .actions()[1]
            .time()
            .as_nanos(),
        500_000_000
    );
    let status = request(
        &other,
        Some(&f.project),
        None,
        Command::MotionRetimeStatus {
            revision: out.revision,
        },
    );
    let ResponseBody::MotionRetimeReceipt(receipt) =
        f.engine.handle(status.clone()).result.unwrap()
    else {
        panic!("receipt")
    };
    assert_eq!(receipt.after.sha256, out.motion.program.sha256);
    assert_eq!(receipt.before.sha256, current.motion.program.sha256);
    assert_eq!(receipt.base_revision, RevisionId::new(3));
    let token = TOKENS.with(|tokens| tokens.borrow().get(f.session.as_str()).cloned().unwrap());
    let captured =
        project_packages::capture(&f.engine, &f.session, &token, &f.project, out.revision).unwrap();
    assert_eq!(
        captured.manifest.format_version,
        PROJECT_PACKAGE_FORMAT_VERSION_WITH_RETIMING_RECEIPTS
    );
    assert_eq!(captured.manifest.counts.retiming_receipts, 1);
    assert_eq!(
        encode(&captured.manifest.retiming_receipts[0]).unwrap(),
        encode(&receipt).unwrap()
    );
    let mut forged = captured.manifest.clone();
    forged.retiming_receipts[0].actor = "forged".into();
    assert!(forged.validate().is_err());
    let mut forged = captured.manifest.clone();
    forged.retiming_receipts.push(*receipt.clone());
    forged.counts = forged.record_counts().unwrap();
    assert!(forged.validate().is_err());
    let verify = |manifest: &PortableProjectManifest| {
        project_package_imports::validate_evidence(
            manifest,
            &f.engine.pool,
            |reference| {
                let object = captured
                    .manifest
                    .objects
                    .iter()
                    .find(|o| o.sha256 == reference.sha256 && o.byte_len == reference.byte_len)
                    .unwrap();
                let mut bytes = Vec::new();
                captured
                    .open_object(object)?
                    .read_to_end(&mut bytes)
                    .map_err(unavailable)?;
                Ok(bytes)
            },
            &mut || Ok(()),
        )
    };
    verify(&captured.manifest).unwrap();
    let mut forged = captured.manifest.clone();
    forged.retiming_receipts[0].axes = vec![Axis::Sway];
    forged.validate().unwrap();
    assert_eq!(
        verify(&forged).unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    drop(captured);
    {
        let db = f.engine.db.lock().unwrap();
        let original: String = db
            .query_row(
                "SELECT receipt FROM motion_retime_receipts WHERE project=?1 AND revision=4",
                [f.project.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        db.execute(
            "UPDATE motion_retime_receipts SET receipt=?1 WHERE project=?2 AND revision=4",
            params!["x".repeat(16385), f.project.as_str()],
        )
        .unwrap();
        drop(db);
        assert!(f.engine.handle(status.clone()).result.is_err());
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "UPDATE motion_retime_receipts SET receipt=?1 WHERE project=?2 AND revision=4",
            params![original, f.project.as_str()],
        )
        .unwrap();
    }
    let db = f.engine.db.lock().unwrap();
    let review = lineage::state(&db, &f.project, out.revision).unwrap();
    assert!(
        review.unknown
            && review
                .flags
                .iter()
                .any(|f| f.reason.as_str() == "existing_motion_explicitly_retimed_review_required")
    );
    drop(db);
    let restored = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(4),
            Command::Undo,
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p,
        _ => panic!("undo"),
    };
    assert_eq!(restored.motion.program, current.motion.program);
    let redone = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(5),
            Command::Redo,
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p,
        _ => panic!("redo"),
    };
    assert_eq!(redone.motion.program, out.motion.program);
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(6),
            Command::SetProtectedRegions {
                regions: vec![ProtectedRegion {
                    axis: Some(Axis::Stroke),
                    range: TimeRange::new(
                        ProjectTime::ZERO,
                        ProjectTime::from_nanos(1_000_000_000),
                    )
                    .unwrap(),
                }],
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(&f.session, Some(&f.project), Some(7), command))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let config = f.engine.config.clone();
    drop(f.engine);
    let reopened = Engine::open(config).unwrap();
    let ResponseBody::Project(snapshot) = reopened
        .handle(request(
            &f.session,
            None,
            None,
            Command::OpenProject {
                project_id: f.project,
            },
        ))
        .result
        .unwrap()
    else {
        panic!("restart")
    };
    assert_eq!(snapshot.motion.program, redone.motion.program);
    assert!(matches!(
        reopened
            .handle(request(
                &f.session,
                Some(&snapshot.project_id),
                None,
                Command::MotionRetimeStatus {
                    revision: RevisionId::new(4)
                }
            ))
            .result
            .unwrap(),
        ResponseBody::MotionRetimeReceipt(_)
    ));
}
#[test]
fn source_placements_preserve_motion_replay_history_and_edit_authority() {
    let f = fixture();
    let input = f.dir.path().join("source.bin");
    fs::write(&input, b"declared source fixture").unwrap();
    let source = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::ImportSource { path: input },
        ))
        .result
        .unwrap()
    {
        ResponseBody::Source { source_version } => source_version,
        _ => panic!("source"),
    };
    let base = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, program(0.3)),
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p,
        _ => panic!("project"),
    };
    let spec = SourcePlacementSpec {
        source_version: source.clone(),
        source_start: SourceTimestamp::new(0, 1).unwrap(),
        source_end: SourceTimestamp::new(10, 1).unwrap(),
        source_anchor: SourceTimestamp::new(1, 2).unwrap(),
        project_anchor: ProjectTime::from_nanos(2_000_000_000),
        rate_numerator: 2,
        rate_denominator: 3,
    };
    let create = request(
        &f.session,
        Some(&f.project),
        Some(base.revision.value()),
        Command::SetSourcePlacement {
            spec: spec.clone(),
            replace: None,
        },
    );
    let placed = match f.engine.handle(create.clone()).result.unwrap() {
        ResponseBody::Project(p) => p,
        _ => panic!("project"),
    };
    assert_eq!(placed.motion.program, base.motion.program);
    assert_eq!(placed.placements.len(), 1);
    let old = placed.placements[0].clone();
    let replay = match f.engine.handle(create.clone()).result.unwrap() {
        ResponseBody::Project(p) => p,
        _ => panic!("project"),
    };
    assert_eq!(replay.revision, placed.revision);
    assert_eq!(replay.placements, placed.placements);
    let mut changed = create;
    let mut different = spec.clone();
    different.rate_numerator = 5;
    changed.command = Command::SetSourcePlacement {
        spec: different,
        replace: None,
    };
    assert_eq!(
        f.engine.handle(changed).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    let mut replacement = spec.clone();
    replacement.rate_numerator = -1;
    let replaced = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(placed.revision.value()),
            Command::SetSourcePlacement {
                spec: replacement,
                replace: Some(old.placement_id.clone()),
            },
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p,
        _ => panic!("project"),
    };
    assert_ne!(replaced.placements[0].placement_id, old.placement_id);
    assert_eq!(replaced.motion.program, base.motion.program);
    let restored = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(replaced.revision.value()),
            Command::Undo,
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p,
        _ => panic!("project"),
    };
    assert_eq!(restored.placements, placed.placements);
    let redone = match f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(restored.revision.value()),
            Command::Redo,
        ))
        .result
        .unwrap()
    {
        ResponseBody::Project(p) => p,
        _ => panic!("project"),
    };
    assert_eq!(redone.placements, replaced.placements);
    assert_eq!(redone.motion.program, base.motion.program);
    let other = pair(&f.engine, "read only");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: other.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(
                &other,
                Some(&f.project),
                Some(redone.revision.value()),
                Command::SetSourcePlacement {
                    spec,
                    replace: None
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::EvictSource {
                    source_version: source
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let db = f.engine.db.lock().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM source_placement_definitions",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    let reviews = lineage::state(&db, &f.project, redone.revision).unwrap();
    assert!(reviews
        .flags
        .iter()
        .any(|f| f.reason.as_str() == "source_alignment_changed_motion_not_retimed"));
    drop(db);
    let token = TOKENS.with(|tokens| tokens.borrow().get(f.session.as_str()).cloned().unwrap());
    let capture =
        project_packages::capture(&f.engine, &f.session, &token, &f.project, redone.revision);
    let capture = capture.unwrap();
    assert_eq!(
        capture.manifest.format_version,
        PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS
    );
    assert_eq!(
        capture
            .manifest
            .placements
            .as_ref()
            .unwrap()
            .definitions
            .len(),
        2
    );
    capture.manifest.validate().unwrap();
}

#[test]
fn retained_cuda_removal_requires_read_admin_revision_and_identity() {
    let f = fixture();
    let reader = pair(&f.engine, "retention reader");
    let operation = Command::RemoveRetainedLiveCudaEvidence {
        epoch: AttemptId::new("absent-capture").unwrap(),
        expected_manifest_sha256: "0".repeat(64),
    };
    assert!(operation.is_mutating() && operation.requires_revision());
    for scopes in [vec![Scope::Read], vec![Scope::ManageGrants]] {
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                    scopes,
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &reader,
                    Some(&f.project),
                    Some(0),
                    operation.clone()
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: reader.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::ManageGrants],
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(
                &reader,
                Some(&f.project),
                Some(1),
                operation.clone()
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        f.engine
            .handle(request(
                &reader,
                Some(&f.project),
                Some(0),
                operation.clone()
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Revoke {
                session: reader.clone(),
                project_id: f.project.clone(),
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine
            .handle(request(&reader, Some(&f.project), Some(0), operation))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}
#[test]
fn retained_cuda_removal_is_durable_replay_and_rechecks_current_authority() {
    let f = fixture();
    let record = {
        let db = f.engine.db.lock().unwrap();
        crate::live_cuda_capture_db::tests::install_removal_fixture(&db, &f.project, &f.session)
    };
    let project = &record.identity.project;
    let revision = record.identity.revision.value();
    let reader = pair(&f.engine, "retention administrator");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: reader.clone(),
                project_id: project.clone(),
                scopes: vec![Scope::Read, Scope::ManageGrants],
            },
        ))
        .result
        .unwrap();
    let operation = Command::RemoveRetainedLiveCudaEvidence {
        epoch: record.identity.epoch.clone(),
        expected_manifest_sha256: record.evidence.manifest.sha256.clone(),
    };
    let mut wrong = request(&reader, Some(project), Some(revision), operation.clone());
    wrong.command = Command::RemoveRetainedLiveCudaEvidence {
        epoch: record.identity.epoch.clone(),
        expected_manifest_sha256: "0".repeat(64),
    };
    assert_eq!(
        f.engine.handle(wrong).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    let original = request(&reader, Some(project), Some(revision), operation.clone());
    let first = f.engine.handle(original.clone()).result.unwrap();
    let ResponseBody::LiveCudaEvidenceRemoval(receipt) = &first else {
        panic!("removal receipt")
    };
    receipt.validate().unwrap();
    assert_eq!(receipt.manifest, record.evidence.manifest);
    assert!(!receipt.physical_erasure);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(f.engine.handle(original.clone()).result.unwrap()).unwrap()
    );
    let mut conflict = original.clone();
    conflict.command = Command::RemoveRetainedLiveCudaEvidence {
        epoch: record.identity.epoch.clone(),
        expected_manifest_sha256: "0".repeat(64),
    };
    assert_eq!(
        f.engine.handle(conflict).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    assert_eq!(
        f.engine
            .handle(request(&reader, Some(project), Some(revision), operation))
            .result
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    {
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM live_cuda_capture_evidence", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT revision FROM projects WHERE id=?1",
                [project.as_str()],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            revision
        );
        let saved: String = db
            .query_row(
                "SELECT response FROM requests WHERE session=?1 AND request=?2",
                params![reader.as_str(), original.request_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved, serde_json::to_string(&first).unwrap());
    }
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Revoke {
                session: reader.clone(),
                project_id: project.clone(),
            },
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine.handle(original).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
}
#[test]
fn retained_cuda_query_requires_current_read_authority_and_revision() {
    for operation in [
        Command::ListLiveCudaEvidence {
            after: None,
            limit: 16,
        },
        Command::InspectLiveCudaEvidence {
            epoch: AttemptId::new("absent-capture").unwrap(),
        },
        Command::BeginLiveCudaEvidenceDownload {
            epoch: AttemptId::new("absent-capture").unwrap(),
            part: LiveCudaEvidencePart::Trace,
        },
    ] {
        let f = fixture();
        let reader = pair(&f.engine, "retained CUDA reader");
        assert_eq!(
            operation.is_mutating(),
            matches!(operation, Command::BeginLiveCudaEvidenceDownload { .. })
        );
        assert!(operation.requires_revision());
        assert_eq!(
            f.engine
                .handle(request(
                    &reader,
                    Some(&f.project),
                    Some(0),
                    operation.clone()
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(
                    &reader,
                    Some(&f.project),
                    Some(1),
                    operation.clone()
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let result = f
            .engine
            .handle(request(
                &reader,
                Some(&f.project),
                Some(0),
                operation.clone(),
            ))
            .result;
        if matches!(operation, Command::ListLiveCudaEvidence { .. }) {
            let ResponseBody::LiveCudaEvidencePage(page) = result.unwrap() else {
                panic!("listing response")
            };
            assert!(page.entries.is_empty());
            page.validate().unwrap();
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
        }
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: reader.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(
            f.engine
                .handle(request(&reader, Some(&f.project), Some(0), operation))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
}
