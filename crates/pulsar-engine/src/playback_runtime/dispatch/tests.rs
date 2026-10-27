use super::super::tests::{clock, fixture};
use super::*;
use crate::device_effect_journal::DeviceEffectJournal;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

struct Fixture {
    _dir: tempfile::TempDir,
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    journal: DeviceEffectJournal,
    admission: PlaybackAdmission,
    controller: ClientId,
    play: PlaybackEffect,
    receipt: PreparedEffectReceipt,
}
impl Fixture {
    fn new() -> Self {
        Self::with_clock(clock())
    }
    fn with_clock(mapping: PlaybackClock) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        // Synthetic local-HTTP bounds, not actual hardware qualification.
        runtime.timing.max_age_ns = 2_000_000_000;
        runtime.timing.max_clock_uncertainty_ns = 2_000_000_000;
        runtime.timing.drift_ppm = 0;
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("dispatch-boot").unwrap())
                .unwrap();
        let pending = runtime.prepare_start(&controller, mapping, 1234).unwrap();
        let (play, receipt) = runtime
            .dispatch_start_durable(
                pending,
                &admission,
                &controller,
                &clock().domain,
                clock().monotonic_ns,
                &mut journal,
            )
            .unwrap();
        Self {
            _dir: dir,
            runtime: Arc::new(Mutex::new(runtime)),
            journal,
            admission,
            controller,
            play,
            receipt,
        }
    }
    fn request(&self) -> DispatchRequest {
        DispatchRequest::Play {
            effect: self.play.clone(),
            receipt: self.receipt.clone(),
        }
    }
    fn transport(&self, origin: String) -> HandyTransport {
        HandyTransport {
            binding: TransportBinding {
                admission: self.admission.clone(),
                session: self.play.session.clone(),
                stream_id: 42,
            },
            origin,
            credentials: Arc::new(crate::handy_credentials::Credentials {
                api: "fixture-api".into(),
                connection: "fixture-connection".into(),
            }),
        }
    }
    fn dispatcher(&self, origin: String) -> HandyDispatcher {
        test_dispatcher(
            self.runtime.clone(),
            self.journal.receipt_binding(),
            self.transport(origin),
        )
    }
    fn authority(
        &self,
    ) -> impl FnOnce(Instant, &AtomicBool) -> Result<DispatchAuthority, DispatchFailure> + Send + 'static
    {
        let admission = self.admission.clone();
        let controller = self.controller.clone();
        move |_, _| {
            Ok(DispatchAuthority {
                admission,
                controller: Some(controller),
                clock_domain: clock().domain,
                observed_tick_ns: clock().monotonic_ns,
                lease_deadline_ns: clock().monotonic_ns + 2_000_000_000,
            })
        }
    }
}
fn test_dispatcher(
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    binding: EffectReceiptBinding,
    transport: HandyTransport,
) -> HandyDispatcher {
    let mut dispatcher = HandyDispatcher::new(runtime, binding, transport);
    dispatcher.clock = Arc::new(|| Ok(clock().monotonic_ns));
    dispatcher
}
impl HandyDispatcher {
    fn start_with_facts<F>(
        &mut self,
        request: DispatchRequest,
        deadline: Instant,
        authorize: F,
    ) -> Result<(), Box<StartFailure>>
    where
        F: FnOnce(Instant, &AtomicBool) -> Result<DispatchAuthority, DispatchFailure>
            + Send
            + 'static,
    {
        self.start(request, deadline, move |end, cancel, gate| {
            gate.apply(authorize(end, cancel)?)
        })
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn start<F>(dispatcher: &mut HandyDispatcher, request: DispatchRequest, authorize: F)
where
    F: FnOnce(Instant, &AtomicBool) -> Result<DispatchAuthority, DispatchFailure> + Send + 'static,
{
    dispatcher
        .start_with_facts(request, deadline(), authorize)
        .unwrap_or_else(|e| panic!("start refused: {:?}", e.reason));
}
fn finish(dispatcher: &mut HandyDispatcher) -> DispatchCompletion {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = dispatcher.poll() {
            return result;
        }
        assert!(Instant::now() < end, "owned dispatcher did not finish");
        thread::sleep(Duration::from_millis(1));
    }
}
fn reply(state: u8, stream: u64) -> String {
    format!(
        r#"{{"result":{{"play_state":{state},"stream_id":{stream},"current_time":1234,"playback_rate":1.0,"loop":false,"pause_on_starving":false}}}}"#
    )
}
fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end, "fixture accept deadline");
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("fixture accept: {e}"),
        }
    }
}
fn request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    request_with_keys(stream, "fixture-api", "fixture-connection")
}
fn request_with_keys(
    stream: &mut TcpStream,
    api_key: &str,
    connection_key: &str,
) -> (String, Vec<u8>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    assert!(
        reader.read_line(&mut line).unwrap() > 0,
        "fixture request ended early"
    );
    let first = line.trim().to_owned();
    let mut length = 0;
    let mut api = false;
    let mut connection = false;
    loop {
        line.clear();
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "fixture request ended early"
        );
        if line == "\r\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().unwrap();
        }
        api |= lower.trim() == format!("x-api-key: {api_key}");
        connection |= lower.trim() == format!("x-connection-key: {connection_key}");
    }
    assert!(api && connection);
    assert!(length < 1024);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (first, body)
}
fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    // Timeouts/cancellation may already have closed the client connection.
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}
type CapturedRequest = (String, Vec<u8>);
type MockServer = JoinHandle<Vec<CapturedRequest>>;
fn mock(responses: Vec<(u16, String, Duration)>) -> (String, MockServer) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let mut requests = vec![];
        for (status, body, delay) in responses {
            let mut stream = accept(&listener);
            requests.push(request(&mut stream));
            thread::sleep(delay);
            respond(&mut stream, status, &body);
        }
        requests
    });
    (origin, worker)
}
fn unused_origin() -> (String, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    (
        format!("http://{}", listener.local_addr().unwrap()),
        listener,
    )
}

