fn handy_registration_settings(label: &str) -> HandyRegistrationSettings {
    HandyRegistrationSettings {
        label: label.into(),
        api_key_env: "PULSAR_HANDY_FIXTURE_API".into(),
        connection_key_env: "PULSAR_HANDY_FIXTURE_CONNECTION".into(),
    }
}
fn handy_registration_request(f: &Fixture, id: &str, expected: u64, label: &str) -> Request {
    let mut r = request(
        &f.session,
        None,
        None,
        Command::SetHandyRegistration {
            device_id: DeviceId::new("handy-fixture").unwrap(),
            expected_registration_revision: expected,
            settings: handy_registration_settings(label),
            disabled: false,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn handy_registration_saved(response: Response) -> HandyRegistration {
    let ResponseBody::HandyRegistrationSaved(receipt) = response.result.unwrap() else {
        panic!("registration receipt");
    };
    receipt.validate().unwrap();
    receipt.registration
}
fn handy_registration_catalog(f: &Fixture) -> HandyRegistrationCatalog {
    let ResponseBody::HandyRegistrations(catalog) = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::ListHandyRegistrations {
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("catalog");
    };
    *catalog
}

#[test]
fn handy_registration_owner_checks_precede_inert_storage_and_project_changes() {
    let f = fixture();
    let mut denied = handy_registration_request(&f, "denied", 0, "local label");
    if let Command::SetHandyRegistration { owner_proof, .. } = &mut denied.command {
        *owner_proof = "f".repeat(64);
    }
    assert_eq!(
        f.engine.handle(denied).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert!(handy_registration_catalog(&f).registrations.is_empty());
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name LIKE 'handy_registration%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    let mut invalid = handy_registration_request(&f, "invalid", 0, "\n");
    assert_eq!(
        f.engine.handle(invalid.clone()).result.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    invalid = handy_registration_request(&f, "project-scope", 0, "label");
    invalid.project = Some(f.project.clone());
    assert_eq!(
        f.engine.handle(invalid).result.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let saved = handy_registration_saved(f.engine.handle(handy_registration_request(
        &f,
        "registered",
        0,
        "local label",
    )));
    assert_eq!(saved.revision, 1);
    assert!(!saved.disabled);
    let db = f.engine.db.lock().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT revision FROM projects WHERE id=?1",
            [f.project.as_str()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let stored: String = db
        .query_row("SELECT settings FROM handy_registrations", [], |r| r.get(0))
        .unwrap();
    assert!(stored.contains("PULSAR_HANDY_FIXTURE_API"));
    assert!(!stored.contains(&f.engine.handy_owner_token));
    drop(db);
    assert_eq!(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::Arm {
                    device_id: saved.device_id,
                    revision: RevisionId::new(0)
                }
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[test]
fn handy_registration_replay_is_historical_and_survives_restart() {
    let f = fixture();
    let original = handy_registration_request(&f, "original", 0, "first");
    let first = handy_registration_saved(f.engine.handle(original.clone()));
    let disable = request(
        &f.session,
        None,
        None,
        Command::DisableHandyRegistration {
            device_id: first.device_id.clone(),
            expected_registration_revision: 1,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    let disabled = handy_registration_saved(f.engine.handle(disable));
    assert!(disabled.disabled);
    assert_ne!(first.binding_sha256, disabled.binding_sha256);
    assert_eq!(
        handy_registration_saved(f.engine.handle(original.clone())),
        first
    );
    assert_eq!(
        handy_registration_catalog(&f).registrations.as_slice(),
        std::slice::from_ref(&disabled)
    );
    assert_eq!(
        f.engine
            .handle(handy_registration_request(&f, "stale", 0, "new"))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut conflict = original.clone();
    if let Command::SetHandyRegistration { settings, .. } = &mut conflict.command {
        settings.label = "changed".into();
    }
    assert_eq!(
        f.engine.handle(conflict).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    assert_eq!(handy_registration_saved(engine.handle(original)), first);
    let f = Fixture {
        engine,
        session,
        project,
        dir,
    };
    assert_eq!(handy_registration_catalog(&f).registrations, [disabled]);
}

#[test]
fn handy_registration_and_receipt_roll_back_together() {
    let f = fixture();
    let original = handy_registration_request(&f, "atomic-registration", 0, "first");
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER refuse_registry_receipt BEFORE INSERT ON requests WHEN NEW.request='atomic-registration' BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
    assert!(f.engine.handle(original.clone()).result.is_err());
    let db = f.engine.db.lock().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'handy_registration%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER refuse_registry_receipt")
        .unwrap();
    drop(db);
    assert_eq!(
        handy_registration_saved(f.engine.handle(original)).revision,
        1
    );
}

#[test]
fn handy_registration_inspection_rechecks_binding_and_authentication_after_io() {
    let f = fixture();
    let first = handy_registration_saved(f.engine.handle(handy_registration_request(
        &f,
        "registration",
        0,
        "first",
    )));
    let inspect = |record: &HandyRegistration| {
        request(
            &f.session,
            None,
            None,
            Command::InspectHandyRegistration {
                device_id: record.device_id.clone(),
                expected_registration_revision: record.revision,
                binding_sha256: record.binding_sha256.clone(),
                allow_online_inspection: true,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        )
    };
    let outcome = f
        .engine
        .inspect_handy_registration_with(&inspect(&first), &f.session, |input| {
            assert_eq!(input.api_key_env, first.settings.api_key_env);
            handy_registration_saved(
                f.engine
                    .handle(handy_registration_request(&f, "changed", 1, "changed")),
            );
            Ok(HandyInspection {
                connected: false,
                identity: None,
                observed_at_unix_ms: 1,
            })
        });
    assert_eq!(outcome.unwrap_err().code, ErrorCode::RevisionConflict);
    let current = handy_registration_catalog(&f).registrations.remove(0);
    let outcome = f
        .engine
        .inspect_handy_registration_with(&inspect(&current), &f.session, |_| {
            f.engine
                .db
                .lock()
                .unwrap()
                .execute(
                    "UPDATE sessions SET token_hash=?1 WHERE id=?2",
                    params!["0".repeat(64), f.session.as_str()],
                )
                .unwrap();
            Ok(HandyInspection {
                connected: false,
                identity: None,
                observed_at_unix_ms: 1,
            })
        });
    assert_eq!(outcome.unwrap_err().code, ErrorCode::Unauthorized);
}

#[test]
fn handy_registration_disabled_and_recreated_bindings_refuse_before_inspection() {
    let f = fixture();
    let first = handy_registration_saved(f.engine.handle(handy_registration_request(
        &f,
        "registration",
        0,
        "same",
    )));
    let inspect = request(
        &f.session,
        None,
        None,
        Command::InspectHandyRegistration {
            device_id: first.device_id.clone(),
            expected_registration_revision: 1,
            binding_sha256: first.binding_sha256.clone(),
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute("DELETE FROM handy_registrations", [])
        .unwrap();
    let recreated = handy_registration_saved(f.engine.handle(handy_registration_request(
        &f,
        "recreated",
        0,
        "same",
    )));
    assert_ne!(first.binding_sha256, recreated.binding_sha256);
    assert_eq!(
        f.engine
            .inspect_handy_registration_with(&inspect, &f.session, |_| panic!(
                "stale binding performed I/O"
            ))
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    let disabled = handy_registration_saved(f.engine.handle(request(
        &f.session,
        None,
        None,
        Command::DisableHandyRegistration {
            device_id: first.device_id,
            expected_registration_revision: 1,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )));
    let disabled_request = request(
        &f.session,
        None,
        None,
        Command::InspectHandyRegistration {
            device_id: disabled.device_id,
            expected_registration_revision: 2,
            binding_sha256: disabled.binding_sha256,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    assert_eq!(
        f.engine
            .inspect_handy_registration_with(&disabled_request, &f.session, |_| panic!(
                "disabled registration performed I/O"
            ))
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
}

#[test]
fn handy_registration_corrupt_metadata_refuses_startup_without_database_changes() {
    for corruption in [
        "DROP TABLE handy_registrations",
        "PRAGMA ignore_check_constraints=ON;UPDATE handy_registration_schema SET version=2",
        "UPDATE handy_registrations SET settings='{\"label\":\"changed\"}'",
    ] {
        let f = fixture();
        handy_registration_saved(f.engine.handle(handy_registration_request(
            &f,
            "registration",
            0,
            "initial",
        )));
        let config = Engine::retire_for_test(f.engine);
        let db = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        db.execute_batch(corruption).unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
        ];
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).ok()).collect();
        assert!(Engine::open(config).is_err());
        assert_eq!(
            paths.iter().map(|p| fs::read(p).ok()).collect::<Vec<_>>(),
            before
        );
    }
}

fn handy_resolution_request(f: &Fixture, original: &Request, id: &str) -> Request {
    let mut resolve = request(
        &f.session,
        None,
        None,
        Command::ResolveHandyRegistration {
            original_request_id: original.request_id.clone(),
            intent: original.command.handy_registration_intent().unwrap(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    resolve.request_id = RequestId::new(id).unwrap();
    resolve
}
fn handy_resolved(response: Response) -> HandyRegistrationResolution {
    let ResponseBody::HandyRegistrationResolution(resolution) = response.result.unwrap() else {
        panic!("resolution");
    };
    resolution.validate().unwrap();
    *resolution
}
#[test]
fn handy_registration_resolution_withdraws_absent_identity_and_survives_restart() {
    let f = fixture();
    let original = handy_registration_request(&f, "late-original", 0, "original");
    let resolve = handy_resolution_request(&f, &original, "withdraw-original");
    let withdrawn = handy_resolved(f.engine.handle(resolve.clone()));
    assert!(matches!(
        withdrawn.outcome,
        HandyRegistrationOutcome::Withdrawn
    ));
    assert_eq!(handy_resolved(f.engine.handle(resolve)), withdrawn);
    assert!(handy_registration_catalog(&f).registrations.is_empty());
    assert_eq!(
        f.engine.handle(original.clone()).result.unwrap_err().code,
        ErrorCode::Cancelled
    );
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = f;
    let config = Engine::retire_for_test(engine);
    let f = Fixture {
        engine: Engine::open(config).unwrap(),
        session,
        project,
        dir,
    };
    assert_eq!(
        f.engine.handle(original).result.unwrap_err().code,
        ErrorCode::Cancelled
    );
    let fresh = handy_registration_saved(
        f.engine
            .handle(handy_registration_request(&f, "fresh", 0, "fresh")),
    );
    assert_eq!(fresh.revision, 1);
    assert_eq!(fresh.settings.label, "fresh");
}
#[test]
fn handy_registration_resolution_preserves_applied_history_and_newer_configuration() {
    let f = fixture();
    let original = handy_registration_request(&f, "applied-original", 0, "first");
    let first = handy_registration_saved(f.engine.handle(original.clone()));
    let second = handy_registration_saved(
        f.engine
            .handle(handy_registration_request(&f, "later", 1, "second")),
    );
    let resolve = handy_resolution_request(&f, &original, "resolve-applied");
    let result = handy_resolved(f.engine.handle(resolve.clone()));
    let HandyRegistrationOutcome::Applied(receipt) = &result.outcome else {
        panic!("applied");
    };
    assert_eq!(receipt.registration, first);
    assert_eq!(handy_resolved(f.engine.handle(resolve)), result);
    assert_eq!(handy_registration_catalog(&f).registrations, [second]);
    let mut wrong = handy_resolution_request(&f, &original, "wrong-intent");
    if let Command::ResolveHandyRegistration {
        intent: HandyRegistrationIntent::Save { settings, .. },
        ..
    } = &mut wrong.command
    {
        settings.label = "different".into();
    }
    assert_eq!(
        f.engine.handle(wrong).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
}
#[test]
fn handy_registration_resolution_receipts_roll_back_as_one_transaction() {
    let f = fixture();
    let original = handy_registration_request(&f, "rollback-original", 0, "first");
    let resolve = handy_resolution_request(&f, &original, "resolution-rollback");
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_resolution BEFORE INSERT ON requests WHEN NEW.request='resolution-rollback' BEGIN SELECT RAISE(ABORT,'synthetic resolution receipt failure'); END;").unwrap();
    assert!(f.engine.handle(resolve.clone()).result.is_err());
    let db = f.engine.db.lock().unwrap();
    assert_eq!(db.query_row("SELECT count(*) FROM requests WHERE request IN ('rollback-original','resolution-rollback')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    db.execute_batch("DROP TRIGGER fail_resolution").unwrap();
    drop(db);
    handy_registration_saved(f.engine.handle(original));
    assert!(matches!(
        handy_resolved(f.engine.handle(resolve)).outcome,
        HandyRegistrationOutcome::Applied(_)
    ));
}
#[test]
fn handy_registration_resolution_race_has_one_consistent_original_outcome() {
    let f = fixture();
    let original = handy_registration_request(&f, "raced-original", 0, "race");
    let resolve = handy_resolution_request(&f, &original, "raced-resolution");
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let e = f.engine.clone();
    let b = barrier.clone();
    let change = std::thread::spawn(move || {
        b.wait();
        e.handle(original)
    });
    let e = f.engine.clone();
    let b = barrier.clone();
    let resolution = std::thread::spawn(move || {
        b.wait();
        e.handle(resolve)
    });
    barrier.wait();
    let changed = change.join().unwrap();
    let resolved = handy_resolved(resolution.join().unwrap());
    match resolved.outcome {
        HandyRegistrationOutcome::Applied(receipt) => {
            assert_eq!(handy_registration_saved(changed), receipt.registration);
            assert_eq!(handy_registration_catalog(&f).registrations.len(), 1);
        }
        HandyRegistrationOutcome::Withdrawn => {
            assert_eq!(changed.result.unwrap_err().code, ErrorCode::Cancelled);
            assert!(handy_registration_catalog(&f).registrations.is_empty());
        }
    }
}
