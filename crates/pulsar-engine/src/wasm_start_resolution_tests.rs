use super::*;
fn reference(request: &Request) -> WasmStartReference {
    let Command::StartWasmEdit { spec } = &request.command else {
        panic!("start");
    };
    WasmStartReference {
        original_request_id: request.request_id.clone(),
        base_revision: request.expected_revision.unwrap(),
        manifest: spec.manifest.clone(),
        worker: spec.expected_worker.clone(),
        guest_memory_bytes: spec.guest_memory_bytes,
        label: spec.label.clone(),
    }
}
#[test]
#[ignore = "requires an explicitly delegated private Linux cgroup and authored WASM worker"]
fn resolution_linearizes_against_delayed_start_and_recovers_accepted_guest_without_cancellation() {
    let f = Fixture::new();
    let start = f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0));
    let original = reference(&start);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    START_PUBLICATION_HOOKS.lock().unwrap().insert(
        start.request_id.to_string(),
        Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(90)).unwrap();
        }),
    );
    let engine = f.engine.clone();
    let delayed = start.clone();
    let thread = std::thread::spawn(move || engine.handle(delayed));
    entered_rx.recv_timeout(Duration::from_secs(90)).unwrap();
    let resolve = f.request(
        Command::ResolveWasmStart {
            reference: original,
        },
        None,
    );
    let ResponseBody::WasmStartResolution(withdrawn) =
        f.engine.handle(resolve.clone()).result.unwrap()
    else {
        panic!("resolution");
    };
    assert!(matches!(withdrawn.outcome, WasmStartOutcome::Withdrawn));
    release_tx.send(()).unwrap();
    assert_eq!(
        thread.join().unwrap().result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    assert!(f.engine.workers.lock().unwrap().is_empty());
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
    let start = f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0));
    let original = reference(&start);
    let ResponseBody::WasmEdit(accepted) = f.engine.handle(start.clone()).result.unwrap() else {
        panic!("accepted");
    };
    let done = f.wait(&accepted.attempt_id);
    assert_eq!(done.state, WasmEditState::Completed, "{:?}", done.error);
    f.retired();
    let recover = f.request(
        Command::ResolveWasmStart {
            reference: original.clone(),
        },
        None,
    );
    let ResponseBody::WasmStartResolution(found) = f.engine.handle(recover.clone()).result.unwrap()
    else {
        panic!("recovered");
    };
    let WasmStartOutcome::Accepted { report } = &found.outcome else {
        panic!("accepted outcome");
    };
    assert_eq!(report.attempt_id, done.attempt_id);
    assert_eq!(
        serde_json::to_vec(&report.receipt).unwrap(),
        serde_json::to_vec(&done.receipt).unwrap()
    );
    let mut wrong = original;
    wrong.label = "changed".into();
    assert_eq!(
        f.engine
            .handle(f.request(Command::ResolveWasmStart { reference: wrong }, None))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RequestConflict
    );
    let ResponseBody::Paired {
        session: admin,
        auth_token: admin_token,
    } = f
        .engine
        .handle(Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: "resolution authority fixture".into(),
                pairing_token: f.engine.token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("pair");
    };
    f.engine
        .handle(f.request(
            Command::Grant {
                session: admin.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::ManageGrants],
            },
            None,
        ))
        .result
        .unwrap();
    let mut read_only = recover.clone();
    read_only.request_id = id!(RequestId);
    read_only.session = Some(admin.clone());
    read_only.auth_token = Some(admin_token.clone());
    assert_eq!(
        f.engine.handle(read_only).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    f.engine
        .handle(f.request(
            Command::Grant {
                session: admin.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Edit],
            },
            None,
        ))
        .result
        .unwrap();
    f.engine
        .handle(f.request(
            Command::Revoke {
                session: f.session.clone(),
                project_id: f.project.clone(),
            },
            None,
        ))
        .result
        .unwrap();
    assert_eq!(
        f.engine.handle(recover.clone()).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    for scope in [Scope::Edit, Scope::Read] {
        let mut grant = f.request(
            Command::Grant {
                session: f.session.clone(),
                project_id: f.project.clone(),
                scopes: vec![scope],
            },
            None,
        );
        grant.session = Some(admin.clone());
        grant.auth_token = Some(admin_token.clone());
        f.engine.handle(grant).result.unwrap();
        if scope == Scope::Edit {
            assert_eq!(
                f.engine.handle(recover.clone()).result.unwrap_err().code,
                ErrorCode::Forbidden
            );
        }
    }
    assert!(matches!(
        f.engine.handle(recover.clone()).result.unwrap(),
        ResponseBody::WasmStartResolution(_)
    ));
    let snapshot = f.request(Command::GetSnapshot, None);
    let config = f.engine.config.clone();
    drop(f.engine);
    let reopened = Engine::open(config).unwrap();
    assert!(matches!(
        reopened.handle(resolve).result.unwrap(),
        ResponseBody::WasmStartResolution(value) if matches!(value.outcome, WasmStartOutcome::Withdrawn)
    ));
    assert!(matches!(
        reopened.handle(recover).result.unwrap(),
        ResponseBody::WasmStartResolution(value) if matches!(value.outcome, WasmStartOutcome::Accepted { .. })
    ));
    let ResponseBody::Project(after) = reopened.handle(snapshot).result.unwrap() else {
        panic!("snapshot");
    };
    assert_eq!(after.revision, RevisionId::new(0));
}