#[test]
fn actual_http_play_stop_and_observation_preserve_uncertainty_and_exact_bytes() {
    let mut f = Fixture::new();
    let (origin, server) = mock(vec![
        (200, reply(1, 42), Duration::ZERO),
        (200, reply(2, 42), Duration::ZERO),
    ]);
    let mut dispatcher = f.dispatcher(origin);
    start(&mut dispatcher, f.request(), f.authority());
    let completion = finish(&mut dispatcher);
    assert!(matches!(
        completion.attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Acknowledged,
            ..
        }
    ));
    let CompletionObservation::Play(mut play) = completion
        .observe(&mut f.runtime.lock().unwrap(), &f.journal.receipt_binding())
        .unwrap()
    else {
        panic!("play observation");
    };
    assert!(play.stop.unwrap().is_none());
    assert!(play.observation.journal_pending());
    play.observation.persist(&mut f.journal).unwrap();
    let stop = f
        .runtime
        .lock()
        .unwrap()
        .request_stop_durable(&f.controller, &mut f.journal)
        .unwrap()
        .unwrap();
    start(&mut dispatcher, DispatchRequest::Stop(stop), f.authority());
    let completion = finish(&mut dispatcher);
    let CompletionObservation::Stop(mut stop) = completion
        .observe(&mut f.runtime.lock().unwrap(), &f.journal.receipt_binding())
        .unwrap()
    else {
        panic!("stop observation");
    };
    stop.persist(&mut f.journal).unwrap();
    assert_eq!(
        f.runtime.lock().unwrap().session().state(),
        DeviceState::StopUnconfirmed
    );
    assert!(f.runtime.lock().unwrap().session().physical_stop_pending());
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "PUT /hssp/play HTTP/1.1");
    assert_eq!(requests[0].1, f.play.request().body().unwrap());
    assert_eq!(requests[1].0, "PUT /hssp/stop HTTP/1.1");
    assert!(requests[1].1.is_empty());
    assert_eq!(
        f.journal
            .history(&f.admission.configuration.device, 10)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn duplicate_claims_and_unread_job_cannot_send_a_second_request() {
    let f = Fixture::new();
    let (origin, server) = mock(vec![(200, reply(1, 42), Duration::ZERO)]);
    let mut first = f.dispatcher(origin.clone());
    start(&mut first, f.request(), f.authority());
    // Do not consume the first completion until the duplicate is checked.
    let end = Instant::now() + Duration::from_secs(5);
    while !first.active.as_ref().unwrap().worker.is_finished() {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    let failure = first
        .start_with_facts(f.request(), deadline(), f.authority())
        .err()
        .unwrap();
    assert_eq!(failure.reason, DispatchFailure::Busy);
    let mut second = f.dispatcher(origin);
    start(&mut second, failure.request, f.authority());
    assert!(matches!(
        finish(&mut second).attempt,
        DispatchAttempt::Rejected(DispatchFailure::Authority(PlaybackFailure::RetiredCommand))
    ));
    assert!(f.runtime.lock().unwrap().effect_is_current(&f.play));
    assert!(matches!(
        finish(&mut first).attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Acknowledged,
            ..
        }
    ));
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn authority_clock_and_loaded_binding_refusals_make_no_network_claim() {
    for mode in 0..7 {
        let f = Fixture::new();
        let (origin, listener) = unused_origin();
        let mut transport = f.transport(origin);
        if mode == 4 {
            transport.binding.admission.program_artifact =
                ArtifactId::new("different-program").unwrap();
        }
        let binding = if mode == 5 {
            DeviceEffectJournal::open(
                f._dir.path(),
                DeviceSessionId::new("different-boot").unwrap(),
            )
            .unwrap()
            .receipt_binding()
        } else {
            f.journal.receipt_binding()
        };
        if mode == 6 {
            f.runtime
                .lock()
                .unwrap()
                .reconnect(DeviceSessionId::new("reconnected").unwrap())
                .unwrap();
        }
        let mut dispatcher = test_dispatcher(f.runtime.clone(), binding, transport);
        let authority = f.authority();
        start(&mut dispatcher, f.request(), move |deadline, cancelled| {
            if mode == 0 {
                return Err(DispatchFailure::Authority(
                    PlaybackFailure::AdmissionChanged,
                ));
            }
            let mut facts = authority(deadline, cancelled)?;
            if mode == 1 {
                facts.observed_tick_ns += 3_000_000_000;
            }
            if mode == 2 {
                facts.clock_domain = ArtifactId::new("different-clock").unwrap();
            }
            if mode == 3 {
                facts.admission.revision = RevisionId::new(99);
            }
            Ok(facts)
        });
        let completion = finish(&mut dispatcher);
        assert!(matches!(completion.attempt, DispatchAttempt::Rejected(_)));
        assert!(listener.accept().is_err());
        assert_eq!(f.runtime.lock().unwrap().claimed_play, None);
        assert_eq!(
            f.runtime.lock().unwrap().effect_is_current(&f.play),
            mode != 6
        );
    }
}

#[test]
fn malformed_error_wrong_stream_and_late_responses_are_unknown_without_retry() {
    for (status, body, delay, budget) in [
        (
            200,
            "{\"error\":{\"message\":\"fixture-connection\"}}".into(),
            Duration::ZERO,
            Duration::from_secs(1),
        ),
        (200, reply(1, 43), Duration::ZERO, Duration::from_secs(1)),
        (200, reply(2, 42), Duration::ZERO, Duration::from_secs(1)),
        (500, reply(1, 42), Duration::ZERO, Duration::from_secs(1)),
        (
            200,
            "x".repeat(handy_wire::RESPONSE_LIMIT + 1),
            Duration::ZERO,
            Duration::from_secs(1),
        ),
        (
            200,
            reply(1, 42),
            Duration::from_millis(150),
            Duration::from_millis(40),
        ),
    ] {
        let f = Fixture::new();
        let (origin, server) = mock(vec![(status, body, delay)]);
        let mut dispatcher = f.dispatcher(origin);
        dispatcher
            .start_with_facts(f.request(), Instant::now() + budget, f.authority())
            .unwrap_or_else(|e| panic!("{:?}", e.reason));
        let completion = finish(&mut dispatcher);
        assert!(matches!(
            completion.attempt,
            DispatchAttempt::Observed {
                outcome: DispatchOutcome::Unknown,
                response: None
            }
        ));
        let CompletionObservation::Play(play) = completion
            .observe(&mut f.runtime.lock().unwrap(), &f.journal.receipt_binding())
            .unwrap()
        else {
            panic!("play observation");
        };
        assert!(play.stop.unwrap().is_some());
        assert!(f.runtime.lock().unwrap().session().physical_stop_pending());
        assert_eq!(server.join().unwrap().len(), 1);
    }
}

#[test]
fn cancellation_before_claim_is_inert_and_after_claim_preserves_unknown_outcome() {
    let f = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut dispatcher = f.dispatcher(origin);
    let (entered, wait) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let authority = f.authority();
    start(&mut dispatcher, f.request(), move |deadline, cancelled| {
        entered.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(3)).unwrap();
        authority(deadline, cancelled)
    });
    wait.recv_timeout(Duration::from_secs(3)).unwrap();
    dispatcher.cancel();
    release.send(()).unwrap();
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Rejected(DispatchFailure::Cancelled)
    ));
    assert!(listener.accept().is_err());
    assert_eq!(f.runtime.lock().unwrap().claimed_play, None);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (entered, wait) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        let data = request(&mut stream);
        entered.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(3)).unwrap();
        respond(&mut stream, 200, &reply(1, 42));
        data
    });
    let mut dispatcher = f.dispatcher(origin);
    start(&mut dispatcher, f.request(), f.authority());
    wait.recv_timeout(Duration::from_secs(3)).unwrap();
    dispatcher.cancel();
    release.send(()).unwrap();
    let completion = dispatcher.shutdown().unwrap();
    assert!(matches!(
        completion.attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Unknown,
            ..
        }
    ));
    assert_eq!(
        f.runtime.lock().unwrap().claimed_play,
        Some(f.play.sequence)
    );
    assert_eq!(server.join().unwrap().0, "PUT /hssp/play HTTP/1.1");
}

