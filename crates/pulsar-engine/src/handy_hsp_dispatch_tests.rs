use super::*;
use crate::{
    device_effect_journal::{HspJournalState, HspStreamReceipt},
    handy_hsp_records::{HspBindingRecord, HspStreamRecord},
    handy_hsp_runtime::{
        HspAuthorityWindow, HspObservedOutcome, HspPlayReference, HspRuntimeState,
    },
};
use pulsar_core::{
    ArtifactId, AttemptId, ClientId, DeviceSessionId, HspPhaseWindow, HspPoint, HspSample,
    HspStreamIdentity, LiveIdentity, ProjectId, RevisionId, SourceVersionId,
};
use pulsar_protocol::{
    DeviceId, HandyRegistration, HandyRegistrationSettings, RequestId, SessionId,
};
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    sync::atomic::AtomicU64,
};

fn binding() -> HspBinding {
    HspBinding::new(
        HspStreamIdentity {
            generation: 1,
            stream_id: 42,
            live: LiveIdentity {
                epoch: AttemptId::new("epoch").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(1),
                source: SourceVersionId::new("source").unwrap(),
            },
        },
        ClientId::new("owner").unwrap(),
        DeviceSessionId::new("session").unwrap(),
        ArtifactId::new("clock").unwrap(),
    )
}
fn facts(now: u64, until: u64) -> HspDispatchFacts {
    HspDispatchFacts::new(
        binding(),
        HspPlayReference::new(
            now,
            HspPhaseWindow {
                earliest_ms: -100,
                latest_ms: -100,
            },
            -100,
            10_000,
        )
        .unwrap(),
        HspAuthorityWindow::new(until, until, until),
    )
    .unwrap()
}
fn device() -> DeviceIdentity {
    DeviceIdentity {
        firmware: "4.0.16".into(),
        model_number: 2,
        model_name: "H02".into(),
        model_variant: 0,
        session_id: "native-session".into(),
        firmware_status: 0,
    }
}
fn registration() -> HandyRegistration {
    HandyRegistration {
        device_id: DeviceId::new("device").unwrap(),
        revision: 1,
        settings: HandyRegistrationSettings {
            label: "synthetic native transport".into(),
            api_key_env: "FIXTURE_API".into(),
            connection_key_env: "FIXTURE_ROUTE".into(),
        },
        disabled: false,
        binding_sha256: "a".repeat(64),
    }
}

