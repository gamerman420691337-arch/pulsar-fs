fn setup_inputs(f: &Fixture) -> (Request, HandyConnectionSession, HandyHostingReport) {
    let p = hosting_prepared(f);
    let h = hosting_request(f, &p, "host-for-setup");
    let hosted = hosting_report(
        hosting_call(f, &h, |bytes| {
            Ok(crate::handy_hosting_wire::fixture_verified(bytes))
        })
        .unwrap(),
    );
    let registration = handy_session_registered(f);
    let open = handy_session_open_request(f, "open-for-setup", &registration);
    let connection = handy_session_report(
        handy_session_call(f, &open, |_| Ok(handy_session_observation())).unwrap(),
    );
    let r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::InspectHandySetup {
            qualification: None,
            engine_epoch: connection.engine_epoch.clone(),
            device_session: connection.device_session.clone(),
            hosting_request_id: hosted.request_id.clone(),
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    (r, connection, hosted)
}
fn setup_sample() -> HandySetupDeviceObservation {
    HandySetupDeviceObservation {
        operative_settings: serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-operative-settings.json"
        ))
        .unwrap(),
        identity: handy_session_observation().identity.unwrap(),
        mode: HandyDeviceMode {
            mode: 1,
            mode_session_id: 10,
        },
        remote_hssp: Some(HandyHsspObservation {
            play_state: HandyRemotePlayState::Stopped,
            stream_id: 7,
            current_time_ms: 0,
            playback_rate: 1.0,
            looping: false,
            pause_on_starving: false,
        }),
        remote_hsp: None,
        observed_at_unix_ms: 1,
    }
}
#[test]
fn handy_setup_inspection_binds_retained_credentials_and_hosted_bytes_without_writes() {
    let f = fixture();
    let (r, c, h) = setup_inputs(&f);
    let mut credentials = None;
    let mut deadline = None;
    let mut samples = 0;
    let before: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    let ResponseBody::HandySetupInspection(report) = f
        .engine
        .inspect_handy_setup_with(
            &r,
            &f.session,
            |hosted, _| Ok(hosted.downloaded.clone()),
            |keys, end| {
                if let Some(prior) = &credentials {
                    assert!(Arc::ptr_eq(prior, &keys));
                    assert_eq!(deadline, Some(end));
                } else {
                    credentials = Some(keys);
                    deadline = Some(end);
                }
                samples += 1;
                Ok(setup_sample())
            },
        )
        .unwrap()
    else {
        panic!("inspection")
    };
    report.validate().unwrap();
    assert_eq!(samples, 2);
    assert_eq!(report.device_session, c.device_session);
    assert_eq!(report.hosting_request_id, h.request_id);
    assert_eq!(report.hosted, h.downloaded.unwrap());
    assert!(!report.qualified && !report.setup_performed);
    let after: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert!(!encode(&report).unwrap().contains("https://"));
    assert!(!encode(&report).unwrap().contains("synthetic-route-secret"));
}
#[test]
fn handy_setup_inspection_checks_consent_scope_and_epoch_before_network() {
    let f = fixture();
    let (r, _, _) = setup_inputs(&f);
    let mut denied = r.clone();
    if let Command::InspectHandySetup {
        allow_online_inspection,
        ..
    } = &mut denied.command
    {
        *allow_online_inspection = false;
    }
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(&denied, &f.session, |_, _| panic!(), |_, _| panic!())
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    denied = r.clone();
    if let Command::InspectHandySetup { engine_epoch, .. } = &mut denied.command {
        *engine_epoch = DeviceSessionId::new("retired").unwrap();
    }
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(&denied, &f.session, |_, _| panic!(), |_, _| panic!())
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    denied = r.clone();
    denied.expected_revision = Some(RevisionId::new(0));
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(&denied, &f.session, |_, _| panic!(), |_, _| panic!())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
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
        f.engine
            .inspect_handy_setup_with(&r, &f.session, |_, _| panic!(), |_, _| panic!())
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}
#[test]
fn handy_setup_inspection_rejects_changed_hosted_identity_and_remote_mode_or_stream() {
    let f = fixture();
    let (r, _, _) = setup_inputs(&f);
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(
                &r,
                &f.session,
                |hosted, _| {
                    let mut value = hosted.downloaded.clone();
                    value.sha256 = "f".repeat(64);
                    Ok(value)
                },
                |_, _| Ok(setup_sample())
            )
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    for mode_change in [true, false] {
        let mut calls = 0;
        assert_eq!(
            f.engine
                .inspect_handy_setup_with(
                    &r,
                    &f.session,
                    |h, _| Ok(h.downloaded.clone()),
                    |_, _| {
                        calls += 1;
                        let mut sample = setup_sample();
                        if calls == 2 {
                            if mode_change {
                                sample.mode.mode_session_id += 1;
                            } else {
                                sample.remote_hssp.as_mut().unwrap().stream_id += 1;
                            }
                        }
                        Ok(sample)
                    }
                )
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
    }
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(
                &r,
                &f.session,
                |_, _| panic!("old identity must refuse first"),
                |_, _| {
                    let mut s = setup_sample();
                    s.identity.session_id = "replacement-device-session".into();
                    Ok(s)
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
}
#[test]
fn handy_setup_inspection_rechecks_forgotten_hosting_and_closed_connection_after_io() {
    let f = fixture();
    let (r, c, h) = setup_inputs(&f);
    let result = f.engine.inspect_handy_setup_with(
        &r,
        &f.session,
        |value, _| {
            f.engine
                .handle(hosting_forget(&f, &h.request_id))
                .result
                .unwrap();
            Ok(value.downloaded.clone())
        },
        |_, _| Ok(setup_sample()),
    );
    assert!(result.is_err());
    let fresh = hosting_request(
        &f,
        &handy_script_receipt(
            f.engine
                .handle(handy_script_request(
                    &f,
                    "prepare-again",
                    f.dir.path().join("again.csv"),
                    1,
                ))
                .result
                .unwrap(),
        ),
        "host-again",
    );
    let hosted = hosting_report(
        hosting_call(&f, &fresh, |b| {
            Ok(crate::handy_hosting_wire::fixture_verified(b))
        })
        .unwrap(),
    );
    let mut r = r;
    if let Command::InspectHandySetup {
        hosting_request_id, ..
    } = &mut r.command
    {
        *hosting_request_id = hosted.request_id;
    }
    assert!(f
        .engine
        .inspect_handy_setup_with(
            &r,
            &f.session,
            |value, _| {
                f.engine
                    .handle(handy_session_close_request(&f, &c))
                    .result
                    .unwrap();
                Ok(value.downloaded.clone())
            },
            |_, _| Ok(setup_sample())
        )
        .is_err());
}
#[test]
fn handy_setup_inspection_rejects_revision_and_registration_changes_during_io() {
    let f = fixture();
    let (r, _, _) = setup_inputs(&f);
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(
                &r,
                &f.session,
                |h, _| {
                    f.engine
                        .handle(handy_registration_request(
                            &f,
                            "change-registration",
                            1,
                            "new label",
                        ))
                        .result
                        .unwrap();
                    Ok(h.downloaded.clone())
                },
                |_, _| Ok(setup_sample())
            )
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let f = fixture();
    let (r, _, _) = setup_inputs(&f);
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(
                &r,
                &f.session,
                |h, _| {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(1),
                            commit_program(&f, 1, program(0.3)),
                        ))
                        .result
                        .unwrap();
                    Ok(h.downloaded.clone())
                },
                |_, _| Ok(setup_sample())
            )
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}