#[test]
fn revocation_during_actual_io_cannot_be_undone_by_a_late_ack() {
    let mut f = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (entered, wait) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        let data = request(&mut stream);
        entered.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(3)).unwrap();
        respond(&mut stream, 200, &reply(1, 42));
        data
    });
    let mut dispatcher = f.dispatcher(origin);
    start(&mut dispatcher, f.request(), f.authority());
    wait.recv_timeout(Duration::from_secs(3)).unwrap();
    let stop = f
        .runtime
        .try_lock()
        .unwrap()
        .revoke_controller()
        .unwrap()
        .unwrap();
    release.send(()).unwrap();
    let completion = finish(&mut dispatcher);
    assert!(matches!(
        completion.observe(&mut f.runtime.lock().unwrap(), &f.journal.receipt_binding()),
        Err(DispatchFailure::Authority(PlaybackFailure::RetiredCommand))
    ));
    assert!(f.runtime.lock().unwrap().effect_is_current(&stop));
    assert!(f.runtime.lock().unwrap().session().controller().is_none());
    let binding = f.journal.receipt_binding();
    let mut historical = completion.historical_observation(&binding).unwrap();
    assert_eq!(historical.outcome(), DispatchOutcome::Acknowledged);
    let db =
        rusqlite::Connection::open(f._dir.path().join("device-effects/journal.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_late BEFORE UPDATE OF outcome ON effects BEGIN SELECT RAISE(ABORT,'synthetic late result write failure'); END;").unwrap();
    assert_eq!(
        historical.persist(&mut f.journal),
        Err(PlaybackFailure::JournalUnavailable)
    );
    assert!(historical.journal_pending());
    // A metadata failure neither loses the newer stop nor permits another play.
    assert!(f.runtime.lock().unwrap().effect_is_current(&stop));
    db.execute_batch("DROP TRIGGER fail_late").unwrap();
    historical.persist(&mut f.journal).unwrap();
    historical.persist(&mut f.journal).unwrap();
    assert!(!historical.journal_pending());
    let stop = f
        .runtime
        .lock()
        .unwrap()
        .prepare_stop_durable(stop, &mut f.journal)
        .unwrap();
    assert!(f.runtime.lock().unwrap().prepared_stop_is_current(&stop));
    assert!(f.runtime.lock().unwrap().session().controller().is_none());
    let history = f
        .journal
        .history(&f.admission.configuration.device, 10)
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].outcome, None);
    assert_eq!(history[1].outcome, Some(DispatchOutcome::Acknowledged));
    assert_eq!(server.join().unwrap().0, "PUT /hssp/play HTTP/1.1");
}