fn remote_state(stream: u32, tail: i32, playing: bool) -> HspState {
    HspState {
        play_state: if playing {
            HspPlayState::Playing
        } else {
            HspPlayState::Stopped
        },
        points: (tail + 1).max(0) as u32,
        max_points: 32,
        current_point: if tail < 0 { -1 } else { 0 },
        current_time: -100,
        looping: false,
        playback_rate: 1.0,
        first_point_time: 0,
        last_point_time: tail.max(0) as u32 * 100,
        stream_id: stream,
        tail_point_stream_index: tail,
        tail_point_stream_index_threshold: 1,
        pause_on_starving: false,
    }
}
#[derive(Clone, Copy)]
enum Reply {
    Normal,
    Malformed,
    Redirect,
    Error,
    Echo,
    Oversized,
    Lost,
}
struct PeerState {
    stream: u32,
    tail: i32,
    playing: bool,
    mode: u64,
    session: String,
    requests: Vec<(String, String, Vec<u8>)>,
    reply: Reply,
}
struct Peer {
    origin: String,
    state: Arc<Mutex<PeerState>>,
    hold: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Peer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(PeerState {
            stream: 9,
            tail: -1,
            playing: false,
            mode: 7,
            session: "native-session".into(),
            requests: vec![],
            reply: Reply::Normal,
        }));
        let hold = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let captured = state.clone();
        let held = hold.clone();
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let (mut socket, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let Some((method, path, body)) = read_request(&mut socket) else {
                    continue;
                };
                let (status, bytes) = {
                    let mut s = captured.lock().unwrap();
                    s.requests
                        .push((method.clone(), path.clone(), body.clone()));
                    if method == "PUT" {
                        match path.as_str() {
                            "/hsp/setup" => {
                                s.stream = 42;
                                s.tail = -1;
                                s.mode += 1;
                            }
                            "/hsp/add" => {
                                s.tail = serde_json::from_slice::<serde_json::Value>(&body).unwrap()
                                    ["tail_point_stream_index"]
                                    .as_i64()
                                    .unwrap() as i32
                            }
                            "/hsp/play" => s.playing = true,
                            "/hsp/stop" => s.playing = false,
                            _ => panic!("unexpected operation"),
                        }
                    }
                    let bytes = match path.as_str() {
                        "/info" => serde_json::to_vec(&serde_json::json!({"result": {
                            "fw_version":"4.0.16","fw_status":0,"hw_model_no":2,"hw_model_name":"H02", "hw_model_variant":0,"session_id":s.session
                        }})).unwrap(),
                        "/mode" => serde_json::to_vec(&serde_json::json!({"result":{"mode":4,"mode_session_id":s.mode}})).unwrap(),
                        _ => state_bytes(remote_state(s.stream, s.tail, s.playing)),
                    };
                    if method == "PUT" {
                        match s.reply {
                            Reply::Normal => (200, bytes),
                            Reply::Lost => (0, vec![]),
                            Reply::Malformed => (200, b"not-json".to_vec()),
                            Reply::Redirect => (307, bytes),
                            Reply::Error => (503, b"test-api-secret".to_vec()),
                            Reply::Echo => (200, br#"{"result":"test-api-secret"}"#.to_vec()),
                            Reply::Oversized => {
                                (200, vec![b'x'; handy_hsp_wire::HSP_RESPONSE_LIMIT + 1])
                            }
                        }
                    } else {
                        (200, bytes)
                    }
                };
                let held_until = Instant::now() + Duration::from_secs(2);
                while method == "PUT"
                    && held.load(Ordering::Acquire)
                    && !stopped.load(Ordering::Acquire)
                    && Instant::now() < held_until
                {
                    thread::sleep(Duration::from_millis(2));
                }
                if status == 0 {
                    continue;
                } // Deliberately lose the actual PUT response.
                let head = format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://127.0.0.1:1/secret\r\n\r\n", bytes.len());
                let _ = socket.write_all(head.as_bytes());
                let _ = socket.write_all(&bytes);
            }
        });
        Self {
            origin,
            state,
            hold,
            stop,
            worker: Some(worker),
        }
    }
    fn puts(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r.0 == "PUT")
            .count()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.hold.store(false, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn read_request(socket: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        if socket.read(&mut byte).ok()? == 0 {
            return None;
        }
        bytes.push(byte[0]);
        if bytes.len() > 16 * 1024 {
            return None;
        }
    }
    let header = std::str::from_utf8(&bytes).ok()?;
    assert!(
        header
            .to_ascii_lowercase()
            .contains("x-api-key: test-api-secret")
            || header
                .to_ascii_lowercase()
                .contains("x-api-key: rotated-api-secret")
    );
    let mut line = header.lines().next()?.split_whitespace();
    let method = line.next()?.to_owned();
    let path = line.next()?.to_owned();
    let length = header
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                .map(|(_, v)| v.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    if length > MAX_REQUEST {
        return None;
    }
    let mut body = vec![0; length];
    socket.read_exact(&mut body).ok()?;
    Some((method, path, body))
}
struct Rig {
    dir: tempfile::TempDir,
    peer: Peer,
    runtime: Arc<Mutex<HandyHspRuntime>>,
    journal: Arc<Mutex<DeviceEffectJournal>>,
    stream: HspStreamReceipt,
    dispatcher: HandyHspDispatcher,
    owner: HspNativeOwner,
    tick: Arc<AtomicU64>,
}
impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let peer = Peer::new();
        let runtime = Arc::new(Mutex::new(
            HandyHspRuntime::new(
                binding(),
                HspPolicy {
                    max_buffered_points: 32,
                    device_capacity_points: 32,
                    max_chunk_points: 2,
                    min_lead_ms: 10,
                    max_future_ms: 1000,
                    max_sample_age_ns: 1_000_000_000,
                    max_gap_ms: 200,
                },
            )
            .unwrap(),
        ));
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        let stream = journal
            .prepare_hsp_stream(&HspStreamRecord {
                boot: DeviceSessionId::new("boot").unwrap(),
                actor_session: SessionId::new("actor").unwrap(),
                request_id: RequestId::new("request").unwrap(),
                registration: registration(),
                device: native_identity(&device()),
                route_sha256: HandyRouteIdentity::from_snapshot(
                    &"f".repeat(64),
                    "test-connection-secret",
                )
                .unwrap()
                .route_sha256()
                .into(),
                route_namespace_sha256: HandyRouteIdentity::from_snapshot(
                    &"f".repeat(64),
                    "test-connection-secret",
                )
                .unwrap()
                .namespace_sha256()
                .into(),
                binding: HspBindingRecord::capture(&binding()),
                qualification_sha256: "d".repeat(64),
                profile_sha256: "e".repeat(64),
            })
            .unwrap();
        let journal = Arc::new(Mutex::new(journal));
        let transport = Self::transport(&peer, 7);
        let owner = transport.owner();
        let tick = Arc::new(AtomicU64::new(100));
        let mut dispatcher = HandyHspDispatcher::new(runtime.clone(), journal.clone(), transport);
        let clock = tick.clone();
        dispatcher.clock = Arc::new(move || Ok(clock.load(Ordering::Acquire)));
        Self {
            dir,
            peer,
            runtime,
            journal,
            stream,
            dispatcher,
            owner,
            tick,
        }
    }
    fn transport(peer: &Peer, mode: u64) -> HandyHspTransport {
        let mut transport = HandyHspTransport::from_snapshot(
            HspTransportBinding {
                binding: binding(),
                registration: registration(),
                device: device(),
                mode: HandyDeviceMode {
                    mode: 4,
                    mode_session_id: mode,
                },
                setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
            },
            Arc::new(Credentials {
                api: "test-api-secret".into(),
                connection: "test-connection-secret".into(),
            }),
            true,
            &"f".repeat(64),
        )
        .unwrap();
        transport.set_test_origin(peer.origin.clone());
        transport
    }
    fn rebind_after_setup(&mut self) {
        assert!(self.dispatcher.active.is_none());
        // This is an explicitly supplied native binding, separately verified by
        // the next actual mode/state preflight. Public admission must acquire it.
        let transport = Self::transport(&self.peer, 8);
        self.owner = transport.owner();
        self.dispatcher =
            HandyHspDispatcher::new(self.runtime.clone(), self.journal.clone(), transport);
        let clock = self.tick.clone();
        self.dispatcher.clock = Arc::new(move || Ok(clock.load(Ordering::Acquire)));
    }
    fn next(&self) -> HspEffect {
        let now = self.tick.load(Ordering::Acquire);
        self.runtime
            .lock()
            .unwrap()
            .next_effect(now, Some(&facts(now, 1_000_000_000)))
            .unwrap()
            .unwrap()
    }
    fn request(&self, effect: HspEffect) -> HspDispatchRequest {
        let receipt = self
            .journal
            .lock()
            .unwrap()
            .prepare_hsp_effect(&self.stream, &effect)
            .unwrap();
        HspDispatchRequest { effect, receipt }
    }
    fn start(&mut self, request: HspDispatchRequest) {
        let owner = self.owner.clone();
        assert!(self
            .dispatcher
            .start(
                request,
                Instant::now() + Duration::from_secs(2),
                move |_, _, gate| {
                    gate.apply(&owner, |now| {
                        Ok((now < 1_000_000_000).then(|| facts(now, 1_000_000_000)))
                    })
                }
            )
            .is_ok());
    }
    fn wait(&mut self) -> HspDispatchCompletion {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(completion) = self.dispatcher.poll() {
                return completion;
            }
            assert!(Instant::now() < deadline, "bounded worker did not settle");
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn finish(&mut self) -> HspCompletionRecord {
        let completion = self.wait();
        let now = self.tick.load(Ordering::Acquire);
        let current = (now < 1_000_000_000).then(|| facts(now, 1_000_000_000));
        let record = completion
            .observe(&mut self.runtime.lock().unwrap(), now, current.as_ref())
            .unwrap();
        completion
            .persist(&mut self.journal.lock().unwrap(), &record)
            .unwrap();
        record
    }
    fn setup(&mut self) {
        let request = self.request(self.next());
        self.start(request);
        assert!(matches!(
            self.finish().outcome,
            HspObservedOutcome::Acknowledged(_)
        ));
        self.rebind_after_setup();
    }
    fn offer(&self, times: &[u32], sequence: u64) {
        let now = self.tick.load(Ordering::Acquire);
        let current = facts(now, 1_000_000_000);
        let mut runtime = self.runtime.lock().unwrap();
        for (index, time) in times.iter().enumerate() {
            runtime
                .offer(
                    &binding().identity().live,
                    HspSample {
                        point: HspPoint::new(*time, (index * 100) as u8).unwrap(),
                        source_sequence: sequence + index as u64,
                        captured_at_ns: now,
                        accepted_at_ns: now,
                        continuous_from_previous: true,
                    },
                    now,
                    &current,
                )
                .unwrap();
        }
    }
}

