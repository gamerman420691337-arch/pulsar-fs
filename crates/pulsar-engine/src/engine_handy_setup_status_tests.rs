fn setup_status_fixture_record(f: &Fixture) -> crate::device_effect_journal::SetupEffectRecord {
    let registration = handy_session_registered(f);
    let route =
        crate::HandyRouteIdentity::from_snapshot(&f.engine.handy_owner_token, "synthetic-route")
            .unwrap();
    let artifact = |c: char| {
        let sha = c.to_string().repeat(64);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha.clone()).unwrap(),
            sha256: sha,
            byte_len: 40,
        }
    };
    crate::device_effect_journal::SetupEffectRecord {
        boot: DeviceSessionId::new(f.engine.engine_epoch.clone()).unwrap(),
        device_session: DeviceSessionId::new("synthetic-device-session").unwrap(),
        actor_session: f.session.clone(),
        request_id: RequestId::new("original-setup").unwrap(),
        hosting_request_id: RequestId::new("original-hosting").unwrap(),
        project_id: f.project.clone(),
        revision: RevisionId::new(0),
        registration,
        qualification: HandyQualificationBinding {
            dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
            revision: 2,
        },
        master_sha256: "c".repeat(64),
        script: artifact('d'),
        hosted: artifact('e'),
        route_sha256: route.route_sha256().into(),
        route_namespace_sha256: Some(route.namespace_sha256().into()),
        acknowledgment_required: true,
        preflight_mode: Some(setup_sample().mode),
    }
}
fn setup_status_request(f: &Fixture, actor: &SessionId) -> Request {
    request(
        actor,
        Some(&f.project),
        None,
        Command::HandySetupStatus {
            setup_request_id: RequestId::new("original-setup").unwrap(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
fn setup_status_report(body: ResponseBody) -> HandySetupStatus {
    let ResponseBody::HandySetupStatus(value) = body else {
        panic!("setup status")
    };
    value.validate().unwrap();
    *value
}
#[test]
fn handy_setup_status_is_scoped_read_only_history_and_survives_restart_without_credentials() {
    let f = fixture();
    let record = setup_status_fixture_record(&f);
    let original_epoch = record.boot.clone();
    let receipt = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .prepare_setup(record.clone())
        .unwrap();
    f.engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .claim_setup(&receipt)
        .unwrap();
    let query = setup_status_request(&f, &f.session);
    let before: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    let value = setup_status_report(f.engine.handle(query.clone()).result.unwrap());
    assert!(!value.loaded && !value.qualified);
    assert_eq!(
        value.history.as_ref().unwrap().state,
        HandySetupState::Claimed
    );
    assert_eq!(
        before,
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM requests", [], |r| r.get::<_, i64>(0))
            .unwrap()
    );
    let other = pair(&f.engine, "other recovery actor");
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT OR IGNORE INTO grants VALUES(?1,?2,?3)",
            params![other.as_str(), f.project.as_str(), scope_name(Scope::Read)],
        )
        .unwrap();
    assert!(setup_status_report(
        f.engine
            .handle(setup_status_request(&f, &other))
            .result
            .unwrap()
    )
    .history
    .is_none());
    let Fixture { engine, dir, .. } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    let recovered = setup_status_report(engine.handle(query).result.unwrap());
    assert_eq!(recovered.history.unwrap().engine_epoch, original_epoch);
    assert!(engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .setup_route_reserved(&record.route_sha256)
        .unwrap());
    drop(engine);
    drop(dir);
}
#[test]
fn handy_setup_status_requires_owner_and_read_grant_and_refuses_corrupt_history() {
    let f = fixture();
    let record = setup_status_fixture_record(&f);
    let receipt = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .prepare_setup(record)
        .unwrap();
    let query = setup_status_request(&f, &f.session);
    let mut bad = query.clone();
    if let Command::HandySetupStatus { owner_proof, .. } = &mut bad.command {
        *owner_proof = "wrong".into();
    }
    assert_eq!(
        f.engine.handle(bad).result.unwrap_err().code,
        ErrorCode::Forbidden
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
                scope_name(Scope::Read)
            ],
        )
        .unwrap();
    assert_eq!(
        f.engine.handle(query.clone()).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO grants VALUES(?1,?2,?3)",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Read)
            ],
        )
        .unwrap();
    let db = Connection::open(
        f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    db.execute(
        "UPDATE setup_effects SET record='{}' WHERE id=?1",
        [receipt.id()],
    )
    .unwrap();
    assert!(f.engine.handle(query).result.is_err());
}
