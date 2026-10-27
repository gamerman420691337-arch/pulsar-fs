use super::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires an explicitly delegated private Linux cgroup and authored WASM worker"]
fn portable_wasm_invocation_survives_clone_restart_reexport_and_refuses_corruption() {
    use std::io::Read;
    let f = Fixture::new();
    let spec = f.spec();
    let module = spec.manifest.artifact.clone();
    let request = f.request(Command::StartWasmEdit { spec }, Some(0));
    let ResponseBody::WasmEdit(accepted) = f.engine.handle(request).result.unwrap() else {
        panic!("start")
    };
    let completed = f.wait(&accepted.attempt_id);
    assert_eq!(
        completed.state,
        WasmEditState::Completed,
        "{:?}",
        completed.error
    );
    f.retired();
    let plan = crate::authority::project_packages::capture(
        &f.engine,
        &f.session,
        &f.token,
        &f.project,
        RevisionId::new(0),
    )
    .unwrap();
    assert_eq!(plan.manifest.format_version, 12);
    let invocation = plan.manifest.authored_lineage[0]
        .wasm_invocation
        .as_ref()
        .unwrap();
    assert_eq!(invocation.manifest.artifact, module);
    let mut objects = std::collections::BTreeMap::new();
    for descriptor in &plan.manifest.objects {
        let mut bytes = vec![];
        plan.open_object(descriptor)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        objects.insert(descriptor.sha256.clone(), bytes);
    }
    let validate = |manifest: &PortableProjectManifest,
                    map: &std::collections::BTreeMap<String, Vec<u8>>| {
        manifest.validate()?;
        crate::authority::project_package_imports::validate_evidence(
            manifest,
            &f.engine.pool,
            |reference| {
                map.get(&reference.sha256)
                    .cloned()
                    .ok_or_else(|| ProtocolError::invalid("absent test object"))
            },
            &mut || Ok(()),
        )
    };
    validate(&plan.manifest, &objects).unwrap();
    for part in ["module", "input", "output"] {
        let mut corrupted = objects.clone();
        let sha = match part {
            "module" => &module.sha256,
            "input" => &invocation.input.sha256,
            _ => &plan.manifest.authored_lineage[0].input.sha256,
        };
        corrupted.get_mut(sha).unwrap()[0] ^= 1;
        assert!(validate(&plan.manifest, &corrupted).is_err(), "{part}");
    }
    let mut forged = plan.manifest.clone();
    forged.authored_lineage[0]
        .wasm_invocation
        .as_mut()
        .unwrap()
        .approved_worker
        .sha256 = "b".repeat(64);
    assert!(forged.validate().is_err());
    let mut missing = plan.manifest.clone();
    missing
        .objects
        .retain(|o| !o.roles.contains(&PackageObjectRole::WasmModule));
    missing.counts = missing.record_counts().unwrap();
    assert!(missing.validate().is_err());
    let mut bytes = vec![];
    crate::project_package_format::write_package(
        &plan.manifest,
        &mut bytes,
        |object| plan.open_object(object),
        crate::project_package_format::PackageCodecLimits::default(),
    )
    .unwrap();
    drop(plan);
    let old = std::env::var_os("PULSAR_TEST_WASM_OLD_READER").map(PathBuf::from);
    crate::authority::project_package_imports::exercise_authored_wasm_portable_lifecycle(
        &bytes,
        &module,
        old.as_deref(),
    );
}

