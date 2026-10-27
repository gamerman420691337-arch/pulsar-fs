use super::*;
fn binding() -> Binding {
    (ProjectId::new("project").unwrap(), RevisionId::new(0))
}
fn info() -> WasmEditorInfo {
    WasmEditorInfo {
        worker: ArtifactIdentity {
            artifact_id: ArtifactId::new("worker").unwrap(),
            sha256: "b".repeat(64),
            byte_len: 1,
        },
        supported: true,
        abi: WASM_PROCESSING_ABI.into(),
        maximum_buffer_bytes: WASM_BUFFER_MAX_BYTES as u64,
        native_runtime_qualified: false,
    }
}
fn spec() -> WasmEditSpec {
    let module = b"\0asm\x01\0\0\0".to_vec();
    WasmEditSpec {
        manifest: ExtensionManifest {
            schema_version: PROTOCOL_VERSION,
            name: "fixture".into(),
            role: ExtensionRole::Editor,
            artifact: ArtifactIdentity {
                artifact_id: ArtifactId::new("module").unwrap(),
                sha256: format!("{:x}", Sha256::digest(&module)),
                byte_len: module.len() as u64,
            },
            abi: WASM_PROCESSING_ABI.into(),
            requested_scopes: vec![],
            confinement: ConfinementRequirement::Wasm,
            budget: ResourceBudget {
                memory_bytes: 256 * 1024 * 1024,
                output_bytes: WASM_BUFFER_MAX_BYTES as u64,
                wall_time_ms: 5000,
                cpu_threads: 2,
            },
        },
        module,
        guest_memory_bytes: 65536,
        expected_worker: info().worker,
        approve_execution: false,
        label: "proposal".into(),
    }
}
fn ready() -> State {
    State {
        binding: Some(binding()),
        info: Some(info()),
        draft: Some(Draft {
            binding: binding(),
            spec: spec(),
        }),
        ..Default::default()
    }
}
fn report(request_id: RequestId) -> WasmEditReport {
    WasmEditReport {
        attempt_id: AttemptId::new("attempt").unwrap(),
        request_id,
        project_id: binding().0,
        base_revision: binding().1,
        engine_epoch: "epoch".into(),
        state: WasmEditState::Running,
        candidate_id: None,
        receipt: None,
        error: None,
    }
}
#[test]
fn approval_is_fresh_and_bound_to_captured_bytes_revision_and_worker() {
    let mut state = ready();
    assert!(state.intent().is_err());
    state.approved = true;
    let intent = state.intent().unwrap();
    assert!(intent.spec.approve_execution);
    state.pending = Some(intent);
    assert!(state.intent().is_err());
    state.bind(Some((binding().0, RevisionId::new(1))));
    assert!(!state.approved && state.draft.is_none() && state.pending.is_some());
    assert!(state
        .accept(
            &binding(),
            Output::Draft(Box::new(Draft {
                binding: binding(),
                spec: spec()
            }))
        )
        .is_err());
    let mut state = ready();
    state.approved = true;
    let mut changed = info();
    changed.worker.sha256 = "c".repeat(64);
    state.accept(&binding(), Output::Info(changed)).unwrap();
    assert!(state.draft.is_none() && !state.approved);
    let mut state = ready();
    state.approved = true;
    state.invalidate_draft();
    assert!(state.intent().is_err());
}
#[test]
fn lost_acknowledgement_stays_pending_until_exact_catalog_correspondence() {
    let mut state = ready();
    state.approved = true;
    let intent = state.intent().unwrap();
    state.pending = Some(intent.clone());
    state
        .accept(
            &binding(),
            Output::Catalog(WasmEditCatalog {
                project_id: binding().0,
                operations: vec![],
            }),
        )
        .unwrap();
    assert!(state.pending.is_some());
    let mut wrong = report(intent.request_id.clone());
    wrong.base_revision = RevisionId::new(3);
    assert!(state
        .accept(
            &binding(),
            Output::Catalog(WasmEditCatalog {
                project_id: binding().0,
                operations: vec![wrong]
            })
        )
        .is_err());
    assert!(state.pending.is_some());
    state
        .accept(
            &binding(),
            Output::Catalog(WasmEditCatalog {
                project_id: binding().0,
                operations: vec![report(intent.request_id)],
            }),
        )
        .unwrap();
    assert!(state.pending.is_none());
    assert_eq!(state.selected.unwrap().as_str(), "attempt");
}
#[test]
fn declaration_capture_refuses_drift_oversize_and_nonregular_files() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    let manifest = dir.path().join("manifest");
    let spec = spec();
    std::fs::write(&module, &spec.module).unwrap();
    std::fs::write(&manifest, serde_json::to_vec(&spec.manifest).unwrap()).unwrap();
    let captured = load(
        &binding(),
        &module,
        &manifest,
        info().worker,
        65536,
        "proposal".into(),
    )
    .unwrap();
    assert!(!captured.spec.approve_execution);
    std::fs::write(&module, b"changed").unwrap();
    assert_eq!(captured.spec.module, spec.module);
    assert!(load(
        &binding(),
        &module,
        &manifest,
        info().worker,
        65536,
        "proposal".into()
    )
    .is_err());
    assert!(read(dir.path(), 4).is_err());
    std::fs::write(&module, [1, 2, 3, 4, 5]).unwrap();
    assert!(read(&module, 4).is_err());
    #[cfg(unix)]
    {
        let fifo = dir.path().join("fifo");
        let result = std::process::Command::new("timeout")
            .args(["5s", "mkfifo"])
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(result.success());
        assert!(read(&fifo, 4).is_err());
    }
}
struct Api {
    body: Option<ResponseBody>,
    calls: Vec<(RequestId, Command, Option<ProjectId>, Option<RevisionId>)>,
}
impl EngineApi for Api {
    fn execute(
        &mut self,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<ResponseBody> {
        self.execute_identified(crate::session::new_request_id(), command, project, revision)
    }
    fn execute_identified(
        &mut self,
        id: RequestId,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<ResponseBody> {
        self.calls.push((id, command, project, revision));
        self.body
            .take()
            .ok_or_else(|| anyhow::anyhow!("authored unknown response"))
    }
}
#[test]
fn controller_preserves_identified_intent_and_refuses_foreign_status() {
    let mut state = ready();
    state.approved = true;
    let intent = state.intent().unwrap();
    let mut api = Api {
        body: Some(ResponseBody::WasmEdit(
            (report(intent.request_id.clone())).into(),
        )),
        calls: vec![],
    };
    query(&mut api, &binding(), Action::Start(intent.clone())).unwrap();
    assert_eq!(api.calls[0].0, intent.request_id);
    assert_eq!(api.calls[0].3, Some(binding().1));
    assert!(matches!(api.calls[0].1, Command::StartWasmEdit { .. }));
    let mut foreign = report(crate::session::new_request_id());
    foreign.project_id = ProjectId::new("other").unwrap();
    api.body = Some(ResponseBody::WasmEdit((foreign).into()));
    assert!(query(
        &mut api,
        &binding(),
        Action::Status(AttemptId::new("attempt").unwrap())
    )
    .is_err());
    let other = (ProjectId::new("other").unwrap(), RevisionId::new(0));
    assert!(query(&mut api, &other, Action::Start(intent)).is_err());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an explicit qualification engine, WASM worker and private delegated Linux cgroup"]
fn desktop_controller_crosses_actual_authenticated_ipc_and_reviews_guest_candidate() {
    use std::time::{Duration, Instant};
    let engine =
        std::env::var_os("PULSAR_TEST_WASM_ENGINE").expect("explicit qualification host required");
    let worker =
        std::env::var_os("PULSAR_TEST_WASM_WORKER").expect("explicit authored worker required");
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    let module = dir.path().join("module.wasm");
    assert!(std::process::Command::new("timeout")
        .arg("15s")
        .arg(&engine)
        .arg("--emit-module")
        .arg(&module)
        .status()
        .unwrap()
        .success());
    struct Host(std::process::Child);
    impl Drop for Host {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let log = std::fs::File::create(dir.path().join("engine.log")).unwrap();
    let mut host = Host(
        std::process::Command::new(&engine)
            .arg(&state_dir)
            .arg(&worker)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let endpoint = state_dir.join("engine.sock");
    let bootstrap = state_dir.join("pairing.token");
    while !endpoint.exists() || !bootstrap.exists() {
        assert!(
            host.0.try_wait().unwrap().is_none(),
            "qualification host exited"
        );
        assert!(Instant::now() < deadline, "host readiness expired");
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    let ResponseBody::Project(project) = client
        .execute(
            Command::CreateProject {
                name: "Desktop WASM controller fixture".into(),
            },
            None,
            None,
        )
        .unwrap()
    else {
        panic!("project")
    };
    let binding = (project.project_id.clone(), project.revision);
    let original = project.motion.clone();
    let mut state = State::default();
    state.bind(Some(binding.clone()));
    state
        .accept(
            &binding,
            query(&mut client, &binding, Action::Info).unwrap(),
        )
        .unwrap();
    assert!(state.info.as_ref().unwrap().supported);
    let bytes = std::fs::read(&module).unwrap();
    let mut fixture = spec();
    fixture.module = bytes;
    fixture.manifest.artifact.sha256 = format!("{:x}", Sha256::digest(&fixture.module));
    fixture.manifest.artifact.byte_len = fixture.module.len() as u64;
    fixture.expected_worker = state.info.as_ref().unwrap().worker.clone();
    let manifest = dir.path().join("manifest.json");
    std::fs::write(&manifest, serde_json::to_vec(&fixture.manifest).unwrap()).unwrap();
    state
        .accept(
            &binding,
            query(
                &mut client,
                &binding,
                Action::Load {
                    module,
                    manifest,
                    worker: fixture.expected_worker,
                    memory: 65536,
                    label: "Desktop authored candidate".into(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    assert!(state.intent().is_err());
    state.approved = true;
    let intent = state.intent().unwrap();
    // Persist before dispatch, then model a lost local acknowledgement with a fresh client/state.
    let session = client.session_id().clone();
    let credential = client.recovery_credential_binding();
    let metadata = crate::wasm_recovery::Intent::capture(
        binding.0.clone(),
        binding.1,
        intent.request_id.clone(),
        &intent.spec,
    )
    .unwrap();
    crate::wasm_recovery::prepare(&state_dir, &endpoint, &session, &credential, &metadata).unwrap();
    let started = query(&mut client, &binding, Action::Start(intent.clone())).unwrap();
    client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    assert_eq!(client.session_id(), &session);
    assert_eq!(client.recovery_credential_binding(), credential);
    let restored =
        crate::wasm_recovery::load(&state_dir, &endpoint, &session, &credential).unwrap();
    assert!(restored[0].attempt.is_none());
    let mut restarted = State::default();
    restarted.bind(Some(binding.clone()));
    restarted.restore(restored);
    assert!(!restarted.approved && restarted.draft.is_none() && restarted.pending.is_none());
    assert!(restarted.recovered_intent(&restarted.recovery[0]).is_err());
    execute_stored(&mut client, &state_dir, &endpoint, &binding, Action::List).unwrap();
    let reconciled =
        crate::wasm_recovery::load(&state_dir, &endpoint, &session, &credential).unwrap();
    assert!(reconciled[0].attempt.is_some());
    state.pending = Some(intent.clone());
    state.accept(&binding, started).unwrap();
    assert!(state.pending.is_none());
    let attempt = state.selected.clone().unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        state
            .accept(
                &binding,
                query(&mut client, &binding, Action::Status(attempt.clone())).unwrap(),
            )
            .unwrap();
        let report = state
            .reports
            .iter()
            .find(|r| r.attempt_id == attempt)
            .unwrap();
        if report.state != WasmEditState::Running {
            assert_eq!(report.state, WasmEditState::Completed, "{:?}", report.error);
            break;
        }
        assert!(Instant::now() < deadline, "guest never completed");
        std::thread::sleep(Duration::from_millis(20));
    }
    state.pending = Some(intent.clone());
    state
        .accept(
            &binding,
            query(&mut client, &binding, Action::List).unwrap(),
        )
        .unwrap();
    assert!(state.pending.is_none());
    let candidate_id = state
        .reports
        .iter()
        .find(|r| r.attempt_id == attempt)
        .unwrap()
        .candidate_id
        .clone()
        .unwrap();
    let ResponseBody::Candidate(candidate) = client
        .execute(
            Command::GetCandidate { candidate_id },
            Some(binding.0.clone()),
            None,
        )
        .unwrap()
    else {
        panic!("candidate")
    };
    assert!(!candidate.program.tracks().is_empty());
    let ResponseBody::Project(after) = client
        .execute(Command::GetSnapshot, Some(binding.0.clone()), None)
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(after.revision, binding.1);
    assert_eq!(after.motion, original);
    // An identical invocation is reconciled rather than executed again; terminal cancel changes no candidate.
    state.pending = Some(intent.clone());
    state
        .accept(
            &binding,
            execute_stored(
                &mut client,
                &state_dir,
                &endpoint,
                &binding,
                Action::Start(intent.clone()),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(state.selected, Some(attempt.clone()));
    // Private-storage failure must precede any new engine dispatch.
    use std::os::unix::fs::PermissionsExt;
    let journal = state_dir.join("desktop-wasm-recovery").join(format!(
        "{:x}.json",
        Sha256::digest(session.as_str().as_bytes())
    ));
    let original_journal = std::fs::read(&journal).unwrap();
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut refused = intent.clone();
    refused.request_id = crate::session::new_request_id();
    assert!(execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::Start(refused)
    )
    .is_err());
    assert_eq!(std::fs::read(&journal).unwrap(), original_journal);
    let Output::Catalog(catalog) = query(&mut client, &binding, Action::List).unwrap() else {
        panic!("catalog")
    };
    assert_eq!(catalog.operations.len(), 1);
    assert_eq!(catalog.operations[0].attempt_id, attempt);
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    state
        .accept(
            &binding,
            query(
                &mut client,
                &binding,
                Action::Cancel {
                    request_id: crate::session::new_request_id(),
                    attempt,
                },
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(state.reports[0].state, WasmEditState::Completed);
    assert!(RemovalIntent::capture(&binding, &state.reports[0]).is_err());
    // Real guest failure, custody retirement and acknowledged cleanup/replay.
    let trap = dir.path().join("trap.wasm");
    assert!(std::process::Command::new("timeout")
        .arg("15s")
        .arg(&engine)
        .arg("--emit-trap")
        .arg(&trap)
        .status()
        .unwrap()
        .success());
    let mut failed_spec = state.draft.as_ref().unwrap().spec.clone();
    failed_spec.module = std::fs::read(&trap).unwrap();
    failed_spec.manifest.artifact.sha256 = format!("{:x}", Sha256::digest(&failed_spec.module));
    failed_spec.manifest.artifact.byte_len = failed_spec.module.len() as u64;
    failed_spec.approve_execution = true;
    let failed_intent = StartIntent {
        binding: binding.clone(),
        request_id: crate::session::new_request_id(),
        spec: failed_spec,
    };
    state.pending = Some(failed_intent.clone());
    let started = execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::Start(failed_intent),
    )
    .unwrap();
    state.accept(&binding, started).unwrap();
    let failed_attempt = state.selected.clone().unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let failed = loop {
        let Output::Report(report) = query(
            &mut client,
            &binding,
            Action::Status(failed_attempt.clone()),
        )
        .unwrap() else {
            panic!("status")
        };
        if report.state != WasmEditState::Running {
            break report;
        }
        assert!(Instant::now() < deadline, "trap completion expired");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(failed.state, WasmEditState::Failed, "{:?}", failed.error);
    assert!(
        failed.error.as_ref().is_some_and(
            |e| e.message.contains("unreachable") && e.message.contains("WASM worker exited")
        ),
        "{:?}",
        failed.error
    );
    state.update(*failed.clone());
    let removal = RemovalIntent::capture(&binding, &failed).unwrap();
    crate::wasm_cleanup::prepare(&state_dir, &endpoint, &session, &credential, &removal).unwrap();
    let cleanup_file = state_dir.join("desktop-wasm-cleanup").join(format!(
        "{:x}.json",
        Sha256::digest(session.as_str().as_bytes())
    ));
    let original_cleanup = std::fs::read(&cleanup_file).unwrap();
    std::fs::set_permissions(&cleanup_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::Remove(removal.clone())
    )
    .is_err());
    assert_eq!(std::fs::read(&cleanup_file).unwrap(), original_cleanup);
    let Output::Catalog(before_cleanup) = query(&mut client, &binding, Action::List).unwrap()
    else {
        panic!("catalog")
    };
    assert_eq!(before_cleanup.operations.len(), 2);
    assert!(before_cleanup
        .operations
        .iter()
        .any(|r| r.attempt_id == failed_attempt));
    std::fs::set_permissions(&cleanup_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // Execute the checked engine request but deliberately omit journal acknowledgement publication.
        match query(&mut client, &binding, Action::Remove(removal.clone())) {
            Ok(Output::Removed { .. }) => break,
            Ok(_) => panic!("cleanup"),
            Err(error) => {
                let message = error.to_string();
                assert!(
                    message.contains("custody") || message.contains("retirement"),
                    "{message}"
                );
                assert!(
                    Instant::now() < deadline,
                    "cleanup custody retirement expired"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    assert_eq!(client.session_id(), &session);
    assert_eq!(client.recovery_credential_binding(), credential);
    state = State::default();
    state.bind(Some(binding.clone()));
    let restored = crate::wasm_cleanup::load(&state_dir, &endpoint, &session, &credential).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].intent, removal);
    assert!(!restored[0].acknowledged);
    state.restore_cleanup(restored);
    assert!(
        !state.approved
            && state.pending_removal.is_none()
            && state.pending.is_none()
            && state.draft.is_none()
    );
    let catalog =
        execute_stored(&mut client, &state_dir, &endpoint, &binding, Action::List).unwrap();
    state.accept(&binding, catalog).unwrap();
    assert!(
        !crate::wasm_cleanup::load(&state_dir, &endpoint, &session, &credential).unwrap()[0]
            .acknowledged
    );
    assert_eq!(state.reports.len(), 1);
    assert_eq!(state.reports[0].state, WasmEditState::Completed);
    // Explicit retry alone installs the in-memory pending intent; authority is checked again by Engine.
    state.pending_removal = Some(removal.clone());
    let replay = execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::Remove(removal.clone()),
    )
    .unwrap();
    state.accept(&binding, replay).unwrap();
    assert!(state.pending_removal.is_none());
    assert!(
        crate::wasm_cleanup::load(&state_dir, &endpoint, &session, &credential).unwrap()[0]
            .acknowledged
    );
    execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::DismissCleanup(removal.request_id),
    )
    .unwrap();
    assert!(
        crate::wasm_cleanup::load(&state_dir, &endpoint, &session, &credential)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crate::wasm_recovery::load(&state_dir, &endpoint, &session, &credential)
            .unwrap()
            .len(),
        2
    );
    let ResponseBody::Project(final_project) = client
        .execute(Command::GetSnapshot, Some(binding.0.clone()), None)
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(final_project.revision, binding.1);
    assert_eq!(final_project.motion, original);
    let completed = state.reports[0].clone();
    let archive = ArchiveIntent::capture(&binding, &completed).unwrap();
    crate::wasm_archive_recovery::prepare(&state_dir, &endpoint, &session, &credential, &archive)
        .unwrap();
    let archive_file = state_dir.join("desktop-wasm-archive").join(format!(
        "{:x}.json",
        Sha256::digest(session.as_str().as_bytes())
    ));
    let original_archive = std::fs::read(&archive_file).unwrap();
    std::fs::set_permissions(&archive_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::Archive(Box::new(archive.clone()))
    )
    .is_err());
    assert_eq!(std::fs::read(&archive_file).unwrap(), original_archive);
    let Output::Catalog(before_archive) = query(&mut client, &binding, Action::List).unwrap()
    else {
        panic!("catalog")
    };
    assert_eq!(before_archive.operations.len(), 1);
    std::fs::set_permissions(&archive_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    // Execute archival but deliberately lose publication of the local acknowledgement.
    let Output::Archived {
        report: archived, ..
    } = query(
        &mut client,
        &binding,
        Action::Archive((archive.clone()).into()),
    )
    .unwrap()
    else {
        panic!("archive IPC")
    };
    assert_eq!(archived.attempt_id, completed.attempt_id);
    client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    assert_eq!(client.session_id(), &session);
    assert_eq!(client.recovery_credential_binding(), credential);
    state = State::default();
    state.bind(Some(binding.clone()));
    let restored =
        crate::wasm_archive_recovery::load(&state_dir, &endpoint, &session, &credential).unwrap();
    assert_eq!(restored.len(), 1);
    assert!(!restored[0].acknowledged && restored[0].intent.matches(&archive).unwrap());
    state.restore_archival(restored);
    assert!(
        state.pending_archive.is_none()
            && state.pending.is_none()
            && state.pending_removal.is_none()
            && state.selected.is_none()
            && state.draft.is_none()
            && !state.approved
    );
    let catalog =
        execute_stored(&mut client, &state_dir, &endpoint, &binding, Action::List).unwrap();
    state.accept(&binding, catalog).unwrap();
    assert!(state.reports.is_empty());
    assert!(
        !crate::wasm_archive_recovery::load(&state_dir, &endpoint, &session, &credential).unwrap()
            [0]
        .acknowledged
    );
    state.pending_archive = Some(archive.clone());
    let replay = execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::Archive((archive.clone()).into()),
    )
    .unwrap();
    state.accept(&binding, replay).unwrap();
    assert!(state.pending_archive.is_none());
    state.restore_archival(
        crate::wasm_archive_recovery::load(&state_dir, &endpoint, &session, &credential).unwrap(),
    );
    assert!(state.archival[0].acknowledged);
    assert_eq!(
        state.archival[0].intent.candidate,
        completed.candidate_id.clone().unwrap()
    );
    let Output::Report(retained) =
        query(&mut client, &binding, Action::Status(completed.attempt_id)).unwrap()
    else {
        panic!("retained status")
    };
    assert_eq!(retained.state, WasmEditState::Completed);
    let ResponseBody::Candidate(candidate) = client
        .execute(
            Command::GetCandidate {
                candidate_id: retained.candidate_id.unwrap(),
            },
            Some(binding.0.clone()),
            None,
        )
        .unwrap()
    else {
        panic!("retained candidate")
    };
    assert!(!candidate.program.tracks().is_empty());
    execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::DismissArchive(archive.request_id),
    )
    .unwrap();
    assert!(
        crate::wasm_archive_recovery::load(&state_dir, &endpoint, &session, &credential)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crate::wasm_recovery::load(&state_dir, &endpoint, &session, &credential)
            .unwrap()
            .len(),
        2
    );
    // History remains discoverable after all local archive recovery facts are dismissed.
    state.history_pending = Some(None);
    let history = query(&mut client, &binding, Action::ListArchived(None)).unwrap();
    state.accept(&binding, history).unwrap();
    let history = state.history.as_ref().unwrap();
    assert_eq!(history.operations.len(), 1);
    assert!(history.next.is_none());
    assert_eq!(history.operations[0].request_id, completed.request_id);
    assert_eq!(
        serde_json::to_vec(&history.operations[0].receipt).unwrap(),
        serde_json::to_vec(&completed.receipt).unwrap()
    );
    let ResponseBody::Candidate(historical_candidate) = client
        .execute(
            Command::GetCandidate {
                candidate_id: history.operations[0].candidate_id.clone().unwrap(),
            },
            Some(binding.0.clone()),
            None,
        )
        .unwrap()
    else {
        panic!("history candidate");
    };
    assert!(!historical_candidate.program.tracks().is_empty());
    let ResponseBody::Project(after_archive) = client
        .execute(Command::GetSnapshot, Some(binding.0.clone()), None)
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(after_archive.revision, binding.1);
    assert_eq!(after_archive.motion, original);
    let found = query(
        &mut client,
        &binding,
        Action::ResolveStart(metadata.clone()),
    )
    .unwrap();
    let Output::Resolved { resolution, .. } = &found else {
        panic!("resolution");
    };
    assert!(
        matches!(&resolution.outcome,WasmStartOutcome::Accepted {report} if report.request_id==metadata.request_id && report.state==WasmEditState::Completed)
    );
    state.accept(&binding, found).unwrap();
    // Publish only metadata locally, deliberately lose server-withdrawal acknowledgement, then reconnect.
    let Output::Info(info) = query(&mut client, &binding, Action::Info).unwrap() else {
        panic!("info");
    };
    let mut unpublished = spec();
    unpublished.module = std::fs::read(dir.path().join("module.wasm")).unwrap();
    unpublished.manifest.artifact.sha256 = format!("{:x}", Sha256::digest(&unpublished.module));
    unpublished.manifest.artifact.byte_len = unpublished.module.len() as u64;
    unpublished.expected_worker = info.worker;
    unpublished.approve_execution = true;
    let original_id = crate::session::new_request_id();
    let unknown = crate::wasm_recovery::Intent::capture(
        binding.0.clone(),
        binding.1,
        original_id.clone(),
        &unpublished,
    )
    .unwrap();
    crate::wasm_recovery::prepare(&state_dir, &endpoint, &session, &credential, &unknown).unwrap();
    let Output::Resolved { resolution, .. } =
        query(&mut client, &binding, Action::ResolveStart(unknown.clone())).unwrap()
    else {
        panic!("withdrawal");
    };
    assert!(matches!(resolution.outcome, WasmStartOutcome::Withdrawn));
    client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    state = State::default();
    state.bind(Some(binding.clone()));
    state
        .restore(crate::wasm_recovery::load(&state_dir, &endpoint, &session, &credential).unwrap());
    assert!(state
        .recovery
        .iter()
        .any(|e| e.intent.request_id == original_id && e.attempt.is_none()));
    assert!(state.pending.is_none() && state.draft.is_none() && !state.approved);
    let out = execute_stored(
        &mut client,
        &state_dir,
        &endpoint,
        &binding,
        Action::ResolveStart(unknown),
    )
    .unwrap();
    state.accept(&binding, out).unwrap();
    assert!(
        !crate::wasm_recovery::load(&state_dir, &endpoint, &session, &credential)
            .unwrap()
            .iter()
            .any(|e| e.intent.request_id == original_id)
    );
    assert_eq!(
        crate::wasm_recovery::load(&state_dir, &endpoint, &session, &credential)
            .unwrap()
            .len(),
        2,
        "independent accepted-start records remain"
    );
    let error = client
        .execute_identified(
            original_id.clone(),
            Command::StartWasmEdit {
                spec: unpublished.clone(),
            },
            Some(binding.0.clone()),
            Some(binding.1),
        )
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<ProtocolError>().unwrap().code,
        ErrorCode::RequestConflict
    );
    // This exact preceding binary must enforce the existing request namespace on the same state.
    if let Some(preceding) = std::env::var_os("PULSAR_TEST_PRECEDING_WASM_ENGINE") {
        host.0.kill().unwrap();
        host.0.wait().unwrap();
        let log = std::fs::File::create(dir.path().join("preceding-engine.log")).unwrap();
        host = Host(
            std::process::Command::new(preceding)
                .arg(&state_dir)
                .arg(&worker)
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        client = loop {
            assert!(
                host.0.try_wait().unwrap().is_none(),
                "preceding engine exited"
            );
            assert!(
                Instant::now() < deadline,
                "preceding engine readiness expired"
            );
            if let Ok(mut candidate) = SessionClient::connect(&endpoint, &bootstrap) {
                if candidate
                    .execute(Command::GetSnapshot, Some(binding.0.clone()), None)
                    .is_ok()
                {
                    break candidate;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(client.session_id(), &session);
        let error = client
            .execute_identified(
                original_id,
                Command::StartWasmEdit { spec: unpublished },
                Some(binding.0.clone()),
                Some(binding.1),
            )
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<ProtocolError>().unwrap().code,
            ErrorCode::RequestConflict
        );
    }
    let ResponseBody::Project(after) = client
        .execute(Command::GetSnapshot, Some(binding.0.clone()), None)
        .unwrap()
    else {
        panic!("snapshot");
    };
    assert_eq!(after.revision, binding.1);
    assert_eq!(after.motion, original);
    assert!(host.0.try_wait().unwrap().is_none());
}

#[test]
fn setting_aside_is_local_and_requires_new_approval() {
    let mut state = ready();
    state.approved = true;
    let intent = state.intent().unwrap();
    state.pending = Some(intent.clone());
    let old = state.set_aside().unwrap();
    assert_eq!(old.request_id, intent.request_id);
    assert!(state.pending.is_none() && !state.approved);
    assert!(state.intent().is_err());
    state.approved = true;
    let new = state.intent().unwrap();
    assert_ne!(new.request_id, old.request_id);
}
#[test]
fn observed_status_view_is_bounded_when_external_cleanup_frees_engine_slots() {
    let mut state = ready();
    for i in 0..80 {
        let mut report = report(RequestId::new(format!("request-{i}")).unwrap());
        report.attempt_id = AttemptId::new(format!("attempt-{i}")).unwrap();
        state.update(report);
    }
    assert_eq!(state.reports.len(), 64);
    assert_eq!(state.selected.unwrap().as_str(), "attempt-79");
    assert_eq!(state.reports[0].attempt_id.as_str(), "attempt-16");
}

#[test]
fn restoring_journal_facts_does_not_restore_draft_approval_or_pending_execution() {
    let mut source = ready();
    source.approved = true;
    let intent = source.intent().unwrap();
    let metadata = crate::wasm_recovery::Intent::capture(
        intent.binding.0.clone(),
        intent.binding.1,
        intent.request_id.clone(),
        &intent.spec,
    )
    .unwrap();
    let entry = crate::wasm_recovery::Entry {
        intent: metadata,
        attempt: None,
    };
    let mut restored = State::default();
    restored.bind(Some(binding()));
    restored.restore(vec![entry.clone()]);
    assert!(!restored.approved && restored.draft.is_none() && restored.pending.is_none());
    assert!(restored.recovered_intent(&entry).is_err());
    let mut fresh = ready();
    fresh.restore(vec![entry.clone()]);
    assert!(fresh.recovered_intent(&entry).is_err());
    fresh.approved = true;
    assert_eq!(
        fresh.recovered_intent(&entry).unwrap().request_id,
        intent.request_id
    );
    fresh.draft.as_mut().unwrap().spec.label = "changed".into();
    assert!(fresh.recovered_intent(&entry).is_err());
}

#[test]
fn cleanup_refuses_live_completed_foreign_or_changed_observations_before_dispatch() {
    let mut observed = report(crate::session::new_request_id());
    assert!(RemovalIntent::capture(&binding(), &observed).is_err());
    observed.state = WasmEditState::Failed;
    observed.error = Some(ProtocolError::new(
        ErrorCode::Unavailable,
        "authored terminal failure",
    ));
    let intent = RemovalIntent::capture(&binding(), &observed).unwrap();
    let mut api = Api {
        body: Some(ResponseBody::WasmEditRemoved((observed.clone()).into())),
        calls: vec![],
    };
    query(&mut api, &binding(), Action::Remove(intent.clone())).unwrap();
    assert_eq!(api.calls[0].0, intent.request_id);
    assert!(api.calls[0].3.is_none());
    assert!(matches!(
        api.calls[0].1,
        Command::RemoveUnsuccessfulWasmEdit { .. }
    ));
    let mut wrong = observed.clone();
    wrong.request_id = crate::session::new_request_id();
    api.body = Some(ResponseBody::WasmEditRemoved((wrong).into()));
    assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_err());
    let mut wrong = observed.clone();
    wrong.base_revision = RevisionId::new(99);
    api.body = Some(ResponseBody::WasmEditRemoved((wrong).into()));
    assert!(query(&mut api, &binding(), Action::Remove(intent.clone())).is_err());
    let count = api.calls.len();
    assert!(query(
        &mut api,
        &(ProjectId::new("foreign").unwrap(), RevisionId::new(0)),
        Action::Remove(intent.clone())
    )
    .is_err());
    assert_eq!(api.calls.len(), count);
    let mut live = intent.clone();
    live.observed_state = WasmEditState::Running;
    assert!(query(&mut api, &binding(), Action::Remove(live)).is_err());
    assert_eq!(api.calls.len(), count);
    for state in [WasmEditState::Cancelled, WasmEditState::Interrupted] {
        observed.state = state;
        assert!(RemovalIntent::capture(&binding(), &observed).is_ok());
    }
    observed.project_id = ProjectId::new("foreign").unwrap();
    assert!(RemovalIntent::capture(&binding(), &observed).is_err());
}
#[test]
fn cleanup_acknowledgement_controls_local_removal_and_preserves_unknown_retry() {
    let mut state = ready();
    let mut observed = report(crate::session::new_request_id());
    observed.state = WasmEditState::Failed;
    observed.error = Some(ProtocolError::new(
        ErrorCode::Unavailable,
        "authored terminal failure",
    ));
    let intent = RemovalIntent::capture(&binding(), &observed).unwrap();
    state.update(observed.clone());
    state.pending_removal = Some(intent.clone());
    let mut unrelated = intent.clone();
    unrelated.request_id = crate::session::new_request_id();
    assert!(state
        .accept(
            &binding(),
            Output::Removed {
                intent: unrelated,
                report: Box::new(observed.clone())
            }
        )
        .is_err());
    assert_eq!(state.reports.len(), 1);
    assert!(state.pending_removal.is_some());
    state
        .accept(
            &binding(),
            Output::Catalog(WasmEditCatalog {
                project_id: binding().0,
                operations: vec![],
            }),
        )
        .unwrap();
    assert!(state.pending_removal.as_ref() == Some(&intent));
    state.bind(Some((binding().0, RevisionId::new(1))));
    assert!(state.pending_removal.as_ref() == Some(&intent));
    assert!(state
        .accept(
            &binding(),
            Output::Removed {
                intent: intent.clone(),
                report: Box::new(observed.clone())
            }
        )
        .is_err());
    let current = state.binding.clone().unwrap();
    state.approved = true;
    state
        .accept(
            &current,
            Output::Removed {
                intent,
                report: (observed).into(),
            },
        )
        .unwrap();
    assert!(
        state.pending_removal.is_none()
            && state.selected.is_none()
            && state.reports.is_empty()
            && !state.approved
    );
}

#[test]
fn restored_cleanup_facts_do_not_dispatch_or_restore_execution_approval() {
    let mut state = State::default();
    let mut observed = report(crate::session::new_request_id());
    observed.state = WasmEditState::Failed;
    observed.error = Some(ProtocolError::new(
        ErrorCode::Unavailable,
        "authored terminal",
    ));
    let intent = RemovalIntent::capture(&binding(), &observed).unwrap();
    state.approved = true;
    state.restore_cleanup(vec![crate::wasm_cleanup::Entry {
        intent: intent.clone(),
        acknowledged: false,
    }]);
    assert!(
        !state.approved
            && state.pending_removal.is_none()
            && state.pending.is_none()
            && state.draft.is_none()
            && state.binding.is_none()
    );
    assert_eq!(state.cleanup[0].intent, intent);
}

#[test]
fn archival_intent_binds_observed_completed_candidate_and_full_receipt() {
    let completed = crate::wasm_archive_recovery::authored_report();
    let intent = ArchiveIntent::capture(&binding(), &completed).unwrap();
    let mut api = Api {
        body: Some(ResponseBody::WasmEditArchived((completed.clone()).into())),
        calls: vec![],
    };
    query(
        &mut api,
        &binding(),
        Action::Archive((intent.clone()).into()),
    )
    .unwrap();
    assert_eq!(api.calls[0].0, intent.request_id);
    assert!(api.calls[0].3.is_none());
    let mut changed = completed.clone();
    changed.receipt.as_mut().unwrap().output_sha256 = "b".repeat(64);
    api.body = Some(ResponseBody::WasmEditArchived((changed).into()));
    assert!(query(
        &mut api,
        &binding(),
        Action::Archive(Box::new(intent.clone()))
    )
    .is_err());
    let mut changed = completed.clone();
    changed.candidate_id = Some(CandidateId::new("other").unwrap());
    api.body = Some(ResponseBody::WasmEditArchived((changed).into()));
    assert!(query(
        &mut api,
        &binding(),
        Action::Archive(Box::new(intent.clone()))
    )
    .is_err());
    let count = api.calls.len();
    assert!(query(
        &mut api,
        &(ProjectId::new("foreign").unwrap(), RevisionId::new(0)),
        Action::Archive(Box::new(intent))
    )
    .is_err());
    assert_eq!(api.calls.len(), count);
    let mut live = completed;
    live.state = WasmEditState::Running;
    live.candidate_id = None;
    live.receipt = None;
    assert!(ArchiveIntent::capture(&binding(), &live).is_err());
}
#[test]
fn restored_archival_is_inert_and_catalog_absence_does_not_settle_pending_request() {
    let completed = crate::wasm_archive_recovery::authored_report();
    let intent = ArchiveIntent::capture(&binding(), &completed).unwrap();
    let mut state = State {
        approved: true,
        ..Default::default()
    };
    state.restore_archival(vec![crate::wasm_archive_recovery::Entry {
        intent: intent.clone(),
        acknowledged: false,
    }]);
    assert!(
        !state.approved
            && state.pending_archive.is_none()
            && state.pending.is_none()
            && state.pending_removal.is_none()
            && state.draft.is_none()
    );
    state.bind(Some(binding()));
    state.update(completed.clone());
    state.pending_archive = Some(intent.clone());
    let mut changed = intent.clone();
    changed.request_id = crate::session::new_request_id();
    assert!(state
        .accept(
            &binding(),
            Output::Archived {
                intent: Box::new(changed),
                report: Box::new(completed.clone())
            }
        )
        .is_err());
    assert_eq!(state.reports.len(), 1);
    state
        .accept(
            &binding(),
            Output::Catalog(WasmEditCatalog {
                project_id: binding().0,
                operations: vec![],
            }),
        )
        .unwrap();
    assert!(state.pending_archive.is_some());
    state.bind(Some((binding().0, RevisionId::new(1))));
    assert!(state.pending_archive.is_some());
    assert!(state
        .accept(
            &binding(),
            Output::Archived {
                intent: Box::new(intent.clone()),
                report: Box::new(completed.clone())
            }
        )
        .is_err());
    let current = state.binding.clone().unwrap();
    state
        .accept(
            &current,
            Output::Archived {
                intent: (intent).into(),
                report: (completed).into(),
            },
        )
        .unwrap();
    assert!(state.pending_archive.is_none() && state.selected.is_none() && !state.approved);
}
#[test]
fn setting_aside_archival_preserves_durable_facts_and_candidate_identity() {
    let completed = crate::wasm_archive_recovery::authored_report();
    let intent = ArchiveIntent::capture(&binding(), &completed).unwrap();
    let mut state = ready();
    state.approved = true;
    state.pending_archive = Some(intent.clone());
    state.restore_archival(vec![crate::wasm_archive_recovery::Entry {
        intent: intent.clone(),
        acknowledged: false,
    }]);
    assert!(state.set_aside_archive().unwrap().matches(&intent).unwrap());
    assert!(state.pending_archive.is_none() && !state.approved);
    assert_eq!(state.archival.len(), 1);
    assert_eq!(
        state.archival[0].intent.candidate,
        completed.candidate_id.unwrap()
    );
}

#[test]
fn historical_pages_require_requested_cursor_and_current_binding_without_authorizing_edits() {
    let completed = crate::wasm_archive_recovery::authored_report();
    let page = WasmArchivePage {
        project_id: binding().0,
        after: None,
        limit: WASM_ARCHIVE_PAGE_LIMIT,
        operations: vec![completed],
        next: None,
    };
    let mut state = ready();
    assert!(state
        .accept(&binding(), Output::History(page.clone()))
        .is_err());
    assert!(state.history.is_none());
    state.history_pending = Some(Some(AttemptId::new("before").unwrap()));
    assert!(state
        .accept(&binding(), Output::History(page.clone()))
        .is_err());
    state.history_pending = Some(None);
    state
        .accept(&binding(), Output::History(page.clone()))
        .unwrap();
    assert!(!state.approved && state.pending.is_none() && state.pending_archive.is_none());
    assert_eq!(state.history.as_ref().unwrap().operations.len(), 1);
    assert!(state
        .accept(&binding(), Output::History(page.clone()))
        .is_err());
    state.bind(Some((binding().0, RevisionId::new(1))));
    assert!(state.history.is_none() && state.history_pending.is_none());
    assert!(state.accept(&binding(), Output::History(page)).is_err());
}
#[test]
fn history_controller_checks_response_correspondence_and_cli_bounds() {
    use clap::Parser;
    let page = WasmArchivePage {
        project_id: binding().0,
        after: None,
        limit: WASM_ARCHIVE_PAGE_LIMIT,
        operations: vec![],
        next: None,
    };
    let mut api = Api {
        body: Some(ResponseBody::WasmArchivePage(page.clone())),
        calls: vec![],
    };
    query(&mut api, &binding(), Action::ListArchived(None)).unwrap();
    assert!(matches!(
        &api.calls[0].1,
        Command::ListArchivedWasmEdits {
            after: None,
            limit: 32
        }
    ));
    assert!(api.calls[0].3.is_none());
    let mut api = Api {
        body: Some(ResponseBody::WasmArchivePage(page)),
        calls: vec![],
    };
    assert!(query(
        &mut api,
        &binding(),
        Action::ListArchived(Some(AttemptId::new("cursor").unwrap()))
    )
    .is_err());
    assert!(crate::cli::Cli::try_parse_from([
        "pulsar",
        "wasm-edit",
        "list-archived",
        "project",
        "--limit",
        "1",
        "--after",
        "cursor"
    ])
    .is_ok());
    for limit in ["0", "33", "65535"] {
        assert!(crate::cli::Cli::try_parse_from([
            "pulsar",
            "wasm-edit",
            "list-archived",
            "project",
            "--limit",
            limit
        ])
        .is_err());
    }
}

#[test]
fn resolution_reuses_metadata_identity_and_clears_only_matching_pending_start_without_approval() {
    let mut state = ready();
    state.approved = true;
    let start = state.intent().unwrap();
    let intent = crate::wasm_recovery::Intent::capture(
        binding().0,
        binding().1,
        start.request_id.clone(),
        &start.spec,
    )
    .unwrap();
    state.pending = Some(start);
    state.approved = false;
    let resolution = WasmStartResolution {
        project_id: binding().0,
        reference: intent.reference().unwrap(),
        outcome: WasmStartOutcome::Withdrawn,
    };
    let mut wrong = resolution.clone();
    wrong.reference.label = "changed".into();
    assert!(state
        .accept(
            &binding(),
            Output::Resolved {
                intent: Box::new(intent.clone()),
                resolution: Box::new(wrong)
            }
        )
        .is_err());
    assert!(state.pending.is_some());
    state
        .accept(
            &binding(),
            Output::Resolved {
                intent: Box::new(intent.clone()),
                resolution: (resolution.clone()).into(),
            },
        )
        .unwrap();
    assert!(state.pending.is_none() && !state.approved && state.reports.is_empty());
    let mut api = Api {
        body: Some(ResponseBody::WasmStartResolution(
            (resolution.clone()).into(),
        )),
        calls: vec![],
    };
    query(&mut api, &binding(), Action::ResolveStart(intent.clone())).unwrap();
    assert_eq!(
        api.calls[0].0,
        intent
            .reference()
            .unwrap()
            .resolution_request_id(&binding().0)
            .unwrap()
    );
    assert!(api.calls[0].3.is_none());
    let mut api = Api {
        body: Some(ResponseBody::WasmStartResolution((resolution).into())),
        calls: vec![],
    };
    assert!(query(
        &mut api,
        &(ProjectId::new("foreign").unwrap(), binding().1),
        Action::ResolveStart(intent)
    )
    .is_err());
    assert!(api.calls.is_empty());
}
#[test]
fn resolution_cli_requires_reference_and_explicit_replay_id() {
    use clap::Parser;
    assert!(crate::cli::Cli::try_parse_from([
        "pulsar",
        "wasm-edit",
        "resolve-start",
        "project",
        "--reference",
        "reference.json",
        "--request-id",
        "resolve"
    ])
    .is_ok());
    assert!(crate::cli::Cli::try_parse_from([
        "pulsar",
        "wasm-edit",
        "resolve-start",
        "project",
        "--reference",
        "reference.json"
    ])
    .is_err());
    assert!(crate::cli::Cli::try_parse_from([
        "pulsar",
        "wasm-edit",
        "resolve-start",
        "project",
        "--request-id",
        "resolve"
    ])
    .is_err());
}

#[test]
fn queued_editor_messages_have_bounded_inline_storage() {
    assert!(std::mem::size_of::<Action>() <= 512);
    assert!(std::mem::size_of::<Output>() <= 256);
}
