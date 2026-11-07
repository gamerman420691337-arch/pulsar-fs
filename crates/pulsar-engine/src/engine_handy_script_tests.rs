fn handy_script_commit(f: &Fixture) {
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(f, 0, program(0.2)),
        ))
        .result
        .unwrap();
}
fn handy_script_request(f: &Fixture, id: &str, path: PathBuf, revision: u64) -> Request {
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(revision),
        Command::PrepareHandyStrokeScript { path },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn handy_script_receipt(body: ResponseBody) -> HandyStrokeScript {
    let ResponseBody::HandyStrokeScript(r) = body else {
        panic!("script receipt")
    };
    r.validate().unwrap();
    *r
}
#[test]
fn handy_script_public_preparation_binds_exact_bytes_and_historical_replay() {
    let f = fixture();
    handy_script_commit(&f);
    let path = f.dir.path().join("stroke.csv");
    let request = handy_script_request(&f, "prepare", path.clone(), 1);
    let before = project_snapshot(&f.engine.db.lock().unwrap(), &f.project).unwrap();
    let report = handy_script_receipt(f.engine.handle(request.clone()).result.unwrap());
    let bytes = fs::read(&path).unwrap();
    assert_eq!(bytes, b"# Pulsar stroke samples v1\n0,20\n1000,20\n");
    assert_eq!(
        report.identity.sha256,
        format!("{:x}", Sha256::digest(&bytes))
    );
    assert_eq!(report.identity.byte_len, bytes.len() as u64);
    assert_eq!(report.master_sha256, before.motion.program.sha256);
    assert!(!report.release_qualified);
    assert_eq!(
        project_snapshot(&f.engine.db.lock().unwrap(), &f.project)
            .unwrap()
            .revision,
        RevisionId::new(1)
    );
    let repeated = handy_script_receipt(f.engine.handle(request.clone()).result.unwrap());
    assert_eq!(encode(&report).unwrap(), encode(&repeated).unwrap());
    fs::write(&path, b"external replacement").unwrap();
    let historical = handy_script_receipt(f.engine.handle(request).result.unwrap());
    assert_eq!(historical.identity, report.identity);
    assert_eq!(fs::read(&path).unwrap(), b"external replacement");
    let error = f
        .engine
        .handle(handy_script_request(&f, "no-clobber", path.clone(), 1))
        .result
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::UnknownOutcome);
    assert_eq!(fs::read(path).unwrap(), b"external replacement");
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT axis FROM exports WHERE request='prepare'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "stroke"
    );
}
#[test]
fn handy_script_scope_revision_and_unknown_review_are_preserved() {
    let f = fixture();
    handy_script_commit(&f);
    let path = f.dir.path().join("stroke.csv");
    assert_eq!(
        f.engine
            .handle(handy_script_request(&f, "stale", path.clone(), 0))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert!(!path.exists());
    let peer = pair(&f.engine, "export-only");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: peer.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Export],
            },
        ))
        .result
        .unwrap();
    let denied = request(
        &peer,
        Some(&f.project),
        Some(1),
        Command::PrepareHandyStrokeScript { path: path.clone() },
    );
    assert_eq!(
        f.engine.handle(denied).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert!(!path.exists());
    {
        let db = f.engine.db.lock().unwrap();
        let mut state = lineage::state(&db, &f.project, RevisionId::new(1)).unwrap();
        state.unknown = true;
        db.execute(
            "UPDATE revision_lineage SET review_state=?1 WHERE project=?2 AND revision=1",
            params![encode(&state).unwrap(), f.project.as_str()],
        )
        .unwrap();
    }
    let receipt = handy_script_receipt(
        f.engine
            .handle(handy_script_request(&f, "unknown-review", path, 1))
            .result
            .unwrap(),
    );
    assert!(receipt.review_state_unknown);
    assert!(!receipt.release_qualified);
}
#[test]
fn handy_script_failed_ack_remains_unknown_and_restart_does_not_republish() {
    let f = fixture();
    handy_script_commit(&f);
    let path = f.dir.path().join("stroke.csv");
    let original = handy_script_request(&f, "unknown-receipt", path.clone(), 1);
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER refuse_handy_script_receipt BEFORE INSERT ON requests WHEN NEW.request='unknown-receipt' BEGIN SELECT RAISE(ABORT,'fixture acknowledgment failure'); END;").unwrap();
    assert!(f.engine.handle(original.clone()).result.is_err());
    let bytes = fs::read(&path).unwrap();
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_handy_script_receipt")
        .unwrap();
    assert_eq!(
        f.engine.handle(original.clone()).result.unwrap_err().code,
        ErrorCode::UnknownOutcome
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    let f = Fixture {
        engine,
        session,
        project,
        dir,
    };
    assert_eq!(
        f.engine.handle(original.clone()).result.unwrap_err().code,
        ErrorCode::UnknownOutcome
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let mut conflict = original;
    if let Command::PrepareHandyStrokeScript { path } = &mut conflict.command {
        *path = f.dir.path().join("other.csv");
    }
    assert_eq!(
        f.engine.handle(conflict).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
}
#[test]
fn handy_script_successful_receipt_survives_restart_and_permission_loss_refuses_replay() {
    let f = fixture();
    handy_script_commit(&f);
    let path = f.dir.path().join("stroke.csv");
    let original = handy_script_request(&f, "retained", path, 1);
    let saved = handy_script_receipt(f.engine.handle(original.clone()).result.unwrap());
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    let f = Fixture {
        engine,
        session,
        project,
        dir,
    };
    assert_eq!(
        encode(&handy_script_receipt(
            f.engine.handle(original.clone()).result.unwrap()
        ))
        .unwrap(),
        encode(&saved).unwrap()
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Export)
            ],
        )
        .unwrap();
    assert_eq!(
        f.engine.handle(original).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
}

#[test]
fn handy_script_project_capture_preserves_export_identity_without_loading_mutable_output() {
    let f = fixture();
    handy_script_commit(&f);
    let path = f.dir.path().join("stroke.csv");
    let request = handy_script_request(&f, "package-script", path.clone(), 1);
    let token = request.auth_token.clone().unwrap();
    let report = handy_script_receipt(f.engine.handle(request).result.unwrap());
    fs::write(&path, b"external replacement is not package authority").unwrap();
    let plan = project_packages::capture(
        &f.engine,
        &f.session,
        &token,
        &f.project,
        RevisionId::new(1),
    )
    .unwrap();
    plan.manifest.validate().unwrap();
    assert_eq!(plan.manifest.export_receipts.len(), 1);
    let export = &plan.manifest.export_receipts[0];
    assert_eq!(export.axis, PackageExportScope::Axis(Axis::Stroke));
    assert_eq!(export.sha256, report.identity.sha256);
    assert_eq!(export.revision, report.revision);
    assert!(matches!(export.state, PackageExportState::Complete));
}