struct Fixture {
    engine: Arc<Engine>,
    session: SessionId,
    token: String,
    project: ProjectId,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let worker = std::env::var_os("PULSAR_TEST_WASM_WORKER")
            .expect("explicit authored worker path required");
        let dir = tempfile::tempdir().unwrap();
        let engine =
            Engine::open(EngineConfig::new(dir.path().join("state"), worker.into())).unwrap();
        let paired = engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "WASM editing qualification fixture".into(),
                    pairing_token: engine.token.clone(),
                },
            ))
            .result
            .unwrap();
        let ResponseBody::Paired {
            session,
            auth_token: token,
        } = paired
        else {
            panic!("pair")
        };
        let mut f = Self {
            engine,
            session,
            token,
            project: ProjectId::new("pending").unwrap(),
            _dir: dir,
        };
        let mut request = f.request(
            Command::CreateProject {
                name: "Authored WASM fixture".into(),
            },
            None,
        );
        request.project = None;
        let ResponseBody::Project(snapshot) = f.engine.handle(request).result.unwrap() else {
            panic!("project")
        };
        f.project = snapshot.project_id;
        f
    }
    fn request(&self, command: Command, revision: Option<u64>) -> Request {
        Request {
            version: PROTOCOL_VERSION,
            request_id: id!(RequestId),
            session: Some(self.session.clone()),
            auth_token: Some(self.token.clone()),
            project: Some(self.project.clone()),
            expected_revision: revision.map(RevisionId::new),
            command,
        }
    }
    fn spec(&self) -> WasmEditSpec {
        use pulsar_core::{
            EditValueAction, EditValueTrack, EditValuesProgram, NormalizedPosition, ProjectTime,
        };
        let values = EditValuesProgram::new(vec![EditValueTrack::new(
            Axis::Stroke,
            vec![
                EditValueAction::new(ProjectTime::ZERO, NormalizedPosition::new(0.2).unwrap())
                    .unwrap(),
                EditValueAction::new(
                    ProjectTime::from_nanos(1_000_000_000),
                    NormalizedPosition::new(0.8).unwrap(),
                )
                .unwrap(),
            ],
            vec![],
        )
        .unwrap()])
        .unwrap();
        let output = serde_json::to_vec(&values).unwrap();
        let escaped = output
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>();
        let module = wat::parse_str(format!(
            "(module (memory (export \"memory\") 1) (data (i32.const 0) \"{escaped}\")
            (func (export \"pulsar_alloc\") (param i32) (result i32) i32.const 4096)
            (func (export \"pulsar_process\") (param i32 i32) (result i64) i64.const {}))",
            output.len()
        ))
        .unwrap();
        WasmEditSpec {
            manifest: ExtensionManifest {
                schema_version: PROTOCOL_VERSION,
                name: "Authored editor".into(),
                role: ExtensionRole::Editor,
                artifact: ArtifactIdentity {
                    artifact_id: ArtifactId::new("authored-module").unwrap(),
                    sha256: hash(&module),
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
            expected_worker: self.engine.binary_identity.clone(),
            approve_execution: true,
            label: "Authored values".into(),
        }
    }
    fn status(&self, id: &AttemptId) -> WasmEditReport {
        let response = self
            .engine
            .handle(self.request(
                Command::WasmEditStatus {
                    attempt_id: id.clone(),
                },
                None,
            ))
            .result
            .unwrap();
        let ResponseBody::WasmEdit(report) = response else {
            panic!("status")
        };
        *report
    }
    fn wait(&self, id: &AttemptId) -> WasmEditReport {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let report = self.status(id);
            if report.state != WasmEditState::Running {
                return report;
            }
            assert!(Instant::now() < deadline, "operation never terminated");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn retired(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.engine.workers.lock().unwrap().is_empty()
            || self
                .engine
                .pool
                .reserve(self.engine.config.max_ram_bytes)
                .is_err()
        {
            assert!(
                Instant::now() < deadline,
                "supervisor retained worker custody"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
#[ignore = "requires an explicitly delegated private Linux cgroup and authored WASM worker"]
fn authenticated_editor_candidate_replay_undo_and_late_cancellation() {
    let f = Fixture::new();
    for field in ["approval", "module", "worker", "revision"] {
        let mut spec = f.spec();
        match field {
            "approval" => spec.approve_execution = false,
            "module" => spec.manifest.artifact.sha256 = "b".repeat(64),
            "worker" => spec.expected_worker.sha256 = "b".repeat(64),
            _ => {}
        }
        assert!(
            f.engine
                .handle(f.request(
                    Command::StartWasmEdit { spec },
                    Some(if field == "revision" { 1 } else { 0 })
                ))
                .result
                .is_err(),
            "{field}"
        );
    }
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM wasm_edits", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let ResponseBody::Paired {
        session: reader,
        auth_token: reader_token,
    } = f
        .engine
        .handle(Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: "read-only client".into(),
                pairing_token: f.engine.token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("pair")
    };
    let mut grant = f.request(
        Command::Grant {
            session: reader.clone(),
            project_id: f.project.clone(),
            scopes: vec![Scope::Read],
        },
        None,
    );
    grant.project = None;
    f.engine.handle(grant).result.unwrap();
    let mut denied = f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0));
    denied.session = Some(reader.clone());
    denied.auth_token = Some(reader_token.clone());
    assert_eq!(
        f.engine.handle(denied).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let start = f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0));
    let ResponseBody::WasmEdit(accepted) = f.engine.handle(start.clone()).result.unwrap() else {
        panic!("start")
    };
    accepted.validate_for(&start).unwrap();
    let mut denied = f.request(
        Command::WasmEditStatus {
            attempt_id: accepted.attempt_id.clone(),
        },
        None,
    );
    denied.session = Some(reader);
    denied.auth_token = Some(reader_token);
    assert_eq!(
        f.engine.handle(denied).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let completed = f.wait(&accepted.attempt_id);
    assert_eq!(
        completed.state,
        WasmEditState::Completed,
        "{:?}",
        completed.error
    );
    completed.validate_for(&start).unwrap();
    assert_eq!(
        completed.receipt.as_ref().unwrap().worker,
        f.engine.binary_identity
    );
    let ResponseBody::Project(before) = f
        .engine
        .handle(f.request(Command::GetSnapshot, None))
        .result
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(
        before.revision,
        RevisionId::new(0),
        "a plugin may only propose a candidate"
    );
    let ResponseBody::WasmEdit(replay) = f.engine.handle(start.clone()).result.unwrap() else {
        panic!("replay")
    };
    assert_eq!(replay.attempt_id, accepted.attempt_id);
    let mut conflict = start.clone();
    let Command::StartWasmEdit { spec } = &mut conflict.command else {
        unreachable!()
    };
    spec.label.push('x');
    assert_eq!(
        f.engine.handle(conflict).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    f.engine
        .handle(f.request(
            Command::CommitCandidate {
                candidate_id: completed.candidate_id.unwrap(),
            },
            Some(0),
        ))
        .result
        .unwrap();
    let ResponseBody::Project(after) = f
        .engine
        .handle(f.request(Command::GetSnapshot, None))
        .result
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(after.revision, RevisionId::new(1));
    let program = f.engine.motion_store.read(&after.motion.program).unwrap();
    assert!(program
        .tracks()
        .iter()
        .flat_map(|t| t.actions())
        .all(|a| a.evidence() == EvidenceKind::Synthesized));
    f.engine
        .handle(f.request(Command::Undo, Some(1)))
        .result
        .unwrap();
    let ResponseBody::Project(undone) = f
        .engine
        .handle(f.request(Command::GetSnapshot, None))
        .result
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(undone.motion.program, before.motion.program);
    f.retired();

    let start = f.request(Command::StartWasmEdit { spec: f.spec() }, Some(2));
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    ADMISSION_HOOKS.lock().unwrap().insert(
        start.request_id.to_string(),
        Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        }),
    );
    let ResponseBody::WasmEdit(accepted) = f.engine.handle(start).result.unwrap() else {
        panic!("start")
    };
    entered_rx.recv_timeout(Duration::from_secs(90)).unwrap();
    f.engine
        .handle(f.request(
            Command::CancelWasmEdit {
                attempt_id: accepted.attempt_id.clone(),
            },
            None,
        ))
        .result
        .unwrap();
    release_tx.send(()).unwrap();
    f.retired();
    let cancelled = f.status(&accepted.attempt_id);
    assert_eq!(cancelled.state, WasmEditState::Cancelled);
    assert!(cancelled.candidate_id.is_none());
    let count: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "late cancellation must not admit a second candidate"
    );

    let ResponseBody::Paired {
        session: admin,
        auth_token: admin_token,
    } = f
        .engine
        .handle(Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: "late admission administrator".into(),
                pairing_token: f.engine.token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("pair")
    };
    let mut grant = f.request(
        Command::Grant {
            session: admin.clone(),
            project_id: f.project.clone(),
            scopes: vec![Scope::Read, Scope::Edit, Scope::ManageGrants],
        },
        None,
    );
    grant.project = None;
    f.engine.handle(grant).result.unwrap();
    for mutation in ["revision", "revoke-and-restore"] {
        let ResponseBody::Project(snapshot) = f
            .engine
            .handle(f.request(Command::GetSnapshot, None))
            .result
            .unwrap()
        else {
            panic!("snapshot")
        };
        let start = f.request(
            Command::StartWasmEdit { spec: f.spec() },
            Some(snapshot.revision.value()),
        );
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        ADMISSION_HOOKS.lock().unwrap().insert(
            start.request_id.to_string(),
            Box::new(move || {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            }),
        );
        let ResponseBody::WasmEdit(accepted) = f.engine.handle(start).result.unwrap() else {
            panic!("start")
        };
        entered_rx.recv_timeout(Duration::from_secs(90)).unwrap();
        if mutation == "revision" {
            f.engine
                .handle(f.request(Command::Redo, Some(snapshot.revision.value())))
                .result
                .unwrap();
        } else {
            for command in [
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
                Command::Grant {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Edit],
                },
            ] {
                let mut request = f.request(command, None);
                request.project = None;
                request.session = Some(admin.clone());
                request.auth_token = Some(admin_token.clone());
                f.engine.handle(request).result.unwrap();
            }
        }
        release_tx.send(()).unwrap();
        f.retired();
        let rejected = f.status(&accepted.attempt_id);
        assert!(rejected.candidate_id.is_none(), "{mutation}");
        assert_eq!(
            rejected.state,
            if mutation == "revision" {
                WasmEditState::Failed
            } else {
                WasmEditState::Cancelled
            }
        );
    }
    let count: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "revision changes and restored grants cannot admit late candidates"
    );
}

#[test]
#[ignore = "requires an explicitly delegated private Linux cgroup and authored WASM worker"]
fn unsuccessful_catalog_cleanup_preserves_authority_retirement_and_replay() {
    let f = Fixture::new();
    let mut spec = f.spec();
    spec.module=wat::parse_str("(module (memory (export \"memory\") 1) (func (export \"pulsar_alloc\") (param i32) (result i32) i32.const 4096) (func (export \"pulsar_process\") (param i32 i32) (result i64) unreachable))").unwrap();
    spec.manifest.artifact.sha256 = hash(&spec.module);
    spec.manifest.artifact.byte_len = spec.module.len() as u64;
    let start = f.request(Command::StartWasmEdit { spec }, Some(0));
    let ResponseBody::WasmEdit(accepted) = f.engine.handle(start.clone()).result.unwrap() else {
        panic!("start")
    };
    let failed = f.wait(&accepted.attempt_id);
    assert_eq!(failed.state, WasmEditState::Failed, "{:?}", failed.error);
    assert!(
        failed
            .error
            .as_ref()
            .is_some_and(|error| error.message.contains("WASM worker exited")
                && error.message.contains("unreachable")),
        "real guest trap not established: {:?}",
        failed.error
    );
    f.retired();
    let list = f.request(Command::ListWasmEdits, None);
    let ResponseBody::WasmEditCatalog(catalog) = f.engine.handle(list.clone()).result.unwrap()
    else {
        panic!("catalog")
    };
    catalog.validate_for(&list).unwrap();
    assert_eq!(catalog.operations.len(), 1);
    assert_eq!(catalog.operations[0].request_id, start.request_id);
    let removal = f.request(
        Command::RemoveUnsuccessfulWasmEdit {
            attempt_id: failed.attempt_id.clone(),
            original_request_id: start.request_id.clone(),
            expected_state: failed.state,
        },
        None,
    );
    let key = super::key(&read_plan(&f.engine.db.lock().unwrap(), &failed.attempt_id).unwrap());
    f.engine
        .workers
        .lock()
        .unwrap()
        .insert(key.clone(), Arc::new(AtomicBool::new(false)));
    assert!(f.engine.handle(removal.clone()).result.is_err());
    f.engine.workers.lock().unwrap().remove(&key);
    let mut stale = removal.clone();
    stale.request_id = id!(RequestId);
    if let Command::RemoveUnsuccessfulWasmEdit {
        original_request_id,
        ..
    } = &mut stale.command
    {
        *original_request_id = id!(RequestId);
    }
    assert!(f.engine.handle(stale).result.is_err());
    // A second paired session gets Read only, sees no foreign operation, and cannot remove it.
    let ResponseBody::Paired {
        session: reader,
        auth_token: reader_token,
    } = f
        .engine
        .handle(Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: "catalog reader".into(),
                pairing_token: f.engine.token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("pair")
    };
    let mut grant = f.request(
        Command::Grant {
            session: reader.clone(),
            project_id: f.project.clone(),
            scopes: vec![Scope::Read],
        },
        None,
    );
    grant.project = None;
    f.engine.handle(grant).result.unwrap();
    let mut foreign = list.clone();
    foreign.request_id = id!(RequestId);
    foreign.session = Some(reader.clone());
    foreign.auth_token = Some(reader_token.clone());
    let ResponseBody::WasmEditCatalog(empty) = f.engine.handle(foreign).result.unwrap() else {
        panic!("foreign catalog")
    };
    assert!(empty.operations.is_empty());
    let mut foreign = removal.clone();
    foreign.request_id = id!(RequestId);
    foreign.session = Some(reader.clone());
    foreign.auth_token = Some(reader_token.clone());
    assert_eq!(
        f.engine.handle(foreign.clone()).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let mut grant = f.request(
        Command::Grant {
            session: reader,
            project_id: f.project.clone(),
            scopes: vec![Scope::Read, Scope::Edit],
        },
        None,
    );
    grant.project = None;
    f.engine.handle(grant).result.unwrap();
    assert_eq!(
        f.engine.handle(foreign).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    // Capacity boundary uses 63 synthetic terminal rows plus one real failed worker.
    // This does not claim 64 separate real executions.
    for i in 0..63 {
        f.engine.db.lock().unwrap().execute("INSERT INTO wasm_edits SELECT ?1,project,session,base_revision,engine_epoch,?2,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,state,candidate,error,receipt,output FROM wasm_edits WHERE id=?3",params![format!("synthetic-failed-{i}"),format!("synthetic-request-{i}"),failed.attempt_id.as_str()]).unwrap();
    }
    assert_eq!(
        f.engine
            .handle(f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0)))
            .result
            .unwrap_err()
            .code,
        ErrorCode::ResourceExhausted
    );
    let before = motion_state::revision_descriptor(
        &f.engine.db.lock().unwrap(),
        &f.project,
        RevisionId::new(0),
    )
    .unwrap();
    let ResponseBody::WasmEditRemoved(removed) = f.engine.handle(removal.clone()).result.unwrap()
    else {
        panic!("removal")
    };
    removed.validate_removal_for(&removal).unwrap();
    assert!(matches!(
        f.engine.handle(removal).result.unwrap(),
        ResponseBody::WasmEditRemoved(_)
    ));
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM wasm_edits", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        63
    );
    let ResponseBody::WasmEdit(replayed) = f.engine.handle(start.clone()).result.unwrap() else {
        panic!("original replay")
    };
    assert_eq!(replayed.attempt_id, accepted.attempt_id);
    f.retired();
    assert_eq!(
        motion_state::revision_descriptor(
            &f.engine.db.lock().unwrap(),
            &f.project,
            RevisionId::new(0)
        )
        .unwrap(),
        before
    );
    // New approved work can reuse the freed slot; completed candidate facts cannot be removed.
    let ResponseBody::WasmEdit(accepted) = f
        .engine
        .handle(f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0)))
        .result
        .unwrap()
    else {
        panic!("next")
    };
    let completed = f.wait(&accepted.attempt_id);
    assert_eq!(
        completed.state,
        WasmEditState::Completed,
        "{:?}",
        completed.error
    );
    f.retired();
    let refusal = f.request(
        Command::RemoveUnsuccessfulWasmEdit {
            attempt_id: completed.attempt_id.clone(),
            original_request_id: completed.request_id,
            expected_state: WasmEditState::Failed,
        },
        None,
    );
    assert!(f.engine.handle(refusal).result.is_err());
    let plan = crate::authority::project_packages::capture(
        &f.engine,
        &f.session,
        &f.token,
        &f.project,
        RevisionId::new(0),
    )
    .unwrap();
    assert!(plan.manifest.authored_lineage[0].wasm_invocation.is_some());
    drop(plan);
    let config = f.engine.config.clone();
    let list = f.request(Command::ListWasmEdits, None);
    drop(f.engine);
    let restarted = Engine::open(config).unwrap();
    let ResponseBody::WasmEditCatalog(catalog) = restarted.handle(list.clone()).result.unwrap()
    else {
        panic!("restart catalog")
    };
    catalog.validate_for(&list).unwrap();
    assert_eq!(catalog.operations.len(), 64);
    assert!(!catalog
        .operations
        .iter()
        .any(|op| op.attempt_id == failed.attempt_id));
    let ResponseBody::WasmEdit(replayed) = restarted.handle(start).result.unwrap() else {
        panic!("restart replay")
    };
    assert_eq!(replayed.attempt_id, failed.attempt_id);
    assert!(restarted.workers.lock().unwrap().is_empty());
}

#[path = "wasm_archive_tests.rs"]
mod archive_tests;

#[path = "wasm_start_resolution_tests.rs"]
mod start_resolution_tests;