#[test]
fn native_ordered_setup_add_play_add_stop_and_changed_mode_requires_rebinding() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    rig.start(request);
    rig.finish();
    rig.offer(&[0, 100], 1);
    let append = rig.next();
    let request = rig.request(append.clone());
    rig.start(request);
    let refused = rig.wait();
    assert!(matches!(
        refused.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::RemoteMismatch)
    ));
    assert_eq!(rig.peer.puts(), 1);
    rig.rebind_after_setup();
    let request = rig.request(append);
    rig.start(request);
    rig.finish();
    rig.runtime
        .lock()
        .unwrap()
        .request_play(100, &facts(100, 1_000_000_000))
        .unwrap();
    let request = rig.request(rig.next());
    rig.start(request);
    rig.finish();
    rig.offer(&[200, 300], 3);
    let request = rig.request(rig.next());
    rig.start(request);
    rig.finish();
    rig.owner.retire_ordinary();
    rig.runtime.lock().unwrap().stop();
    rig.tick.store(2_000_000_000, Ordering::Release);
    let stop = rig
        .runtime
        .lock()
        .unwrap()
        .next_effect(2_000_000_000, None)
        .unwrap()
        .unwrap();
    let request = rig.request(stop);
    rig.start(request);
    rig.finish();
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopAcknowledged
    );
    let peer = rig.peer.state.lock().unwrap();
    let paths: Vec<_> = peer
        .requests
        .iter()
        .filter(|r| r.0 == "PUT")
        .map(|r| r.1.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "/hsp/setup",
            "/hsp/add",
            "/hsp/play",
            "/hsp/add",
            "/hsp/stop"
        ]
    );
    assert_eq!(
        peer.requests
            .iter()
            .filter(|r| r.1 == "/hsp/add")
            .map(
                |r| serde_json::from_slice::<serde_json::Value>(&r.2).unwrap()
                    ["tail_point_stream_index"]
                    .as_u64()
                    .unwrap()
            )
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
}

