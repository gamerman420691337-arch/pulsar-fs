fn controller_inputs(f: &Fixture, duration_ms: u64) -> (Request, HandyPlaybackPreparation) {
    let prep_request = playback_preparation_inputs(f);
    let prep = playback_preparation_report(playback_preparation_call(f, &prep_request).unwrap());
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::AcquireHandyController {
            preparation_request_id: prep.request_id.clone(),
            preparation_id: prep.preparation_id.clone(),
            duration_ms,
            allow_arm: true,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new("acquire-controller").unwrap();
    (r, prep)
}
fn controller_report(body: ResponseBody) -> HandyControllerReceipt {
    let ResponseBody::HandyController(value) = body else {
        panic!("controller")
    };
    value.validate().unwrap();
    *value
}
fn controller_call(f: &Fixture, r: &Request, now: u64) -> PResult<ResponseBody> {
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine.handy_controller_with(
        r,
        &f.session,
        &fingerprint,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(program_device_sample()),
        || Ok(now),
    )
}
fn controller_status_request(f: &Fixture, lease: &HandyControllerLease) -> Request {
    request(
        &f.session,
        Some(&f.project),
        None,
        Command::HandyControllerStatus {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
fn controller_renew_request(f: &Fixture, lease: &HandyControllerLease, id: &str) -> Request {
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::RenewHandyController {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            controller_id: lease.controller_id.clone(),
            expected_generation: lease.generation,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
#[test]
fn handy_controller_renewal_generation_replay_and_expiry_do_not_restore_or_extend_old_authority() {
    let f = fixture();
    let (acquire, prep) = controller_inputs(&f, 1000);
    let base = 1_000_000_000;
    let first = controller_report(controller_call(&f, &acquire, base).unwrap());
    assert!(first.lease.armed);
    assert!(!first.lease.playback_enabled);
    assert_eq!(
        playback_preparation_report(
            f.engine
                .handle(preparation_status_request(&f, &prep.request_id))
                .result
                .unwrap()
        )
        .state,
        HandyPlaybackPreparationState::Retired
    );
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 1);
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(request_fingerprint(&acquire).unwrap())
    );
    let repeat = controller_report(
        f.engine
            .handy_controller_with(
                &acquire,
                &f.session,
                &fingerprint,
                |_, _| panic!("no replay GET"),
                |_, _| panic!(),
                || Ok(base + 1),
            )
            .unwrap(),
    );
    assert_eq!(
        repeat.lease.deadline_boottime_ns,
        first.lease.deadline_boottime_ns
    );
    let renewal = controller_renew_request(&f, &first.lease, "renew-once");
    let renewed = controller_report(controller_call(&f, &renewal, base + 50_000_000).unwrap());
    assert_eq!(renewed.lease.generation, 1);
    let retry = controller_report(controller_call(&f, &renewal, base + 75_000_000).unwrap());
    assert_eq!(
        retry.lease.deadline_boottime_ns,
        renewed.lease.deadline_boottime_ns
    );
    assert!(controller_call(
        &f,
        &controller_renew_request(&f, &first.lease, "stale-renewal"),
        base + 100_000_000
    )
    .is_err());
    let status = controller_status_request(&f, &first.lease);
    assert_eq!(
        controller_report(controller_call(&f, &status, first.lease.deadline_boottime_ns).unwrap())
            .lease
            .state,
        HandyControllerState::Active
    );
    assert_eq!(
        controller_report(
            controller_call(&f, &status, renewed.lease.deadline_boottime_ns).unwrap()
        )
        .lease
        .state,
        HandyControllerState::Retired
    );
    assert_eq!(
        controller_report(
            controller_call(&f, &acquire, renewed.lease.deadline_boottime_ns + 1).unwrap()
        )
        .lease
        .state,
        HandyControllerState::Retired
    );
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
}
#[test]
fn handy_controller_failed_renewal_and_expiry_during_commit_do_not_publish_new_lifetime() {
    let f = fixture();
    let (acquire, _) = controller_inputs(&f, 1000);
    let base = 1_000_000_000;
    let first = controller_report(controller_call(&f, &acquire, base).unwrap());
    let renew = controller_renew_request(&f, &first.lease, "renew-fail");
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_renew BEFORE INSERT ON requests WHEN NEW.request='renew-fail' BEGIN SELECT RAISE(ABORT,'fixture renewal failure'); END;").unwrap();
    assert!(controller_call(&f, &renew, base + 1).is_err());
    let status = controller_report(
        controller_call(&f, &controller_status_request(&f, &first.lease), base + 2).unwrap(),
    );
    assert_eq!(status.lease.generation, 0);
    assert_eq!(
        status.lease.deadline_boottime_ns,
        first.lease.deadline_boottime_ns
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_renew")
        .unwrap();
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&renew).unwrap()));
    let mut calls = 0;
    let result = controller_report(
        f.engine
            .handy_controller_with(
                &renew,
                &f.session,
                &fingerprint,
                |_, _| panic!(),
                |_, _| panic!(),
                || {
                    calls += 1;
                    Ok(if calls < 4 {
                        base + 3
                    } else {
                        base + 2_000_000_000
                    })
                },
            )
            .unwrap(),
    );
    assert_eq!(result.lease.state, HandyControllerState::Retired);
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
}
#[test]
fn handy_controller_release_and_all_preparation_retirement_boundaries_disarm_without_effects() {
    for event in 0..7 {
        let f = fixture();
        let (r, prep) = controller_inputs(&f, 1000);
        let lease = controller_report(controller_call(&f, &r, 1_000_000_000).unwrap()).lease;
        let i = &prep.correlation.inspection;
        let action = match event {
            0 => request(
                &f.session,
                Some(&f.project),
                None,
                Command::ReleaseHandyController {
                    acquisition_request_id: lease.acquisition_request_id.clone(),
                    controller_id: lease.controller_id.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            1 => request(
                &f.session,
                Some(&f.project),
                Some(1),
                commit_program(&f, 1, program(0.3)),
            ),
            2 => request(
                &f.session,
                None,
                None,
                Command::CloseHandySession {
                    engine_epoch: i.engine_epoch.clone(),
                    device_session: i.device_session.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            3 => request(
                &f.session,
                None,
                None,
                Command::DisableHandyRegistration {
                    device_id: i.registration.device_id.clone(),
                    expected_registration_revision: i.registration.revision,
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            4 => qualification_withdraw(
                &f,
                &i.accepted_qualification.as_ref().unwrap().dossier_id,
                i.accepted_qualification.as_ref().unwrap().revision,
            ),
            5 => request(
                &f.session,
                Some(&f.project),
                None,
                Command::ForgetHandyHosting {
                    upload_request_id: i.hosting_request_id.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            _ => request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ),
        };
        if event == 0 {
            controller_call(&f, &action, 1_000_000_001).unwrap();
        } else {
            f.engine.handle(action).result.unwrap();
        }
        assert_eq!(
            f.engine.handy_preparations.lock().unwrap().len(),
            0,
            "event {event}"
        );
        assert!(!f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .has_device_effects(&lease.device_id)
            .unwrap());
    }
}
#[test]
fn handy_controller_clock_reversal_and_restart_retire_authority_without_reconstruction() {
    let f = fixture();
    let (r, _) = controller_inputs(&f, 1000);
    let initial = controller_report(controller_call(&f, &r, 1_000_000_000).unwrap());
    assert_eq!(
        controller_report(
            controller_call(
                &f,
                &controller_status_request(&f, &initial.lease),
                999_999_999
            )
            .unwrap()
        )
        .lease
        .state,
        HandyControllerState::Retired
    );
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
    let status = controller_status_request(&f, &initial.lease);
    let Fixture { engine, dir, .. } = f;
    let engine = Engine::open(Engine::retire_for_test(engine)).unwrap();
    let saved = controller_report(engine.handle(status).result.unwrap());
    assert_eq!(saved.lease.state, HandyControllerState::Retired);
    assert!(!saved.lease.armed);
    drop(engine);
    drop(dir);
}
#[test]
#[cfg(target_os = "linux")]
fn handy_controller_owned_monitor_expires_without_requests_or_database_lock_and_joins() {
    let f = fixture();
    let (r, _) = controller_inputs(&f, 100);
    // Fixture creation and durable request publication may take longer than the
    // lease. Advance its clock only after the active precondition is established.
    let tick = Arc::new(std::sync::atomic::AtomicU64::new(1_000_000_000));
    let monitor_tick = tick.clone();
    f.engine
        .start_handy_player_monitor_for_test(Arc::new(move || {
            Ok(monitor_tick.load(Ordering::Acquire))
        }))
        .unwrap();
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    let value = controller_report(
        f.engine
            .handy_controller_with(
                &r,
                &f.session,
                &fingerprint,
                |h, _| Ok(h.downloaded.clone()),
                |_, _| Ok(program_device_sample()),
                || Ok(tick.load(Ordering::Acquire)),
            )
            .unwrap(),
    );
    assert_eq!(value.lease.state, HandyControllerState::Active);
    let db = f.engine.db.lock().unwrap();
    assert_eq!(value.lease.deadline_boottime_ns, 1_100_000_000);
    tick.store(value.lease.deadline_boottime_ns, Ordering::Release);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while f.engine.handy_preparations.lock().unwrap().len() != 0 {
        assert!(
            std::time::Instant::now() < until,
            "expiry worker waited for DB"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(db);
    let monitor = f.engine.handy_controller_monitor.lock().unwrap().take();
    drop(monitor);
    let Fixture { engine, dir, .. } = f;
    let _ = Engine::retire_for_test(engine);
    drop(dir);
}
#[test]
#[cfg(target_os = "linux")]
fn handy_controller_monitor_spawn_failure_creates_no_armed_state_or_receipt() {
    let f = fixture();
    let (r, _) = controller_inputs(&f, 1000);
    f.engine.fail_next_handy_controller_monitor();
    assert_eq!(
        f.engine.handle(r.clone()).result.unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM requests WHERE request=?1",
                [r.request_id.as_str()],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 1);
}

#[test]
#[cfg(target_os = "linux")]
fn handy_controller_expiry_progresses_while_database_writer_refuses_or_delays_renewal() {
    let f = fixture();
    let (r, _) = controller_inputs(&f, 500);
    f.engine.ensure_handy_controller_monitor().unwrap();
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    let first = controller_report(
        f.engine
            .handy_controller_with(
                &r,
                &f.session,
                &fingerprint,
                |h, _| Ok(h.downloaded.clone()),
                |_, _| Ok(program_device_sample()),
                handy_playback_preparation::controller_test_clock,
            )
            .unwrap(),
    );
    let blocker = Connection::open(f.engine.config.state_dir.join("projects.sqlite3")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let renewal = controller_renew_request(&f, &first.lease, "blocked-renewal");
    let engine = f.engine.clone();
    let mut task = Some(std::thread::spawn(move || engine.handle(renewal)));
    std::thread::sleep(std::time::Duration::from_millis(50));
    if task.as_ref().unwrap().is_finished() {
        let response = task.take().unwrap().join().unwrap();
        let error = response
            .result
            .expect_err("writer contention must not publish a renewal");
        assert!(
            error.message.contains("locked"),
            "unexpected early renewal error: {error:?}"
        );
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while f.engine.handy_preparations.lock().unwrap().len() != 0 {
        assert!(
            std::time::Instant::now() < until,
            "expiry waited behind a SQLite writer"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    blocker.execute_batch("ROLLBACK").unwrap();
    if let Some(task) = task {
        if let Ok(body) = task.join().unwrap().result {
            assert_eq!(
                controller_report(body).lease.state,
                HandyControllerState::Retired
            );
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn handy_controller_missing_monitor_retires_active_authority_before_deadline() {
    let f = fixture();
    let (r, _) = controller_inputs(&f, 30_000);
    f.engine.ensure_handy_controller_monitor().unwrap();
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    let first = controller_report(
        f.engine
            .handy_controller_with(
                &r,
                &f.session,
                &fingerprint,
                |h, _| Ok(h.downloaded.clone()),
                |_, _| Ok(program_device_sample()),
                handy_playback_preparation::controller_test_clock,
            )
            .unwrap(),
    );
    let monitor = f.engine.handy_controller_monitor.lock().unwrap().take();
    drop(monitor);
    assert!(
        handy_playback_preparation::controller_test_clock().unwrap()
            < first.lease.deadline_boottime_ns
    );
    let status = controller_report(
        f.engine
            .handle(controller_status_request(&f, &first.lease))
            .result
            .unwrap(),
    );
    assert_eq!(status.lease.state, HandyControllerState::Retired);
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
}

#[test]
fn handy_controller_old_release_cannot_retire_a_replacement_controller() {
    let f = fixture();
    let (r, prep) = controller_inputs(&f, 1000);
    let first = controller_report(controller_call(&f, &r, 1_000_000_000).unwrap());
    let release = request(
        &f.session,
        Some(&f.project),
        None,
        Command::ReleaseHandyController {
            acquisition_request_id: first.lease.acquisition_request_id.clone(),
            controller_id: first.lease.controller_id.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    controller_call(&f, &release, 1_000_000_001).unwrap();
    let prepare = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::PrepareHandyPlayback {
            program: prep.selection().unwrap(),
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    let next = playback_preparation_report(playback_preparation_call(&f, &prepare).unwrap());
    let acquire = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::AcquireHandyController {
            preparation_request_id: next.request_id,
            preparation_id: next.preparation_id,
            duration_ms: 1000,
            allow_arm: true,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    let second = controller_report(controller_call(&f, &acquire, 1_000_000_002).unwrap());
    controller_call(&f, &release, 1_000_000_003).unwrap();
    let status = controller_report(
        controller_call(
            &f,
            &controller_status_request(&f, &second.lease),
            1_000_000_004,
        )
        .unwrap(),
    );
    assert_eq!(status.lease.state, HandyControllerState::Active);
    assert_ne!(status.lease.controller_id, first.lease.controller_id);
}
