fn server_clock_inputs(f: &Fixture) -> (Request, HandyControllerLease) {
    server_clock_inputs_with_subject(f, |_| {})
}
fn server_clock_inputs_with_subject(
    f: &Fixture,
    change: impl FnOnce(&mut HandyQualificationSubject),
) -> (Request, HandyControllerLease) {
    let (setup, _) = setup_send_request_with(f, "clock-setup", |input| {
        input.subject.schema_version = 3;
        input.subject.server_clock = Some(HandyServerClockQualification {
            model: HandyServerClockModel::RequestBracketMonotonic,
            timestamp_error_ns: 1_000,
        });
        input.subject.profile.maximum_command_age_ns = 100_000_000;
        input.subject.profile.maximum_clock_uncertainty_ns = 10_000_000;
        change(&mut input.subject);
    });
    let h = setup_status_report(
        setup_send_call(
            f,
            &setup,
            |h, _| Ok(h.downloaded.clone()),
            |_, _| Ok(setup_sample()),
            |_, _, _, _| {
                crate::device_effect_journal::SetupCompletion::Acknowledged(
                    crate::handy_wire::fixture_setup_acknowledgment(),
                )
            },
        )
        .unwrap(),
    )
    .history
    .unwrap();
    let mut preparation = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::PrepareHandyPlayback {
            program: HandyProgramSelection {
                setup_request_id: setup.request_id,
                record_id: h.record_id,
                qualification: h.qualification,
                engine_epoch: h.engine_epoch,
                device_session: h.device_session,
                hosting_request_id: h.hosting_request_id,
            },
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    preparation.request_id = RequestId::new("clock-preparation").unwrap();
    let prep = playback_preparation_report(playback_preparation_call(f, &preparation).unwrap());
    let mut acquire = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::AcquireHandyController {
            preparation_request_id: prep.request_id,
            preparation_id: prep.preparation_id,
            duration_ms: 1000,
            allow_arm: true,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    acquire.request_id = RequestId::new("clock-controller").unwrap();
    let lease = controller_report(controller_call(f, &acquire, 1_000_000_000).unwrap()).lease;
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::SampleHandyServerClock {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            controller_id: lease.controller_id.clone(),
            expected_generation: lease.generation,
            allow_online_sampling: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new("server-clock-sample").unwrap();
    (r, lease)
}
fn server_clock_report(body: ResponseBody) -> HandyServerClockObservation {
    let ResponseBody::HandyServerClock(r) = body else {
        panic!("clock report")
    };
    r.validate().unwrap();
    *r
}
fn server_clock_call(
    f: &Fixture,
    r: &Request,
    now: u64,
    sample: impl FnOnce(&mut dyn FnMut() -> PResult<u64>) -> PResult<crate::handy_server_clock::Sample>,
) -> PResult<ResponseBody> {
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine
        .handy_server_clock_with(r, &f.session, &fingerprint, sample, || Ok(now))
}
fn server_clock_sample(
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> PResult<crate::handy_server_clock::Sample> {
    Ok(crate::handy_server_clock::Sample {
        sent_ns: clock()?,
        received_ns: clock()?,
        server_ms: 1_000_000,
    })
}
#[test]
fn handy_server_clock_installs_native_mapping_and_replay_does_not_resample_or_refresh_age() {
    let f = fixture();
    let (r, lease) = server_clock_inputs(&f);
    let first = server_clock_report(
        server_clock_call(&f, &r, 1_000_000_000, |clock| {
            assert!(f.engine.db.try_lock().is_ok());
            assert!(f.engine.handy_preparations.try_lock().is_ok());
            assert!(f.engine._device_effect_journal.try_lock().is_ok());
            server_clock_sample(clock)
        })
        .unwrap(),
    );
    assert!(first.current);
    assert!(!first.playback_enabled);
    for (now, current) in [
        (1_000_000_001, true),
        (1_100_000_000, true),
        (1_100_000_001, false),
    ] {
        let replay = server_clock_report(
            server_clock_call(&f, &r, now, |_| panic!("replay must not GET")).unwrap(),
        );
        assert_eq!(replay.current, current);
        assert_eq!(replay.sent_boottime_ns, first.sent_boottime_ns);
        assert_eq!(replay.clock_domain, first.clock_domain);
        assert_eq!(
            replay.controller.deadline_boottime_ns,
            lease.deadline_boottime_ns
        );
    }
    let mut changed = r.clone();
    changed.expected_revision = Some(RevisionId::new(2));
    assert!(server_clock_call(&f, &changed, 1_100_000_002, |_| panic!()).is_err());
}
#[test]
fn handy_server_clock_legacy_qualification_and_wrong_scope_refuse_before_io() {
    let f = fixture();
    let (acquire, _) = controller_inputs(&f, 1000);
    let lease = controller_report(controller_call(&f, &acquire, 1_000_000_000).unwrap()).lease;
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::SampleHandyServerClock {
            acquisition_request_id: lease.acquisition_request_id,
            controller_id: lease.controller_id,
            expected_generation: 0,
            allow_online_sampling: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new("legacy-sample").unwrap();
    assert_eq!(
        server_clock_call(&f, &r, 1_000_000_000, |_| panic!())
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let f = fixture();
    let (r, _) = server_clock_inputs(&f);
    for case in 0..6 {
        let mut bad = r.clone();
        let Command::SampleHandyServerClock {
            expected_generation,
            allow_online_sampling,
            owner_proof,
            controller_id,
            acquisition_request_id,
        } = &mut bad.command
        else {
            unreachable!()
        };
        match case {
            0 => *expected_generation = 1,
            1 => *allow_online_sampling = false,
            2 => *owner_proof = "wrong".into(),
            3 => *controller_id = ClientId::new("foreign").unwrap(),
            4 => *acquisition_request_id = RequestId::new("foreign").unwrap(),
            _ => bad.project = Some(ProjectId::new("foreign").unwrap()),
        }
        assert!(
            server_clock_call(&f, &bad, 1_000_000_000, |_| panic!("no unauthorized GET")).is_err()
        );
    }
}
#[test]
fn handy_server_clock_expiry_renewal_and_revision_change_during_io_prevent_installation() {
    for event in 0..4 {
        let f = fixture();
        let (r, lease) = server_clock_inputs(&f);
        let result = server_clock_call(&f, &r, 1_000_000_000, |clock| {
            let s = server_clock_sample(clock)?;
            match event {
                0 => {
                    controller_call(
                        &f,
                        &controller_status_request(&f, &lease),
                        lease.deadline_boottime_ns,
                    )
                    .unwrap();
                }
                1 => {
                    controller_call(
                        &f,
                        &controller_renew_request(&f, &lease, "renew-during-sample"),
                        1_000_000_000,
                    )
                    .unwrap();
                }
                2 => {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(1),
                            commit_program(&f, 1, program(0.3)),
                        ))
                        .result
                        .unwrap();
                }
                _ => {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            None,
                            Command::ReleaseHandyController {
                                acquisition_request_id: lease.acquisition_request_id.clone(),
                                controller_id: lease.controller_id.clone(),
                                owner_proof: f.engine.handy_owner_token.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                }
            }
            Ok(s)
        });
        assert!(result.is_err());
    }
}
#[test]
fn handy_server_clock_public_monitor_loss_and_restart_never_restore_mapping() {
    let f = fixture();
    let (r, _) = server_clock_inputs(&f);
    server_clock_call(&f, &r, 1_000_000_000, server_clock_sample).unwrap();
    // The test seam deliberately has no real monitor: public routing must retire it.
    assert!(f.engine.handle(r.clone()).result.is_err());
    let Fixture { engine, dir, .. } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    let report = server_clock_report(engine.handle(r).result.unwrap());
    assert!(!report.current);
    assert_eq!(engine.handy_preparations.lock().unwrap().len(), 0);
    drop(engine);
    drop(dir);
}
#[test]
fn handy_server_clock_failed_commit_and_new_attempt_do_not_restore_prior_sample() {
    let f = fixture();
    let (r, _) = server_clock_inputs(&f);
    let first =
        server_clock_report(server_clock_call(&f, &r, 1_000_000_000, server_clock_sample).unwrap());
    let mut next = r.clone();
    next.request_id = RequestId::new("clock-fail-write").unwrap();
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER clock_fail BEFORE INSERT ON requests WHEN NEW.request='clock-fail-write' BEGIN SELECT RAISE(ABORT,'clock fixture write failure'); END;").unwrap();
    assert!(server_clock_call(&f, &next, 1_000_000_001, server_clock_sample).is_err());
    let replay =
        server_clock_report(server_clock_call(&f, &r, 1_000_000_002, |_| panic!()).unwrap());
    assert!(!replay.current);
    assert_eq!(first.clock_domain, replay.clock_domain);
}

#[test]
fn handy_server_clock_superseded_request_cannot_replace_newer_native_observation() {
    let f = fixture();
    let (r, lease) = server_clock_inputs(&f);
    let mut newer = r.clone();
    newer.request_id = RequestId::new("newer-sample").unwrap();
    let result = server_clock_call(&f, &r, 1_000_000_000, |clock| {
        let old = server_clock_sample(clock)?;
        assert!(
            server_clock_report(
                server_clock_call(&f, &newer, 1_000_000_000, server_clock_sample).unwrap()
            )
            .current
        );
        Ok(old)
    });
    assert!(result.is_err());
    assert!(
        server_clock_report(server_clock_call(&f, &newer, 1_000_000_000, |_| panic!()).unwrap())
            .current
    );
    controller_call(
        &f,
        &controller_renew_request(&f, &lease, "after-sample-renewal"),
        1_000_000_001,
    )
    .unwrap();
    assert!(
        !server_clock_report(server_clock_call(&f, &newer, 1_000_000_002, |_| panic!()).unwrap())
            .current
    );
}