#[test]
fn invalid_credentials_budget_and_authorizer_panic_preserve_original_request() {
    let f = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut transport = f.transport(origin.clone());
    Arc::get_mut(&mut transport.credentials).unwrap().api = "bad\ncredential".into();
    let mut dispatcher = test_dispatcher(f.runtime.clone(), f.journal.receipt_binding(), transport);
    let failure = dispatcher
        .start_with_facts(f.request(), deadline(), f.authority())
        .err()
        .unwrap();
    assert_eq!(failure.reason, DispatchFailure::Credentials);
    let mut dispatcher = f.dispatcher(origin);
    let failure = dispatcher
        .start_with_facts(failure.request, Instant::now(), f.authority())
        .err()
        .unwrap();
    assert_eq!(failure.reason, DispatchFailure::InvalidBudget);
    start(&mut dispatcher, failure.request, |_, _| {
        panic!("synthetic authority failure")
    });
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Rejected(DispatchFailure::WorkerUnavailable)
    ));
    assert!(listener.accept().is_err());
    assert_eq!(f.runtime.lock().unwrap().claimed_play, None);
    for name in ["", "1KEY", "KEY=value", "KEY\nVALUE"] {
        assert!(matches!(
            HandyTransport::from_environment(
                TransportBinding {
                    admission: f.admission.clone(),
                    session: f.play.session.clone(),
                    stream_id: 42
                },
                name,
                "SECOND_KEY"
            ),
            Err(DispatchFailure::Credentials)
        ));
    }
}

#[test]
fn stop_claims_are_single_use_across_dispatchers() {
    let mut f = Fixture::new();
    let raw = f
        .runtime
        .lock()
        .unwrap()
        .request_stop(&f.controller)
        .unwrap()
        .unwrap();
    let first_stop = f
        .runtime
        .lock()
        .unwrap()
        .prepare_stop_durable(raw.clone(), &mut f.journal)
        .unwrap();
    let duplicate = f
        .runtime
        .lock()
        .unwrap()
        .prepare_stop_durable(raw.clone(), &mut f.journal)
        .unwrap();
    let (origin, server) = mock(vec![(200, reply(2, 42), Duration::ZERO)]);
    let mut first = f.dispatcher(origin.clone());
    start(&mut first, DispatchRequest::Stop(first_stop), f.authority());
    let completed = finish(&mut first);
    let mut second = f.dispatcher(origin);
    start(&mut second, DispatchRequest::Stop(duplicate), f.authority());
    assert!(matches!(
        finish(&mut second).attempt,
        DispatchAttempt::Rejected(DispatchFailure::Authority(PlaybackFailure::RetiredCommand))
    ));
    assert!(f.runtime.lock().unwrap().effect_is_current(&raw));
    let CompletionObservation::Stop(mut observation) = completed
        .observe(&mut f.runtime.lock().unwrap(), &f.journal.receipt_binding())
        .unwrap()
    else {
        panic!("stop observation");
    };
    observation.persist(&mut f.journal).unwrap();
    assert_eq!(server.join().unwrap().len(), 1);
    assert_eq!(
        f.runtime.lock().unwrap().session().state(),
        DeviceState::StopUnconfirmed
    );
}

#[test]
fn emergency_and_durable_stops_share_one_claim_and_retrospective_metadata() {
    for emergency_first in [false, true] {
        let mut f = Fixture::new();
        let binding = f.journal.receipt_binding();
        let raw = f
            .runtime
            .lock()
            .unwrap()
            .request_stop(&f.controller)
            .unwrap()
            .unwrap();
        let intent = binding
            .capture_intent(&f.runtime.lock().unwrap(), &raw)
            .unwrap();
        let id = intent.record_id().unwrap();
        let emergency = f
            .runtime
            .lock()
            .unwrap()
            .prepare_emergency_stop(raw.clone(), intent, &binding)
            .unwrap();
        let durable = f
            .runtime
            .lock()
            .unwrap()
            .prepare_stop_durable(raw.clone(), &mut f.journal)
            .unwrap();
        let (first, second) = if emergency_first {
            (
                DispatchRequest::EmergencyStop(Box::new(emergency)),
                DispatchRequest::Stop(durable),
            )
        } else {
            (
                DispatchRequest::Stop(durable),
                DispatchRequest::EmergencyStop(Box::new(emergency)),
            )
        };
        let (origin, server) = mock(vec![(200, reply(2, 42), Duration::ZERO)]);
        let mut primary = f.dispatcher(origin.clone());
        primary.clock = Arc::new(|| panic!("emergency stop must not need a play clock"));
        start(&mut primary, first, f.authority());
        let completion = finish(&mut primary);
        let mut duplicate = f.dispatcher(origin);
        start(&mut duplicate, second, f.authority());
        assert!(matches!(
            finish(&mut duplicate).attempt,
            DispatchAttempt::Rejected(DispatchFailure::Authority(PlaybackFailure::RetiredCommand))
        ));
        match completion
            .observe(&mut f.runtime.lock().unwrap(), &binding)
            .unwrap()
        {
            CompletionObservation::EmergencyStop(mut record) => {
                assert!(record.journal_pending());
                record.try_persist(&mut f.journal).unwrap();
                record.try_persist(&mut f.journal).unwrap();
                assert!(f.journal.emergency_stop_recorded(&id).unwrap());
            }
            CompletionObservation::Stop(mut record) => record.try_persist(&mut f.journal).unwrap(),
            _ => panic!("stop cannot become play"),
        }
        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(
            f.runtime.lock().unwrap().session().state(),
            DeviceState::StopUnconfirmed
        );
        assert!(f.runtime.lock().unwrap().session().physical_stop_pending());
    }
}

