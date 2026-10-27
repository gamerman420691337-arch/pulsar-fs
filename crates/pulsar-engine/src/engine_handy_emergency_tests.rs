fn handy_stop_request(f: &Fixture, lease: &HandyControllerLease) -> Request {
    request(
        &f.session,
        Some(&f.project),
        None,
        Command::HandyStopStatus {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
fn handy_stop_report(f: &Fixture, lease: &HandyControllerLease) -> HandyStopStatus {
    let ResponseBody::HandyStop(report) = f
        .engine
        .handle(handy_stop_request(f, lease))
        .result
        .unwrap()
    else {
        panic!("stop status");
    };
    report.validate().unwrap();
    *report
}

#[test]
fn handy_emergency_stop_outcome_retries_only_metadata_and_remains_physically_unconfirmed() {
    for reply in [false, true] {
        let x = handy_execution_fixture();
        let writer = Connection::open(
            x.f.engine
                .config
                .state_dir
                .join("device-effects/journal.sqlite3"),
        )
        .unwrap();
        writer.execute_batch("CREATE TRIGGER refuse_stop_insert BEFORE INSERT ON effects WHEN json_extract(NEW.record,'$.stop')=1 BEGIN SELECT RAISE(ABORT,'synthetic stop storage failure'); END;").unwrap();
        let (started, entered) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::sync_channel(1);
        let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, reply);
        x.f.engine.ensure_handy_execution().unwrap();
        x.f.engine
            .retire_handy_execution_for_test(&x.lease.preparation_id);
        entered
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        release.send(()).unwrap();
        server.join().unwrap();
        handy_execution_wait(|| {
            handy_stop_report(&x.f, &x.lease)
                .native
                .is_some_and(|n| n.phase == HandyStopPhase::Observed)
        });
        let status = handy_stop_report(&x.f, &x.lease);
        assert!(!status.history_complete);
        assert!(status.history.is_empty());
        let native = status.native.unwrap();
        assert!(native.emergency);
        assert!(!native.metadata_recorded);
        assert_eq!(
            native.outcome,
            Some(if reply {
                HandyStopOutcome::Acknowledged
            } else {
                HandyStopOutcome::Unknown
            })
        );
        assert_eq!(
            x.runtime.lock().unwrap().session().state(),
            DeviceState::StopUnconfirmed
        );
        assert!(x.pending.load(Ordering::Acquire));
        for _ in 0..3 {
            x.f.engine
                .retire_handy_execution_for_test(&x.lease.preparation_id);
        }
        writer
            .execute_batch("DROP TRIGGER refuse_stop_insert")
            .unwrap();
        handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
        let status = handy_stop_report(&x.f, &x.lease);
        assert_eq!(status.history.len(), 1);
        assert_eq!(status.history[0].record_id, native.record_id);
        assert_eq!(status.history[0].outcome, native.outcome);
        assert!(status.history[0].emergency);
        assert!(status.native.unwrap().metadata_recorded);
        assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
        assert!(x.listener.accept().is_err());
    }
}

#[test]
fn handy_emergency_stop_does_not_wait_for_the_shared_journal_mutex() {
    let x = handy_execution_fixture();
    x.f.engine.ensure_handy_execution().unwrap();
    let journal = x.f.engine._device_effect_journal.lock().unwrap();
    let (started, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, true);
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    entered
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    handy_execution_wait(|| {
        x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed
    });
    assert!(x.pending.load(Ordering::Acquire));
    drop(journal);
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert!(handy_stop_report(&x.f, &x.lease).history[0].emergency);
    assert!(x.listener.accept().is_err());
}

#[test]
fn handy_emergency_stop_refuses_a_known_foreign_journal_epoch() {
    let x = handy_execution_fixture();
    x.f.engine.ensure_handy_execution().unwrap();
    let replacement = crate::device_effect_journal::DeviceEffectJournal::open(
        &x.f.engine.config.state_dir,
        DeviceSessionId::new("replacement-owner").unwrap(),
    )
    .unwrap();
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(x.listener.accept().is_err());
    let error = x.f.engine.finish_shutdown().unwrap_err();
    assert!(error
        .to_string()
        .contains("stop dispatch or metadata remains unresolved"));
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    drop(replacement);
}

#[test]
fn handy_emergency_stop_shutdown_reports_unrecorded_metadata_after_attempting_stop() {
    let x = handy_execution_fixture();
    x.f.engine.ensure_handy_execution().unwrap();
    let writer = Connection::open(
        x.f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (started, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, true);
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    entered
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    let error = x.f.engine.finish_shutdown().unwrap_err();
    assert!(error
        .to_string()
        .contains("stop dispatch or metadata remains unresolved"));
    assert!(x.pending.load(Ordering::Acquire));
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    writer.execute_batch("ROLLBACK").unwrap();
    assert!(x.listener.accept().is_err());
}

#[test]
fn handy_stop_status_preserves_scope_and_distinguishes_durable_transport() {
    let x = handy_execution_fixture();
    x.f.engine.ensure_handy_execution().unwrap();
    let mut wrong = handy_stop_request(&x.f, &x.lease);
    if let Command::HandyStopStatus { owner_proof, .. } = &mut wrong.command {
        *owner_proof = "wrong-owner".into();
    }
    assert!(x.f.engine.handle(wrong).result.is_err());
    let mut wrong = handy_stop_request(&x.f, &x.lease);
    if let Command::HandyStopStatus {
        acquisition_request_id,
        ..
    } = &mut wrong.command
    {
        *acquisition_request_id = RequestId::new("missing-acquisition").unwrap();
    }
    assert!(x.f.engine.handle(wrong).result.is_err());
    let (started, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, true);
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    entered
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    let status = handy_stop_report(&x.f, &x.lease);
    assert_eq!(status.controller_id, x.lease.controller_id);
    assert!(!status.history_complete);
    assert!(!status.history[0].emergency);
    assert!(!status.native.unwrap().emergency);
    let revoke = request(
        &x.f.session,
        None,
        None,
        Command::Revoke {
            project_id: x.f.project.clone(),
            session: x.f.session.clone(),
        },
    );
    x.f.engine.handle(revoke).result.unwrap();
    assert!(x
        .f
        .engine
        .handle(handy_stop_request(&x.f, &x.lease))
        .result
        .is_err());
}

#[test]
fn handy_emergency_stop_after_native_play_keeps_the_original_runtime_and_request() {
    let x = handy_play_fixture();
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    let writer = Connection::open(
        x.f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    writer.execute_batch("CREATE TRIGGER refuse_stop_insert BEFORE INSERT ON effects WHEN json_extract(NEW.record,'$.stop')=1 BEGIN SELECT RAISE(ABORT,'synthetic stop storage failure'); END;").unwrap();
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    assert_eq!(server.join().unwrap().len(), 2);
    handy_execution_wait(|| {
        handy_stop_report(&x.f, &x.lease)
            .native
            .is_some_and(|n| n.phase == HandyStopPhase::Observed)
    });
    let native = handy_stop_report(&x.f, &x.lease).native.unwrap();
    assert!(native.emergency);
    assert!(!native.metadata_recorded);
    let replay = handy_play_call(
        &x,
        Arc::new(|| panic!("original retry cannot reacquire timing")),
    )
    .unwrap();
    assert_eq!(
        replay.history.unwrap().state,
        HandyPlaybackOperationState::Acknowledged
    );
    writer
        .execute_batch("DROP TRIGGER refuse_stop_insert")
        .unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert_eq!(
        handy_stop_report(&x.f, &x.lease).history[0].record_id,
        native.record_id
    );
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    assert!(x.listener.accept().is_err());
}