#[test]
fn native_duplicate_claim_cannot_resend_original_bytes() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    let effect = rig.next();
    let original = effect.request().body().unwrap().to_vec();
    let request = rig.request(effect.clone());
    let duplicate = HspDispatchRequest {
        effect: effect.clone(),
        receipt: request.receipt.clone(),
    };
    rig.start(request);
    let completed = rig.wait();
    assert!(completed.claim.is_some());
    let owner = rig.owner.clone();
    let failure = rig
        .dispatcher
        .start(
            duplicate,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| gate.apply(&owner, |now| Ok(Some(facts(now, 1_000_000_000)))),
        )
        .expect_err("native consumed reservation refuses duplicate");
    assert_eq!(
        failure.reason,
        HspDispatchFailure::Authority(HspRuntimeError::NativeDispatchBusy)
    );
    assert_eq!(rig.peer.puts(), 2);
    assert_eq!(
        failure.request.effect.request().body(),
        Some(original.as_slice())
    );
    // Even a forged rejection must not release the original settled claim.
    let refused = HspDispatchCompletion {
        request: failure.request,
        claim: None,
        attempt: HspDispatchAttempt::Rejected(HspDispatchFailure::Journal),
    };
    assert_eq!(
        refused.abandon_unclaimed(
            &mut rig.runtime.lock().unwrap(),
            &mut rig.journal.lock().unwrap(),
        ),
        Err(HspDispatchFailure::Authority(
            HspRuntimeError::NativeDispatchBusy
        ))
    );
    assert_eq!(rig.runtime.lock().unwrap().in_flight(), Some(&effect));
    assert_eq!(
        rig.journal
            .lock()
            .unwrap()
            .hsp_effect(refused.request.receipt.id())
            .unwrap()
            .state,
        HspJournalState::Claimed
    );
    let record = completed
        .observe(
            &mut rig.runtime.lock().unwrap(),
            100,
            Some(&facts(100, 1_000_000_000)),
        )
        .unwrap();
    completed
        .persist(&mut rig.journal.lock().unwrap(), &record)
        .unwrap();
    assert!(matches!(
        record.outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_held_append_cancel_and_stop_wait_for_join_then_keep_unknown() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    let request = rig.request(rig.next());
    let duplicate = HspDispatchRequest {
        effect: request.effect.clone(),
        receipt: request.receipt.clone(),
    };
    rig.peer.hold.store(true, Ordering::Release);
    rig.start(request);
    let deadline = Instant::now() + Duration::from_secs(2);
    while rig.peer.puts() < 2 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    rig.runtime.lock().unwrap().stop();
    rig.dispatcher.cancel();
    assert!(rig.dispatcher.poll().is_none());
    assert!(rig
        .runtime
        .lock()
        .unwrap()
        .next_effect(100, None)
        .unwrap()
        .is_none());
    assert!(matches!(
        rig.dispatcher
            .start(
                duplicate,
                Instant::now() + Duration::from_secs(1),
                |_, _, _| panic!("busy callback")
            )
            .err()
            .unwrap()
            .reason,
        HspDispatchFailure::Busy
    ));
    rig.peer.hold.store(false, Ordering::Release);
    let record = rig.finish();
    assert_eq!(record.outcome, HspObservedOutcome::Unknown);
    let stop = rig
        .runtime
        .lock()
        .unwrap()
        .next_effect(100, None)
        .unwrap()
        .unwrap();
    let request = rig.request(stop);
    rig.start(request);
    rig.finish();
    assert_eq!(rig.peer.puts(), 3);
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopAcknowledged
    );
}