#[test]
fn emergency_envelope_refuses_play_and_changed_intent_or_runtime() {
    let f = Fixture::new();
    let binding = f.journal.receipt_binding();
    let play_intent = binding
        .capture_intent(&f.runtime.lock().unwrap(), &f.play)
        .unwrap();
    assert!(f
        .runtime
        .lock()
        .unwrap()
        .prepare_emergency_stop(f.play.clone(), play_intent, &binding)
        .is_err());
    let raw = f
        .runtime
        .lock()
        .unwrap()
        .revoke_controller()
        .unwrap()
        .unwrap();
    let intent = binding
        .capture_intent(&f.runtime.lock().unwrap(), &raw)
        .unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let other = DeviceEffectJournal::open(
        other_dir.path(),
        DeviceSessionId::new("foreign-boot").unwrap(),
    )
    .unwrap();
    assert!(f
        .runtime
        .lock()
        .unwrap()
        .prepare_emergency_stop(raw.clone(), intent.clone(), &other.receipt_binding())
        .is_err());
    let emergency = f
        .runtime
        .lock()
        .unwrap()
        .prepare_emergency_stop(raw, intent, &binding)
        .unwrap();
    f.runtime
        .lock()
        .unwrap()
        .reconnect(DeviceSessionId::new("replacement-session").unwrap())
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut dispatcher = f.dispatcher(format!("http://{}", listener.local_addr().unwrap()));
    start(
        &mut dispatcher,
        DispatchRequest::EmergencyStop(Box::new(emergency)),
        f.authority(),
    );
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Rejected(DispatchFailure::Authority(PlaybackFailure::RetiredCommand))
    ));
    assert!(listener.accept().is_err());
}

#[test]
fn emergency_late_observation_can_record_history_without_restoring_a_session() {
    let mut f = Fixture::new();
    let binding = f.journal.receipt_binding();
    let raw = f
        .runtime
        .lock()
        .unwrap()
        .revoke_controller()
        .unwrap()
        .unwrap();
    let intent = binding
        .capture_intent(&f.runtime.lock().unwrap(), &raw)
        .unwrap();
    let id = intent.record_id().unwrap();
    let emergency = f
        .runtime
        .lock()
        .unwrap()
        .prepare_emergency_stop(raw, intent, &binding)
        .unwrap();
    let (origin, server) = mock(vec![(200, reply(2, 42), Duration::ZERO)]);
    let mut dispatcher = f.dispatcher(origin);
    start(
        &mut dispatcher,
        DispatchRequest::EmergencyStop(Box::new(emergency)),
        f.authority(),
    );
    let completed = finish(&mut dispatcher);
    f.runtime
        .lock()
        .unwrap()
        .reconnect(DeviceSessionId::new("replacement-session").unwrap())
        .unwrap();
    assert!(matches!(
        completed.observe(&mut f.runtime.lock().unwrap(), &binding),
        Err(DispatchFailure::Authority(PlaybackFailure::RetiredCommand))
    ));
    let mut record = completed
        .historical_emergency_observation(&binding)
        .unwrap();
    record.try_persist(&mut f.journal).unwrap();
    assert!(f.journal.emergency_stop_recorded(&id).unwrap());
    assert!(f.runtime.lock().unwrap().session().controller().is_none());
    assert!(f.runtime.lock().unwrap().session().physical_stop_pending());
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn redirect_never_forwards_the_request_or_credentials() {
    let f = Fixture::new();
    let (destination, destination_listener) = unused_origin();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        let captured = request(&mut stream);
        write!(stream,"HTTP/1.1 302 Found\r\nLocation: {destination}/hssp/play\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        captured
    });
    let mut dispatcher = f.dispatcher(origin);
    start(&mut dispatcher, f.request(), f.authority());
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Unknown,
            response: None
        }
    ));
    assert!(destination_listener.accept().is_err());
    assert_eq!(server.join().unwrap().0, "PUT /hssp/play HTTP/1.1");
}

#[test]
fn dropping_an_inflight_job_joins_and_keeps_the_durable_outcome_unknown() {
    let f = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (entered, wait) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        let captured = request(&mut stream);
        entered.send(()).unwrap();
        thread::sleep(Duration::from_millis(80));
        respond(&mut stream, 200, &reply(1, 42));
        captured
    });
    let mut dispatcher = f.dispatcher(origin);
    start(&mut dispatcher, f.request(), f.authority());
    wait.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(dispatcher);
    assert_eq!(Arc::strong_count(&f.runtime), 1);
    assert_eq!(
        f.runtime.lock().unwrap().claimed_play,
        Some(f.play.sequence)
    );
    assert!(f.runtime.lock().unwrap().session().physical_stop_pending());
    assert_eq!(
        f.journal
            .history(&f.admission.configuration.device, 10)
            .unwrap()[0]
            .outcome,
        None
    );
    assert_eq!(server.join().unwrap().0, "PUT /hssp/play HTTP/1.1");
}

#[test]
fn authority_lookup_cannot_extend_the_absolute_transport_deadline() {
    let f = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut dispatcher = f.dispatcher(origin);
    let authority = f.authority();
    dispatcher
        .start_with_facts(
            f.request(),
            Instant::now() + Duration::from_millis(40),
            move |deadline, cancelled| {
                thread::sleep(Duration::from_millis(100));
                authority(deadline, cancelled)
            },
        )
        .unwrap_or_else(|error| panic!("{:?}", error.reason));
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Rejected(DispatchFailure::InvalidBudget)
    ));
    assert!(listener.accept().is_err());
    assert_eq!(f.runtime.lock().unwrap().claimed_play, None);
}

