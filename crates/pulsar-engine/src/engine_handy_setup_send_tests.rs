fn setup_send_request(f: &Fixture, id: &str) -> (Request, HandyQualificationReport) {
    setup_send_request_with(f, id, |_| {})
}
fn setup_send_request_with(
    f: &Fixture,
    id: &str,
    change: impl FnOnce(&mut HandyQualificationInput),
) -> (Request, HandyQualificationReport) {
    let (inspection, connection, _) = setup_inputs(f);
    let (path, mut input) = qualification_input(f, HandyEvidenceOrigin::Physical);
    change(&mut input);
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(
                f,
                "setup-qualification",
                &path,
                &input,
            ))
            .result
            .unwrap(),
    );
    let accepted = qualification_saved(
        qualification_call(
            f,
            &qualification_accept(f, &imported.dossier_id, &connection),
            |_| Ok(setup_sample()),
        )
        .unwrap(),
    );
    let Command::InspectHandySetup {
        engine_epoch,
        device_session,
        hosting_request_id,
        owner_proof,
        ..
    } = inspection.command.clone()
    else {
        panic!()
    };
    let mut request = inspection;
    request.command = Command::SetupHandyDevice {
        qualification: HandyQualificationBinding {
            dossier_id: accepted.dossier_id.clone(),
            revision: accepted.revision,
        },
        engine_epoch,
        device_session,
        hosting_request_id,
        owner_proof,
        allow_online_setup: true,
    };
    request.request_id = RequestId::new(id).unwrap();
    (request, accepted)
}
fn setup_send_call<R, D, S>(
    f: &Fixture,
    r: &Request,
    recheck: R,
    device: D,
    send: S,
) -> PResult<ResponseBody>
where
    R: FnOnce(
        &crate::handy_hosting_wire::VerifiedHosted,
        std::time::Instant,
    ) -> PResult<ArtifactIdentity>,
    D: FnMut(
        Arc<crate::handy_credentials::Credentials>,
        std::time::Instant,
    ) -> PResult<HandySetupDeviceObservation>,
    S: FnOnce(
        &crate::handy_hosting_wire::VerifiedHosted,
        Arc<crate::handy_credentials::Credentials>,
        u64,
        std::time::Instant,
    ) -> crate::device_effect_journal::SetupCompletion,
{
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine
        .handy_setup_send_with(r, &f.session, &fingerprint, recheck, device, send)
}
#[test]
fn handy_setup_send_claims_before_io_and_replays_history_without_resending() {
    use crate::device_effect_journal::SetupCompletion;
    let f = fixture();
    let (request, _) = setup_send_request(&f, "setup-once");
    let first = setup_status_report(
        setup_send_call(
            &f,
            &request,
            |h, _| Ok(h.downloaded.clone()),
            |_, _| Ok(setup_sample()),
            |_, _, stream, _| {
                assert_eq!(stream, 7);
                assert!(f.engine.db.try_lock().is_ok());
                let journal = f.engine._device_effect_journal.try_lock().unwrap();
                assert_eq!(
                    journal
                        .setup_request_history(&f.session, &request.request_id)
                        .unwrap()
                        .unwrap()
                        .state,
                    crate::device_effect_journal::SetupJournalState::Claimed
                );
                SetupCompletion::Unknown
            },
        )
        .unwrap(),
    );
    assert_eq!(first.history.unwrap().state, HandySetupState::Unknown);
    assert!(!first.loaded && !first.qualified);
    let replay = setup_status_report(
        setup_send_call(
            &f,
            &request,
            |_, _| panic!("no replay GET"),
            |_, _| panic!("no replay observation"),
            |_, _, _, _| panic!("no repeated setup"),
        )
        .unwrap(),
    );
    assert_eq!(replay.history.unwrap().state, HandySetupState::Unknown);
    let mut different = request.clone();
    different.request_id = RequestId::new("another-attempt").unwrap();
    assert!(setup_send_call(
        &f,
        &different,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(setup_sample()),
        |_, _, _, _| panic!("reserved route must not send")
    )
    .is_err());
    let mut changed = request.clone();
    if let Command::SetupHandyDevice {
        hosting_request_id, ..
    } = &mut changed.command
    {
        *hosting_request_id = RequestId::new("changed").unwrap();
    }
    assert!(setup_send_call(
        &f,
        &changed,
        |_, _| panic!(),
        |_, _| panic!(),
        |_, _, _, _| panic!()
    )
    .is_err());
}
#[test]
fn handy_setup_send_refuses_active_modes_missing_consent_and_withdrawn_preflight_without_claims() {
    let f = fixture();
    let (request, accepted) = setup_send_request(&f, "setup-refuse");
    let mut denied = request.clone();
    if let Command::SetupHandyDevice {
        allow_online_setup, ..
    } = &mut denied.command
    {
        *allow_online_setup = false;
    }
    assert!(setup_send_call(
        &f,
        &denied,
        |_, _| panic!(),
        |_, _| panic!(),
        |_, _, _, _| panic!()
    )
    .is_err());
    for variant in 0..4 {
        let mut r = request.clone();
        r.request_id = RequestId::new(format!("mode-{variant}")).unwrap();
        assert!(setup_send_call(
            &f,
            &r,
            |h, _| Ok(h.downloaded.clone()),
            |_, _| {
                let mut s = setup_sample();
                match variant {
                    0 => {
                        s.mode.mode = 7;
                        s.remote_hssp = None;
                    }
                    1 => s.remote_hssp.as_mut().unwrap().play_state = HandyRemotePlayState::Playing,
                    2 => s.remote_hssp.as_mut().unwrap().looping = true,
                    _ => s.remote_hssp.as_mut().unwrap().playback_rate = 2.0,
                };
                Ok(s)
            },
            |_, _, _, _| panic!("inadmissible device state cannot set up")
        )
        .is_err());
        assert!(f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .setup_request_history(&f.session, &r.request_id)
            .unwrap()
            .is_none());
    }
    assert!(setup_send_call(
        &f,
        &request,
        |h, _| {
            f.engine
                .handle(qualification_withdraw(
                    &f,
                    &accepted.dossier_id,
                    accepted.revision,
                ))
                .result
                .unwrap();
            Ok(h.downloaded.clone())
        },
        |_, _| Ok(setup_sample()),
        |_, _, _, _| panic!("withdrawn preflight cannot send")
    )
    .is_err());
    assert!(f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .setup_request_history(&f.session, &request.request_id)
        .unwrap()
        .is_none());
}
#[test]
fn handy_setup_send_reservation_and_claim_failures_never_send() {
    let f = fixture();
    let (request, _) = setup_send_request(&f, "reserve-failure");
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER setup_reserve_failure BEFORE INSERT ON requests WHEN NEW.request='reserve-failure' BEGIN SELECT RAISE(ABORT,'fixture reserve failure'); END;").unwrap();
    assert!(setup_send_call(
        &f,
        &request,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(setup_sample()),
        |_, _, _, _| panic!("no send without reservation")
    )
    .is_err());
    assert!(f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .setup_request_history(&f.session, &request.request_id)
        .unwrap()
        .is_none());
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER setup_reserve_failure")
        .unwrap();
    let journal = Connection::open(
        f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    journal.execute_batch("CREATE TRIGGER setup_claim_failure BEFORE UPDATE ON setup_effects WHEN NEW.state='claimed' BEGIN SELECT RAISE(ABORT,'fixture claim failure'); END;").unwrap();
    assert!(setup_send_call(
        &f,
        &request,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(setup_sample()),
        |_, _, _, _| panic!("no send without claim")
    )
    .is_err());
    journal
        .execute_batch("DROP TRIGGER setup_claim_failure")
        .unwrap();
    let replay = setup_status_report(
        setup_send_call(
            &f,
            &request,
            |_, _| panic!(),
            |_, _| panic!(),
            |_, _, _, _| panic!("no automatic claim retry"),
        )
        .unwrap(),
    );
    assert_eq!(replay.history.unwrap().state, HandySetupState::Prepared);
}
#[test]
fn handy_setup_send_preserves_acknowledgment_after_withdrawal_without_loaded_authority() {
    let f = fixture();
    let (request, accepted) = setup_send_request(&f, "setup-ack");
    let report = setup_status_report(
        setup_send_call(
            &f,
            &request,
            |h, _| Ok(h.downloaded.clone()),
            |_, _| Ok(setup_sample()),
            |_, _, _, _| {
                f.engine
                    .handle(qualification_withdraw(
                        &f,
                        &accepted.dossier_id,
                        accepted.revision,
                    ))
                    .result
                    .unwrap();
                crate::device_effect_journal::SetupCompletion::Acknowledged(
                    crate::handy_wire::fixture_setup_acknowledgment(),
                )
            },
        )
        .unwrap(),
    );
    let observed = report.history.unwrap();
    assert_eq!(observed.state, HandySetupState::Acknowledged);
    assert_eq!(
        observed.acknowledgment,
        Some(crate::handy_wire::fixture_setup_acknowledgment())
    );
    assert!(!report.loaded && !report.qualified);
    let history = setup_status_report(f.engine.handle(request.clone()).result.unwrap());
    let repeated = history.history.unwrap();
    assert_eq!(repeated.state, HandySetupState::Acknowledged);
    assert_eq!(repeated.acknowledgment, observed.acknowledgment);
}
#[test]
fn handy_setup_send_completion_failure_leaves_claimed_history_and_never_reissues() {
    let f = fixture();
    let (request, _) = setup_send_request(&f, "setup-write-fail");
    let result = setup_send_call(
        &f,
        &request,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(setup_sample()),
        |_, _, _, _| {
            let journal = Connection::open(
                f.engine
                    .config
                    .state_dir
                    .join("device-effects/journal.sqlite3"),
            )
            .unwrap();
            journal.execute_batch("CREATE TRIGGER setup_completion_failure BEFORE UPDATE ON setup_effects WHEN NEW.state='acknowledged' BEGIN SELECT RAISE(ABORT,'fixture completion failure'); END;").unwrap();
            crate::device_effect_journal::SetupCompletion::Acknowledged(
                crate::handy_wire::fixture_setup_acknowledgment(),
            )
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::UnknownOutcome);
    let report = setup_status_report(
        setup_send_call(
            &f,
            &request,
            |_, _| panic!(),
            |_, _| panic!(),
            |_, _, _, _| panic!(),
        )
        .unwrap(),
    );
    assert_eq!(report.history.unwrap().state, HandySetupState::Claimed);
}

#[test]
fn handy_setup_send_missing_journal_or_record_refuses_restart_before_recreating_history() {
    for missing_file in [false, true] {
        let f = fixture();
        let (request, _) = setup_send_request(&f, "linked-setup");
        setup_send_call(
            &f,
            &request,
            |h, _| Ok(h.downloaded.clone()),
            |_, _| Ok(setup_sample()),
            |_, _, _, _| {
                crate::device_effect_journal::SetupCompletion::Acknowledged(
                    crate::handy_wire::fixture_setup_acknowledgment(),
                )
            },
        )
        .unwrap();
        let Fixture { engine, dir, .. } = f;
        let config = Engine::retire_for_test(engine);
        let journal = config.state_dir.join("device-effects/journal.sqlite3");
        if missing_file {
            std::fs::remove_file(&journal).unwrap();
        } else {
            let db = Connection::open(&journal).unwrap();
            db.execute("DELETE FROM setup_effects", []).unwrap();
        }
        let main = config.state_dir.join("projects.sqlite3");
        let before = std::fs::read(&main).unwrap();
        assert!(Engine::open(config).is_err());
        assert_eq!(std::fs::read(main).unwrap(), before);
        if missing_file {
            assert!(!journal.exists());
        }
        drop(dir);
    }
}
#[test]
fn handy_setup_send_failed_witness_write_never_claims_or_sends() {
    let f = fixture();
    let (request, _) = setup_send_request(&f, "witness-failure");
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER setup_witness_failure BEFORE UPDATE ON requests WHEN NEW.request='witness-failure' BEGIN SELECT RAISE(ABORT,'fixture witness failure'); END;").unwrap();
    assert!(setup_send_call(
        &f,
        &request,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(setup_sample()),
        |_, _, _, _| panic!("no send before durable witness")
    )
    .is_err());
    assert_eq!(
        f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .setup_request_history(&f.session, &request.request_id)
            .unwrap()
            .unwrap()
            .state,
        crate::device_effect_journal::SetupJournalState::Prepared
    );
}