#[test]
fn native_malformed_redirect_http_error_echo_and_oversized_are_unknown_without_retry() {
    for reply in [
        Reply::Malformed,
        Reply::Redirect,
        Reply::Error,
        Reply::Echo,
        Reply::Oversized,
    ] {
        let mut rig = Rig::new();
        rig.setup();
        rig.offer(&[0, 100], 1);
        rig.peer.state.lock().unwrap().reply = reply;
        let request = rig.request(rig.next());
        rig.start(request);
        let completion = rig.wait();
        assert!(matches!(
            completion.attempt,
            HspDispatchAttempt::Observed { response: None }
        ));
        assert!(completion.claim.is_some());
        assert_eq!(rig.peer.puts(), 2);
        assert!(!format!("{:?}", completion.attempt).contains("test-api-secret"));
    }
}

#[test]
fn native_original_expiry_and_source_revocation_refuse_claim_despite_fresh_facts() {
    for revoke in [false, true] {
        let mut rig = Rig::new();
        let effect = rig
            .runtime
            .lock()
            .unwrap()
            .next_effect(100, Some(&facts(100, 200)))
            .unwrap()
            .unwrap();
        let request = rig.request(effect);
        let owner = rig.owner.clone();
        let tick = rig.tick.clone();
        assert!(rig
            .dispatcher
            .start(
                request,
                Instant::now() + Duration::from_secs(2),
                move |_, _, gate| {
                    if revoke {
                        owner.retire_ordinary();
                    } else {
                        tick.store(200, Ordering::Release);
                    }
                    gate.apply(&owner, |now| {
                        Ok((now < 1_000_000_000).then(|| facts(now, 1_000_000_000)))
                    })
                }
            )
            .is_ok());
        let completion = rig.wait();
        assert!(matches!(
            completion.attempt,
            HspDispatchAttempt::Rejected(_)
        ));
        assert!(completion.claim.is_none());
        assert_eq!(rig.peer.puts(), 0);
    }
}

#[test]
fn native_failed_durable_claim_does_not_put_and_post_io_metadata_retry_does_not_resend() {
    let mut rig = Rig::new();
    let writer =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    let request = rig.request(rig.next());
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    rig.start(request);
    let refused = rig.wait();
    assert!(matches!(
        refused.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::Journal)
    ));
    assert_eq!(rig.peer.puts(), 0);
    writer.execute_batch("ROLLBACK").unwrap();
    rig.start(refused.request);
    let completion = rig.wait();
    let record = completion
        .observe(
            &mut rig.runtime.lock().unwrap(),
            100,
            Some(&facts(100, 1_000_000_000)),
        )
        .unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        completion.persist(&mut rig.journal.lock().unwrap(), &record),
        Err(HspDispatchFailure::Journal)
    );
    writer.execute_batch("ROLLBACK").unwrap();
    completion
        .persist(&mut rig.journal.lock().unwrap(), &record)
        .unwrap();
    completion
        .persist(&mut rig.journal.lock().unwrap(), &record)
        .unwrap();
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn native_stop_refuses_known_replacement_stream_session_and_mode() {
    for changed in 0..3 {
        let mut rig = Rig::new();
        rig.setup();
        rig.runtime.lock().unwrap().stop();
        let stop = rig
            .runtime
            .lock()
            .unwrap()
            .next_effect(100, None)
            .unwrap()
            .unwrap();
        let request = rig.request(stop);
        {
            let mut state = rig.peer.state.lock().unwrap();
            match changed {
                0 => state.stream = 99,
                1 => state.session = "replacement-session".into(),
                _ => state.mode = 99,
            }
        }
        rig.start(request);
        let completion = rig.wait();
        assert!(matches!(
            completion.attempt,
            HspDispatchAttempt::Rejected(HspDispatchFailure::RemoteMismatch)
        ));
        assert!(completion.claim.is_none());
        assert_eq!(rig.peer.puts(), 1);
    }
}

#[test]
fn native_panic_after_claim_preserves_exact_claim_unknown_and_no_put() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let original = request.effect.clone();
    let owner = rig.owner.clone();
    assert!(rig
        .dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let _handoff = gate.apply(&owner, |now| {
                    Ok((now < 1_000_000_000).then(|| facts(now, 1_000_000_000)))
                })?;
                panic!("synthetic claimed worker panic");
            }
        )
        .is_ok());
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert!(completion.claim.is_some());
    assert_eq!(completion.request.effect, original);
    assert_eq!(rig.peer.puts(), 0);
    let record = completion
        .observe(
            &mut rig.runtime.lock().unwrap(),
            100,
            Some(&facts(100, 1_000_000_000)),
        )
        .unwrap();
    completion
        .persist(&mut rig.journal.lock().unwrap(), &record)
        .unwrap();
}