#[test]
fn suspend_between_authorization_and_claim_expires_each_original_budget() {
    use std::sync::atomic::AtomicU64;
    for limit in ["source", "lease", "command"] {
        let base = clock().monotonic_ns;
        let mut mapping = clock();
        if limit == "source" {
            mapping.valid_until_ns = base + 1_000_000_000;
        }
        let f = Fixture::with_clock(mapping);
        let (origin, listener) = unused_origin();
        let tick = Arc::new(AtomicU64::new(base));
        let read = tick.clone();
        let mut dispatcher = f.dispatcher(origin);
        dispatcher.clock = Arc::new(move || Ok(read.load(Ordering::Acquire)));
        let authority = f.authority();
        start(&mut dispatcher, f.request(), move |end, cancel| {
            let mut a = authority(end, cancel)?;
            a.lease_deadline_ns = if limit == "lease" {
                base + 1_000_000_000
            } else {
                u64::MAX
            };
            // Instant barely advances; the same-domain clock includes suspend.
            tick.store(base + 2_000_000_000, Ordering::Release);
            Ok(a)
        });
        assert!(matches!(
            finish(&mut dispatcher).attempt,
            DispatchAttempt::Rejected(DispatchFailure::Authority(PlaybackFailure::StaleCommand))
        ));
        assert!(listener.accept().is_err());
        assert_eq!(f.runtime.lock().unwrap().claimed_play, None);
    }
}

#[test]
fn clock_failure_or_reversal_before_claim_sends_nothing() {
    for failure in [false, true] {
        let f = Fixture::new();
        let (origin, listener) = unused_origin();
        let mut dispatcher = f.dispatcher(origin);
        dispatcher.clock = Arc::new(move || {
            if failure {
                Err(DispatchFailure::Authority(PlaybackFailure::InvalidClock))
            } else {
                Ok(clock().monotonic_ns - 1)
            }
        });
        start(&mut dispatcher, f.request(), f.authority());
        assert!(matches!(
            finish(&mut dispatcher).attempt,
            DispatchAttempt::Rejected(DispatchFailure::Authority(PlaybackFailure::InvalidClock))
        ));
        assert!(listener.accept().is_err());
        assert_eq!(f.runtime.lock().unwrap().claimed_play, None);
    }
}

#[test]
fn suspend_or_clock_loss_after_claim_prevents_send_without_releasing_the_claim() {
    use std::sync::atomic::AtomicUsize;
    for changed_at in [1, 2] {
        for mode in ["expiry", "reversal", "failure"] {
            let f = Fixture::new();
            let (origin, listener) = unused_origin();
            let mut dispatcher = f.dispatcher(origin);
            let reads = AtomicUsize::new(0);
            dispatcher.clock = Arc::new(move || {
                if reads.fetch_add(1, Ordering::AcqRel) < changed_at {
                    return Ok(clock().monotonic_ns);
                }
                match mode {
                    "expiry" => Ok(clock().monotonic_ns + 3_000_000_000),
                    "reversal" => Ok(clock().monotonic_ns - 1),
                    _ => Err(DispatchFailure::Authority(PlaybackFailure::InvalidClock)),
                }
            });
            start(&mut dispatcher, f.request(), f.authority());
            let completion = finish(&mut dispatcher);
            assert!(matches!(
                completion.attempt,
                DispatchAttempt::Observed {
                    outcome: DispatchOutcome::Unknown,
                    response: None
                }
            ));
            assert!(listener.accept().is_err());
            assert_eq!(
                f.runtime.lock().unwrap().claimed_play,
                Some(f.play.sequence)
            );
            let CompletionObservation::Play(p) = completion
                .observe(&mut f.runtime.lock().unwrap(), &f.journal.receipt_binding())
                .unwrap()
            else {
                panic!()
            };
            assert!(p.stop.unwrap().is_some());
            assert!(f.runtime.lock().unwrap().session().physical_stop_pending());
        }
    }
}

#[test]
fn response_after_suspend_cannot_acknowledge_an_expired_play() {
    use std::sync::atomic::AtomicU64;
    for source_expires in [false, true] {
        let base = clock().monotonic_ns;
        let mut mapping = clock();
        if source_expires {
            mapping.valid_until_ns = base + 1_000_000_000;
        }
        let f = Fixture::with_clock(mapping);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let tick = Arc::new(AtomicU64::new(base));
        let update = tick.clone();
        let server = thread::spawn(move || {
            let mut stream = accept(&listener);
            let captured = request(&mut stream);
            update.store(base + 1_000_000_000, Ordering::Release);
            respond(&mut stream, 200, &reply(1, 42));
            captured
        });
        let mut dispatcher = f.dispatcher(origin);
        dispatcher.clock = Arc::new(move || Ok(tick.load(Ordering::Acquire)));
        let authority = f.authority();
        start(&mut dispatcher, f.request(), move |end, cancel| {
            let mut a = authority(end, cancel)?;
            if !source_expires {
                a.lease_deadline_ns = base + 1_000_000_000;
            }
            Ok(a)
        });
        assert!(matches!(
            finish(&mut dispatcher).attempt,
            DispatchAttempt::Observed {
                outcome: DispatchOutcome::Unknown,
                response: None
            }
        ));
        assert_eq!(server.join().unwrap().0, "PUT /hssp/play HTTP/1.1");
    }
}

#[test]
fn original_source_and_lease_bound_the_actual_http_wait() {
    for source_expires in [false, true] {
        let base = clock().monotonic_ns;
        let mut mapping = clock();
        if source_expires {
            mapping.valid_until_ns = base + 30_000_000;
        }
        let f = Fixture::with_clock(mapping);
        let (origin, server) = mock(vec![(200, reply(1, 42), Duration::from_millis(100))]);
        let mut dispatcher = f.dispatcher(origin);
        let authority = f.authority();
        start(&mut dispatcher, f.request(), move |end, cancel| {
            let mut a = authority(end, cancel)?;
            if !source_expires {
                a.lease_deadline_ns = base + 30_000_000;
            }
            Ok(a)
        });
        // The fixture host clock is fixed; the original 30 ms remainder still
        // bounds HTTP even though the outer Instant budget is two seconds.
        assert!(matches!(
            finish(&mut dispatcher).attempt,
            DispatchAttempt::Observed {
                outcome: DispatchOutcome::Unknown,
                response: None
            }
        ));
        assert_eq!(server.join().unwrap().len(), 1);
    }
}

