struct HandyPlayFixture {
    f: Fixture,
    lease: HandyControllerLease,
    start: Request,
    runtime: Arc<Mutex<crate::playback_runtime::HandyPlaybackRuntime>>,
    pending: Arc<AtomicBool>,
    listener: std::net::TcpListener,
}
fn handy_play_fixture() -> HandyPlayFixture {
    let f = fixture();
    let (clock_request, lease, _) = playback_clock_inputs(&f);
    assert!(
        playback_clock_report(playback_clock_call(&f, &clock_request, 1_000_000_020).unwrap())
            .current
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let (runtime, pending) = f.engine.bind_handy_play_transport_for_test(
        &lease.preparation_id,
        format!("http://{}", listener.local_addr().unwrap()),
    );
    let mut start = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::StartHandyPlayback {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            controller_id: lease.controller_id.clone(),
            expected_generation: lease.generation,
            clock_request_id: clock_request.request_id,
            start_time_ms: 0,
            allow_play: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    start.request_id = RequestId::new("play-submit").unwrap();
    HandyPlayFixture {
        f,
        lease,
        start,
        runtime,
        pending,
        listener,
    }
}
fn handy_play_call(
    x: &HandyPlayFixture,
    clock: Arc<dyn Fn() -> PResult<u64> + Send + Sync>,
) -> PResult<HandyPlaybackStatus> {
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(request_fingerprint(&x.start).unwrap())
    );
    let body =
        x.f.engine
            .handy_playback_with(&x.start, &x.f.session, &fingerprint, clock, |_| Ok(()))?;
    let ResponseBody::HandyPlayback(status) = body else {
        panic!("play reply");
    };
    status.validate()?;
    Ok(*status)
}
fn handy_play_status(x: &HandyPlayFixture) -> HandyPlaybackStatus {
    let r = request(
        &x.f.session,
        Some(&x.f.project),
        None,
        Command::HandyPlaybackStatus {
            play_request_id: x.start.request_id.clone(),
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    );
    let ResponseBody::HandyPlayback(s) = x.f.engine.handle(r).result.unwrap() else {
        panic!("play status");
    };
    s.validate().unwrap();
    *s
}
fn handy_play_read_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>) {
    use std::io::{BufRead, Read};
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let first = line.trim().to_owned();
    let mut length = 0;
    loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().unwrap();
        }
    }
    assert!(length <= 1024);
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).unwrap();
    (first, bytes)
}
fn handy_play_accept(listener: &std::net::TcpListener) -> std::net::TcpStream {
    let mut result = None;
    handy_execution_wait(|| match listener.accept() {
        Ok((s, _)) => {
            result = Some(s);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(e) => panic!("fixture accept {e}"),
    });
    result.unwrap()
}
fn handy_play_reply(stream: &mut std::net::TcpStream, state: u8) {
    use std::io::Write;
    let body = format!(
        r#"{{"result":{{"play_state":{state},"stream_id":8,"current_time":0,"playback_rate":1.0,"loop":false,"pause_on_starving":false}}}}"#
    );
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}
fn handy_play_server(
    listener: std::net::TcpListener,
    ack: bool,
) -> std::thread::JoinHandle<Vec<(String, Vec<u8>)>> {
    std::thread::spawn(move || {
        let mut play = handy_play_accept(&listener);
        let first = handy_play_read_request(&mut play);
        assert_eq!(first.0, "PUT /hssp/play HTTP/1.1");
        if ack {
            handy_play_reply(&mut play, 1);
        }
        drop(play);
        let mut stop = handy_play_accept(&listener);
        let second = handy_play_read_request(&mut stop);
        assert_eq!(second.0, "PUT /hssp/stop HTTP/1.1");
        handy_play_reply(&mut stop, 2);
        vec![first, second]
    })
}

#[test]
fn handy_playback_owned_play_release_stop_and_original_retry_share_one_runtime() {
    let x = handy_play_fixture();
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    let first = handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    assert_eq!(
        first.history.as_ref().unwrap().state,
        HandyPlaybackOperationState::Prepared
    );
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    let repeat = handy_play_call(&x, Arc::new(|| panic!("replay cannot acquire timing"))).unwrap();
    assert_eq!(
        repeat.history.as_ref().unwrap().record_id,
        first.history.as_ref().unwrap().record_id
    );
    assert_eq!(
        repeat.history.unwrap().state,
        HandyPlaybackOperationState::Acknowledged
    );
    assert_eq!(
        x.runtime.lock().unwrap().session().state(),
        DeviceState::Playing
    );
    let status = controller_status_request(&x.f, &x.lease);
    let current = controller_report(controller_call(&x.f, &status, 1_000_000_040).unwrap());
    assert_eq!(current.lease.state, HandyControllerState::Playing);
    assert!(current.lease.physical_stop_pending);
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    let play: serde_json::Value = serde_json::from_slice(&requests[0].1).unwrap();
    assert_eq!(play["start_time"], 0);
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert_eq!(
        x.runtime.lock().unwrap().session().state(),
        DeviceState::StopUnconfirmed
    );
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    assert!(x.listener.accept().is_err());
}

#[test]
fn handy_playback_unknown_stops_before_failed_play_metadata_can_be_repaired() {
    let x = handy_play_fixture();
    let db = Connection::open(
        x.f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    db.execute_batch("CREATE TRIGGER hold_play_outcome BEFORE UPDATE OF state ON play_operations WHEN NEW.state IN ('acknowledged','unknown') BEGIN SELECT RAISE(ABORT,'synthetic play outcome failure'); END;").unwrap();
    let server = handy_play_server(x.listener.try_clone().unwrap(), false);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    server.join().unwrap();
    handy_execution_wait(|| {
        x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed
    });
    assert_eq!(
        handy_play_status(&x).history.unwrap().state,
        HandyPlaybackOperationState::Claimed
    );
    assert!(x.pending.load(Ordering::Acquire));
    db.execute_batch("DROP TRIGGER hold_play_outcome").unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert_eq!(
        handy_play_status(&x).history.unwrap().state,
        HandyPlaybackOperationState::Unknown
    );
    assert!(x.listener.accept().is_err());
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
}

#[test]
fn handy_playback_final_authority_revocation_prevents_play_but_preserves_stop_custody() {
    let x = handy_play_fixture();
    let (entered, wait) = std::sync::mpsc::sync_channel(1);
    let gate = Arc::new(std::sync::Barrier::new(2));
    let held = gate.clone();
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
    wait.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
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
    gate.wait();
    received
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    handy_execution_wait(|| {
        x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed
    });
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
    assert!(x.runtime.lock().unwrap().session().controller().is_none());
}

#[test]
fn handy_playback_wrong_selection_or_missing_consent_creates_no_effect() {
    for mode in 0..4 {
        let mut x = handy_play_fixture();
        if let Command::StartHandyPlayback {
            allow_play,
            expected_generation,
            clock_request_id,
            start_time_ms,
            ..
        } = &mut x.start.command
        {
            match mode {
                0 => *allow_play = false,
                1 => *expected_generation = 1,
                2 => *clock_request_id = RequestId::new("wrong-clock").unwrap(),
                _ => *start_time_ms = u64::MAX,
            }
        }
        assert!(handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).is_err());
        assert!(x
            .f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .history(&x.lease.device_id, 10)
            .unwrap()
            .is_empty());
        assert!(x.listener.accept().is_err());
    }
}

#[test]
fn handy_playback_failed_request_reservation_prevents_journal_or_runtime_issue() {
    let x = handy_play_fixture();
    x.f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER refuse_play_request BEFORE INSERT ON requests WHEN NEW.request='play-submit' BEGIN SELECT RAISE(ABORT,'synthetic reservation failure'); END;").unwrap();
    assert!(handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).is_err());
    assert_eq!(
        x.runtime.lock().unwrap().session().state(),
        DeviceState::Armed
    );
    assert!(x
        .f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .history(&x.lease.device_id, 10)
        .unwrap()
        .is_empty());
    assert!(x.listener.accept().is_err());
    x.f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_play_request")
        .unwrap();
}

