use super::*;
#[test]
#[ignore = "requires an explicitly delegated private Linux cgroup and authored WASM worker"]
fn completed_archival_reclaims_operational_capacity_preserving_evidence_restart_and_portable_bytes()
{
    let f = Fixture::new();
    let start = f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0));
    let ResponseBody::WasmEdit(accepted) = f.engine.handle(start.clone()).result.unwrap() else {
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
    let before = crate::authority::project_packages::capture(
        &f.engine,
        &f.session,
        &f.token,
        &f.project,
        RevisionId::new(0),
    )
    .unwrap();
    assert_eq!(before.manifest.format_version, 12);
    let stored = f
        .engine
        .package_store
        .materialize(
            &before.manifest,
            |object| before.open_object(object),
            crate::project_package_format::PackageCodecLimits::default(),
            || Ok(()),
            |_, _| Ok(()),
        )
        .unwrap();
    let mut original_package = vec![];
    use std::io::Read;
    f.engine
        .package_store
        .open_readonly(&stored)
        .unwrap()
        .read_to_end(&mut original_package)
        .unwrap();
    let before_lineage = serde_json::to_vec(&before.manifest.authored_lineage).unwrap();
    drop(before);
    let original_row: (Vec<u8>, Vec<u8>, Vec<u8>, String) = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT module,input,output,receipt FROM wasm_edits WHERE id=?1",
            [completed.attempt_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    // These 63 pressure rows are explicitly synthetic unsuccessful records, not guest executions.
    for i in 0..63 {
        f.engine.db.lock().unwrap().execute("INSERT INTO wasm_edits SELECT ?1,project,session,base_revision,engine_epoch,?2,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,'failed',NULL,?4,NULL,NULL FROM wasm_edits WHERE id=?3",params![format!("synthetic-pressure-{i}"),format!("synthetic-request-{i}"),completed.attempt_id.as_str(),encode(&ProtocolError::new(ErrorCode::Unavailable,"synthetic catalog pressure")).unwrap()]).unwrap();
    }
    let refused = f
        .engine
        .handle(f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0)))
        .result
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::ResourceExhausted);
    let archive = f.request(
        Command::ArchiveCompletedWasmEdit {
            attempt_id: completed.attempt_id.clone(),
            original_request_id: completed.request_id.clone(),
        },
        None,
    );
    let mut wrong = archive.clone();
    wrong.request_id = id!(RequestId);
    if let Command::ArchiveCompletedWasmEdit {
        original_request_id,
        ..
    } = &mut wrong.command
    {
        *original_request_id = id!(RequestId);
    }
    assert!(f.engine.handle(wrong).result.is_err());
    let ResponseBody::WasmEditArchived(archived) = f.engine.handle(archive.clone()).result.unwrap()
    else {
        panic!("archive")
    };
    archived.validate_archive_for(&archive).unwrap();
    let ResponseBody::WasmEditCatalog(catalog) = f
        .engine
        .handle(f.request(Command::ListWasmEdits, None))
        .result
        .unwrap()
    else {
        panic!("catalog")
    };
    assert_eq!(catalog.operations.len(), 63);
    assert!(!catalog
        .operations
        .iter()
        .any(|r| r.attempt_id == completed.attempt_id));
    let after = crate::authority::project_packages::capture(
        &f.engine,
        &f.session,
        &f.token,
        &f.project,
        RevisionId::new(0),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&after.manifest.authored_lineage).unwrap(),
        before_lineage
    );
    drop(after);
    let retained: (Vec<u8>, Vec<u8>, Vec<u8>, String) = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT module,input,output,receipt FROM wasm_edits WHERE id=?1",
            [completed.attempt_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(retained, original_row);
    let mut reopened = vec![];
    f.engine
        .package_store
        .open_readonly(&stored)
        .unwrap()
        .read_to_end(&mut reopened)
        .unwrap();
    assert_eq!(reopened, original_package);

    let second_start = f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0));
    let ResponseBody::WasmEdit(second) = f.engine.handle(second_start).result.unwrap() else {
        panic!("second start")
    };
    let second = f.wait(&second.attempt_id);
    assert_eq!(second.state, WasmEditState::Completed, "{:?}", second.error);
    f.retired();
    let config = f.engine.config.clone();
    let status = f.request(
        Command::WasmEditStatus {
            attempt_id: completed.attempt_id.clone(),
        },
        None,
    );
    let list = f.request(Command::ListWasmEdits, None);
    drop(f.engine);
    let restarted = Engine::open(config).unwrap();
    let ResponseBody::WasmEdit(report) = restarted.handle(status).result.unwrap() else {
        panic!("status")
    };
    assert_eq!(
        serde_json::to_vec(&report.receipt).unwrap(),
        serde_json::to_vec(&completed.receipt).unwrap()
    );
    let ResponseBody::WasmEditArchived(replayed) = restarted.handle(archive).result.unwrap() else {
        panic!("archive replay")
    };
    assert_eq!(replayed.attempt_id, completed.attempt_id);
    let ResponseBody::WasmEdit(start_replay) = restarted.handle(start).result.unwrap() else {
        panic!("start replay")
    };
    assert_eq!(start_replay.attempt_id, completed.attempt_id);
    assert!(restarted.workers.lock().unwrap().is_empty());
    let ResponseBody::WasmEditCatalog(catalog) = restarted.handle(list).result.unwrap() else {
        panic!("catalog")
    };
    assert_eq!(catalog.operations.len(), 64);
    let after = crate::authority::project_packages::capture(
        &restarted,
        &f.session,
        &f.token,
        &f.project,
        RevisionId::new(0),
    )
    .unwrap();
    assert!(after.manifest.authored_lineage.iter().any(|entry| entry
        .wasm_invocation
        .as_ref()
        .is_some_and(
            |i| i.receipt.edit_receipt == completed.receipt.as_ref().unwrap().edit_receipt
        )));
    crate::authority::wasm_edits::validate(&restarted.db.lock().unwrap()).unwrap();
    let mut reopened = vec![];
    restarted
        .package_store
        .open_readonly(&stored)
        .unwrap()
        .read_to_end(&mut reopened)
        .unwrap();
    assert_eq!(reopened, original_package);
}