#[test]
fn stop_transport_remains_available_without_a_play_clock_or_live_lease() {
    let mut f = Fixture::new();
    let stop = f
        .runtime
        .lock()
        .unwrap()
        .revoke_controller_durable(&mut f.journal)
        .unwrap()
        .unwrap();
    let (origin, server) = mock(vec![(200, reply(2, 42), Duration::ZERO)]);
    let mut dispatcher = f.dispatcher(origin);
    dispatcher.clock = Arc::new(|| panic!("stop must not depend on play clock availability"));
    let authority = f.authority();
    start(
        &mut dispatcher,
        DispatchRequest::Stop(stop),
        move |end, cancel| {
            let mut a = authority(end, cancel)?;
            a.controller = None;
            a.lease_deadline_ns = 0;
            Ok(a)
        },
    );
    let result = finish(&mut dispatcher);
    assert!(matches!(
        result.attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Acknowledged,
            ..
        }
    ));
    result
        .observe(&mut f.runtime.lock().unwrap(), &f.journal.receipt_binding())
        .unwrap();
    assert_eq!(
        f.runtime.lock().unwrap().session().state(),
        DeviceState::StopUnconfirmed
    );
    assert!(f.runtime.lock().unwrap().session().physical_stop_pending());
    assert_eq!(server.join().unwrap()[0].0, "PUT /hssp/stop HTTP/1.1");
}

#[test]
fn historical_observation_refuses_rejection_and_foreign_receipt_binding() {
    let f = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut dispatcher = f.dispatcher(origin);
    start(&mut dispatcher, f.request(), |_, _| {
        Err(DispatchFailure::Cancelled)
    });
    let rejected = finish(&mut dispatcher);
    assert!(matches!(
        rejected.historical_observation(&f.journal.receipt_binding()),
        Err(DispatchFailure::Cancelled)
    ));
    assert!(listener.accept().is_err());
    let completion = DispatchCompletion {
        request: f.request(),
        attempt: DispatchAttempt::Observed {
            outcome: DispatchOutcome::Unknown,
            response: None,
        },
    };
    let other = tempfile::tempdir().unwrap();
    let foreign =
        DeviceEffectJournal::open(other.path(), DeviceSessionId::new("foreign").unwrap()).unwrap();
    assert!(matches!(
        completion.historical_observation(&foreign.receipt_binding()),
        Err(DispatchFailure::Authority(
            PlaybackFailure::JournalUnavailable
        ))
    ));
    assert_eq!(
        f.journal
            .history(&f.admission.configuration.device, 10)
            .unwrap()[0]
            .outcome,
        None
    );
}

#[test]
fn numeric_credential_echo_in_an_otherwise_valid_ack_is_withheld() {
    let f = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        request_with_keys(&mut stream, "1234", "fixture-connection");
        respond(&mut stream, 200, &reply(1, 42));
    });
    let mut transport = f.transport(origin);
    Arc::get_mut(&mut transport.credentials).unwrap().api = "1234".into();
    let mut dispatcher = test_dispatcher(f.runtime.clone(), f.journal.receipt_binding(), transport);
    start(&mut dispatcher, f.request(), f.authority());
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Unknown,
            response: None
        }
    ));
    server.join().unwrap();
}

#[test]
fn clock_error_growth_after_claim_is_rechecked_before_send() {
    use std::sync::atomic::AtomicUsize;
    let f = Fixture::new();
    f.runtime.lock().unwrap().timing.max_clock_uncertainty_ns = 200_000;
    let (origin, listener) = unused_origin();
    let mut dispatcher = f.dispatcher(origin);
    let reads = AtomicUsize::new(0);
    dispatcher.clock = Arc::new(move || {
        Ok(clock().monotonic_ns
            + if reads.fetch_add(1, Ordering::AcqRel) == 0 {
                0
            } else {
                500_000
            })
    });
    start(&mut dispatcher, f.request(), f.authority());
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Unknown,
            response: None
        }
    ));
    assert!(listener.accept().is_err());
    assert_eq!(
        f.runtime.lock().unwrap().claimed_play,
        Some(f.play.sequence)
    );
}

#[test]
fn final_claim_occurs_inside_the_authority_guard_and_http_occurs_after_it() {
    let f = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut dispatcher = f.dispatcher(origin);
    let mutation_guard = Arc::new(Mutex::new(()));
    let guard = mutation_guard.clone();
    let authority = f.authority();
    let (claimed, wait) = mpsc::sync_channel(1);
    let (release, hold) = mpsc::sync_channel(1);
    dispatcher
        .start(f.request(), deadline(), move |end, cancel, claim| {
            let _guard = guard.lock().unwrap();
            let handoff = claim.apply(authority(end, cancel)?)?;
            claimed.send(()).unwrap();
            hold.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(handoff)
        })
        .unwrap_or_else(|e| panic!("{:?}", e.reason));
    wait.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(mutation_guard.try_lock().is_err());
    assert_eq!(
        f.runtime.lock().unwrap().claimed_play,
        Some(f.play.sequence)
    );
    assert!(
        listener.accept().is_err(),
        "HTTP escaped the authority continuation"
    );
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        let captured = request(&mut stream);
        respond(&mut stream, 200, &reply(1, 42));
        captured
    });
    release.send(()).unwrap();
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Acknowledged,
            ..
        }
    ));
    assert!(mutation_guard.try_lock().is_ok());
    assert_eq!(server.join().unwrap().0, "PUT /hssp/play HTTP/1.1");
}

