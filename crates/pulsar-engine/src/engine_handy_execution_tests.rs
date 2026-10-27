struct HandyExecutionFixture {
    f: Fixture,
    lease: HandyControllerLease,
    runtime: Arc<Mutex<crate::playback_runtime::HandyPlaybackRuntime>>,
    pending: Arc<AtomicBool>,
    listener: std::net::TcpListener,
}
fn handy_execution_fixture() -> HandyExecutionFixture {
    let f = fixture();
    let (r, lease, _) = playback_clock_inputs(&f);
    assert!(playback_clock_report(playback_clock_call(&f, &r, 1_000_000_020).unwrap()).current);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let (runtime, pending) = f.engine.seed_handy_execution_for_test(
        &lease.preparation_id,
        format!("http://{}", listener.local_addr().unwrap()),
    );
    HandyExecutionFixture {
        f,
        lease,
        runtime,
        pending,
        listener,
    }
}
fn handy_execution_wait(mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "execution owner did not progress"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn handy_execution_server(
    listener: std::net::TcpListener,
    started: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
    reply: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use std::io::{BufRead, Write};
        let mut accepted = None;
        handy_execution_wait(|| match listener.accept() {
            Ok((stream, _)) => {
                accepted = Some(stream);
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(e) => panic!("local fixture accept: {e}"),
        });
        let mut stream = accepted.unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "PUT /hssp/stop HTTP/1.1");
        loop {
            line.clear();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
        }
        started.send(()).unwrap();
        release
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        if reply {
            let body = r#"{"result":{"play_state":2,"stream_id":8,"current_time":0,"playback_rate":1.0,"loop":false,"pause_on_starving":false}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    })
}

#[test]
fn handy_execution_consumes_one_retirement_stop_and_preserves_unknown_physical_state() {
    for reply in [false, true] {
        let x = handy_execution_fixture();
        let (started, entered) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::sync_channel(1);
        let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, reply);
        x.f.engine.ensure_handy_execution().unwrap();
        x.f.engine
            .retire_handy_execution_for_test(&x.lease.preparation_id);
        entered
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(x.pending.load(Ordering::Acquire));
        release.send(()).unwrap();
        server.join().unwrap();
        handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
        for _ in 0..3 {
            x.f.engine
                .retire_handy_execution_for_test(&x.lease.preparation_id);
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(
            x.listener.accept().is_err(),
            "retirement must not retry a stop"
        );
        let runtime = x.runtime.lock().unwrap();
        assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
        assert!(runtime.session().controller().is_none());
        assert!(runtime.session().physical_stop_pending());
        let journal = x.f.engine._device_effect_journal.lock().unwrap();
        let history = journal.history(&x.lease.device_id, 10).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history[0].record.stop);
        assert_eq!(
            history[0].outcome,
            Some(if reply {
                crate::playback_runtime::DispatchOutcome::Acknowledged
            } else {
                crate::playback_runtime::DispatchOutcome::Unknown
            })
        );
        assert_eq!(history[1].outcome, None);
    }
}

#[test]
#[cfg(target_os = "linux")]
fn handy_execution_expiry_and_emergency_stop_progress_while_project_and_effect_writers_are_held() {
    let x = handy_execution_fixture();
    let writer = Connection::open(
        x.f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let db = x.f.engine.db.lock().unwrap();
    let (started, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, true);
    // The fixture's old BOOTTIME lease expires under the real monitor. No new
    // client request can run while the project DB guard remains held.
    x.f.engine.ensure_handy_controller_monitor().unwrap();
    handy_execution_wait(|| x.pending.load(Ordering::Acquire));
    std::thread::sleep(std::time::Duration::from_millis(50));
    let runtime = x
        .runtime
        .try_lock()
        .expect("SQLite intent write held control ownership");
    assert!(runtime.session().controller().is_none());
    assert_eq!(runtime.session().state(), DeviceState::StopRequested);
    drop(runtime);
    // The pump must also relinquish the shared journal promptly instead of
    // spending the ordinary SQLite busy timeout behind this external writer.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        if let Ok(journal) = x.f.engine._device_effect_journal.try_lock() {
            drop(journal);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pump waited for SQLite while owning the journal"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    handy_execution_wait(|| {
        x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed
    });
    assert!(x.pending.load(Ordering::Acquire));
    drop(db);
    let status = handy_stop_report(&x.f, &x.lease);
    let native = status.native.unwrap();
    assert!(native.emergency);
    assert!(!native.metadata_recorded);
    assert!(status.history.is_empty());
    writer.execute_batch("COMMIT").unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    let status = handy_stop_report(&x.f, &x.lease);
    assert!(status.history[0].emergency);
    assert!(status.native.unwrap().metadata_recorded);
    assert!(x.listener.accept().is_err());
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
}

#[test]
fn handy_execution_retries_completion_metadata_without_resending_stop() {
    let x = handy_execution_fixture();
    let writer = Connection::open(
        x.f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    writer.execute_batch("CREATE TRIGGER refuse_stop_outcome BEFORE UPDATE OF outcome ON effects BEGIN SELECT RAISE(ABORT,'synthetic stop outcome failure'); END;").unwrap();
    let (started, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, true);
    x.f.engine.ensure_handy_execution().unwrap();
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    handy_execution_wait(|| {
        x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed
    });
    assert!(x.pending.load(Ordering::Acquire));
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    writer
        .execute_batch("DROP TRIGGER refuse_stop_outcome")
        .unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert!(x.listener.accept().is_err());
    assert!(x.runtime.lock().unwrap().session().controller().is_none());
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
}

#[test]
fn handy_execution_engine_drop_drains_and_joins_without_retaining_engine() {
    let x = handy_execution_fixture();
    let (started, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, true);
    x.f.engine.ensure_handy_execution().unwrap();
    let HandyExecutionFixture {
        f: Fixture { engine, dir, .. },
        runtime,
        pending,
        ..
    } = x;
    let (finished, done) = std::sync::mpsc::sync_channel(1);
    let retiring = std::thread::spawn(move || {
        let config = Engine::retire_for_test(engine);
        finished.send(config).unwrap();
    });
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(
        done.try_recv().is_err(),
        "Engine teardown lost active stop custody"
    );
    release.send(()).unwrap();
    server.join().unwrap();
    let config = done
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    retiring.join().unwrap();
    assert!(!pending.load(Ordering::Acquire));
    assert_eq!(Arc::strong_count(&runtime), 1);
    assert!(runtime.lock().unwrap().session().physical_stop_pending());
    let journal = crate::device_effect_journal::DeviceEffectJournal::open(
        &config.state_dir,
        DeviceSessionId::new("execution-restart").unwrap(),
    )
    .unwrap();
    let device = runtime.lock().unwrap().session().device().clone();
    let history = journal.history(&device, 10).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(
        history[0].outcome,
        Some(crate::playback_runtime::DispatchOutcome::Acknowledged)
    );
    drop(journal);
    drop(dir);
}
