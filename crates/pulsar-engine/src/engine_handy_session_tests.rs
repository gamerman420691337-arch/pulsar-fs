fn handy_session_open_request(f: &Fixture, id: &str, registration: &HandyRegistration) -> Request {
    let mut r = request(
        &f.session,
        None,
        None,
        Command::OpenHandySession {
            engine_epoch: DeviceSessionId::new(f.engine.engine_epoch.clone()).unwrap(),
            device_id: registration.device_id.clone(),
            expected_registration_revision: registration.revision,
            binding_sha256: registration.binding_sha256.clone(),
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn handy_session_credentials() -> Arc<crate::handy_credentials::Credentials> {
    Arc::new(crate::handy_credentials::Credentials {
        api: "synthetic-api-secret".into(),
        connection: "synthetic-route-secret".into(),
    })
}
fn handy_session_observation() -> HandyInspection {
    HandyInspection {
        connected: true,
        identity: Some(HandyIdentity {
            firmware: "4.0.16".into(),
            model_number: 2,
            model_name: "H02".into(),
            model_variant: 0,
            session_id: "synthetic-remote-session".into(),
            firmware_status: 0,
        }),
        observed_at_unix_ms: 1,
    }
}
fn handy_session_call<F>(f: &Fixture, r: &Request, inspect: F) -> PResult<ResponseBody>
where
    F: FnOnce(Arc<crate::handy_credentials::Credentials>) -> PResult<HandyInspection>,
{
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine.handy_session_control_with(
        r,
        &f.session,
        &fingerprint,
        |_| Ok(handy_session_credentials()),
        inspect,
    )
}
fn handy_session_report(body: ResponseBody) -> HandyConnectionSession {
    let ResponseBody::HandySession(report) = body else {
        panic!("expected connection session")
    };
    report.validate().unwrap();
    *report
}
fn handy_session_catalog(f: &Fixture) -> HandyConnectionCatalog {
    let ResponseBody::HandySessions(c) = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::HandySessions {
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("catalog")
    };
    c.validate().unwrap();
    *c
}
fn handy_session_registered(f: &Fixture) -> HandyRegistration {
    handy_registration_saved(f.engine.handle(handy_registration_request(
        f,
        "registration",
        0,
        "session fixture",
    )))
}
fn handy_session_close_request(f: &Fixture, report: &HandyConnectionSession) -> Request {
    request(
        &f.session,
        None,
        None,
        Command::CloseHandySession {
            engine_epoch: report.engine_epoch.clone(),
            device_session: report.device_session.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
#[test]
fn handy_session_open_replay_close_and_late_open_never_reconnect() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    let open = handy_session_open_request(&f, "open", &reg);
    let observed = handy_session_report(
        handy_session_call(&f, &open, |_| Ok(handy_session_observation())).unwrap(),
    );
    assert_eq!(observed.state, HandyConnectionState::Observed);
    assert_eq!(handy_session_catalog(&f).sessions.len(), 1);
    let replay = handy_session_report(
        handy_session_call(&f, &open, |_| panic!("no repeat network")).unwrap(),
    );
    assert_eq!(replay.device_session, observed.device_session);
    let close = handy_session_close_request(&f, &observed);
    let closed = handy_session_report(f.engine.handle(close.clone()).result.unwrap());
    assert_eq!(closed.state, HandyConnectionState::Retired);
    assert_eq!(
        handy_session_report(f.engine.handle(close).result.unwrap()).state,
        HandyConnectionState::Retired
    );
    assert!(handy_session_catalog(&f).sessions.is_empty());
    assert_eq!(
        handy_session_report(handy_session_call(&f, &open, |_| panic!("late original")).unwrap())
            .state,
        HandyConnectionState::Retired
    );
    let fresh = handy_session_open_request(&f, "fresh", &reg);
    assert_ne!(
        handy_session_report(
            handy_session_call(&f, &fresh, |_| Ok(handy_session_observation())).unwrap()
        )
        .device_session,
        observed.device_session
    );
    let mut conflict = open.clone();
    if let Command::OpenHandySession { binding_sha256, .. } = &mut conflict.command {
        *binding_sha256 = "f".repeat(64);
    }
    assert_eq!(
        handy_session_call(&f, &conflict, |_| panic!())
            .unwrap_err()
            .code,
        ErrorCode::RequestConflict
    );
}
#[test]
fn handy_session_owner_consent_scope_and_boot_are_checked_before_loading() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    let original = handy_session_open_request(&f, "open", &reg);
    for (r, expected) in [
        {
            let mut r = original.clone();
            if let Command::OpenHandySession { owner_proof, .. } = &mut r.command {
                *owner_proof = "f".repeat(64);
            }
            (r, ErrorCode::Forbidden)
        },
        {
            let mut r = original.clone();
            if let Command::OpenHandySession {
                allow_online_inspection,
                ..
            } = &mut r.command
            {
                *allow_online_inspection = false;
            }
            (r, ErrorCode::Forbidden)
        },
        {
            let mut r = original.clone();
            r.project = Some(f.project.clone());
            (r, ErrorCode::InvalidRequest)
        },
        {
            let mut r = original.clone();
            if let Command::OpenHandySession { engine_epoch, .. } = &mut r.command {
                *engine_epoch = DeviceSessionId::new("other-boot").unwrap();
            }
            (r, ErrorCode::DependencyMismatch)
        },
    ] {
        let hash = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
        assert_eq!(
            f.engine
                .handy_session_control_with(
                    &r,
                    &f.session,
                    &hash,
                    |_| panic!("load forbidden"),
                    |_| panic!("network forbidden")
                )
                .unwrap_err()
                .code,
            expected
        );
    }
    assert!(handy_session_catalog(&f).sessions.is_empty());
}
#[test]
fn handy_session_freezes_credentials_and_excludes_alias_routes() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    let open = handy_session_open_request(&f, "open", &reg);
    let credentials = handy_session_credentials();
    let hash = format!("{:x}", Sha256::digest(request_fingerprint(&open).unwrap()));
    f.engine
        .handy_session_control_with(
            &open,
            &f.session,
            &hash,
            |_| Ok(credentials.clone()),
            |received| {
                assert!(Arc::ptr_eq(&credentials, &received));
                Ok(handy_session_observation())
            },
        )
        .unwrap();
    let mut alias = handy_registration_request(&f, "alias", 0, "alias");
    if let Command::SetHandyRegistration {
        device_id,
        settings,
        ..
    } = &mut alias.command
    {
        *device_id = DeviceId::new("alias-device").unwrap();
        settings.api_key_env = "OTHER_API".into();
        settings.connection_key_env = "OTHER_CONNECTION".into();
    }
    let alias = handy_registration_saved(f.engine.handle(alias));
    assert_eq!(
        handy_session_call(
            &f,
            &handy_session_open_request(&f, "alias-open", &alias),
            |_| panic!("alias must not inspect")
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceExhausted
    );
    let db = f.engine.db.lock().unwrap();
    let text: String = db
        .query_row(
            "SELECT response FROM requests WHERE request='open'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!text.contains("synthetic-api-secret") && !text.contains("synthetic-route-secret"));
}
#[test]
fn handy_session_pending_close_keeps_route_until_observer_returns() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    let open = handy_session_open_request(&f, "open", &reg);
    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let f_ref = &f;
        let open_ref = &open;
        let job = scope.spawn(move || {
            handy_session_call(f_ref, open_ref, |_| {
                ready_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                Ok(handy_session_observation())
            })
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let report = handy_session_catalog(&f).sessions.remove(0);
        assert_eq!(report.state, HandyConnectionState::Opening);
        assert_eq!(
            handy_session_report(handy_session_call(&f, &open, |_| panic!()).unwrap()).state,
            HandyConnectionState::Opening
        );
        let closed = handy_session_report(
            f.engine
                .handle(handy_session_close_request(&f, &report))
                .result
                .unwrap(),
        );
        assert_eq!(closed.state, HandyConnectionState::Retired);
        assert_eq!(
            handy_session_call(
                &f,
                &handy_session_open_request(&f, "second", &reg),
                |_| panic!()
            )
            .unwrap_err()
            .code,
            ErrorCode::ResourceExhausted
        );
        release_tx.send(()).unwrap();
        assert_eq!(job.join().unwrap().unwrap_err().code, ErrorCode::Cancelled);
    });
    assert!(handy_session_catalog(&f).sessions.is_empty());
}
#[test]
fn handy_session_registration_change_retires_pending_and_observed_connections() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    let open = handy_session_open_request(&f, "open", &reg);
    assert_eq!(
        handy_session_call(&f, &open, |_| {
            handy_registration_saved(
                f.engine
                    .handle(handy_registration_request(&f, "update", 1, "changed")),
            );
            assert_eq!(
                handy_session_catalog(&f).sessions[0].state,
                HandyConnectionState::Retired
            );
            Ok(handy_session_observation())
        })
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    assert!(handy_session_catalog(&f).sessions.is_empty());
    let reg = handy_registration_catalog(&f).registrations.remove(0);
    let next = handy_session_open_request(&f, "next", &reg);
    handy_session_call(&f, &next, |_| Ok(handy_session_observation())).unwrap();
    handy_registration_saved(f.engine.handle(handy_registration_request(
        &f,
        "update2",
        2,
        "changed again",
    )));
    assert!(handy_session_catalog(&f).sessions.is_empty());
    assert_eq!(
        handy_session_report(handy_session_call(&f, &next, |_| panic!()).unwrap()).state,
        HandyConnectionState::Retired
    );
}
#[test]
fn handy_session_inspection_error_or_panic_releases_without_replaying_network() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    let failed = handy_session_open_request(&f, "error", &reg);
    assert!(handy_session_call(&f, &failed, |_| Err(ProtocolError::new(
        ErrorCode::Unavailable,
        "fixture"
    )))
    .is_err());
    assert!(handy_session_catalog(&f).sessions.is_empty());
    assert_eq!(
        handy_session_report(handy_session_call(&f, &failed, |_| panic!()).unwrap()).state,
        HandyConnectionState::Retired
    );
    let panic_request = handy_session_open_request(&f, "panic", &reg);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handy_session_call(
            &f,
            &panic_request,
            |_| panic!("fixture inspector panic")
        )))
        .is_err()
    );
    assert!(handy_session_catalog(&f).sessions.is_empty());
    assert_eq!(
        handy_session_report(handy_session_call(&f, &panic_request, |_| panic!()).unwrap()).state,
        HandyConnectionState::Retired
    );
}
#[test]
fn handy_session_restart_rejects_old_boot_and_restores_no_credentials() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    let open = handy_session_open_request(&f, "open", &reg);
    handy_session_call(&f, &open, |_| Ok(handy_session_observation())).unwrap();
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
    assert!(handy_session_catalog(&f).sessions.is_empty());
    assert_eq!(
        handy_session_call(&f, &open, |_| panic!())
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    let mut retarget = open;
    if let Command::OpenHandySession { engine_epoch, .. } = &mut retarget.command {
        *engine_epoch = DeviceSessionId::new(f.engine.engine_epoch.clone()).unwrap();
    }
    assert_eq!(
        handy_session_call(&f, &retarget, |_| panic!())
            .unwrap_err()
            .code,
        ErrorCode::RequestConflict
    );
}
#[test]
fn handy_session_request_receipt_failure_prevents_inspection_and_reservation() {
    let f = fixture();
    let reg = handy_session_registered(&f);
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fixture_fail_request BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END;").unwrap();
    let open = handy_session_open_request(&f, "open", &reg);
    assert!(handy_session_call(&f, &open, |_| panic!("no unrecorded inspection")).is_err());
    assert!(handy_session_catalog(&f).sessions.is_empty());
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fixture_fail_request")
        .unwrap();
    handy_session_call(&f, &open, |_| Ok(handy_session_observation())).unwrap();
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fixture_fail_update BEFORE UPDATE ON handy_registrations BEGIN SELECT RAISE(ABORT,'fixture registration failure'); END;").unwrap();
    assert!(f
        .engine
        .handle(handy_registration_request(&f, "failed-update", 1, "failed"))
        .result
        .is_err());
    assert_eq!(
        handy_session_catalog(&f).sessions[0].state,
        HandyConnectionState::Observed
    );
}