#[test]
#[ignore = "requires an explicitly delegated private Linux cgroup and authored WASM worker"]
fn archive_history_paginates_own_custody_rechecks_read_and_survives_restart() {
    let f = Fixture::new();
    let ResponseBody::WasmEdit(accepted) = f
        .engine
        .handle(f.request(Command::StartWasmEdit { spec: f.spec() }, Some(0)))
        .result
        .unwrap()
    else {
        panic!("start");
    };
    let completed = f.wait(&accepted.attempt_id);
    assert_eq!(
        completed.state,
        WasmEditState::Completed,
        "{:?}",
        completed.error
    );
    f.retired();
    let first = f.request(
        Command::ListArchivedWasmEdits {
            after: None,
            limit: 32,
        },
        None,
    );
    let ResponseBody::WasmArchivePage(empty) = f.engine.handle(first.clone()).result.unwrap()
    else {
        panic!("history");
    };
    assert!(empty.operations.is_empty());
    f.engine
        .handle(f.request(
            Command::ArchiveCompletedWasmEdit {
                attempt_id: completed.attempt_id.clone(),
                original_request_id: completed.request_id.clone(),
            },
            None,
        ))
        .result
        .unwrap();
    // These 33 copied rows are synthetic pagination pressure, not guest executions.
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        for i in 0..33 {
            let id = format!("synthetic-history-{i:02}");
            tx.execute("INSERT INTO wasm_edits SELECT ?1,project,session,base_revision,engine_epoch,?2,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,state,candidate,error,receipt,output FROM wasm_edits WHERE id=?3",params![id,format!("synthetic-history-request-{i:02}"),completed.attempt_id.as_str()]).unwrap();
            tx.execute("INSERT INTO wasm_edit_archives VALUES(?1)", [id])
                .unwrap();
        }
        tx.commit().unwrap();
    }
    let ResponseBody::WasmArchivePage(page) = f.engine.handle(first.clone()).result.unwrap() else {
        panic!("history");
    };
    page.validate_for(&first).unwrap();
    assert_eq!(page.operations.len(), 32);
    let continuation = page.next.clone().unwrap();
    let second = f.request(
        Command::ListArchivedWasmEdits {
            after: Some(continuation.clone()),
            limit: 32,
        },
        None,
    );
    let ResponseBody::WasmArchivePage(tail) = f.engine.handle(second.clone()).result.unwrap()
    else {
        panic!("tail");
    };
    tail.validate_for(&second).unwrap();
    assert_eq!(tail.operations.len(), 2);
    assert!(tail.next.is_none());
    let all = page
        .operations
        .iter()
        .chain(tail.operations.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        all.iter()
            .map(|r| r.attempt_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        34
    );
    assert!(all.iter().any(|r| r.attempt_id == completed.attempt_id));
    let ResponseBody::WasmEditCatalog(active) = f
        .engine
        .handle(f.request(Command::ListWasmEdits, None))
        .result
        .unwrap()
    else {
        panic!("active");
    };
    assert!(active.operations.is_empty());
    let ResponseBody::Paired {
        session: reader,
        auth_token: reader_token,
    } = f
        .engine
        .handle(Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: "archive reader".into(),
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
                session: reader.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read],
            },
            None,
        ))
        .result
        .unwrap();
    let mut foreign = first.clone();
    foreign.request_id = id!(RequestId);
    foreign.session = Some(reader.clone());
    foreign.auth_token = Some(reader_token.clone());
    let ResponseBody::WasmArchivePage(foreign_page) = f.engine.handle(foreign).result.unwrap()
    else {
        panic!("reader");
    };
    assert!(foreign_page.operations.is_empty() && foreign_page.next.is_none());
    f.engine
        .handle(f.request(
            Command::Grant {
                session: reader.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::ManageGrants],
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
        f.engine.handle(first.clone()).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let mut restore = f.request(
        Command::Grant {
            session: f.session.clone(),
            project_id: f.project.clone(),
            scopes: vec![Scope::Read],
        },
        None,
    );
    restore.session = Some(reader);
    restore.auth_token = Some(reader_token);
    f.engine.handle(restore).result.unwrap();
    let beyond = f.request(
        Command::ListArchivedWasmEdits {
            after: Some(AttemptId::new("zzzz").unwrap()),
            limit: 1,
        },
        None,
    );
    let ResponseBody::WasmArchivePage(last) = f.engine.handle(beyond).result.unwrap() else {
        panic!("last");
    };
    assert!(last.operations.is_empty() && last.next.is_none());
    let original = serde_json::to_vec(&completed.receipt).unwrap();
    let status = f.request(
        Command::GetCandidate {
            candidate_id: completed.candidate_id.unwrap(),
        },
        None,
    );
    let config = f.engine.config.clone();
    drop(f.engine);
    let restarted = Engine::open(config).unwrap();
    let ResponseBody::WasmArchivePage(reopened) = restarted.handle(first).result.unwrap() else {
        panic!("reopened");
    };
    assert_eq!(reopened.operations.len(), 32);
    assert_eq!(reopened.next, Some(continuation));
    let ResponseBody::WasmArchivePage(tail) = restarted.handle(second).result.unwrap() else {
        panic!("tail restart");
    };
    let actual = reopened
        .operations
        .iter()
        .chain(tail.operations.iter())
        .find(|r| r.attempt_id == completed.attempt_id)
        .unwrap();
    assert_eq!(serde_json::to_vec(&actual.receipt).unwrap(), original);
    assert!(matches!(
        restarted.handle(status).result.unwrap(),
        ResponseBody::Candidate(_)
    ));
    crate::authority::wasm_edits::validate(&restarted.db.lock().unwrap()).unwrap();
}
