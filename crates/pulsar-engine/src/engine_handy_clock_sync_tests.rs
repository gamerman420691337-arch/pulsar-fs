fn sync_inputs(f: &Fixture) -> (Request, HandyControllerLease) {
    let (mut r, l) = device_timing_inputs(f);
    r.request_id = RequestId::new("clock-sync-original").unwrap();
    r.command = Command::SynchronizeHandyClock {
        acquisition_request_id: l.acquisition_request_id.clone(),
        controller_id: l.controller_id.clone(),
        expected_generation: l.generation,
        allow_online_synchronization: true,
        owner_proof: f.engine.handy_owner_token.clone(),
    };
    (r, l)
}
fn sync_report(body: ResponseBody) -> HandyClockSyncStatus {
    let ResponseBody::HandyClockSync(v) = body else {
        panic!("clock sync")
    };
    v.validate().unwrap();
    *v
}
fn sync_call(
    f: &Fixture,
    r: &Request,
    send: impl FnOnce(
        Arc<crate::handy_credentials::Credentials>,
        std::time::Instant,
        &mut dyn FnMut() -> PResult<u64>,
    ) -> crate::device_effect_journal::ClockSyncCompletion,
) -> PResult<ResponseBody> {
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine.handy_clock_sync_with(
        r,
        &f.session,
        &fingerprint,
        |_, _, clock| device_timing_read_fixture(clock),
        send,
        || Ok(1_000_000_000),
    )
}
fn sync_ack(
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> crate::device_effect_journal::ClockSyncCompletion {
    crate::device_effect_journal::ClockSyncCompletion::Acknowledged(HandyClockSyncAcknowledgment {
        info: HandyDeviceTimeInfo {
            device_uptime_ms: 100,
            reported_clock_offset_ms: 125,
            last_sync_round_trip_ms: 10,
        },
        sent_boottime_ns: clock().unwrap(),
        received_boottime_ns: clock().unwrap(),
    })
}
fn sync_status_request(f: &Fixture, id: &RequestId) -> Request {
    request(
        &f.session,
        Some(&f.project),
        None,
        Command::HandyClockSyncStatus {
            sync_request_id: id.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
fn replacement_preparation(f: &Fixture) -> Request {
    let old = playback_preparation_report(
        f.engine
            .handle(preparation_status_request(
                f,
                &RequestId::new("prepare-program").unwrap(),
            ))
            .result
            .unwrap(),
    );
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::PrepareHandyPlayback {
            program: old.selection().unwrap(),
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new("after-sync-preparation").unwrap();
    r
}
#[test]
fn handy_clock_sync_claim_precedes_io_retires_controller_and_replay_never_resends() {
    let f = fixture();
    let (r, l) = sync_inputs(&f);
    let first = sync_report(
        sync_call(&f, &r, |_, _, clock| {
            assert!(f.engine.db.try_lock().is_ok());
            assert!(f.engine.handy_preparations.try_lock().is_ok());
            let j = f.engine._device_effect_journal.try_lock().unwrap();
            let h = j
                .clock_sync_request_history(&f.session, &r.request_id)
                .unwrap()
                .unwrap();
            assert_eq!(h.state, HandyClockSyncState::Claimed);
            drop(j);
            assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
            assert!(playback_preparation_call(&f, &replacement_preparation(&f)).is_err());
            sync_ack(clock)
        })
        .unwrap(),
    );
    assert_eq!(
        first.history.as_ref().unwrap().state,
        HandyClockSyncState::Acknowledged
    );
    assert!(!first.synchronization_verified && !first.playback_enabled);
    assert_eq!(
        controller_report(
            controller_call(&f, &controller_status_request(&f, &l), 1_000_000_000).unwrap()
        )
        .lease
        .state,
        HandyControllerState::Retired
    );
    let repeated = sync_report(sync_call(&f, &r, |_, _, _| panic!("no repeated GET")).unwrap());
    assert_eq!(encode(&first).unwrap(), encode(&repeated).unwrap());
    assert_eq!(
        playback_preparation_report(
            playback_preparation_call(&f, &replacement_preparation(&f)).unwrap()
        )
        .state,
        HandyPlaybackPreparationState::Prepared
    );
}
#[test]
fn handy_clock_sync_unknown_and_lost_completion_remain_reserved_across_restart() {
    for lost in [false, true] {
        let f = fixture();
        let (r, _) = sync_inputs(&f);
        let result = sync_call(&f, &r, |_, _, clock| {
            if lost {
                let path = f
                    .engine
                    .config
                    .state_dir
                    .join("device-effects/journal.sqlite3");
                Connection::open(path).unwrap().execute_batch("CREATE TRIGGER fail_clock_complete BEFORE UPDATE ON clock_sync_effects WHEN NEW.state='acknowledged' BEGIN SELECT RAISE(ABORT,'fixture completion write failure'); END;").unwrap();
                sync_ack(clock)
            } else {
                crate::device_effect_journal::ClockSyncCompletion::Unknown
            }
        });
        if lost {
            assert_eq!(result.unwrap_err().code, ErrorCode::UnknownOutcome);
            Connection::open(
                f.engine
                    .config
                    .state_dir
                    .join("device-effects/journal.sqlite3"),
            )
            .unwrap()
            .execute_batch("DROP TRIGGER fail_clock_complete")
            .unwrap();
        } else {
            assert_eq!(
                sync_report(result.unwrap()).history.unwrap().state,
                HandyClockSyncState::Unknown
            );
        }
        let value = sync_report(sync_call(&f, &r, |_, _, _| panic!()).unwrap());
        let h = value.history.unwrap();
        assert!(h.state.route_reserved());
        assert!(playback_preparation_call(&f, &replacement_preparation(&f)).is_err());
        let cancel = request(
            &f.session,
            Some(&f.project),
            None,
            Command::CancelHandyClockSync {
                sync_request_id: r.request_id.clone(),
                record_id: h.record_id.clone(),
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        assert!(f.engine.handle(cancel).result.is_err());
        let query = sync_status_request(&f, &r.request_id);
        let Fixture { engine, dir, .. } = f;
        let config = Engine::retire_for_test(engine);
        let engine = Engine::open(config).unwrap();
        assert_eq!(
            sync_report(engine.handle(query).result.unwrap())
                .history
                .unwrap()
                .state,
            h.state
        );
        assert_eq!(
            sync_report(engine.handle(r).result.unwrap())
                .history
                .unwrap()
                .state,
            h.state
        );
        drop(engine);
        drop(dir);
    }
}
#[test]
fn handy_clock_sync_witness_failure_never_claims_and_unclaimed_cancellation_is_exact() {
    let f = fixture();
    let (r, _) = sync_inputs(&f);
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_sync_witness BEFORE UPDATE ON requests WHEN NEW.request='clock-sync-original' BEGIN SELECT RAISE(ABORT,'fixture witness write failure'); END;").unwrap();
    assert!(sync_call(&f, &r, |_, _, _| panic!("no claim or GET without witness")).is_err());
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_sync_witness")
        .unwrap();
    let v = sync_report(
        sync_call(&f, &r, |_, _, _| panic!("prepared replay cannot dispatch")).unwrap(),
    );
    let h = v.history.unwrap();
    assert_eq!(h.state, HandyClockSyncState::Prepared);
    let mut cancel = request(
        &f.session,
        Some(&f.project),
        None,
        Command::CancelHandyClockSync {
            sync_request_id: r.request_id.clone(),
            record_id: h.record_id,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    cancel.request_id = RequestId::new("cancel-clock-prepared").unwrap();
    let first = sync_report(f.engine.handle(cancel.clone()).result.unwrap());
    assert_eq!(first.history.unwrap().state, HandyClockSyncState::Cancelled);
    assert_eq!(
        sync_report(f.engine.handle(cancel).result.unwrap())
            .history
            .unwrap()
            .state,
        HandyClockSyncState::Cancelled
    );
    assert_eq!(
        sync_report(sync_call(&f, &r, |_, _, _| panic!()).unwrap())
            .history
            .unwrap()
            .state,
        HandyClockSyncState::Cancelled
    );
}
#[test]
fn handy_clock_sync_revocation_during_io_keeps_only_historical_response() {
    let f = fixture();
    let (r, _) = sync_inputs(&f);
    let value = sync_report(
        sync_call(&f, &r, |_, _, clock| {
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(1),
                    commit_program(&f, 1, program(0.3)),
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
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
}

#[test]
fn handy_clock_sync_lost_read_grant_records_completion_without_disclosing_result() {
    let f = fixture();
    let (r, _) = sync_inputs(&f);
    let error = sync_call(&f, &r, |_, _, clock| {
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        sync_ack(clock)
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(
        f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .clock_sync_request_history(&f.session, &r.request_id)
            .unwrap()
            .unwrap()
            .state,
        HandyClockSyncState::Acknowledged
    );
}
#[test]
fn handy_clock_sync_missing_linked_journal_refuses_restart_before_recreation() {
    let f = fixture();
    let (r, _) = sync_inputs(&f);
    sync_call(&f, &r, |_, _, _| {
        crate::device_effect_journal::ClockSyncCompletion::Unknown
    })
    .unwrap();
    let Fixture { engine, dir, .. } = f;
    let config = Engine::retire_for_test(engine);
    let path = config.state_dir.join("device-effects/journal.sqlite3");
    fs::remove_file(&path).unwrap();
    assert!(Engine::open(config).is_err());
    assert!(!path.exists());
    drop(dir);
}
#[test]
fn handy_clock_sync_consent_scope_and_request_fingerprint_refuse_before_effects() {
    let f = fixture();
    let (r, _) = sync_inputs(&f);
    for case in 0..4 {
        let mut bad = r.clone();
        let Command::SynchronizeHandyClock {
            allow_online_synchronization,
            expected_generation,
            owner_proof,
            ..
        } = &mut bad.command
        else {
            unreachable!()
        };
        match case {
            0 => *allow_online_synchronization = false,
            1 => *expected_generation = 1,
            2 => *owner_proof = "wrong".into(),
            _ => bad.expected_revision = Some(RevisionId::new(2)),
        }
        assert!(sync_call(&f, &bad, |_, _, _| panic!()).is_err());
    }
    sync_call(&f, &r, |_, _, clock| sync_ack(clock)).unwrap();
    let mut changed = r.clone();
    changed.expected_revision = Some(RevisionId::new(2));
    assert!(sync_call(&f, &changed, |_, _, _| panic!()).is_err());
}
