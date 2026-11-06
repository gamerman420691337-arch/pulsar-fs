fn qualified_sync_inputs(f: &Fixture) -> (Request, HandyControllerLease) {
    let (mut r, l) = server_clock_inputs_with_subject(f, |s| {
        s.schema_version = 4;
        s.device_clock = Some(HandyDeviceClockQualification {
            model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
            user_offset_ms: -50,
            device_error_ns: 2_000_000,
            maximum_sync_age_ns: 500_000_000,
            maximum_sync_round_trip_ms: 100,
            drift_ppm: 20,
        });
    });
    r.request_id = RequestId::new("qualified-sync").unwrap();
    r.command = Command::SynchronizeHandyClock {
        acquisition_request_id: l.acquisition_request_id.clone(),
        controller_id: l.controller_id.clone(),
        expected_generation: 0,
        allow_online_synchronization: true,
        owner_proof: f.engine.handy_owner_token.clone(),
    };
    (r, l)
}
fn clock_replacement(
    f: &Fixture,
    source: &HandyControllerLease,
    name: &str,
    now: u64,
) -> HandyControllerLease {
    let prior = playback_preparation_report(
        f.engine
            .handle(preparation_status_request(
                f,
                &source.preparation_request_id,
            ))
            .result
            .unwrap(),
    );
    let mut p = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::PrepareHandyPlayback {
            program: prior.selection().unwrap(),
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    p.request_id = RequestId::new(format!("{name}-preparation")).unwrap();
    let prepared = playback_preparation_report(playback_preparation_call(f, &p).unwrap());
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::AcquireHandyController {
            preparation_request_id: prepared.request_id,
            preparation_id: prepared.preparation_id,
            duration_ms: 1000,
            allow_arm: true,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(name).unwrap();
    controller_report(controller_call(f, &r, now).unwrap()).lease
}
fn playback_clock_inputs(f: &Fixture) -> (Request, HandyControllerLease, HandyControllerLease) {
    let (sync, old) = qualified_sync_inputs(f);
    let began = std::time::Instant::now();
    let sender_ran = std::cell::Cell::new(false);
    let h = sync_report(
        // sync_call supplies a fixed BOOTTIME. Keep its deadline clock fixed as
        // well; real elapsed-time behavior starts after this fixture is built.
        Engine::with_handy_sync_instant_for_test(began, || {
            sync_call(f, &sync, |_, _, clock| {
                sender_ran.set(true);
                sync_ack(clock)
            })
        })
        .unwrap(),
    )
    .history
    .unwrap();
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (1, 1),
        "sync fixture lost native state: history={:?}, acknowledgment={}, sender_ran={}, elapsed={:?}, monitor={:?}",
        h.state, h.acknowledgment.is_some(), sender_ran.get(), began.elapsed(),
        f.engine.handy_controller_monitor.try_lock().ok().map(|monitor| monitor.is_some())
    );
    let controller = clock_replacement(f, &old, "clock-replacement", 1_000_000_010);
    assert_ne!(controller.controller_id, old.controller_id);
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::AcquireHandyPlaybackClock {
            acquisition_request_id: controller.acquisition_request_id.clone(),
            controller_id: controller.controller_id.clone(),
            expected_generation: 0,
            synchronization: HandyClockSyncBinding {
                sync_request_id: sync.request_id,
                record_id: h.record_id,
            },
            allow_online_timing: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new("combined-clock").unwrap();
    (r, controller, old)
}
fn qualified_device_read(
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> PResult<crate::handy_online::DeviceTimingRead> {
    let mut v = device_timing_read_fixture(clock)?;
    v.first.info.reported_clock_offset_ms = 125;
    v.second.info.reported_clock_offset_ms = 125;
    Ok(v)
}
fn playback_clock_call(f: &Fixture, r: &Request, now: u64) -> PResult<ResponseBody> {
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine.handy_playback_clock_with(
        r,
        &f.session,
        &fingerprint,
        |_, _, clock| qualified_device_read(clock),
        server_clock_sample,
        || Ok(now),
    )
}
fn playback_clock_report(body: ResponseBody) -> HandyPlaybackClockObservation {
    let ResponseBody::HandyPlaybackClock(v) = body else {
        panic!("combined clock")
    };
    v.validate().unwrap();
    *v
}
#[test]
fn handy_playback_clock_full_native_chain_rebinds_new_controller_and_adds_both_errors() {
    let f = fixture();
    let (r, _, _) = playback_clock_inputs(&f);
    let first = playback_clock_report(playback_clock_call(&f, &r, 1_000_000_020).unwrap());
    assert!(first.current);
    assert!(!first.playback_enabled);
    assert_eq!(first.server_uncertainty_ns, 1000);
    assert_eq!(first.device_uncertainty_ns, 2_000_001);
    assert_eq!(first.combined_uncertainty_ns, 2_001_001);
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    let repeat = playback_clock_report(
        f.engine
            .handy_playback_clock_with(
                &r,
                &f.session,
                &fingerprint,
                |_, _, _| panic!("no replay device read"),
                |_| panic!("no replay server GET"),
                || Ok(1_000_000_030),
            )
            .unwrap(),
    );
    assert!(repeat.current);
    assert_eq!(
        repeat.server_sent_boottime_ns,
        first.server_sent_boottime_ns
    );
    assert_eq!(repeat.synchronization_sent_boottime_ns, 1_000_000_000);
    let stale = playback_clock_report(playback_clock_call(&f, &r, 1_100_000_021).unwrap());
    assert!(!stale.current);
    let mut newer = r.clone();
    newer.request_id = RequestId::new("combined-refresh").unwrap();
    let fresh = playback_clock_report(playback_clock_call(&f, &newer, 1_110_000_000).unwrap());
    assert!(fresh.current);
    assert_eq!(
        fresh.synchronization_sent_boottime_ns,
        first.synchronization_sent_boottime_ns
    );
    let mut expired = r;
    expired.request_id = RequestId::new("expired-sync").unwrap();
    assert!(playback_clock_call(&f, &expired, 1_500_000_001).is_err());
}
#[test]
fn handy_playback_clock_renewal_and_release_drop_binding_but_preserve_original_sync_age() {
    let f = fixture();
    let (r, l, old) = playback_clock_inputs(&f);
    playback_clock_call(&f, &r, 1_000_000_020).unwrap();
    let renewed = controller_report(
        controller_call(
            &f,
            &controller_renew_request(&f, &l, "renew-combined"),
            1_000_000_030,
        )
        .unwrap(),
    )
    .lease;
    assert!(!playback_clock_report(playback_clock_call(&f, &r, 1_000_000_040).unwrap()).current);
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (1, 1)
    );
    let mut next = r.clone();
    next.request_id = RequestId::new("renewed-combined").unwrap();
    if let Command::AcquireHandyPlaybackClock {
        expected_generation,
        ..
    } = &mut next.command
    {
        *expected_generation = renewed.generation;
    }
    assert!(playback_clock_report(playback_clock_call(&f, &next, 1_000_000_050).unwrap()).current);
    let release = request(
        &f.session,
        Some(&f.project),
        None,
        Command::ReleaseHandyController {
            acquisition_request_id: renewed.acquisition_request_id,
            controller_id: renewed.controller_id,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    controller_call(&f, &release, 1_000_000_060).unwrap();
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (1, 1)
    );
    let new = clock_replacement(&f, &old, "next-controller", 1_000_000_070);
    assert_ne!(new.controller_id, l.controller_id);
}
#[test]
fn handy_playback_clock_changed_offset_sync_report_or_uptime_invalidates_native_completion() {
    for case in 0..4 {
        let f = fixture();
        let (r, _, _) = playback_clock_inputs(&f);
        let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
        let result = f.engine.handy_playback_clock_with(
            &r,
            &f.session,
            &fingerprint,
            |_, _, clock| {
                let mut v = qualified_device_read(clock)?;
                match case {
                    0 => v.first.user_offset.offset_ms += 1,
                    1 => v.second.info.reported_clock_offset_ms += 1,
                    2 => v.first.info.last_sync_round_trip_ms += 1,
                    _ => {
                        v.first.info.device_uptime_ms = 99;
                        v.second.info.device_uptime_ms = 99;
                    }
                };
                Ok(v)
            },
            |_| panic!("no server acquisition after changed device clock"),
            || Ok(1_000_000_020),
        );
        assert!(result.is_err());
        assert_eq!(
            f.engine
                .handy_preparations
                .lock()
                .unwrap()
                .native_sync_counts(),
            (0, 0)
        );
    }
}
#[test]
fn handy_playback_clock_pending_sync_remembers_revoke_regrant_and_never_revives_from_history() {
    let f = fixture();
    let (r, _) = qualified_sync_inputs(&f);
    let admin = pair(&f.engine, "sync observer administrator");
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::Grant {
                session: admin.clone(),
                project_id: f.project.clone(),
                scopes: vec![Scope::Read, Scope::Export, Scope::ManageGrants],
            },
        ))
        .result
        .unwrap();
    let value = sync_report(
        sync_call(&f, &r, |_, _, clock| {
            assert_eq!(
                f.engine
                    .handy_preparations
                    .lock()
                    .unwrap()
                    .native_sync_counts(),
                (1, 0)
            );
            f.engine
                .handle(request(
                    &admin,
                    None,
                    None,
                    Command::Revoke {
                        session: f.session.clone(),
                        project_id: f.project.clone(),
                    },
                ))
                .result
                .unwrap();
            assert_eq!(
                f.engine
                    .handy_preparations
                    .lock()
                    .unwrap()
                    .native_sync_counts(),
                (0, 0)
            );
            f.engine
                .handle(request(
                    &admin,
                    None,
                    None,
                    Command::Grant {
                        session: f.session.clone(),
                        project_id: f.project.clone(),
                        scopes: vec![Scope::Read, Scope::Export],
                    },
                ))
                .result
                .unwrap();
            sync_ack(clock)
        })
        .unwrap(),
    );
    assert_eq!(
        value.history.unwrap().state,
        HandyClockSyncState::Acknowledged
    );
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (0, 0)
    );
    sync_call(&f, &r, |_, _, _| {
        panic!("historical replay must not resync")
    })
    .unwrap();
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (0, 0)
    );
}
#[test]
fn handy_playback_clock_dependency_retirement_clears_independent_sync_with_no_original_entry() {
    for event in 0..6 {
        let f = fixture();
        let (r, l) = qualified_sync_inputs(&f);
        let report = sync_report(sync_call(&f, &r, |_, _, clock| sync_ack(clock)).unwrap());
        assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
        let original = playback_preparation_report(
            f.engine
                .handle(preparation_status_request(&f, &l.preparation_request_id))
                .result
                .unwrap(),
        );
        let i = &original.correlation.inspection;
        let command = match event {
            0 => request(
                &f.session,
                Some(&f.project),
                Some(1),
                commit_program(&f, 1, program(0.3)),
            ),
            1 => request(
                &f.session,
                None,
                None,
                Command::CloseHandySession {
                    engine_epoch: l.engine_epoch.clone(),
                    device_session: l.device_session.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            2 => request(
                &f.session,
                None,
                None,
                Command::DisableHandyRegistration {
                    device_id: l.device_id.clone(),
                    expected_registration_revision: i.registration.revision,
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            3 => qualification_withdraw(
                &f,
                &report.history.unwrap().preparation_qualification.dossier_id,
                2,
            ),
            4 => request(
                &f.session,
                Some(&f.project),
                None,
                Command::ForgetHandyHosting {
                    upload_request_id: i.hosting_request_id.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            _ => preparation_retire_request(&f, &original),
        };
        f.engine.handle(command).result.unwrap();
        assert_eq!(
            f.engine
                .handy_preparations
                .lock()
                .unwrap()
                .native_sync_counts(),
            (0, 0)
        );
    }
}
#[test]
fn handy_playback_clock_restart_and_missing_monitor_cannot_restore_native_clock() {
    let f = fixture();
    let (r, _, _) = playback_clock_inputs(&f);
    playback_clock_call(&f, &r, 1_000_000_020).unwrap();
    assert!(f.engine.handle(r.clone()).result.is_err());
    let Fixture { engine, dir, .. } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    let report = playback_clock_report(engine.handle(r).result.unwrap());
    assert!(!report.current);
    assert_eq!(
        engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (0, 0)
    );
    drop(engine);
    drop(dir);
}

#[test]
fn handy_playback_clock_new_unknown_sync_invalidates_earlier_native_completion() {
    let f = fixture();
    let (r, l, _) = playback_clock_inputs(&f);
    playback_clock_call(&f, &r, 1_000_000_020).unwrap();
    let mut sync = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::SynchronizeHandyClock {
            acquisition_request_id: l.acquisition_request_id,
            controller_id: l.controller_id,
            expected_generation: 0,
            allow_online_synchronization: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    sync.request_id = RequestId::new("new-sync-unknown").unwrap();
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&sync).unwrap()));
    let result = f
        .engine
        .handy_clock_sync_with(
            &sync,
            &f.session,
            &fingerprint,
            |_, _, clock| qualified_device_read(clock),
            |_, _, _| crate::device_effect_journal::ClockSyncCompletion::Unknown,
            || Ok(1_000_000_030),
        )
        .unwrap();
    assert_eq!(
        sync_report(result).history.unwrap().state,
        HandyClockSyncState::Unknown
    );
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (0, 0)
    );
    assert!(!playback_clock_report(playback_clock_call(&f, &r, 1_000_000_040).unwrap()).current);
}

#[test]
fn handy_playback_clock_wrong_selection_or_consent_refuses_before_remote_reads() {
    let f = fixture();
    let (r, _, _) = playback_clock_inputs(&f);
    for case in 0..6 {
        let mut bad = r.clone();
        let Command::AcquireHandyPlaybackClock {
            expected_generation,
            synchronization,
            allow_online_timing,
            owner_proof,
            ..
        } = &mut bad.command
        else {
            unreachable!()
        };
        match case {
            0 => *expected_generation = 1,
            1 => synchronization.sync_request_id = RequestId::new("other-sync").unwrap(),
            2 => synchronization.record_id = ArtifactId::new("a".repeat(64)).unwrap(),
            3 => *allow_online_timing = false,
            4 => *owner_proof = "wrong".into(),
            _ => bad.expected_revision = Some(RevisionId::new(2)),
        };
        let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&bad).unwrap()));
        assert!(f
            .engine
            .handy_playback_clock_with(
                &bad,
                &f.session,
                &fingerprint,
                |_, _, _| panic!("no device read"),
                |_| panic!("no server read"),
                || Ok(1_000_000_020)
            )
            .is_err());
    }
}
#[test]
fn handy_playback_clock_completion_write_failure_cannot_create_native_success() {
    let f = fixture();
    let (r, _) = qualified_sync_inputs(&f);
    let result = sync_call(&f, &r, |_, _, clock| {
        Connection::open(f.engine.config.state_dir.join("device-effects/journal.sqlite3")).unwrap().execute_batch("CREATE TRIGGER reject_native_sync BEFORE UPDATE ON clock_sync_effects WHEN NEW.state='acknowledged' BEGIN SELECT RAISE(ABORT,'fixture native sync completion failure'); END;").unwrap();
        sync_ack(clock)
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::UnknownOutcome);
    Connection::open(
        f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap()
    .execute_batch("DROP TRIGGER reject_native_sync")
    .unwrap();
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts()
            .1,
        0
    );
    assert_eq!(
        sync_report(sync_call(&f, &r, |_, _, _| panic!()).unwrap())
            .history
            .unwrap()
            .state,
        HandyClockSyncState::Claimed
    );
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts()
            .1,
        0
    );
}
#[test]
fn handy_playback_clock_server_refresh_invalidates_combined_binding_without_refreshing_device_age()
{
    let f = fixture();
    let (r, l, _) = playback_clock_inputs(&f);
    playback_clock_call(&f, &r, 1_000_000_020).unwrap();
    let mut sample = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::SampleHandyServerClock {
            acquisition_request_id: l.acquisition_request_id,
            controller_id: l.controller_id,
            expected_generation: l.generation,
            allow_online_sampling: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    sample.request_id = RequestId::new("standalone-server-refresh").unwrap();
    server_clock_call(&f, &sample, 1_000_000_030, server_clock_sample).unwrap();
    assert!(!playback_clock_report(playback_clock_call(&f, &r, 1_000_000_040).unwrap()).current);
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (1, 1)
    );
}
