// These acknowledged-play/qualification fixtures and localhost responses are
// synthetic software evidence, not physical motion or browser synchronization.
// A guard assertion may include durable authority mutations. Keep the fixture
// server owned until that assertion finishes instead of starting a five-second
// accept timer while those unrelated writes are still running. Production
// transport deadlines and all injected native lease/timing bounds stay intact.
struct PlayerGuardServer {
    cancelled: Arc<AtomicBool>,
    task: Option<ContinuousServer>,
}
impl PlayerGuardServer {
    fn finish(mut self) -> Vec<(String, Vec<u8>)> {
        self.task.take().unwrap().join().unwrap()
    }
}
impl Drop for PlayerGuardServer {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}
fn player_guard_server(listener: std::net::TcpListener, acknowledge: bool) -> PlayerGuardServer {
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let task = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (expected, state) in [
            ("PUT /hssp/play HTTP/1.1", 1),
            ("PUT /hssp/stop HTTP/1.1", 2),
        ] {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            let mut stream = loop {
                if worker_cancelled.load(Ordering::Acquire) {
                    return requests;
                }
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("guard fixture accept failed: {error}"),
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "guard fixture never received {expected}"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            };
            let request = handy_play_read_request(&mut stream);
            assert_eq!(request.0, expected);
            if state == 2 || acknowledge {
                handy_play_reply(&mut stream, state);
            }
            requests.push(request);
        }
        requests
    });
    PlayerGuardServer {
        cancelled,
        task: Some(task),
    }
}
fn player_guard_wait(mut condition: impl FnMut() -> bool) {
    // This bounds the test harness, not a device timing or authority policy.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "guard fixture execution did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn player_guard_fixture() -> HandyPlayFixture {
    // The injected server/device samples already share fixed native time. Keep
    // the synchronous fixture's transport observation clock fixed as well;
    // actual transport tests continue to use the real clock.
    Engine::with_handy_sync_instant_for_test(std::time::Instant::now(), continuous_fixture)
}
fn player_guard_playing() -> (HandyPlayFixture, PlayerGuardServer) {
    let x = player_guard_fixture();
    let server = player_guard_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    player_guard_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|history| history.state == HandyPlaybackOperationState::Acknowledged)
    });
    (x, server)
}
fn player_guard_finish(x: &HandyPlayFixture, server: PlayerGuardServer) {
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "PUT /hssp/play HTTP/1.1");
    assert_eq!(requests[1].0, "PUT /hssp/stop HTTP/1.1");
    player_guard_wait(|| !x.pending.load(Ordering::Acquire));
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    assert!(x.listener.accept().is_err());
}
fn player_guard_request(x: &HandyPlayFixture) -> Request {
    let Command::RefreshHandyPlaybackTiming { selection, .. } =
        continuous_request(x, "selection-only", 0).command
    else {
        unreachable!()
    };
    let mut r = request(
        &x.f.session,
        Some(&x.f.project),
        Some(1),
        Command::BindHandyPlayerGuard {
            browser_media: None,
            selection,
            player: HandyPlayerIdentity {
                launch_id: AttemptId::new("guard-launch").unwrap(),
                page_id: AttemptId::new("guard-page").unwrap(),
                player_id: AttemptId::new("guard-player").unwrap(),
                source_id: AttemptId::new("guard-source").unwrap(),
                generation: 1,
                time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
            },
            policy: HandyPlayerGuardPolicy {
                heartbeat_timeout_ms: 100,
                media_stall_timeout_ms: 100,
                challenge_timeout_ms: 50,
            },
            allow_monitoring: true,
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new("bind-player-guard").unwrap();
    r
}
fn player_guard_report(body: ResponseBody) -> HandyPlayerGuardReport {
    let ResponseBody::HandyPlayerGuard(report) = body else {
        panic!("player report")
    };
    report.validate().unwrap();
    *report
}
fn player_guard_call(
    x: &HandyPlayFixture,
    r: &Request,
    now: u64,
) -> PResult<HandyPlayerGuardReport> {
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    x.f.engine
        .handy_player_guard_with(r, &x.f.session, &fingerprint, || Ok(now))
        .map(player_guard_report)
}
fn player_guard_started() -> (
    HandyPlayFixture,
    PlayerGuardServer,
    Request,
    HandyPlayerGuardReport,
) {
    let (x, server) = player_guard_playing();
    x.f.engine
        .start_handy_player_monitor_for_test(Arc::new(|| Ok(1_100_000_000)))
        .unwrap();
    let r = player_guard_request(&x);
    let report = player_guard_call(&x, &r, 1_100_000_000).unwrap();
    assert!(report.current && report.physical_stop_pending);
    assert!(!report.synchronized && !report.playback_enabled);
    (x, server, r, report)
}
fn player_guard_status(
    x: &HandyPlayFixture,
    report: &HandyPlayerGuardReport,
    now: u64,
) -> PResult<HandyPlayerGuardReport> {
    let r = request(
        &x.f.session,
        Some(&x.f.project),
        None,
        Command::HandyPlayerGuardStatus {
            binding_request_id: report.binding.request_id.clone(),
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    );
    player_guard_call(x, &r, now)
}
fn player_guard_command(x: &HandyPlayFixture, command: Command) -> Request {
    request(&x.f.session, Some(&x.f.project), None, command)
}
fn player_guard_issue(
    x: &HandyPlayFixture,
    report: &HandyPlayerGuardReport,
    sequence: u64,
) -> Request {
    player_guard_command(
        x,
        Command::IssueHandyPlayerChallenge {
            binding_id: report.binding.binding_id.clone(),
            player: report.binding.player.clone(),
            sequence,
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    )
}
fn player_guard_sample(time: u64, frames: u64) -> HandyPlayerSample {
    HandyPlayerSample {
        browser_tick_us: time,
        media_time_us: time,
        presented_frames: frames,
        playing: true,
        seeking: false,
        ended: false,
        visible: true,
        playback_rate_milli: 1000,
    }
}
fn player_guard_observe(
    x: &HandyPlayFixture,
    challenge: &HandyPlayerChallenge,
    sample: HandyPlayerSample,
) -> Request {
    player_guard_command(
        x,
        Command::ObserveHandyPlayer {
            binding_id: challenge.binding_id.clone(),
            player: challenge.player.clone(),
            sequence: challenge.sequence,
            challenge_id: challenge.challenge_id.clone(),
            sample,
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    )
}
fn player_guard_release(x: &HandyPlayFixture, report: &HandyPlayerGuardReport) -> Request {
    player_guard_command(
        x,
        Command::ReleaseHandyPlayerGuard {
            binding_id: report.binding.binding_id.clone(),
            player: report.binding.player.clone(),
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    )
}
fn player_guard_fast(engine: &Arc<Engine>, r: Request, now: u64) -> PResult<ResponseBody> {
    Engine::with_handy_player_clock_for_test(now, || engine.handle(r).result)
}
fn player_guard_challenge(body: ResponseBody) -> HandyPlayerChallenge {
    let ResponseBody::HandyPlayerChallenge(challenge) = body else {
        panic!("player challenge")
    };
    challenge.validate().unwrap();
    *challenge
}
#[test]
fn handy_player_guard_nonce_replay_stale_scope_and_credentials_cannot_extend_or_retire() {
    let (x, server, bind, initial) = player_guard_started();
    let issue = player_guard_issue(&x, &initial, 1);
    let first = player_guard_challenge(
        player_guard_fast(&x.f.engine, issue.clone(), 1_110_000_000).unwrap(),
    );
    let duplicate = player_guard_challenge(
        player_guard_fast(&x.f.engine, issue.clone(), 1_120_000_000).unwrap(),
    );
    assert_eq!(first, duplicate);
    for case in 0..6 {
        let mut wrong = issue.clone();
        wrong.request_id = RequestId::new(format!("wrong-pulse-{case}")).unwrap();
        match case {
            0 => wrong.auth_token = Some("different-session-token".into()),
            1 => wrong.session = Some(SessionId::new("foreign-session").unwrap()),
            2 => wrong.project = Some(ProjectId::new("foreign-project").unwrap()),
            _ => {
                if let Command::IssueHandyPlayerChallenge {
                    binding_id,
                    player,
                    sequence,
                    ..
                } = &mut wrong.command
                {
                    match case {
                        3 => *binding_id = ArtifactId::new("a".repeat(64)).unwrap(),
                        4 => player.page_id = AttemptId::new("old-page").unwrap(),
                        _ => *sequence = 2,
                    }
                }
            }
        }
        assert!(player_guard_fast(&x.f.engine, wrong, 1_120_000_000).is_err());
    }
    let mut wrong = player_guard_observe(&x, &first, player_guard_sample(100, 1));
    if let Command::ObserveHandyPlayer { challenge_id, .. } = &mut wrong.command {
        *challenge_id = ArtifactId::new("b".repeat(64)).unwrap();
    }
    assert!(player_guard_fast(&x.f.engine, wrong, 1_120_000_000).is_err());
    let observation = player_guard_observe(&x, &first, player_guard_sample(100, 1));
    let accepted = player_guard_report(
        player_guard_fast(&x.f.engine, observation.clone(), 1_140_000_000).unwrap(),
    );
    assert_eq!(
        accepted.heartbeat_deadline_boottime_ns, 1_210_000_000,
        "issue time anchors freshness"
    );
    assert_eq!(accepted.media_progress_deadline_boottime_ns, 1_210_000_000);
    assert!(player_guard_fast(&x.f.engine, observation, 1_140_000_000).is_err());
    assert!(player_guard_fast(&x.f.engine, issue, 1_140_000_000).is_err());
    let historical = player_guard_call(&x, &bind, 1_140_000_000).unwrap();
    assert_eq!(
        historical.heartbeat_deadline_boottime_ns,
        accepted.heartbeat_deadline_boottime_ns
    );
    assert_eq!(historical.last_sequence, 1);
    let second = player_guard_challenge(
        player_guard_fast(
            &x.f.engine,
            player_guard_issue(&x, &initial, 2),
            1_150_000_000,
        )
        .unwrap(),
    );
    assert!(
        player_guard_fast(
            &x.f.engine,
            player_guard_observe(&x, &second, player_guard_sample(200, 2)),
            1_200_000_000
        )
        .is_err(),
        "challenge expires exactly at its deadline"
    );
    assert_eq!(
        player_guard_status(&x, &initial, 1_200_000_000)
            .unwrap()
            .heartbeat_deadline_boottime_ns,
        accepted.heartbeat_deadline_boottime_ns
    );
    assert!(player_guard_fast(
        &x.f.engine,
        player_guard_issue(&x, &initial, 2),
        1_210_000_000
    )
    .is_err());
    assert!(!player_guard_call(&x, &bind, 1_210_000_000).unwrap().current);
    player_guard_finish(&x, server);
}
#[test]
fn handy_player_guard_frames_without_media_advance_cannot_keep_stream_alive() {
    let (x, server, _, initial) = player_guard_started();
    for (sequence, now, time, frames) in [(1, 1_110_000_000, 100, 1), (2, 1_170_000_000, 100, 2)] {
        let challenge = player_guard_challenge(
            player_guard_fast(&x.f.engine, player_guard_issue(&x, &initial, sequence), now)
                .unwrap(),
        );
        let report = player_guard_report(
            player_guard_fast(
                &x.f.engine,
                player_guard_observe(&x, &challenge, player_guard_sample(time, frames)),
                now + 1,
            )
            .unwrap(),
        );
        assert_eq!(report.media_progress_deadline_boottime_ns, 1_210_000_000);
        assert_eq!(report.heartbeat_deadline_boottime_ns, now + 100_000_000);
    }
    assert!(player_guard_fast(
        &x.f.engine,
        player_guard_issue(&x, &initial, 3),
        1_210_000_000
    )
    .is_err());
    player_guard_finish(&x, server);
}
#[test]
fn handy_player_guard_stopped_hidden_seeking_rate_and_backward_samples_retire_exact_play() {
    for case in 0..8 {
        let (x, server, _, initial) = player_guard_started();
        let first = player_guard_challenge(
            player_guard_fast(
                &x.f.engine,
                player_guard_issue(&x, &initial, 1),
                1_110_000_000,
            )
            .unwrap(),
        );
        player_guard_fast(
            &x.f.engine,
            player_guard_observe(&x, &first, player_guard_sample(100, 2)),
            1_110_000_001,
        )
        .unwrap();
        let second = player_guard_challenge(
            player_guard_fast(
                &x.f.engine,
                player_guard_issue(&x, &initial, 2),
                1_120_000_000,
            )
            .unwrap(),
        );
        let mut sample = player_guard_sample(200, 3);
        match case {
            0 => sample.playing = false,
            1 => sample.visible = false,
            2 => sample.seeking = true,
            3 => sample.ended = true,
            4 => sample.playback_rate_milli = 2000,
            5 => sample.browser_tick_us = 99,
            6 => sample.media_time_us = 99,
            _ => sample.presented_frames = 1,
        }
        let report = player_guard_report(
            player_guard_fast(
                &x.f.engine,
                player_guard_observe(&x, &second, sample),
                1_120_000_001,
            )
            .unwrap(),
        );
        assert!(
            !report.current && report.physical_stop_pending,
            "case {case}"
        );
        assert!(player_guard_fast(
            &x.f.engine,
            player_guard_issue(&x, &initial, 3),
            1_120_000_002
        )
        .is_err());
        player_guard_finish(&x, server);
    }
}
#[test]
fn handy_player_guard_heartbeat_release_and_expiry_stop_bypass_held_main_database() {
    for expire in [false, true] {
        let (x, server, _, initial) = player_guard_started();
        // Construct authenticated requests before changing threads: fixture tokens
        // are thread-local, while production tokens remain native client-owned.
        let issue = player_guard_issue(&x, &initial, 1);
        let release = player_guard_release(&x, &initial);
        let next = player_guard_issue(&x, &initial, 2);
        let engine = x.f.engine.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let db = x.f.engine.db.lock().unwrap();
        let task = std::thread::spawn(move || {
            let result = (|| {
                let c = player_guard_challenge(player_guard_fast(
                    &engine,
                    issue.clone(),
                    1_110_000_000,
                )?);
                let mut observe = issue;
                observe.command = Command::ObserveHandyPlayer {
                    binding_id: c.binding_id,
                    player: c.player,
                    sequence: c.sequence,
                    challenge_id: c.challenge_id,
                    sample: player_guard_sample(100, 1),
                    owner_proof: engine.handy_owner_token.clone(),
                };
                let pulse =
                    player_guard_report(player_guard_fast(&engine, observe, 1_120_000_000)?);
                assert!(pulse.current);
                if expire {
                    assert!(player_guard_fast(&engine, next, 1_210_000_000).is_err());
                } else {
                    assert!(
                        !player_guard_report(player_guard_fast(&engine, release, 1_120_000_001)?)
                            .current
                    );
                }
                Ok::<_, ProtocolError>(())
            })();
            tx.send(result).unwrap();
        });
        let result = rx.recv_timeout(std::time::Duration::from_secs(3));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while result.as_ref().is_ok_and(|r| r.is_ok())
            && x.runtime.lock().unwrap().session().state() != DeviceState::StopUnconfirmed
            && std::time::Instant::now() < until
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let stopped = x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed;
        drop(db);
        task.join().unwrap();
        result.expect("fast path waited for main DB").unwrap();
        assert!(stopped, "stop transport waited for main DB");
        player_guard_finish(&x, server);
    }
}
#[test]
fn handy_player_guard_monitor_expires_guard_without_any_request_while_database_is_held() {
    let (x, server) = player_guard_playing();
    let bind = player_guard_request(&x);
    let tick = Arc::new(std::sync::atomic::AtomicU64::new(1_100_000_000));
    let monitor_tick = tick.clone();
    x.f.engine
        .start_handy_player_monitor_for_test(Arc::new(move || {
            Ok(monitor_tick.load(Ordering::Acquire))
        }))
        .unwrap();
    let initial = player_guard_call(&x, &bind, 1_100_000_000).unwrap();
    let db = x.f.engine.db.lock().unwrap();
    tick.store(initial.heartbeat_deadline_boottime_ns, Ordering::Release);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while x.runtime.lock().unwrap().session().state() != DeviceState::StopUnconfirmed
        && std::time::Instant::now() < until
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let stopped = x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed;
    drop(db);
    assert!(stopped, "guard expiry required a request or DB ownership");
    // At 1.2s the original qualified sync still has 0.3s and the controller 0.8s.
    assert!(initial.heartbeat_deadline_boottime_ns < 1_500_000_000);
    player_guard_finish(&x, server);
}
#[test]
fn handy_player_guard_public_authority_mutations_retire_cached_fast_credentials_before_return() {
    for event in 0..6 {
        let (x, server, _, initial) = player_guard_started();
        let issue = player_guard_issue(&x, &initial, 1);
        let inspection = {
            let db = x.f.engine.db.lock().unwrap();
            let raw: String = db
                .query_row(
                    "SELECT response FROM requests WHERE session=?1 AND request=?2",
                    params![
                        x.f.session.as_str(),
                        x.lease.preparation_request_id.as_str()
                    ],
                    |row| row.get(0),
                )
                .unwrap();
            playback_preparation_report(decode(&raw).unwrap())
                .correlation
                .inspection
        };
        let mutation = match event {
            0 => request(
                &x.f.session,
                Some(&x.f.project),
                Some(1),
                commit_program(&x.f, 1, program(0.3)),
            ),
            1 => request(
                &x.f.session,
                None,
                None,
                Command::Revoke {
                    session: x.f.session.clone(),
                    project_id: x.f.project.clone(),
                },
            ),
            2 => request(
                &x.f.session,
                None,
                None,
                Command::CloseHandySession {
                    engine_epoch: inspection.engine_epoch.clone(),
                    device_session: inspection.device_session.clone(),
                    owner_proof: x.f.engine.handy_owner_token.clone(),
                },
            ),
            3 => request(
                &x.f.session,
                None,
                None,
                Command::DisableHandyRegistration {
                    device_id: inspection.registration.device_id.clone(),
                    expected_registration_revision: inspection.registration.revision,
                    owner_proof: x.f.engine.handy_owner_token.clone(),
                },
            ),
            4 => qualification_withdraw(
                &x.f,
                &inspection
                    .accepted_qualification
                    .as_ref()
                    .unwrap()
                    .dossier_id,
                inspection.accepted_qualification.as_ref().unwrap().revision,
            ),
            _ => request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::ForgetHandyHosting {
                    upload_request_id: inspection.hosting_request_id.clone(),
                    owner_proof: x.f.engine.handy_owner_token.clone(),
                },
            ),
        };
        x.f.engine.handle(mutation).result.unwrap();
        assert!(
            player_guard_fast(&x.f.engine, issue, 1_110_000_000).is_err(),
            "event {event}"
        );
        player_guard_finish(&x, server);
    }
}
#[test]
fn handy_player_guard_renewal_preserves_deadlines_and_explicit_timing_lineage_removal_retires() {
    let (x, server, _, initial) = player_guard_started();
    let renewal = controller_renew_request(&x.f, &x.lease, "guard-controller-renewal");
    assert_eq!(
        controller_report(controller_call(&x.f, &renewal, 1_110_000_000).unwrap())
            .lease
            .generation,
        1
    );
    let view = player_guard_status(&x, &initial, 1_110_000_000).unwrap();
    assert!(view.current);
    assert_eq!(
        view.binding.controller.generation, 0,
        "historical binding remains immutable"
    );
    assert_eq!(
        view.heartbeat_deadline_boottime_ns,
        initial.heartbeat_deadline_boottime_ns
    );
    assert_eq!(
        view.media_progress_deadline_boottime_ns,
        initial.media_progress_deadline_boottime_ns
    );
    player_guard_fast(
        &x.f.engine,
        player_guard_issue(&x, &initial, 1),
        1_110_000_001,
    )
    .unwrap();
    x.f.engine
        .remove_handy_player_sync_for_test(&x.lease.preparation_id);
    assert!(player_guard_fast(
        &x.f.engine,
        player_guard_issue(&x, &initial, 1),
        1_110_000_002
    )
    .is_err());
    player_guard_finish(&x, server);
}
#[test]
fn handy_player_guard_receipt_failure_late_commit_and_bind_replay_never_install_or_revive() {
    for late in [false, true] {
        let (x, server) = player_guard_playing();
        x.f.engine
            .start_handy_player_monitor_for_test(Arc::new(|| Ok(1_100_000_000)))
            .unwrap();
        let bind = player_guard_request(&x);
        if !late {
            x.f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER refuse_player_binding BEFORE INSERT ON requests WHEN NEW.request='bind-player-guard' BEGIN SELECT RAISE(ABORT,'injected binding receipt failure'); END;").unwrap();
            assert!(player_guard_call(&x, &bind, 1_100_000_000).is_err());
            x.f.engine
                .db
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER refuse_player_binding")
                .unwrap();
        } else {
            let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&bind).unwrap()));
            let calls = std::cell::Cell::new(0);
            let report = player_guard_report(
                x.f.engine
                    .handy_player_guard_with(&bind, &x.f.session, &fingerprint, || {
                        let n = calls.get() + 1;
                        calls.set(n);
                        // controller_view twice, capture, precommit and postcommit.
                        Ok(if n < 5 { 1_100_000_000 } else { 1_200_000_000 })
                    })
                    .unwrap(),
            );
            assert!(!report.current);
            assert!(!player_guard_call(&x, &bind, 1_200_000_000).unwrap().current);
        }
        assert_eq!(
            x.f.engine.handy_player_guard_count_for_test(),
            0,
            "no native authority from failed receipt or late completion"
        );
        player_guard_finish(&x, server);
    }
}

#[test]
fn handy_player_guard_session_rotation_and_failed_monitor_retire_native_cache() {
    for rotate in [false, true] {
        let (x, server, _, initial) = player_guard_started();
        if rotate {
            let db = x.f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE sessions SET token_hash=?1 WHERE id=?2",
                params!["f".repeat(64), x.f.session.as_str()],
            )
            .unwrap();
            // External SQL is not an immediate fast-path signal; every public
            // mutation executes this DB-backed retirement before returning.
            x.f.engine.retire_changed_handy_preparations(&db);
        } else {
            let monitor = x.f.engine.handy_controller_monitor.lock().unwrap().take();
            drop(monitor);
        }
        assert!(player_guard_fast(
            &x.f.engine,
            player_guard_issue(&x, &initial, 1),
            1_110_000_000
        )
        .is_err());
        assert_eq!(x.f.engine.handy_player_guard_count_for_test(), 0);
        player_guard_finish(&x, server);
    }
}
#[test]
fn handy_player_guard_existing_binding_rejects_replacement_and_expired_history_cannot_rebind() {
    let (x, server, bind, initial) = player_guard_started();
    let mut replacement = bind.clone();
    replacement.request_id = RequestId::new("replacement-player-binding").unwrap();
    if let Command::BindHandyPlayerGuard { player, .. } = &mut replacement.command {
        player.page_id = AttemptId::new("replacement-page").unwrap();
    }
    assert!(player_guard_call(&x, &replacement, 1_110_000_000).is_err());
    assert_eq!(
        player_guard_status(&x, &initial, 1_110_000_000)
            .unwrap()
            .binding
            .binding_id,
        initial.binding.binding_id
    );
    assert!(player_guard_fast(
        &x.f.engine,
        player_guard_issue(&x, &initial, 1),
        1_200_000_000
    )
    .is_err());
    assert!(!player_guard_call(&x, &bind, 1_200_000_000).unwrap().current);
    assert!(player_guard_call(&x, &replacement, 1_200_000_000).is_err());
    player_guard_finish(&x, server);
}
#[test]
fn handy_player_guard_cannot_extend_continuous_timing_or_rescue_unknown_original_play() {
    let (x, server) = player_guard_playing();
    let refresh = continuous_request(&x, "guard-independent-timing", 0);
    let timing = continuous_call(&x, &refresh, 1_050_000_000).unwrap();
    let mut bind = player_guard_request(&x);
    if let Command::BindHandyPlayerGuard { policy, .. } = &mut bind.command {
        *policy = HandyPlayerGuardPolicy {
            heartbeat_timeout_ms: 1000,
            media_stall_timeout_ms: 1000,
            challenge_timeout_ms: 500,
        };
    }
    x.f.engine
        .start_handy_player_monitor_for_test(Arc::new(|| Ok(1_100_000_000)))
        .unwrap();
    let initial = player_guard_call(&x, &bind, 1_100_000_000).unwrap();
    let renewal = controller_renew_request(&x.f, &x.lease, "renew-with-player-timing");
    let renewed = controller_report(controller_call(&x.f, &renewal, 1_800_000_000).unwrap()).lease;
    assert!(timing.valid_until_boottime_ns < initial.heartbeat_deadline_boottime_ns);
    assert!(timing.valid_until_boottime_ns < renewed.deadline_boottime_ns);
    assert!(player_guard_fast(
        &x.f.engine,
        player_guard_issue(&x, &initial, 1),
        timing.valid_until_boottime_ns
    )
    .is_err());
    player_guard_finish(&x, server);

    let x = player_guard_fixture();
    let server = player_guard_server(x.listener.try_clone().unwrap(), false);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    player_guard_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Unknown)
    });
    x.f.engine
        .start_handy_player_monitor_for_test(Arc::new(|| Ok(1_100_000_000)))
        .unwrap();
    let bind = player_guard_request(&x);
    assert!(player_guard_call(&x, &bind, 1_100_000_000).is_err());
    assert_eq!(x.f.engine.handy_player_guard_count_for_test(), 0);
    player_guard_finish(&x, server);
}
#[test]
fn handy_player_guard_public_bind_requires_live_monitor_and_release_does_not_wait_for_owner_slots()
{
    let (x, server) = player_guard_playing();
    let bind = player_guard_request(&x);
    assert!(x.f.engine.handle(bind.clone()).result.is_err());
    assert_eq!(x.f.engine.handy_player_guard_count_for_test(), 0);
    assert_eq!(
        x.f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM requests WHERE session=?1 AND request=?2",
                params![x.f.session.as_str(), bind.request_id.as_str()],
                |row| row.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
    player_guard_finish(&x, server);

    let (x, server, _, initial) = player_guard_started();
    let monitor = x.f.engine.handy_controller_monitor.lock().unwrap();
    let execution = x.f.engine.handy_execution.lock().unwrap();
    assert_eq!(
        player_guard_fast(
            &x.f.engine,
            player_guard_issue(&x, &initial, 1),
            1_110_000_000
        )
        .unwrap_err()
        .code,
        ErrorCode::Unavailable
    );
    assert!(
        !player_guard_report(
            player_guard_fast(
                &x.f.engine,
                player_guard_release(&x, &initial),
                1_110_000_001
            )
            .unwrap()
        )
        .current
    );
    drop(execution);
    drop(monitor);
    player_guard_finish(&x, server);
}

#[test]
fn handy_player_guard_first_zero_frame_heartbeat_cannot_extend_initial_media_expiry() {
    for frames in [0, 1] {
        let (x, server, _, initial) = player_guard_started();
        let issue_time = initial.media_progress_deadline_boottime_ns - 10_000_000;
        let first = player_guard_challenge(
            player_guard_fast(&x.f.engine, player_guard_issue(&x, &initial, 1), issue_time)
                .unwrap(),
        );
        // Zero media time is valid for a reported first frame. A heartbeat
        // before any frame has been reported is not media progress.
        let observed = player_guard_report(
            player_guard_fast(
                &x.f.engine,
                player_guard_observe(&x, &first, player_guard_sample(0, frames)),
                issue_time + 1,
            )
            .unwrap(),
        );
        assert_eq!(
            observed.heartbeat_deadline_boottime_ns,
            issue_time + 100_000_000
        );
        if frames == 0 {
            assert_eq!(
                observed.media_progress_deadline_boottime_ns,
                initial.media_progress_deadline_boottime_ns
            );
            let second = player_guard_challenge(
                player_guard_fast(
                    &x.f.engine,
                    player_guard_issue(&x, &initial, 2),
                    issue_time + 2,
                )
                .unwrap(),
            );
            let repeated = player_guard_report(
                player_guard_fast(
                    &x.f.engine,
                    player_guard_observe(&x, &second, player_guard_sample(0, 0)),
                    issue_time + 3,
                )
                .unwrap(),
            );
            assert_eq!(
                repeated.media_progress_deadline_boottime_ns,
                initial.media_progress_deadline_boottime_ns
            );
            assert!(player_guard_fast(
                &x.f.engine,
                player_guard_issue(&x, &initial, 3),
                initial.media_progress_deadline_boottime_ns
            )
            .is_err());
            assert_eq!(x.f.engine.handy_player_guard_count_for_test(), 0);
        } else {
            assert_eq!(
                observed.media_progress_deadline_boottime_ns,
                issue_time + 100_000_000
            );
            assert!(player_guard_fast(
                &x.f.engine,
                player_guard_issue(&x, &initial, 2),
                initial.media_progress_deadline_boottime_ns
            )
            .is_ok());
        }
        player_guard_finish(&x, server);
    }
}