#[test]
fn authorizer_failure_after_consuming_claim_is_unknown_and_never_rejected() {
    let f = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut dispatcher = f.dispatcher(origin);
    let authority = f.authority();
    dispatcher
        .start(f.request(), deadline(), move |end, cancel, claim| {
            let _handoff = claim.apply(authority(end, cancel)?)?;
            Err(DispatchFailure::Cancelled)
        })
        .unwrap_or_else(|e| panic!("{:?}", e.reason));
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Unknown,
            response: None
        }
    ));
    assert_eq!(
        f.runtime.lock().unwrap().claimed_play,
        Some(f.play.sequence)
    );
    assert!(listener.accept().is_err());
}

#[test]
fn claimed_handoff_from_another_dispatch_cannot_bypass_this_requests_claim() {
    let first = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut dispatcher = first.dispatcher(origin);
    let authority = first.authority();
    let (save, receive) = mpsc::sync_channel(1);
    dispatcher
        .start(first.request(), deadline(), move |end, cancel, gate| {
            save.send(gate.apply(authority(end, cancel)?)?).unwrap();
            Err(DispatchFailure::Cancelled)
        })
        .unwrap_or_else(|e| panic!("{:?}", e.reason));
    let captured = receive.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Observed {
            outcome: DispatchOutcome::Unknown,
            ..
        }
    ));
    assert!(listener.accept().is_err());
    let second = Fixture::new();
    let (origin, listener) = unused_origin();
    let mut dispatcher = second.dispatcher(origin);
    dispatcher
        .start(second.request(), deadline(), move |_, _, _| Ok(captured))
        .unwrap_or_else(|e| panic!("{:?}", e.reason));
    assert!(matches!(
        finish(&mut dispatcher).attempt,
        DispatchAttempt::Rejected(DispatchFailure::BindingMismatch)
    ));
    assert_eq!(second.runtime.lock().unwrap().claimed_play, None);
    assert!(listener.accept().is_err());
}

#[test]
fn detached_stop_intent_cannot_restore_a_retired_runtime() {
    let mut f = Fixture::new();
    let binding = f.journal.receipt_binding();
    let (effect, intent) = {
        let mut runtime = f.runtime.lock().unwrap();
        let effect = runtime.revoke_controller().unwrap().unwrap();
        let intent = binding.capture_intent(&runtime, &effect).unwrap();
        runtime
            .reconnect(DeviceSessionId::new("intent-retired").unwrap())
            .unwrap();
        assert!(binding.capture_intent(&runtime, &effect).is_err());
        (effect, intent)
    };
    let receipt = f.journal.prepare_intent(&intent).unwrap();
    assert!(matches!(
        f.runtime
            .lock()
            .unwrap()
            .attach_stop_receipt(effect, receipt, &binding),
        Err(PlaybackFailure::RetiredCommand)
    ));
    let history = f
        .journal
        .history(&f.admission.configuration.device, 10)
        .unwrap();
    assert_eq!(history.len(), 2);
    assert!(history[0].record.stop && history[0].outcome.is_none());
    assert!(f.runtime.lock().unwrap().session().controller().is_none());
}

#[test]
fn detached_stop_intent_remains_bound_to_its_original_engine_boot() {
    let mut f = Fixture::new();
    let intent = {
        let mut runtime = f.runtime.lock().unwrap();
        let effect = runtime.revoke_controller().unwrap().unwrap();
        f.journal
            .receipt_binding()
            .capture_intent(&runtime, &effect)
            .unwrap()
    };
    let mut replacement = DeviceEffectJournal::open(
        f._dir.path(),
        DeviceSessionId::new("replacement-boot").unwrap(),
    )
    .unwrap();
    assert!(f.journal.prepare_intent(&intent).is_err());
    assert!(replacement.prepare_intent(&intent).is_err());
    assert_eq!(
        replacement
            .history(&f.admission.configuration.device, 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn referenced_media_drift_is_retained_from_claim_through_later_transport_checks() {
    let mut f = Fixture::new();
    // Manufacture only the private runtime mode for this clock-window test.
    // Full browser decision/journal admission is exercised by journal and
    // Engine fixtures; no network or physical qualification occurs here.
    let mode = StartReference::SharedInstant {
        media_rate_error_ppm: 500_000,
    };
    f.play.reference = Some(mode);
    let mut runtime = f.runtime.lock().unwrap();
    runtime.timing.max_age_ns = 10_000_000;
    runtime.timing.max_clock_uncertainty_ns = 1_000_000;
    runtime.timing.drift_ppm = 0;
    runtime.play_inflight = Some(f.play.clone());
    let authority = DispatchAuthority {
        admission: f.admission.clone(),
        controller: Some(f.controller.clone()),
        clock_domain: clock().domain,
        observed_tick_ns: clock().monotonic_ns,
        lease_deadline_ns: clock().monotonic_ns + 10_000_000,
    };
    let mut window = claim(
        &mut runtime,
        &f.play,
        &f.receipt,
        &f.journal.receipt_binding(),
        &authority,
        Some(clock().monotonic_ns + 1_000_000),
    )
    .unwrap()
    .unwrap();
    assert_eq!(window.source, clock());
    assert_eq!(window.timing.drift_ppm, 500_000);
    assert_eq!(runtime.timing.drift_ppm, 0);
    let later: HostClock = Arc::new(|| Ok(clock().monotonic_ns + 2_000_000));
    assert_eq!(
        window.remaining(&later),
        Err(DispatchFailure::Authority(PlaybackFailure::InvalidClock))
    );
    assert_eq!(window.source, clock());
}