#[test]
fn handy_playback_failed_witness_can_be_repaired_without_enqueue_then_cancelled_exactly() {
    let x = handy_play_fixture();
    let writer = Connection::open(x.f.engine.config.state_dir.join("projects.sqlite3")).unwrap();
    writer.execute_batch("CREATE TRIGGER refuse_play_witness BEFORE UPDATE OF response ON requests WHEN NEW.request='play-submit' AND json_type(NEW.response,'$.value.history')='object' BEGIN SELECT RAISE(ABORT,'synthetic witness failure'); END;").unwrap();
    let (started, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let server = handy_execution_server(x.listener.try_clone().unwrap(), started, gate, true);
    assert!(handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).is_err());
    entered
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert_eq!(
        handy_play_status(&x).history.unwrap().state,
        HandyPlaybackOperationState::Prepared
    );
    writer
        .execute_batch("DROP TRIGGER refuse_play_witness")
        .unwrap();
    let recovered = handy_play_call(
        &x,
        Arc::new(|| panic!("witness retry cannot enqueue or acquire timing")),
    )
    .unwrap();
    let id = recovered.history.unwrap().record_id;
    let mut cancel = request(
        &x.f.session,
        Some(&x.f.project),
        None,
        Command::CancelHandyPlayback {
            play_request_id: x.start.request_id.clone(),
            record_id: id.clone(),
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    );
    cancel.request_id = RequestId::new("cancel-unclaimed-play").unwrap();
    for _ in 0..2 {
        let ResponseBody::HandyPlayback(report) = x.f.engine.handle(cancel.clone()).result.unwrap()
        else {
            panic!()
        };
        assert_eq!(
            report.history.unwrap().state,
            HandyPlaybackOperationState::Cancelled
        );
    }
    let report =
        handy_play_call(&x, Arc::new(|| panic!("cancelled replay cannot enqueue"))).unwrap();
    assert_eq!(
        report.history.unwrap().state,
        HandyPlaybackOperationState::Cancelled
    );
    assert!(x.listener.accept().is_err());
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
}

#[test]
fn handy_playback_witness_subject_corruption_is_not_silently_replaced_by_history() {
    let x = handy_play_fixture();
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    server.join().unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    x.f.engine.db.lock().unwrap().execute("UPDATE requests SET response=json_set(response,'$.value.history.clock_request_id','corrupted-clock') WHERE session=?1 AND request=?2",params![x.f.session.as_str(),x.start.request_id.as_str()]).unwrap();
    let status = request(
        &x.f.session,
        Some(&x.f.project),
        None,
        Command::HandyPlaybackStatus {
            play_request_id: x.start.request_id.clone(),
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    );
    assert!(x.f.engine.handle(status).result.is_err());
    assert!(handy_play_call(&x, Arc::new(|| panic!("corruption cannot restart work"))).is_err());
}

#[test]
fn handy_playback_renewal_keeps_playing_but_cannot_refresh_original_sync_age() {
    let x = handy_play_fixture();
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    let renewal = controller_renew_request(&x.f, &x.lease, "renew-playing");
    let renewed = controller_report(controller_call(&x.f, &renewal, 1_100_000_000).unwrap());
    assert_eq!(renewed.lease.state, HandyControllerState::Playing);
    assert_eq!(renewed.lease.generation, 1);
    assert!(renewed.lease.physical_stop_pending);
    let retry = controller_report(controller_call(&x.f, &renewal, 1_200_000_000).unwrap());
    assert_eq!(
        retry.lease.deadline_boottime_ns,
        renewed.lease.deadline_boottime_ns
    );
    let expired = controller_report(
        controller_call(
            &x.f,
            &controller_status_request(&x.f, &x.lease),
            1_600_000_000,
        )
        .unwrap(),
    );
    assert!(1_600_000_000 < renewed.lease.deadline_boottime_ns);
    assert_eq!(expired.lease.state, HandyControllerState::Retired);
    server.join().unwrap();
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    assert!(x.listener.accept().is_err());
}

#[test]
fn handy_playback_generation_change_or_source_expiry_before_claim_prevents_play() {
    for renew in [false, true] {
        let x = handy_play_fixture();
        let (entered, wait) = std::sync::mpsc::sync_channel(1);
        let gate = Arc::new(std::sync::Barrier::new(2));
        let held = gate.clone();
        let first = AtomicBool::new(true);
        let tick = Arc::new(std::sync::atomic::AtomicU64::new(1_000_000_030));
        let native_tick = tick.clone();
        let clock = Arc::new(move || {
            if std::thread::current().name() == Some("pulsar-handy-dispatch")
                && first.swap(false, Ordering::AcqRel)
            {
                entered.send(()).unwrap();
                held.wait();
            }
            Ok(native_tick.load(Ordering::Acquire))
        });
        let (started, received) = std::sync::mpsc::sync_channel(1);
        let (release, stop_gate) = std::sync::mpsc::sync_channel(1);
        let server =
            handy_execution_server(x.listener.try_clone().unwrap(), started, stop_gate, true);
        handy_play_call(&x, clock).unwrap();
        wait.recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        if renew {
            let request = controller_renew_request(&x.f, &x.lease, "renew-before-claim");
            controller_call(&x.f, &request, 1_000_000_040).unwrap();
            tick.store(1_000_000_040, Ordering::Release);
        } else {
            tick.store(1_600_000_000, Ordering::Release);
        }
        gate.wait();
        received
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        release.send(()).unwrap();
        server.join().unwrap();
        handy_execution_wait(|| {
            x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed
        });
        assert_eq!(
            handy_play_status(&x).history.unwrap().state,
            HandyPlaybackOperationState::Claimed
        );
        assert!(x.listener.accept().is_err());
    }
}

#[test]
fn handy_playback_engine_drop_stops_and_joins_without_native_ownership_cycle() {
    let x = handy_play_fixture();
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    let HandyPlayFixture {
        f: Fixture { engine, dir, .. },
        runtime,
        pending,
        ..
    } = x;
    let (finished, done) = std::sync::mpsc::sync_channel(1);
    let retiring =
        std::thread::spawn(move || finished.send(Engine::retire_for_test(engine)).unwrap());
    let config = done
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    retiring.join().unwrap();
    assert_eq!(server.join().unwrap().len(), 2);
    assert!(!pending.load(Ordering::Acquire));
    assert_eq!(Arc::strong_count(&runtime), 1);
    assert!(runtime.lock().unwrap().session().physical_stop_pending());
    let journal = crate::device_effect_journal::DeviceEffectJournal::open(
        &config.state_dir,
        DeviceSessionId::new("play-restart").unwrap(),
    )
    .unwrap();
    let device = runtime.lock().unwrap().session().device().clone();
    assert_eq!(journal.history(&device, 10).unwrap().len(), 2);
    drop(journal);
    drop(dir);
}