#[test]
fn native_control_consent_is_separate_from_read_inspection_consent() {
    let peer = Peer::new();
    let binding = HspTransportBinding {
        binding: binding(),
        registration: registration(),
        device: device(),
        mode: HandyDeviceMode {
            mode: 4,
            mode_session_id: 7,
        },
        setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
    };
    let result = HandyHspTransport::from_environment(
        binding,
        &HandyInspectionRequest {
            api_key_env: "ABSENT_HSP_API".into(),
            connection_key_env: "ABSENT_HSP_CONNECTION".into(),
            allow_online_inspection: true,
        },
        false,
        &"f".repeat(64),
    );
    assert!(matches!(result, Err(HspDispatchFailure::ConsentRequired)));
    assert_eq!(peer.puts(), 0);
}

#[test]
fn native_suspend_clock_is_checked_before_put_and_after_held_reply() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let owner = rig.owner.clone();
    let tick = rig.tick.clone();
    assert!(rig
        .dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let handoff = gate.apply(&owner, |now| Ok(Some(facts(now, 1_000_000_000))))?;
                tick.store(1_000_000_000, Ordering::Release);
                Ok(handoff)
            }
        )
        .is_ok());
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert!(completion.claim.is_some());
    assert_eq!(rig.peer.puts(), 0);

    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    rig.peer.hold.store(true, Ordering::Release);
    let request = rig.request(rig.next());
    rig.start(request);
    let deadline = Instant::now() + Duration::from_secs(2);
    while rig.peer.puts() < 2 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    rig.tick.store(1_000_000_000, Ordering::Release);
    rig.peer.hold.store(false, Ordering::Release);
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert!(completion.claim.is_some());
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_shutdown_joins_a_held_reply_and_returns_claimed_unknown() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    rig.peer.hold.store(true, Ordering::Release);
    let request = rig.request(rig.next());
    rig.start(request);
    let deadline = Instant::now() + Duration::from_secs(2);
    while rig.peer.puts() < 2 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    let held = rig.peer.hold.clone();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        held.store(false, Ordering::Release);
    });
    let began = Instant::now();
    let HspJoinedCompletion::Durable(completion) = rig.dispatcher.shutdown().unwrap() else {
        panic!("expected durable completion");
    };
    assert!(began.elapsed() >= Duration::from_millis(50));
    release.join().unwrap();
    assert!(rig.dispatcher.active.is_none());
    assert!(completion.claim.is_some());
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_second_route_wrong_credentials_namespace_device_and_registration_cannot_use_receipt() {
    for changed in 0..4 {
        let mut rig = Rig::new();
        let second = Peer::new();
        let mut native_device = device();
        let mut native_registration = registration();
        if changed == 2 {
            native_device.session_id = "second-device-session".into();
            second.state.lock().unwrap().session = native_device.session_id.clone();
        }
        if changed == 3 {
            native_registration.device_id = DeviceId::new("second-registration").unwrap();
        }
        let mut transport = HandyHspTransport::from_snapshot(
            HspTransportBinding {
                binding: binding(),
                registration: native_registration,
                device: native_device,
                mode: HandyDeviceMode {
                    mode: 4,
                    mode_session_id: 7,
                },
                setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
            },
            Arc::new(Credentials {
                api: "test-api-secret".into(),
                connection: if changed == 0 {
                    "second-connection-secret"
                } else {
                    "test-connection-secret"
                }
                .into(),
            }),
            true,
            &if changed == 1 { "e" } else { "f" }.repeat(64),
        )
        .unwrap();
        transport.set_test_origin(second.origin.clone());
        rig.owner = transport.owner();
        rig.dispatcher =
            HandyHspDispatcher::new(rig.runtime.clone(), rig.journal.clone(), transport);
        let tick = rig.tick.clone();
        rig.dispatcher.clock = Arc::new(move || Ok(tick.load(Ordering::Acquire)));
        let request = rig.request(rig.next());
        rig.start(request);
        let refused = rig.wait();
        assert!(matches!(
            refused.attempt,
            HspDispatchAttempt::Rejected(HspDispatchFailure::BindingMismatch)
        ));
        assert!(refused.claim.is_none());
        assert_eq!(rig.peer.puts(), 0);
        assert_eq!(second.puts(), 0);
        assert_eq!(second.state.lock().unwrap().requests.len(), 3);
    }
}

