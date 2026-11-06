// Engine-owned localhost lifecycle tests. Every source, qualification digest,
// device identity and phase observation is authored test data. No trained model,
// real device, physical Stop, or useful synchronization is qualified here.
#[cfg(target_os = "linux")]
mod hsp_engine_execution_tests {
    use super::*;
    use crate::{
        device_effect_journal::{HspJournalState, HspStreamJournalState},
        handy_credentials::{Credentials, HandyRouteIdentity},
        handy_hsp_dispatch::{
            HandyHspTransport, HspClaimGate, HspClaimedHandoff, HspDispatchFailure, HspNativeOwner,
            HspSetupPreviousState, HspTransportBinding,
        },
        handy_hsp_execution::{
            HspAdmission, HspAuthoritySnapshot, HspExecutionError, HspExecutionHandle,
            HspNativeAuthority, HspSessionHandle, HspSessionSnapshot,
        },
        handy_hsp_records::{HspBindingRecord, HspEffectRecordKind, HspStreamRecord},
        handy_hsp_runtime::{
            HandyHspRuntime, HspAuthorityWindow, HspBinding, HspDispatchFacts, HspPlayReference,
        },
        handy_hsp_wire::HspPlayState,
        handy_wire::DeviceIdentity,
    };
    use pulsar_core::{
        HspOfferOutcome, HspPhaseWindow, HspPoint, HspPolicy, HspSample, HspStreamIdentity,
    };
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{atomic::AtomicU64, Condvar, Weak},
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    const WAIT: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct Hold {
        path: Option<String>,
        reached: bool,
        release: bool,
        timed_out: bool,
    }
    struct PeerState {
        stream: u32,
        mode: u64,
        tail: i64,
        first_time: u64,
        last_time: u64,
        playing: bool,
        replace_after_setup: bool,
        native_session: String,
        requests: Vec<(String, String, Vec<u8>)>,
    }
    struct Peer {
        origin: String,
        state: Arc<Mutex<PeerState>>,
        hold: Arc<(Mutex<Hold>, Condvar)>,
        stop: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }
    impl Peer {
        fn new(replace_after_setup: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let state = Arc::new(Mutex::new(PeerState {
                stream: 9,
                mode: 7,
                tail: -1,
                first_time: 0,
                last_time: 0,
                playing: false,
                replace_after_setup,
                native_session: "engine-native-session".into(),
                requests: Vec::new(),
            }));
            let hold = Arc::new((Mutex::new(Hold::default()), Condvar::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let captured = state.clone();
            let held = hold.clone();
            let stopped = stop.clone();
            let worker = thread::Builder::new()
                .name("pulsar-hsp-engine-test-peer".into())
                .spawn(move || {
                    while !stopped.load(Ordering::Acquire) {
                        let (mut socket, _) = match listener.accept() {
                            Ok(socket) => socket,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::park_timeout(Duration::from_millis(2));
                                continue;
                            }
                            Err(e) => panic!("local HSP fixture accept: {e}"),
                        };
                        socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                        socket.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                        let Some((method, path, body)) = read_request(&mut socket) else {
                            continue;
                        };
                        let bytes = {
                            let mut s = captured.lock().unwrap();
                            assert!(s.requests.len() < 128, "unbounded native request loop");
                            s.requests.push((method.clone(), path.clone(), body.clone()));
                            if method == "PUT" {
                                match path.as_str() {
                                    "/hsp/setup" => {
                                        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                                        s.stream = body["stream_id"]
                                            .as_u64()
                                            .filter(|value| *value != 0)
                                            .and_then(|value| u32::try_from(value).ok())
                                            .expect("Setup must name the actual bounded stream");
                                        s.mode = 8;
                                        s.tail = -1;
                                    }
                                    "/hsp/add" => {
                                        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                                        let points = body["points"].as_array().unwrap();
                                        let tail = body["tail_point_stream_index"].as_i64().unwrap();
                                        assert_eq!(tail, s.tail + points.len() as i64, "cumulative tail");
                                        if s.tail < 0 {
                                            s.first_time = points[0]["t"].as_u64().unwrap();
                                        }
                                        s.last_time = points.last().unwrap()["t"].as_u64().unwrap();
                                        s.tail = tail;
                                    }
                                    "/hsp/play" => s.playing = true,
                                    "/hsp/stop" => s.playing = false,
                                    _ => panic!("unexpected fixture PUT {path}"),
                                }
                            }
                            let bytes = match path.as_str() {
                                "/info" => serde_json::to_vec(&serde_json::json!({"result": {
                                    "fw_version":"4.0.16","fw_status":0,"hw_model_no":2,
                                    "hw_model_name":"H02","hw_model_variant":0,"session_id":s.native_session
                                }})).unwrap(),
                                "/mode" => serde_json::to_vec(&serde_json::json!({"result": {
                                    "mode":4,"mode_session_id":s.mode
                                }})).unwrap(),
                                _ => serde_json::to_vec(&serde_json::json!({"result": {
                                    "play_state":if s.playing {1} else {2},
                                    "points":(s.tail+1).max(0),"max_points":32,
                                    "current_point":if s.tail < 0 {-1} else {0},"current_time":-10000,
                                    "loop":false,"playback_rate":1.0,
                                    "first_point_time":s.first_time,"last_point_time":s.last_time,
                                    "stream_id":s.stream,"tail_point_stream_index":s.tail,
                                    "tail_point_stream_index_threshold":1,"pause_on_starving":false
                                }})).unwrap(),
                            };
                            // Setup's captured acknowledgment is original; the separate
                            // following GET observes the replacement and must reject it.
                            if method == "PUT" && path == "/hsp/setup" && s.replace_after_setup {
                                s.stream = 99;
                                s.mode = 9;
                            }
                            bytes
                        };
                        if method == "PUT" {
                            let (mutex, ready) = &*held;
                            let mut h = mutex.lock().unwrap();
                            if h.path.as_deref() == Some(path.as_str()) {
                                h.reached = true;
                                ready.notify_all();
                                let (next, _) = ready.wait_timeout_while(h, WAIT, |h| {
                                    !h.release && !stopped.load(Ordering::Acquire)
                                }).unwrap();
                                h = next;
                                if !h.release && !stopped.load(Ordering::Acquire) {
                                    h.timed_out = true;
                                }
                            }
                        }
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        );
                        // A cancelled native request may already have closed its socket.
                        let _ = socket.write_all(header.as_bytes());
                        let _ = socket.write_all(&bytes);
                    }
                }).unwrap();
            Self {
                origin,
                state,
                hold,
                stop,
                worker: Some(worker),
            }
        }
        fn puts(&self) -> Vec<String> {
            self.state
                .lock()
                .unwrap()
                .requests
                .iter()
                .filter(|r| r.0 == "PUT")
                .map(|r| r.1.clone())
                .collect()
        }
        fn hold_next(&self, path: &str) {
            *self.hold.0.lock().unwrap() = Hold {
                path: Some(path.into()),
                ..Hold::default()
            };
        }
        fn wait_held(&self) {
            let (mutex, ready) = &*self.hold;
            let h = mutex.lock().unwrap();
            let (h, _) = ready.wait_timeout_while(h, WAIT, |h| !h.reached).unwrap();
            let (reached, timed_out) = (h.reached, h.timed_out);
            drop(h);
            assert!(
                reached,
                "fixture did not receive held PUT; requests={:?}",
                self.puts()
            );
            assert!(!timed_out, "fixture released a held PUT on its deadline");
        }
        fn release(&self) {
            self.hold
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .release = true;
            self.hold.1.notify_all();
        }
    }
    impl Drop for Peer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            self.release();
            if let Some(worker) = self.worker.take() {
                worker.thread().unpark();
                let joined = worker.join();
                if !thread::panicking() {
                    joined.unwrap();
                    assert!(
                        !self.hold.0.lock().unwrap().timed_out,
                        "held fixture reply timed out"
                    );
                }
            }
        }
    }
    fn read_request(socket: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            socket
                .set_read_timeout(Some(deadline.checked_duration_since(Instant::now())?))
                .ok()?;
            let mut byte = [0];
            if socket.read(&mut byte).ok()? == 0 {
                return None;
            }
            bytes.push(byte[0]);
            if bytes.len() > 16 * 1024 {
                return None;
            }
        }
        let headers = std::str::from_utf8(&bytes).ok()?;
        let mut first = headers.lines().next()?.split_whitespace();
        let method = first.next()?.to_owned();
        let path = first.next()?.to_owned();
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if length > 16 * 1024 {
            return None;
        }
        let mut body = vec![0; length];
        let mut read = 0;
        while read < length {
            socket
                .set_read_timeout(Some(deadline.checked_duration_since(Instant::now())?))
                .ok()?;
            let count = socket.read(&mut body[read..]).ok()?;
            if count == 0 {
                return None;
            }
            read += count;
        }
        Some((method, path, body))
    }

    #[derive(Default)]
    struct FactPauseState {
        armed: bool,
        entered_at_ns: Option<u64>,
        released: bool,
        timed_out: bool,
    }
    struct PreLockFactPause {
        runtime: Weak<Mutex<HandyHspRuntime>>,
        state: Mutex<FactPauseState>,
        changed: Condvar,
    }
    impl PreLockFactPause {
        fn new(runtime: &Arc<Mutex<HandyHspRuntime>>) -> Self {
            Self {
                runtime: Arc::downgrade(runtime),
                state: Mutex::new(FactPauseState {
                    armed: true,
                    ..FactPauseState::default()
                }),
                changed: Condvar::new(),
            }
        }
        fn pause_once(&self, at_ns: u64) -> Result<(), HspDispatchFailure> {
            if thread::current().name() != Some("pulsar-hsp-owner") {
                return Ok(());
            }
            let runtime = self
                .runtime
                .upgrade()
                .ok_or(HspDispatchFailure::InvalidClock)?;
            // Only pause a fact read made outside runtime custody. A provider
            // read under that mutex must never wait for a concurrent producer.
            let Ok(guard) = runtime.try_lock() else {
                return Ok(());
            };
            drop(guard);
            let mut state = self.state.lock().unwrap();
            if !state.armed {
                return Ok(());
            }
            state.armed = false;
            state.entered_at_ns = Some(at_ns);
            self.changed.notify_all();
            let (mut state, _) = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(2), |state| !state.released)
                .unwrap();
            if !state.released {
                state.timed_out = true;
                return Err(HspDispatchFailure::InvalidClock);
            }
            Ok(())
        }
        fn wait_entered(&self) -> u64 {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .changed
                .wait_timeout_while(state, WAIT, |state| state.entered_at_ns.is_none())
                .unwrap();
            assert!(
                !state.timed_out,
                "pre-lock fact hook released on its deadline"
            );
            state
                .entered_at_ns
                .expect("owner never entered a pre-lock fact read")
        }
        fn release(&self) {
            self.state.lock().unwrap().released = true;
            self.changed.notify_all();
        }
    }
    struct ReleaseFactPause(Arc<PreLockFactPause>);
    impl Drop for ReleaseFactPause {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    #[derive(Default)]
    struct ClaimPauseState {
        entered: bool,
        released: bool,
        timed_out: bool,
    }
    #[derive(Default)]
    struct ClaimPause {
        state: Mutex<ClaimPauseState>,
        changed: Condvar,
    }
    impl ClaimPause {
        fn enter(&self) {
            let mut state = self.state.lock().unwrap();
            state.entered = true;
            self.changed.notify_all();
            let (mut state, _) = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(2), |state| !state.released)
                .unwrap();
            state.timed_out = !state.released;
        }
        fn wait_entered(&self) {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .changed
                .wait_timeout_while(state, WAIT, |state| !state.entered)
                .unwrap();
            assert!(
                state.entered && !state.timed_out,
                "durable claim pause was not reached"
            );
        }
        fn release(&self) {
            self.state.lock().unwrap().released = true;
            self.changed.notify_all();
        }
    }
    struct ReleaseClaimPause(Arc<ClaimPause>);
    impl Drop for ReleaseClaimPause {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    struct SyntheticAuthority {
        binding: HspBinding,
        anchor: u64,
        lease_until: AtomicU64,
        finite_stop_budget: AtomicU64,
        retired: AtomicBool,
        pre_lock_pause: Mutex<Option<Arc<PreLockFactPause>>>,
        claim_pause: Mutex<Option<Arc<ClaimPause>>>,
        claim_observations: Mutex<Vec<(Duration, Option<HspDispatchFailure>)>>,
    }
    impl HspNativeAuthority for SyntheticAuthority {
        fn current_snapshot(&self, at: u64) -> Result<HspAuthoritySnapshot, HspDispatchFailure> {
            let facts = self.current_facts(at)?;
            let budget = self.finite_stop_budget.load(Ordering::Acquire);
            let projected = if budget == 0 {
                None
            } else {
                self.current_facts(
                    at.checked_add(budget)
                        .ok_or(HspDispatchFailure::InvalidClock)?,
                )?
            };
            Ok(HspAuthoritySnapshot { facts, projected })
        }
        fn current_facts(&self, now: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure> {
            let until = self.lease_until.load(Ordering::Acquire);
            if self.retired.load(Ordering::Acquire) || now >= until {
                return Ok(None);
            }
            let elapsed = now
                .checked_sub(self.anchor)
                .ok_or(HspDispatchFailure::InvalidClock)?
                / 1_000_000;
            let lead = if self.finite_stop_budget.load(Ordering::Acquire) == 0 {
                10_000
            } else {
                2_000
            };
            let phase =
                -lead + i64::try_from(elapsed).map_err(|_| HspDispatchFailure::InvalidClock)?;
            let phase_i32 = i32::try_from(phase).map_err(|_| HspDispatchFailure::InvalidClock)?;
            // An authored correlated reference, deliberately independent of HTTP
            // arrival and inference/sample acceptance. This is not real timing.
            let reference = HspPlayReference::new(
                now,
                HspPhaseWindow {
                    earliest_ms: phase,
                    latest_ms: phase,
                },
                phase_i32,
                10_000 + elapsed,
            )
            .map_err(HspDispatchFailure::Authority)?;
            let facts = HspDispatchFacts::new(
                self.binding.clone(),
                reference,
                HspAuthorityWindow::new(
                    until,
                    self.anchor + 60_000_000_000,
                    self.anchor + 60_000_000_000,
                ),
            )
            .map_err(HspDispatchFailure::Authority)?;
            let hook = self.pre_lock_pause.lock().unwrap().clone();
            if let Some(hook) = hook {
                // Return the exact earlier observation after the pause. The
                // fixture deliberately does not repair or reanchor old facts.
                hook.pause_once(now)?;
            }
            Ok(Some(facts))
        }
        fn authorize(
            &self,
            deadline: Instant,
            cancel: &AtomicBool,
            gate: HspClaimGate<'_>,
            owner: &HspNativeOwner,
        ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(HspDispatchFailure::Cancelled);
            }
            let pause = self.claim_pause.lock().unwrap().take();
            let gate = match pause {
                Some(pause) => gate.with_after_claim_hook(Box::new(move || pause.enter())),
                None => gate,
            };
            let started = Instant::now();
            let result = gate.apply(owner, |now| self.current_facts(now));
            let mut observations = self.claim_observations.lock().unwrap();
            if observations.len() < 16 {
                observations.push((started.elapsed(), result.as_ref().err().cloned()));
            }
            result
        }
    }
    struct Rig {
        // Engine drops before the HTTP fixture so its Stop still has a peer.
        f: Fixture,
        peer: Peer,
        execution: HspExecutionHandle,
        session: HspSessionHandle,
        authority: Arc<SyntheticAuthority>,
        record: HspStreamRecord,
        binding: HspBinding,
    }
    impl Rig {
        fn new(replace_after_setup: bool) -> Self {
            Self::with_claim_pause(replace_after_setup, None)
        }
        fn with_claim_pause(
            replace_after_setup: bool,
            claim_pause: Option<Arc<ClaimPause>>,
        ) -> Self {
            Self::with_finite_budget(replace_after_setup, claim_pause, 0)
        }
        fn with_finite_budget(
            replace_after_setup: bool,
            claim_pause: Option<Arc<ClaimPause>>,
            finite_stop_budget: u64,
        ) -> Self {
            Self::with_authority_wrapper(
                replace_after_setup,
                claim_pause,
                finite_stop_budget,
                |authority| authority,
            )
        }
        fn with_authority_wrapper(
            replace_after_setup: bool,
            claim_pause: Option<Arc<ClaimPause>>,
            finite_stop_budget: u64,
            wrap: impl FnOnce(Arc<SyntheticAuthority>) -> Arc<dyn HspNativeAuthority>,
        ) -> Self {
            let f = fixture();
            let peer = Peer::new(replace_after_setup);
            let binding = HspBinding::new(
                HspStreamIdentity {
                    generation: 1,
                    stream_id: 42,
                    live: LiveIdentity {
                        epoch: AttemptId::new("engine-hsp-live").unwrap(),
                        project: f.project.clone(),
                        revision: RevisionId::new(1),
                        source: SourceVersionId::new("engine-hsp-source").unwrap(),
                    },
                },
                ClientId::new("engine-hsp-owner").unwrap(),
                DeviceSessionId::new("engine-hsp-device").unwrap(),
                ArtifactId::new("engine-hsp-clock").unwrap(),
            );
            let registration = HandyRegistration {
                device_id: DeviceId::new("engine-hsp-device").unwrap(),
                revision: 1,
                settings: HandyRegistrationSettings {
                    label: "authored localhost HSP engine fixture".into(),
                    api_key_env: "FIXTURE_API".into(),
                    connection_key_env: "FIXTURE_ROUTE".into(),
                },
                disabled: false,
                binding_sha256: "a".repeat(64),
            };
            let device = DeviceIdentity {
                firmware: "4.0.16".into(),
                model_number: 2,
                model_name: "H02".into(),
                model_variant: 0,
                session_id: "engine-native-session".into(),
                firmware_status: 0,
            };
            let route = HandyRouteIdentity::from_snapshot(
                &f.engine.handy_owner_token,
                "fixture-connection",
            )
            .unwrap();
            let record = HspStreamRecord {
                boot: DeviceSessionId::new(f.engine.engine_epoch.clone()).unwrap(),
                actor_session: f.session.clone(),
                request_id: RequestId::new("engine-hsp-original").unwrap(),
                registration: registration.clone(),
                device: HandyIdentity {
                    firmware: device.firmware.clone(),
                    model_number: device.model_number,
                    model_name: device.model_name.clone(),
                    model_variant: device.model_variant,
                    session_id: device.session_id.clone(),
                    firmware_status: device.firmware_status,
                },
                route_sha256: route.route_sha256().into(),
                route_namespace_sha256: route.namespace_sha256().into(),
                binding: HspBindingRecord::capture(&binding),
                qualification_sha256: "d".repeat(64),
                profile_sha256: "e".repeat(64),
            };
            let mut transport = HandyHspTransport::from_snapshot(
                HspTransportBinding {
                    binding: binding.clone(),
                    registration,
                    device,
                    mode: HandyDeviceMode {
                        mode: 4,
                        mode_session_id: 7,
                    },
                    setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
                },
                Arc::new(Credentials {
                    api: "fixture-api".into(),
                    connection: "fixture-connection".into(),
                }),
                true,
                &f.engine.handy_owner_token,
            )
            .unwrap();
            transport.set_test_origin(peer.origin.clone());
            let anchor = crate::handy_clock::boottime_ns().unwrap();
            let authority = Arc::new(SyntheticAuthority {
                binding: binding.clone(),
                anchor,
                lease_until: AtomicU64::new(anchor + 60_000_000_000),
                finite_stop_budget: AtomicU64::new(finite_stop_budget),
                retired: AtomicBool::new(false),
                pre_lock_pause: Mutex::new(None),
                claim_pause: Mutex::new(claim_pause),
                claim_observations: Mutex::new(Vec::new()),
            });
            let admission = HspAdmission::new(
                binding.clone(),
                policy(),
                transport,
                record.clone(),
                wrap(authority.clone()),
            )
            .unwrap();
            let execution = f.engine.ensure_handy_hsp_execution().unwrap();
            let session = execution.try_admit(admission).unwrap();
            Self {
                f,
                peer,
                execution,
                session,
                authority,
                record,
                binding,
            }
        }
        fn duplicate_admission(&self) -> HspAdmission {
            self.admission_with_authority(self.authority.clone())
        }
        fn admission_with_authority(&self, authority: Arc<dyn HspNativeAuthority>) -> HspAdmission {
            let device = &self.record.device;
            let mut transport = HandyHspTransport::from_snapshot(
                HspTransportBinding {
                    binding: self.binding.clone(),
                    registration: self.record.registration.clone(),
                    device: DeviceIdentity {
                        firmware: device.firmware.clone(),
                        model_number: device.model_number,
                        model_name: device.model_name.clone(),
                        model_variant: device.model_variant,
                        session_id: device.session_id.clone(),
                        firmware_status: device.firmware_status,
                    },
                    mode: HandyDeviceMode {
                        mode: 4,
                        mode_session_id: 7,
                    },
                    setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
                },
                Arc::new(Credentials {
                    api: "fixture-api".into(),
                    connection: "fixture-connection".into(),
                }),
                true,
                &self.f.engine.handy_owner_token,
            )
            .unwrap();
            transport.set_test_origin(self.peer.origin.clone());
            HspAdmission::new(
                self.binding.clone(),
                policy(),
                transport,
                self.record.clone(),
                authority,
            )
            .unwrap()
        }
        fn wait(
            &self,
            reason: &str,
            predicate: impl Fn(&HspSessionSnapshot) -> bool,
        ) -> HspSessionSnapshot {
            let deadline = Instant::now() + WAIT;
            let mut last = None;
            loop {
                match self.execution.snapshot_for_session(
                    &self.f.session,
                    &self.f.project,
                    &self.record.request_id,
                ) {
                    Ok(snapshot) => {
                        if predicate(&snapshot) {
                            return snapshot;
                        }
                        last = Some(snapshot);
                    }
                    Err(HspExecutionError::Busy) => {}
                    Err(error) => panic!("{reason}: snapshot failed: {error:?}"),
                }
                if Instant::now() >= deadline {
                    let observations = self.authority.claim_observations.try_lock();
                    let requests = self.peer.state.try_lock().ok().map(|state| {
                        state
                            .requests
                            .iter()
                            .map(|(method, path, _)| (method.clone(), path.clone()))
                            .collect::<Vec<_>>()
                    });
                    let metadata_busy = self.session.metadata_busy_for_test();
                    let journal_lock = match self.f.engine._device_effect_journal.try_lock() {
                        Ok(_) => "available",
                        Err(std::sync::TryLockError::WouldBlock) => "held",
                        Err(std::sync::TryLockError::Poisoned(_)) => "poisoned",
                    };
                    let pump_running = self
                        .f
                        .engine
                        .handy_hsp_execution
                        .try_lock()
                        .ok()
                        .map(|owner| owner.as_ref().is_some_and(|pump| pump.is_running()));
                    panic!("{reason}: last={last:?}; requests={requests:?}; claims={observations:?}; watchdog_busy={}; watchdog_progress={:?}; metadata_busy={metadata_busy:?}; journal_lock={journal_lock}; pump_running={pump_running:?}",
                        self.session.watchdog_busy_ticks_for_test(), self.session.watchdog_progress_for_test());
                }
                thread::park_timeout(Duration::from_millis(2));
            }
        }
        fn ready(&self) {
            self.wait("post-Setup probe must bind original mode generation", |s| {
                s.post_setup_bound && !s.native_active && !s.metadata_pending
            });
        }
        fn offer(&self, sequence: u64) {
            let deadline = Instant::now() + WAIT;
            loop {
                let now = crate::handy_clock::boottime_ns().unwrap();
                let sample = HspSample {
                    point: HspPoint::new(
                        (sequence as u32 - 1) * 100,
                        if sequence % 2 == 1 { 20 } else { 80 },
                    )
                    .unwrap(),
                    source_sequence: sequence,
                    captured_at_ns: now,
                    accepted_at_ns: now,
                    continuous_from_previous: true,
                };
                match self.session.offer(&self.binding.identity().live, sample) {
                    Ok(HspOfferOutcome::Accepted) => return,
                    Err(HspExecutionError::Busy) if Instant::now() < deadline => {
                        thread::park_timeout(Duration::from_millis(2))
                    }
                    result => panic!("sample {sequence} offer: {result:?}"),
                }
            }
        }
        fn status(&self) -> HandyHspStatus {
            match self
                .f
                .engine
                .handle(status_request(&self.f, &self.record.request_id))
                .result
                .unwrap()
            {
                ResponseBody::HandyHsp(status) => *status,
                other => panic!("unexpected HSP status response {other:?}"),
            }
        }
        fn stopped(&self) -> HspSessionSnapshot {
            self.wait("one original Stop must settle", |s| {
                s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::StopAcknowledged
                    && !s.native_active
            })
        }
        fn history(&self) -> Vec<(HspEffectRecordKind, HspJournalState)> {
            let journal = self.f.engine._device_effect_journal.lock().unwrap();
            let stream = journal.hsp_stream(&self.record.id().unwrap()).unwrap();
            assert_eq!(stream.state, HspStreamJournalState::Custody);
            stream
                .effect_ids
                .iter()
                .map(|id| {
                    let effect = journal.hsp_effect(id).unwrap();
                    (effect.record.kind, effect.state)
                })
                .collect()
        }
    }
    fn policy() -> HspPolicy {
        HspPolicy {
            max_buffered_points: 32,
            device_capacity_points: 32,
            max_chunk_points: 2,
            min_lead_ms: 10,
            max_future_ms: 20_000,
            max_sample_age_ns: 5_000_000_000,
            max_gap_ms: 200,
        }
    }
    fn status_request(f: &Fixture, id: &RequestId) -> Request {
        request(
            &f.session,
            Some(&f.project),
            None,
            Command::HandyHspStatus {
                stream_request_id: id.clone(),
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        )
    }

    #[test]
    fn engine_hsp_status_authorizes_before_lookup_and_never_starts_an_owner() {
        let f = fixture();
        let id = RequestId::new("absent-native-stream").unwrap();
        assert_eq!(
            f.engine
                .handle(status_request(&f, &id))
                .result
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert!(f.engine.handy_hsp_execution.lock().unwrap().is_none());
        let mut unauthenticated = status_request(&f, &id);
        unauthenticated.auth_token = None;
        assert_eq!(
            f.engine.handle(unauthenticated).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        let mut wrong_owner = status_request(&f, &id);
        if let Command::HandyHspStatus { owner_proof, .. } = &mut wrong_owner.command {
            *owner_proof = "0".repeat(64);
        }
        assert_eq!(
            f.engine.handle(wrong_owner).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let stranger = pair(&f.engine, "HSP observer without a project grant");
        let foreign = request(
            &stranger,
            Some(&f.project),
            None,
            Command::HandyHspStatus {
                stream_request_id: id,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        assert_eq!(
            f.engine.handle(foreign).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        assert!(f.engine.handy_hsp_execution.lock().unwrap().is_none());
        f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_owns_setup_probe_explicit_play_append_and_single_stop() {
        let rig = Rig::new(false);
        rig.ready();
        assert!(matches!(
            rig.execution.try_admit(rig.duplicate_admission()),
            Err(HspExecutionError::InvalidAdmission)
        ));
        rig.offer(1);
        rig.offer(2);
        rig.wait("prefill must settle without implicit Play", |s| {
            s.total_added == 2 && !s.native_active && !s.metadata_pending
        });
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add"]);
        let status = rig.status();
        assert!(!status.qualified && !status.emergency_history_durable);
        assert_eq!(
            status.native.unwrap().runtime_state,
            HandyHspRuntimeState::Prefilling
        );
        rig.session.request_play().unwrap();
        rig.wait("explicit Play must settle", |s| {
            s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::Playing
                && !s.native_active
                && !s.metadata_pending
        });
        assert!(
            rig.session.request_play().is_err(),
            "original Play is single use"
        );
        rig.offer(3);
        rig.wait("later source point must append", |s| {
            s.total_added == 3 && !s.native_active && !s.metadata_pending
        });
        rig.session.cancel();
        rig.stopped();
        rig.wait("original outcomes must persist", |s| !s.metadata_pending);
        assert_eq!(
            rig.peer.puts(),
            [
                "/hsp/setup",
                "/hsp/add",
                "/hsp/play",
                "/hsp/add",
                "/hsp/stop"
            ]
        );
        assert_original_stop_history(
            &rig,
            &[
                (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
                (HspEffectRecordKind::Append, HspJournalState::Acknowledged),
                (HspEffectRecordKind::Play, HspJournalState::Acknowledged),
                (HspEffectRecordKind::Append, HspJournalState::Acknowledged),
            ],
        );
        let stale = rig.execution.snapshot_for_session(
            &rig.f.session,
            &rig.f.project,
            &RequestId::new("not-original").unwrap(),
        );
        assert!(matches!(stale, Err(HspExecutionError::NotFound)));
        let stranger = pair(
            &rig.f.engine,
            "HSP reader with Read but foreign original session",
        );
        rig.f
            .engine
            .handle(request(
                &rig.f.session,
                None,
                None,
                Command::Grant {
                    session: stranger.clone(),
                    project_id: rig.f.project.clone(),
                    scopes: vec![Scope::Read],
                },
            ))
            .result
            .unwrap();
        let foreign = request(
            &stranger,
            Some(&rig.f.project),
            None,
            Command::HandyHspStatus {
                stream_request_id: rig.record.request_id.clone(),
                owner_proof: rig.f.engine.handy_owner_token.clone(),
            },
        );
        assert_eq!(
            rig.f.engine.handle(foreign).result.unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(matches!(
            rig.execution.snapshot(
                &ClientId::new("foreign-owner").unwrap(),
                &rig.f.project,
                &rig.record.request_id
            ),
            Err(HspExecutionError::NotFound)
        ));
        assert!(matches!(
            rig.execution.snapshot_for_session(
                &rig.f.session,
                &ProjectId::new("foreign-project").unwrap(),
                &rig.record.request_id
            ),
            Err(HspExecutionError::NotFound)
        ));
        rig.f.engine.finish_shutdown().unwrap();
        assert_eq!(
            rig.peer.puts().iter().filter(|p| *p == "/hsp/stop").count(),
            1
        );
        assert_eq!(
            Arc::strong_count(&rig.f.engine),
            1,
            "HSP workers must not retain Engine"
        );
    }

    fn assert_metadata_independent_stop(rig: &Rig) {
        rig.offer(1);
        rig.offer(2);
        let snapshot = rig.stopped();
        assert!(snapshot.retired && snapshot.metadata_pending && snapshot.emergency_observed);
        assert_eq!(
            rig.peer.puts(),
            ["/hsp/setup", "/hsp/stop"],
            "unprepared Append must never be sent"
        );
        assert!(!rig.status().emergency_history_durable);
        assert!(rig.session.request_play().is_err());
    }
    fn assert_recovered_history_stays_unsent(rig: &Rig) {
        let deadline = Instant::now() + WAIT;
        loop {
            if rig
                .history()
                .contains(&(HspEffectRecordKind::Append, HspJournalState::Cancelled))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "known-unsent metadata did not recover"
            );
            thread::park_timeout(Duration::from_millis(2));
        }
        assert_eq!(
            rig.history(),
            [
                (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
                (HspEffectRecordKind::Append, HspJournalState::Cancelled),
            ]
        );
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        rig.wait(
            "retrospective emergency observation must become durable",
            |s| !s.metadata_pending,
        );
        assert!(rig.status().emergency_history_durable);
        assert_original_emergency_is_durable(rig);
        rig.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_journal_mutex_outage_retires_unsent_append_and_stops() {
        let rig = Rig::new(false);
        rig.ready();
        let unavailable = rig.f.engine._device_effect_journal.lock().unwrap();
        assert_metadata_independent_stop(&rig);
        drop(unavailable);
        assert_recovered_history_stays_unsent(&rig);
    }

    #[test]
    fn engine_hsp_sqlite_writer_outage_retires_unsent_append_and_stops() {
        let rig = Rig::new(false);
        rig.ready();
        let writer = rusqlite::Connection::open(
            rig.f
                .dir
                .path()
                .join("private-state/device-effects/journal.sqlite3"),
        )
        .unwrap();
        writer.busy_timeout(Duration::ZERO).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert_metadata_independent_stop(&rig);
        writer.execute_batch("ROLLBACK").unwrap();
        assert_recovered_history_stays_unsent(&rig);
    }

    #[test]
    fn engine_hsp_expiry_while_append_reply_is_held_joins_before_stop_and_keeps_unknown() {
        let rig = Rig::new(false);
        rig.ready();
        rig.peer.hold_next("/hsp/add");
        rig.offer(1);
        rig.offer(2);
        rig.peer.wait_held();
        rig.authority.lease_until.store(
            crate::handy_clock::boottime_ns().unwrap(),
            Ordering::Release,
        );
        let snapshot = rig.wait("watchdog must retire held ordinary custody", |s| {
            s.retired && s.native_active
        });
        assert_eq!(
            snapshot.current_effect.map(|e| e.1),
            Some(crate::handy_hsp_runtime::HspEffectKind::Append)
        );
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add"]);
        assert!(rig.session.request_play().is_err());
        let now = crate::handy_clock::boottime_ns().unwrap();
        assert!(rig
            .session
            .offer(
                &rig.binding.identity().live,
                HspSample {
                    point: HspPoint::new(200, 40).unwrap(),
                    source_sequence: 3,
                    captured_at_ns: now,
                    accepted_at_ns: now,
                    continuous_from_previous: true,
                }
            )
            .is_err());
        rig.peer.release();
        rig.stopped();
        rig.wait("unknown append and Stop outcomes must persist", |s| {
            !s.metadata_pending
        });
        assert_original_stop_history(
            &rig,
            &[
                (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
                (HspEffectRecordKind::Append, HspJournalState::Unknown),
            ],
        );
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
        rig.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_shutdown_while_append_reply_is_held_keeps_worker_until_join() {
        let rig = Rig::new(false);
        rig.ready();
        rig.peer.hold_next("/hsp/add");
        rig.offer(1);
        rig.offer(2);
        rig.peer.wait_held();
        rig.f.engine.request_shutdown();
        rig.wait("shutdown must retain active ordinary worker", |s| {
            s.retired && s.native_active
        });
        assert!(rig.session.request_play().is_err());
        assert!(rig.f.engine.ensure_handy_hsp_execution().is_err());
        thread::scope(|scope| {
            let (finished, done) = std::sync::mpsc::sync_channel(1);
            let engine = &rig.f.engine;
            let worker = scope.spawn(move || {
                let result = engine.finish_shutdown();
                let _ = finished.send(result);
            });
            assert!(matches!(
                done.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
            assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add"]);
            rig.peer.release();
            done.recv_timeout(WAIT).unwrap().unwrap();
            worker.join().unwrap();
        });
        let snapshot = rig
            .execution
            .snapshot_for_session(&rig.f.session, &rig.f.project, &rig.record.request_id)
            .unwrap();
        assert!(!snapshot.native_active);
        assert_eq!(
            snapshot.runtime_state,
            crate::handy_hsp_runtime::HspRuntimeState::StopAcknowledged
        );
        assert_original_stop_history(
            &rig,
            &[
                (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
                (HspEffectRecordKind::Append, HspJournalState::Unknown),
            ],
        );
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
        assert_eq!(Arc::strong_count(&rig.f.engine), 1);
    }

    #[test]
    fn engine_hsp_drop_releases_engine_and_joins_original_stop_with_retained_handles() {
        let rig = Rig::new(false);
        rig.ready();
        assert_eq!(Arc::strong_count(&rig.f.engine), 1);
        let Rig {
            f,
            peer,
            execution,
            session,
            authority,
            record,
            binding,
        } = rig;
        let weak = Arc::downgrade(&f.engine);
        let Fixture {
            engine,
            session: actor,
            project,
            dir,
        } = f;
        drop(engine);
        assert!(
            weak.upgrade().is_none(),
            "pump or HTTP worker retained Engine"
        );
        let snapshot = execution
            .snapshot_for_session(&actor, &project, &record.request_id)
            .unwrap();
        assert_eq!(
            snapshot.runtime_state,
            crate::handy_hsp_runtime::HspRuntimeState::StopAcknowledged
        );
        assert!(!snapshot.native_active);
        assert!(session.request_play().is_err());
        assert_eq!(peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        drop((session, execution, authority, record, binding, peer, dir));
    }

    #[test]
    fn engine_hsp_durable_claim_pause_keeps_watchdog_live_without_false_retirement() {
        let pause = Arc::new(ClaimPause::default());
        let rig = Rig::with_claim_pause(false, Some(pause.clone()));
        let release_on_unwind = ReleaseClaimPause(pause.clone());
        pause.wait_entered();
        let before = rig.session.watchdog_progress_for_test().0;
        // Deliberately cross the unchanged 100ms runtime-contention deadline
        // while a durable claim is suspended. No command reaches the peer yet.
        thread::park_timeout(Duration::from_millis(160));
        let snapshot = rig
            .execution
            .snapshot_for_session(&rig.f.session, &rig.f.project, &rig.record.request_id)
            .unwrap();
        assert!(!pause.state.lock().unwrap().timed_out);
        assert!(
            !snapshot.retired,
            "durable claim falsely retired its own runtime: {snapshot:?}; watchdog={:?}; busy={}",
            rig.session.watchdog_progress_for_test(),
            rig.session.watchdog_busy_ticks_for_test()
        );
        let (after, contention_retired) = rig.session.watchdog_progress_for_test();
        assert!(
            after > before && !contention_retired,
            "watchdog must progress during claim I/O"
        );
        assert!(snapshot.native_active);
        assert!(rig.peer.puts().is_empty());
        pause.release();
        rig.ready();
        rig.session.cancel();
        rig.stopped();
        rig.f.engine.finish_shutdown().unwrap();
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        drop(release_on_unwind);
    }

    #[test]
    fn engine_hsp_prelock_fact_race_preserves_original_sample_without_false_clock_regression() {
        let rig = Rig::new(false);
        rig.ready();
        let runtime = rig.session.runtime_for_test();
        let pause = Arc::new(PreLockFactPause::new(&runtime));
        let release_on_unwind = ReleaseFactPause(pause.clone());
        *rig.authority.pre_lock_pause.lock().unwrap() = Some(pause.clone());
        let sampled_before_producer = pause.wait_entered();
        let sample_time = crate::handy_clock::boottime_ns().unwrap();
        assert!(sample_time > sampled_before_producer);
        let original = HspSample {
            point: HspPoint::new(0, 37).unwrap(),
            source_sequence: 1,
            captured_at_ns: sample_time,
            accepted_at_ns: sample_time,
            continuous_from_previous: false,
        };
        assert_eq!(
            rig.session
                .offer(&rig.binding.identity().live, original)
                .unwrap(),
            HspOfferOutcome::Accepted
        );
        // The producer advanced last_context while the owner still holds an
        // older fact pair. Release that old pair exactly, without editing it.
        pause.release();
        let resumed = rig.wait("owner must publish the concurrent accepted sample", |s| {
            s.accepted_samples == 1
        });
        assert!(
            !resumed.retired,
            "older pre-lock clock observation falsely retired the stream: {resumed:?}"
        );
        assert_eq!(
            resumed.runtime_state,
            crate::handy_hsp_runtime::HspRuntimeState::Prefilling
        );
        assert_eq!(rig.peer.puts(), ["/hsp/setup"]);
        assert!(!pause.state.lock().unwrap().timed_out);
        rig.offer(2);
        rig.wait(
            "concurrent sample must become the original first append",
            |s| s.total_added == 2 && !s.native_active && !s.metadata_pending,
        );
        {
            let journal = rig.f.engine._device_effect_journal.lock().unwrap();
            let stream = journal.hsp_stream(&rig.record.id().unwrap()).unwrap();
            let append = stream
                .effect_ids
                .iter()
                .map(|id| journal.hsp_effect(id).unwrap())
                .find(|effect| effect.record.kind == HspEffectRecordKind::Append)
                .unwrap();
            let retained = &append.record.chunk.as_ref().unwrap().samples[0];
            assert_eq!(retained.time_ms, original.point.time_ms);
            assert_eq!(retained.position_percent, original.point.position_percent);
            assert_eq!(retained.source_sequence, original.source_sequence);
            assert_eq!(retained.captured_at_ns, original.captured_at_ns);
            assert_eq!(retained.accepted_at_ns, original.accepted_at_ns);
            assert_eq!(
                retained.continuous_from_previous,
                original.continuous_from_previous
            );
        }
        rig.session.cancel();
        rig.stopped();
        rig.wait("original Stop metadata must settle", |s| {
            !s.metadata_pending
        });
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
        rig.f.engine.finish_shutdown().unwrap();
        drop(release_on_unwind);
    }

    #[test]
    fn engine_hsp_short_runtime_contention_survives_a_real_watchdog_tick() {
        let rig = Rig::new(false);
        rig.ready();
        let runtime = rig.session.runtime_for_test();
        let guard = runtime.lock().unwrap();
        let observed = rig.session.watchdog_busy_ticks_for_test();
        let began = crate::handy_clock::boottime_ns().unwrap();
        let deadline = Instant::now() + WAIT;
        while rig.session.watchdog_busy_ticks_for_test() == observed {
            assert!(
                Instant::now() < deadline,
                "watchdog never observed the held runtime mutex"
            );
            thread::park_timeout(Duration::from_millis(1));
        }
        // This is an actual short-contention scenario, not a relaxed assertion
        // if the host stalls longer than the configured watchdog allowance.
        let held_ns = crate::handy_clock::boottime_ns().unwrap() - began;
        drop(guard);
        assert!(
            held_ns < 100_000_000,
            "short-hold test exceeded native allowance: {held_ns}ns"
        );
        rig.offer(1);
        rig.offer(2);
        rig.wait(
            "ordinary append must continue after brief contention",
            |s| s.total_added == 2 && !s.native_active && !s.metadata_pending,
        );
        rig.session.cancel();
        rig.stopped();
        rig.wait("Stop metadata must settle", |s| !s.metadata_pending);
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
        rig.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_continuous_runtime_contention_retires_admission_and_stops_after_release() {
        let rig = Rig::new(false);
        rig.ready();
        let runtime = rig.session.runtime_for_test();
        let guard = runtime.lock().unwrap();
        let began = crate::handy_clock::boottime_ns().unwrap();
        let deadline = Instant::now() + WAIT;
        loop {
            match rig.session.request_play() {
                Err(HspExecutionError::Busy) if Instant::now() < deadline => {
                    thread::park_timeout(Duration::from_millis(2));
                }
                Err(HspExecutionError::Unavailable) => break,
                other => panic!("held runtime must remain Busy then retire: {other:?}"),
            }
        }
        assert!(crate::handy_clock::boottime_ns().unwrap() - began >= 100_000_000);
        assert_eq!(rig.peer.puts(), ["/hsp/setup"]);
        drop(guard);
        rig.stopped();
        rig.wait("post-contention Stop metadata must settle", |s| {
            !s.metadata_pending
        });
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        rig.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_source_retirement_is_observed_while_runtime_mutex_is_held() {
        let rig = Rig::new(false);
        rig.ready();
        let runtime = rig.session.runtime_for_test();
        let guard = runtime.lock().unwrap();
        rig.authority.retired.store(true, Ordering::Release);
        let snapshot = rig.wait(
            "source retirement must publish even when runtime is unavailable",
            |s| s.retired,
        );
        assert!(!snapshot.native_active);
        assert!(matches!(
            rig.session.request_play(),
            Err(HspExecutionError::Unavailable)
        ));
        assert_eq!(rig.peer.puts(), ["/hsp/setup"]);
        drop(guard);
        rig.stopped();
        rig.wait("Stop metadata must settle after runtime release", |s| {
            !s.metadata_pending
        });
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        rig.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_poisoned_runtime_is_retired_without_recovering_motion_authority() {
        let rig = Rig::new(false);
        rig.ready();
        let runtime = rig.session.runtime_for_test();
        let poisoner = thread::spawn(move || {
            let _guard = runtime.lock().unwrap();
            panic!("intentional HSP runtime poison fixture");
        });
        assert!(poisoner.join().is_err());
        let snapshot = rig.wait(
            "poisoned runtime must retain a blocked Stop obligation",
            |s| s.retired && s.stop_blocked,
        );
        assert!(!snapshot.native_active);
        assert!(rig.session.request_play().is_err());
        assert_eq!(rig.peer.puts(), ["/hsp/setup"]);
        assert!(rig.f.engine.finish_shutdown().is_err());
        assert_eq!(rig.peer.puts(), ["/hsp/setup"]);
    }

    #[test]
    fn engine_hsp_replaced_post_setup_probe_never_moves_replacement() {
        let rig = Rig::new(true);
        let snapshot = rig.wait("replacement observed by separate post-Setup probe", |s| {
            s.stop_blocked && !s.native_active
        });
        assert!(snapshot.retired && !snapshot.post_setup_bound);
        assert_eq!(rig.peer.puts(), ["/hsp/setup"]);
        assert!(rig.session.request_play().is_err());
        assert!(
            rig.f.engine.finish_shutdown().is_err(),
            "replacement prevents original Stop proof"
        );
        assert_eq!(rig.peer.puts(), ["/hsp/setup"]);
    }
    #[test]
    fn engine_hsp_first_stop_retains_missing_facts_before_later_cancel_and_shutdown() {
        use crate::handy_hsp_execution::HspFirstStopReason;
        let rig = Rig::new(false);
        rig.ready();
        assert!(rig.session.first_stop_for_test().is_none());
        let runtime = rig.session.runtime_for_test();
        let guard = runtime.lock().unwrap();
        rig.authority.retired.store(true, Ordering::Release);
        rig.wait(
            "missing authority must retain its first native branch",
            |s| s.retired,
        );
        let first = rig.session.first_stop_for_test().unwrap();
        assert_eq!(first.reason, HspFirstStopReason::WatchdogMissingFacts);
        assert!(first.sampled_at_ns.is_some() && first.failure.is_none() && first.facts.is_none());
        assert!(first.source_file.ends_with("handy_hsp_execution.rs") && first.source_line > 0);
        rig.session.cancel();
        assert_eq!(rig.session.first_stop_for_test().as_ref(), Some(&first));
        drop(guard);
        rig.stopped();
        rig.f.engine.finish_shutdown().unwrap();
        assert_eq!(rig.session.first_stop_for_test().as_ref(), Some(&first));
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    }
    #[test]
    fn engine_hsp_first_stop_explicit_native_cancel_stays_distinct_from_watchdog_failure() {
        use crate::handy_hsp_execution::HspFirstStopReason;
        let rig = Rig::new(false);
        rig.ready();
        assert!(rig.session.first_stop_for_test().is_none());
        rig.session.cancel();
        rig.stopped();
        let first = rig.session.first_stop_for_test().unwrap();
        assert_eq!(first.reason, HspFirstStopReason::CancellationObserved);
        assert!(first.failure.is_none());
        let actual = rig
            .execution
            .first_stop_for_session(&rig.f.session, &rig.f.project, &rig.record.request_id)
            .unwrap()
            .unwrap();
        assert_eq!(actual, first);
        assert!(matches!(
            rig.execution.first_stop_for_session(
                &SessionId::new("different-owner").unwrap(),
                &rig.f.project,
                &rig.record.request_id
            ),
            Err(HspExecutionError::NotFound)
        ));
        assert!(serde_json::to_vec(&actual.diagnostic()).unwrap().len() < 4096);
        rig.authority.retired.store(true, Ordering::Release);
        rig.f.engine.finish_shutdown().unwrap();
        assert_eq!(rig.session.first_stop_for_test().as_ref(), Some(&first));
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        // A directly requested native cancellation is distinct here; the reason
        // does not attest which UI gesture or cleanup path requested it.
    }
    struct FirstStopRefusalAuthority {
        inner: Arc<SyntheticAuthority>,
        refuse: Arc<AtomicBool>,
        first_failed_sample: Arc<AtomicU64>,
    }
    impl HspNativeAuthority for FirstStopRefusalAuthority {
        fn current_facts(&self, now: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure> {
            self.inner.current_facts(now)
        }
        fn observe_current(
            &self,
            clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
        ) -> Result<crate::handy_hsp_execution::HspAuthorityEvaluation, HspDispatchFailure>
        {
            let now = clock()?;
            if self.refuse.load(Ordering::Acquire) {
                let _ = self.first_failed_sample.compare_exchange(
                    0,
                    now,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return Err(HspDispatchFailure::BindingMismatch);
            }
            Ok(crate::handy_hsp_execution::HspAuthorityEvaluation {
                sampled_at_ns: now,
                snapshot: HspAuthoritySnapshot {
                    facts: self.inner.current_facts(now)?,
                    projected: None,
                },
            })
        }
        fn authorize(
            &self,
            deadline: Instant,
            cancel: &AtomicBool,
            gate: HspClaimGate<'_>,
            owner: &HspNativeOwner,
        ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
            self.inner.authorize(deadline, cancel, gate, owner)
        }
    }
    #[test]
    fn engine_hsp_first_stop_keeps_actual_provider_error_and_its_original_sample() {
        use crate::handy_hsp_execution::HspFirstStopReason;
        let refuse = Arc::new(AtomicBool::new(false));
        let sampled = Arc::new(AtomicU64::new(0));
        let rig = Rig::with_authority_wrapper(false, None, 0, |inner| {
            Arc::new(FirstStopRefusalAuthority {
                inner,
                refuse: refuse.clone(),
                first_failed_sample: sampled.clone(),
            })
        });
        rig.ready();
        // Freeze the runtime only: this forces the next failing read to be the
        // independent pre-lock watchdog read, never a later next-effect read.
        let runtime = rig.session.runtime_for_test();
        let guard = runtime.lock().unwrap();
        refuse.store(true, Ordering::Release);
        rig.wait(
            "provider refusal must retire without the runtime mutex",
            |s| s.retired,
        );
        let first = rig.session.first_stop_for_test().unwrap();
        assert_eq!(first.reason, HspFirstStopReason::WatchdogCurrentObservation);
        assert_eq!(first.failure, Some(HspDispatchFailure::BindingMismatch));
        assert_eq!(
            first.sampled_at_ns,
            Some(sampled.load(Ordering::Acquire)),
            "diagnostic cannot replace the already sampled failing observation"
        );
        rig.session.cancel();
        drop(guard);
        rig.stopped();
        rig.f.engine.finish_shutdown().unwrap();
        assert_eq!(rig.session.first_stop_for_test().as_ref(), Some(&first));
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    }

    include!("engine_handy_hsp_fresh_observation_execution_tests.rs");
    include!("engine_handy_hsp_recovery_execution_tests.rs");
    include!("engine_handy_hsp_preparation_tests.rs");
    include!("engine_handy_hsp_finite_execution_tests.rs");
}
