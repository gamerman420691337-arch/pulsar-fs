#[test]
fn engine_shutdown_rejects_new_requests_and_tasks_and_cancels_registered_work() {
    let f = fixture();
    let cancel = Arc::new(AtomicBool::new(false));
    f.engine
        .workers
        .lock()
        .unwrap()
        .insert("shutdown-fixture".into(), cancel.clone());
    f.engine.request_shutdown();
    assert!(cancel.load(Ordering::Acquire));
    assert!(f.engine.is_shutting_down());
    let r = request(&f.session, Some(&f.project), None, Command::GetSnapshot);
    assert_eq!(
        f.engine.handle(r).result.unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert!(f
        .engine
        .spawn_task("forbidden-after-shutdown", None, || panic!("must not run"))
        .is_err());
    assert!(f.engine.ensure_handy_execution().is_err());
    f.engine.finish_shutdown().unwrap();
    f.engine.finish_shutdown().unwrap();
}

#[test]
fn engine_shutdown_joins_supervisors_without_holding_their_database_guard() {
    let f = fixture();
    let (entered, ready) = std::sync::mpsc::sync_channel(1);
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let engine = f.engine.clone();
    f.engine
        .spawn_task("shutdown-held-worker", None, move || {
            entered.send(()).unwrap();
            wait.recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
            let value: u32 = engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT 1", [], |r| r.get(0))
                .unwrap();
            assert_eq!(value, 1);
        })
        .unwrap();
    ready
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    f.engine.request_shutdown();
    let engine = f.engine.clone();
    let (finished, done) = std::sync::mpsc::sync_channel(1);
    let join = std::thread::spawn(move || finished.send(engine.finish_shutdown()).unwrap());
    assert!(done
        .recv_timeout(std::time::Duration::from_millis(30))
        .is_err());
    release.send(()).unwrap();
    done.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap();
    join.join().unwrap();
    assert_eq!(Arc::strong_count(&f.engine), 1);
}

#[test]
fn engine_shutdown_stops_playback_while_external_engine_reference_remains() {
    let x = handy_play_fixture();
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    x.f.engine.request_shutdown();
    x.f.engine.finish_shutdown().unwrap();
    assert_eq!(server.join().unwrap().len(), 2);
    assert!(!x.pending.load(Ordering::Acquire));
    assert_eq!(
        x.runtime.lock().unwrap().session().state(),
        DeviceState::StopUnconfirmed
    );
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    assert_eq!(Arc::strong_count(&x.f.engine), 1);
    assert!(x.listener.accept().is_err());
}

#[test]
fn engine_shutdown_fences_a_pending_play_and_joins_the_resulting_stop() {
    let x = handy_play_fixture();
    let (entered, ready) = std::sync::mpsc::sync_channel(1);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let held = barrier.clone();
    let first = AtomicBool::new(true);
    let clock = Arc::new(move || {
        if std::thread::current().name() == Some("pulsar-handy-dispatch")
            && first.swap(false, Ordering::AcqRel)
        {
            entered.send(()).unwrap();
            held.wait();
        }
        Ok(1_000_000_030)
    });
    let (started, received) = std::sync::mpsc::sync_channel(1);
    let (release, stop_gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, stop_gate, true);
    handy_play_call(&x, clock).unwrap();
    ready
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    x.f.engine.request_shutdown();
    let engine = x.f.engine.clone();
    let (finished, done) = std::sync::mpsc::sync_channel(1);
    let join = std::thread::spawn(move || finished.send(engine.finish_shutdown()).unwrap());
    assert!(done
        .recv_timeout(std::time::Duration::from_millis(30))
        .is_err());
    barrier.wait();
    received
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert!(done.try_recv().is_err());
    release.send(()).unwrap();
    server.join().unwrap();
    done.recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    join.join().unwrap();
    let history =
        x.f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .play_request_history(&x.f.session, &x.start.request_id)
            .unwrap()
            .unwrap();
    assert_eq!(
        history.state,
        crate::device_effect_journal::PlayOperationState::Claimed
    );
    assert!(x.listener.accept().is_err());
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
}

#[test]
fn engine_shutdown_reports_handy_worker_panics_and_still_joins_other_work() {
    for monitor in [false, true] {
        let f = fixture();
        if monitor {
            *f.engine.handy_controller_monitor.lock().unwrap() =
                Some(handy_playback_preparation::ControllerMonitor::panicked_for_test());
        } else {
            *f.engine.handy_execution.lock().unwrap() =
                Some(handy_playback_preparation::ExecutionPump::panicked_for_test());
        }
        let done = Arc::new(AtomicBool::new(false));
        let finished = done.clone();
        f.engine
            .spawn_task("shutdown-panic-survivor", None, move || {
                finished.store(true, Ordering::Release)
            })
            .unwrap();
        let error = f.engine.finish_shutdown().unwrap_err();
        assert!(error.to_string().contains("panicked during retirement"));
        assert!(done.load(Ordering::Acquire));
        assert_eq!(Arc::strong_count(&f.engine), 1);
    }
}