#[test]
fn native_api_key_rotation_keeps_same_keyed_route_and_can_claim() {
    let mut rig = Rig::new();
    let mut transport = HandyHspTransport::from_snapshot(
        HspTransportBinding {
            binding: binding(),
            registration: registration(),
            device: device(),
            mode: HandyDeviceMode {
                mode: 4,
                mode_session_id: 7,
            },
            setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
        },
        Arc::new(Credentials {
            api: "rotated-api-secret".into(),
            connection: "test-connection-secret".into(),
        }),
        true,
        &"f".repeat(64),
    )
    .unwrap();
    assert_eq!(
        transport.route.route_sha256(),
        rig.dispatcher.transport.route.route_sha256()
    );
    assert_eq!(
        transport.route.namespace_sha256(),
        rig.dispatcher.transport.route.namespace_sha256()
    );
    transport.set_test_origin(rig.peer.origin.clone());
    rig.owner = transport.owner();
    rig.dispatcher = HandyHspDispatcher::new(rig.runtime.clone(), rig.journal.clone(), transport);
    let tick = rig.tick.clone();
    rig.dispatcher.clock = Arc::new(move || Ok(tick.load(Ordering::Acquire)));
    let request = rig.request(rig.next());
    rig.start(request);
    assert!(matches!(
        rig.finish().outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn native_handoff_from_another_worker_cannot_authorize_a_put() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let duplicate = HspDispatchRequest {
        effect: request.effect.clone(),
        receipt: request.receipt.clone(),
    };
    let retained = Arc::new(Mutex::new(None));
    let saved = retained.clone();
    let owner = rig.owner.clone();
    assert!(rig
        .dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                *saved.lock().unwrap() =
                    Some(gate.apply(&owner, |now| Ok(Some(facts(now, 1_000_000_000))))?);
                Err(HspDispatchFailure::Cancelled)
            }
        )
        .is_ok());
    let first = rig.wait();
    assert!(first.claim.is_some());
    assert!(matches!(
        first.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    let failure = rig
        .dispatcher
        .start(
            duplicate,
            Instant::now() + Duration::from_secs(2),
            move |_, _, _| Ok(retained.lock().unwrap().take().unwrap()),
        )
        .expect_err("consumed native slot rejects reuse before callback");
    assert_eq!(
        failure.reason,
        HspDispatchFailure::Authority(HspRuntimeError::NativeDispatchBusy)
    );
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_unexpected_same_stream_motion_refuses_prefill_and_play() {
    for play in [false, true] {
        let mut rig = Rig::new();
        rig.setup();
        rig.offer(&[0, 100], 1);
        if play {
            let request = rig.request(rig.next());
            rig.start(request);
            rig.finish();
            rig.runtime
                .lock()
                .unwrap()
                .request_play(100, &facts(100, 1_000_000_000))
                .unwrap();
        }
        let request = rig.request(rig.next());
        let before = rig.peer.puts();
        rig.peer.state.lock().unwrap().playing = true;
        rig.start(request);
        let completion = rig.wait();
        assert!(matches!(
            completion.attempt,
            HspDispatchAttempt::Rejected(HspDispatchFailure::RemoteMismatch)
        ));
        assert!(completion.claim.is_none());
        assert_eq!(rig.peer.puts(), before);
    }
}

#[test]
fn native_abandon_definitely_unclaimed_append_unblocks_retained_stop_without_resend() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    let request = rig.request(rig.next());
    rig.start(request);
    rig.finish();
    rig.runtime
        .lock()
        .unwrap()
        .request_play(100, &facts(100, 1_000_000_000))
        .unwrap();
    let request = rig.request(rig.next());
    rig.start(request);
    rig.finish();
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::Playing
    );
    rig.offer(&[200, 300], 3);
    let append = rig.next();
    let request = rig.request(append.clone());
    let writer =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    rig.start(request);
    let rejected = rig.wait();
    assert!(matches!(
        rejected.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::Journal)
    ));
    assert!(rejected.claim.is_none());
    assert_eq!(rig.peer.puts(), 3);
    rig.runtime.lock().unwrap().stop();
    assert!(rig
        .runtime
        .lock()
        .unwrap()
        .next_effect(100, None)
        .unwrap()
        .is_none());
    assert_eq!(
        rejected.abandon_unclaimed(
            &mut rig.runtime.lock().unwrap(),
            &mut rig.journal.lock().unwrap()
        ),
        Err(HspDispatchFailure::Journal)
    );
    assert_eq!(rig.runtime.lock().unwrap().in_flight(), Some(&append));
    writer.execute_batch("ROLLBACK").unwrap();
    rejected
        .abandon_unclaimed(
            &mut rig.runtime.lock().unwrap(),
            &mut rig.journal.lock().unwrap(),
        )
        .unwrap();
    assert!(rig.runtime.lock().unwrap().in_flight().is_none());
    assert!(rig.runtime.lock().unwrap().uncertain_effect().is_none());
    assert_eq!(
        rig.journal
            .lock()
            .unwrap()
            .hsp_effect(rejected.request.receipt.id())
            .unwrap()
            .state,
        HspJournalState::Cancelled
    );
    let stop = rig
        .runtime
        .lock()
        .unwrap()
        .next_effect(100, None)
        .unwrap()
        .unwrap();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    let request = rig.request(stop);
    rig.start(request);
    rig.finish();
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopAcknowledged
    );
    let peer = rig.peer.state.lock().unwrap();
    assert_eq!(
        peer.requests
            .iter()
            .filter(|r| r.0 == "PUT")
            .map(|r| r.1.as_str())
            .collect::<Vec<_>>(),
        ["/hsp/setup", "/hsp/add", "/hsp/play", "/hsp/stop"]
    );
}

#[test]
fn native_foreign_receipt_and_foreign_runtime_cannot_abandon_other_prepared_work() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    let effect = rig.next();
    let original = rig.request(effect.clone());
    let other_binding = HspBinding::new(
        HspStreamIdentity {
            generation: 2,
            stream_id: 43,
            live: binding().identity().live.clone(),
        },
        ClientId::new("other-owner").unwrap(),
        DeviceSessionId::new("other-session").unwrap(),
        ArtifactId::new("clock").unwrap(),
    );
    let mut other_runtime =
        HandyHspRuntime::new(other_binding.clone(), rig.runtime.lock().unwrap().policy()).unwrap();
    let other_facts = HspDispatchFacts::new(
        other_binding.clone(),
        HspPlayReference::new(
            100,
            HspPhaseWindow {
                earliest_ms: -100,
                latest_ms: -100,
            },
            -100,
            10_000,
        )
        .unwrap(),
        HspAuthorityWindow::new(1_000_000_000, 1_000_000_000, 1_000_000_000),
    )
    .unwrap();
    let other_effect = other_runtime
        .next_effect(100, Some(&other_facts))
        .unwrap()
        .unwrap();
    let mut record = rig
        .journal
        .lock()
        .unwrap()
        .hsp_stream(rig.stream.id())
        .unwrap()
        .record;
    record.request_id = RequestId::new("other-request").unwrap();
    record.registration.device_id = DeviceId::new("other-device").unwrap();
    record.registration.binding_sha256 = "9".repeat(64);
    record.device.session_id = "other-native-session".into();
    record.binding = HspBindingRecord::capture(&other_binding);
    record.route_sha256 = HandyRouteIdentity::from_snapshot(&"f".repeat(64), "other-route")
        .unwrap()
        .route_sha256()
        .into();
    let other_stream = rig
        .journal
        .lock()
        .unwrap()
        .prepare_hsp_stream(&record)
        .unwrap();
    let other_receipt = rig
        .journal
        .lock()
        .unwrap()
        .prepare_hsp_effect(&other_stream, &other_effect)
        .unwrap();
    let forged = HspDispatchCompletion {
        request: HspDispatchRequest {
            effect: effect.clone(),
            receipt: other_receipt.clone(),
        },
        claim: None,
        attempt: HspDispatchAttempt::Rejected(HspDispatchFailure::BindingMismatch),
    };
    assert_eq!(
        forged.abandon_unclaimed(
            &mut rig.runtime.lock().unwrap(),
            &mut rig.journal.lock().unwrap()
        ),
        Err(HspDispatchFailure::Journal)
    );
    assert_eq!(rig.runtime.lock().unwrap().in_flight(), Some(&effect));
    let foreign = HspDispatchCompletion {
        request: HspDispatchRequest {
            effect: other_effect.clone(),
            receipt: other_receipt.clone(),
        },
        claim: None,
        attempt: HspDispatchAttempt::Rejected(HspDispatchFailure::BindingMismatch),
    };
    assert_eq!(
        foreign.abandon_unclaimed(
            &mut rig.runtime.lock().unwrap(),
            &mut rig.journal.lock().unwrap()
        ),
        Err(HspDispatchFailure::Authority(
            HspRuntimeError::ForeignEffect
        ))
    );
    assert_eq!(other_runtime.in_flight(), Some(&other_effect));
    for receipt in [original.receipt, other_receipt] {
        assert_eq!(
            rig.journal
                .lock()
                .unwrap()
                .hsp_effect(receipt.id())
                .unwrap()
                .state,
            HspJournalState::Prepared
        );
    }
    assert_eq!(rig.peer.puts(), 1);
}

include!("handy_hsp_emergency_tests.rs");

include!("handy_hsp_native_lifecycle_tests.rs");

include!("handy_hsp_dispatch_preclaim_tests.rs");
