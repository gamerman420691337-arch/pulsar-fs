// All media, qualification labels, device observations and clocks here are
// authored fixtures. Localhost HTTP proves custody, never physical acceptance.
#[cfg(target_os = "linux")]
mod browser_playback_tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;

    struct BrowserPlayFixture {
        x: HandyPlayFixture,
        timing: BrowserTimingBinding,
        tick: Arc<AtomicU64>,
        dossier: HandyBrowserStartQualificationReport,
    }
    fn prepare_controller(
        f: &Fixture,
        program: &HandyProgramSelection,
        id: &str,
        now: u64,
    ) -> HandyControllerLease {
        let mut prepare = request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::PrepareHandyPlayback {
                program: program.clone(),
                allow_online_inspection: true,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        prepare.request_id = RequestId::new(format!("{id}-prepare")).unwrap();
        let prep = playback_preparation_report(playback_preparation_call(f, &prepare).unwrap());
        let mut acquire = request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::AcquireHandyController {
                preparation_request_id: prep.request_id,
                preparation_id: prep.preparation_id,
                duration_ms: 1000,
                allow_arm: true,
                allow_online_inspection: true,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        acquire.request_id = RequestId::new(id).unwrap();
        controller_report(controller_call(f, &acquire, now).unwrap()).lease
    }
    fn fixture_browser_play() -> BrowserPlayFixture {
        let (f, media, dossier, setup) =
            browser_start_qualification_tests::browser_execution_fixture();
        let history = setup_status_report(
            setup_send_call(
                &f,
                &setup,
                |h, _| Ok(h.downloaded.clone()),
                |_, _| Ok(setup_sample()),
                |_, _, _, _| {
                    crate::device_effect_journal::SetupCompletion::Acknowledged(
                        crate::handy_wire::fixture_setup_acknowledgment(),
                    )
                },
            )
            .unwrap(),
        )
        .history
        .unwrap();
        let program = HandyProgramSelection {
            setup_request_id: setup.request_id,
            record_id: history.record_id,
            qualification: history.qualification,
            engine_epoch: history.engine_epoch,
            device_session: history.device_session,
            hosting_request_id: history.hosting_request_id,
        };
        let old = prepare_controller(&f, &program, "browser-sync-owner", 1_000_000_000);
        let mut sync = request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::SynchronizeHandyClock {
                acquisition_request_id: old.acquisition_request_id,
                controller_id: old.controller_id,
                expected_generation: old.generation,
                allow_online_synchronization: true,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        sync.request_id = RequestId::new("browser-original-sync").unwrap();
        let h = sync_report(sync_call(&f, &sync, |_, _, clock| sync_ack(clock)).unwrap())
            .history
            .unwrap();
        let lease = prepare_controller(&f, &program, "browser-playing-owner", 1_000_000_010);
        let mut clock = request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::AcquireHandyPlaybackClock {
                acquisition_request_id: lease.acquisition_request_id.clone(),
                controller_id: lease.controller_id.clone(),
                expected_generation: lease.generation,
                synchronization: HandyClockSyncBinding {
                    sync_request_id: sync.request_id,
                    record_id: h.record_id,
                },
                allow_online_timing: true,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        clock.request_id = RequestId::new("browser-original-clock").unwrap();
        assert!(
            playback_clock_report(playback_clock_call(&f, &clock, 1_000_000_020).unwrap()).current
        );
        f.engine.seed_browser_timing_clock_for_test(1_090_000_000);
        let open = request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::OpenBrowserTiming {
                player: HandyPlayerIdentity {
                    launch_id: AttemptId::new("browser-launch").unwrap(),
                    page_id: AttemptId::new("browser-page").unwrap(),
                    player_id: AttemptId::new("browser-player").unwrap(),
                    source_id: AttemptId::new("browser-source").unwrap(),
                    generation: 1,
                    time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
                },
                browser_media: media,
                policy: BrowserTimingPolicy {
                    probe_timeout_ms: 100,
                    observation_timeout_ms: 500,
                },
            },
        );
        let ResponseBody::BrowserTiming(opened) = f.engine.handle(open).result.unwrap() else {
            panic!("timing open")
        };
        let timing = opened.binding;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let (runtime, pending) = f.engine.bind_handy_play_transport_for_test(
            &lease.preparation_id,
            format!("http://{}", listener.local_addr().unwrap()),
        );
        let mut start = request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::StartHandyPlaybackFromBrowser {
                acquisition_request_id: lease.acquisition_request_id.clone(),
                controller_id: lease.controller_id.clone(),
                expected_generation: lease.generation,
                clock_request_id: clock.request_id,
                browser_timing_binding_id: timing.binding_id.clone(),
                qualification: HandyBrowserStartQualificationBinding {
                    dossier_id: dossier.dossier_id.clone(),
                    revision: dossier.revision,
                },
                acknowledge_current_environment: true,
                allow_play: true,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        start.request_id = RequestId::new("browser-original-play").unwrap();
        let value = BrowserPlayFixture {
            x: HandyPlayFixture {
                f,
                lease,
                start,
                runtime,
                pending,
                listener,
            },
            timing,
            tick: Arc::new(AtomicU64::new(1_100_000_000)),
            dossier,
        };
        observe(&value, 1, 1_100_000_000, true);
        value
    }
    fn observe(b: &BrowserPlayFixture, sequence: u64, now: u64, playing: bool) {
        let f = &b.x.f;
        f.engine.set_browser_timing_clock_for_test(now - 1_000_000);
        let ResponseBody::BrowserTimingProbe(probe) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::IssueBrowserTimingProbe {
                    binding_id: b.timing.binding_id.clone(),
                    player: b.timing.player.clone(),
                    sequence,
                },
            ))
            .result
            .unwrap()
        else {
            panic!("probe")
        };
        f.engine.set_browser_timing_clock_for_test(now);
        let us = (now - 1_000_000_000) / 1000;
        let body = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ObserveBrowserTiming {
                    binding_id: b.timing.binding_id.clone(),
                    player: b.timing.player.clone(),
                    sequence,
                    probe_id: probe.probe_id,
                    frame: BrowserTimingFrame {
                        sample: HandyPlayerSample {
                            browser_tick_us: us,
                            media_time_us: 5_000_000 + us,
                            presented_frames: sequence,
                            playing,
                            seeking: false,
                            ended: false,
                            visible: true,
                            playback_rate_milli: 1000,
                        },
                        callback_tick_us: us,
                        presentation_time_us: Some(us),
                        expected_display_time_us: Some(us),
                    },
                },
            ))
            .result
            .unwrap();
        let ResponseBody::BrowserTiming(report) = body else {
            panic!("observation")
        };
        assert_eq!(report.current, playing);
        b.tick.store(now, Ordering::Release);
    }
    fn reserve(b: &BrowserPlayFixture) {
        b.x.f
            .engine
            .reserve_handy_browser_start_owner_with(&b.x.start, || {
                Ok(b.tick.load(Ordering::Acquire))
            })
            .unwrap();
    }
    fn start(b: &BrowserPlayFixture) -> PResult<HandyPlaybackStatus> {
        let tick = b.tick.clone();
        handy_play_call(&b.x, Arc::new(move || Ok(tick.load(Ordering::Acquire))))
    }
    fn owner_request(b: &BrowserPlayFixture, release: bool) -> Request {
        request(
            &b.x.f.session,
            Some(&b.x.f.project),
            None,
            if release {
                Command::ReleaseHandyBrowserPlayback {
                    play_request_id: b.x.start.request_id.clone(),
                    play_record_id: None,
                    browser_timing_binding_id: b.timing.binding_id.clone(),
                    owner_proof: b.x.f.engine.handy_owner_token.clone(),
                }
            } else {
                Command::RenewHandyBrowserPlayback {
                    play_request_id: b.x.start.request_id.clone(),
                    play_record_id: None,
                    browser_timing_binding_id: b.timing.binding_id.clone(),
                    owner_proof: b.x.f.engine.handy_owner_token.clone(),
                }
            },
        )
    }
    fn owner(b: &BrowserPlayFixture, release: bool) -> PResult<HandyBrowserPlaybackReport> {
        let ResponseBody::HandyBrowserPlayback(value) =
            b.x.f
                .engine
                .handy_browser_owner_with(&owner_request(b, release), || {
                    Ok(b.tick.load(Ordering::Acquire))
                })?
        else {
            panic!("owner")
        };
        value.validate().unwrap();
        Ok(*value)
    }
    fn wait_ack(b: &BrowserPlayFixture) {
        handy_execution_wait(|| {
            handy_play_status(&b.x)
                .history
                .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
        });
    }
    fn finish(b: &BrowserPlayFixture, server: ContinuousServer) -> Vec<(String, Vec<u8>)> {
        let released = owner(b, true).unwrap();
        assert!(!released.current);
        assert!(
            released.play_record_id.is_some()
                && released.alignment.is_some()
                && released.browser_observation_id.is_some()
        );
        let requests = server.join().unwrap();
        handy_execution_wait(|| !b.x.pending.load(Ordering::Acquire));
        assert!(b
            .x
            .runtime
            .lock()
            .unwrap()
            .session()
            .physical_stop_pending());
        requests
    }
    fn refresh(
        b: &BrowserPlayFixture,
        id: &str,
        now: u64,
    ) -> PResult<HandyContinuousTimingObservation> {
        let mut r = request(
            &b.x.f.session,
            Some(&b.x.f.project),
            None,
            Command::RefreshHandyBrowserPlaybackTiming {
                play_request_id: b.x.start.request_id.clone(),
                play_record_id: None,
                browser_timing_binding_id: b.timing.binding_id.clone(),
                allow_online_inspection: true,
                owner_proof: b.x.f.engine.handy_owner_token.clone(),
            },
        );
        r.request_id = RequestId::new(id).unwrap();
        let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
        b.x.f
            .engine
            .handy_browser_timing_with(
                &r,
                &b.x.f.session,
                &fingerprint,
                (
                    |h, _| Ok(h.downloaded.clone()),
                    |_, _, clock| continuous_device_read(clock),
                    continuous_server_sample,
                ),
                || Ok(now),
            )
            .map(continuous_report)
    }

    #[test]
    fn browser_play_release_before_arrival_is_db_independent_and_prevents_late_effect() {
        let b = fixture_browser_play();
        {
            let _held = b.x.f.engine.db.lock().unwrap();
            let report = owner(&b, true).unwrap();
            assert!(!report.current && report.play_record_id.is_none());
        }
        reserve(&b);
        assert_eq!(start(&b).unwrap_err().code, ErrorCode::Cancelled);
        assert!(b.x.listener.accept().is_err());
        assert!(b
            .x
            .f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .play_request_history(&b.x.f.session, &b.x.start.request_id)
            .unwrap()
            .is_none());
        assert_eq!(
            b.x.runtime.lock().unwrap().session().state(),
            DeviceState::Armed
        );
    }

    #[test]
    fn browser_play_queued_owner_renewal_and_new_sample_preserve_original_wire_pair_and_replay() {
        let b = fixture_browser_play();
        reserve(&b);
        {
            let _held = b.x.f.engine.db.lock().unwrap();
            let report = owner(&b, false).unwrap();
            assert!(report.current);
            assert!(report.play_record_id.is_none());
            assert_eq!(report.controller.unwrap().generation, 1);
        }
        let (entered, wait) = std::sync::mpsc::sync_channel(1);
        let gate = Arc::new(std::sync::Barrier::new(2));
        let blocked = gate.clone();
        let first = AtomicBool::new(true);
        let tick = b.tick.clone();
        let clock = Arc::new(move || {
            if std::thread::current().name() == Some("pulsar-handy-dispatch")
                && first.swap(false, Ordering::AcqRel)
            {
                entered.send(()).unwrap();
                blocked.wait();
            }
            Ok(tick.load(Ordering::Acquire))
        });
        let server = handy_play_server(b.x.listener.try_clone().unwrap(), true);
        let original = handy_play_call(&b.x, clock).unwrap();
        wait.recv_timeout(Duration::from_secs(3)).unwrap();
        let record =
            b.x.f
                .engine
                ._device_effect_journal
                .lock()
                .unwrap()
                .play_request_history(&b.x.f.session, &b.x.start.request_id)
                .unwrap()
                .unwrap()
                .record;
        let encoded = serde_json::to_vec(&record).unwrap();
        observe(&b, 2, 1_110_000_000, true);
        assert_eq!(owner(&b, false).unwrap().controller.unwrap().generation, 2);
        gate.wait();
        wait_ack(&b);
        let replay = handy_play_call(&b.x, Arc::new(|| panic!("replay must not sample"))).unwrap();
        assert_eq!(
            original.history.unwrap().record_id,
            replay.history.unwrap().record_id
        );
        assert_eq!(
            serde_json::to_vec(
                &b.x.f
                    .engine
                    ._device_effect_journal
                    .lock()
                    .unwrap()
                    .play_request_history(&b.x.f.session, &b.x.start.request_id)
                    .unwrap()
                    .unwrap()
                    .record
            )
            .unwrap(),
            encoded
        );
        let requests = finish(&b, server);
        let wire: serde_json::Value = serde_json::from_slice(&requests[0].1).unwrap();
        assert_eq!(wire["start_time"], record.start_time_ms);
        assert_eq!(wire["server_time"], record.clock.estimated_server_ms);
        assert_eq!(requests.len(), 2);
    }

    #[test]
    fn browser_play_fresh_alignment_extends_past_original_window_without_changing_originals() {
        let b = fixture_browser_play();
        reserve(&b);
        let server = handy_play_server(b.x.listener.try_clone().unwrap(), true);
        start(&b).unwrap();
        wait_ack(&b);
        let original =
            b.x.f
                .engine
                ._device_effect_journal
                .lock()
                .unwrap()
                .play_request_history(&b.x.f.session, &b.x.start.request_id)
                .unwrap()
                .unwrap()
                .record;
        observe(&b, 2, 1_400_000_000, true);
        let first = refresh(&b, "browser-refresh-1", 1_400_000_000).unwrap();
        assert!(first.current);
        observe(&b, 3, 1_600_000_000, true);
        let current = owner(&b, false).unwrap();
        assert!(current.current);
        assert!(1_600_000_000 > original.clock.valid_until_ns);
        assert!(
            !b.x.f
                .engine
                .continuous_lineage_for_test(&b.x.lease.preparation_id, 1_600_000_000)
                .unwrap()
                .1
        );
        let second = refresh(&b, "browser-refresh-2", 1_600_000_000).unwrap();
        assert!(second.current);
        let repeated = refresh(&b, "browser-refresh-1", 1_600_000_000).unwrap();
        assert_eq!(
            repeated.observed_at_boottime_ns,
            first.observed_at_boottime_ns
        );
        assert_eq!(
            repeated.valid_until_boottime_ns,
            first.valid_until_boottime_ns
        );
        assert!(!repeated.current);
        assert_eq!(
            serde_json::to_vec(&original).unwrap(),
            serde_json::to_vec(
                &b.x.f
                    .engine
                    ._device_effect_journal
                    .lock()
                    .unwrap()
                    .play_request_history(&b.x.f.session, &b.x.start.request_id)
                    .unwrap()
                    .unwrap()
                    .record
            )
            .unwrap()
        );
        finish(&b, server);
    }

    #[test]
    fn browser_play_new_browser_samples_alone_cannot_extend_initial_device_or_decision_expiry() {
        let b = fixture_browser_play();
        reserve(&b);
        let server = handy_play_server(b.x.listener.try_clone().unwrap(), true);
        start(&b).unwrap();
        wait_ack(&b);
        observe(&b, 2, 1_400_000_000, true);
        assert!(owner(&b, false).unwrap().current);
        observe(&b, 3, 1_600_000_000, true);
        assert!(owner(&b, false).is_err());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        handy_execution_wait(|| !b.x.pending.load(Ordering::Acquire));
        assert!(b
            .x
            .runtime
            .lock()
            .unwrap()
            .session()
            .physical_stop_pending());
        let replay =
            handy_play_call(&b.x, Arc::new(|| panic!("expired replay must not sample"))).unwrap();
        assert_eq!(
            replay.history.unwrap().state,
            HandyPlaybackOperationState::Acknowledged
        );
        assert!(!owner(&b, true).unwrap().current);
    }

    #[test]
    fn browser_play_pause_before_claim_prevents_play_and_still_owns_stop() {
        let b = fixture_browser_play();
        reserve(&b);
        let (entered, wait) = std::sync::mpsc::sync_channel(1);
        let gate = Arc::new(std::sync::Barrier::new(2));
        let blocked = gate.clone();
        let first = AtomicBool::new(true);
        let tick = b.tick.clone();
        let clock = Arc::new(move || {
            if std::thread::current().name() == Some("pulsar-handy-dispatch")
                && first.swap(false, Ordering::AcqRel)
            {
                entered.send(()).unwrap();
                blocked.wait();
            }
            Ok(tick.load(Ordering::Acquire))
        });
        let (started, received) = std::sync::mpsc::sync_channel(1);
        let (release, stop_gate) = std::sync::mpsc::sync_channel(1);
        let server =
            handy_execution_server(b.x.listener.try_clone().unwrap(), started, stop_gate, true);
        handy_play_call(&b.x, clock).unwrap();
        wait.recv_timeout(Duration::from_secs(3)).unwrap();
        observe(&b, 2, 1_110_000_000, false);
        gate.wait();
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        release.send(()).unwrap();
        server.join().unwrap();
        handy_execution_wait(|| {
            b.x.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed
        });
        // A rejected already-claimed play retains unresolved original metadata;
        // the successful stop response cannot discard that custody.
        assert!(b.x.pending.load(Ordering::Acquire));
        assert!(b.x.listener.accept().is_err());
        assert_eq!(
            handy_play_status(&b.x).history.unwrap().state,
            HandyPlaybackOperationState::Claimed
        );
    }

    #[test]
    fn browser_play_release_while_ack_and_db_are_held_preserves_unknown_and_stop_progress() {
        let b = fixture_browser_play();
        reserve(&b);
        let listener = b.x.listener.try_clone().unwrap();
        let (sent, received) = std::sync::mpsc::sync_channel(1);
        let (stop_sent, stop_received) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let mut play = handy_play_accept(&listener);
            let first = handy_play_read_request(&mut play);
            assert_eq!(first.0, "PUT /hssp/play HTTP/1.1");
            sent.send(()).unwrap();
            let mut stop = handy_play_accept(&listener);
            let second = handy_play_read_request(&mut stop);
            assert_eq!(second.0, "PUT /hssp/stop HTTP/1.1");
            handy_play_reply(&mut stop, 2);
            stop_sent.send(()).unwrap();
            handy_play_reply(&mut play, 1);
            vec![first, second]
        });
        start(&b).unwrap();
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        {
            let _held = b.x.f.engine.db.lock().unwrap();
            assert!(owner(&b, false).unwrap().current);
            assert!(!owner(&b, true).unwrap().current);
            stop_received.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        assert_eq!(server.join().unwrap().len(), 2);
        handy_execution_wait(|| !b.x.pending.load(Ordering::Acquire));
        assert!(b
            .x
            .runtime
            .lock()
            .unwrap()
            .session()
            .physical_stop_pending());
        assert_ne!(
            b.x.runtime.lock().unwrap().session().state(),
            DeviceState::Playing
        );
        assert!(!owner(&b, true).unwrap().current);
    }

    #[test]
    fn browser_play_dossier_withdrawal_retires_cached_owner_and_cannot_be_replayed_live() {
        let b = fixture_browser_play();
        reserve(&b);
        let server = handy_play_server(b.x.listener.try_clone().unwrap(), true);
        start(&b).unwrap();
        wait_ack(&b);
        b.x.f
            .engine
            .handle(request(
                &b.x.f.session,
                Some(&b.x.f.project),
                None,
                Command::WithdrawHandyBrowserStartQualification {
                    dossier_id: b.dossier.dossier_id.clone(),
                    expected_record_revision: b.dossier.revision,
                    reason: "synthetic applicability withdrawal".into(),
                    owner_proof: b.x.f.engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(owner(&b, false).is_err());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        handy_execution_wait(|| !b.x.pending.load(Ordering::Acquire));
        assert!(handy_play_call(
            &b.x,
            Arc::new(|| panic!("withdrawn replay must not sample"))
        )
        .unwrap()
        .history
        .is_some());
        assert!(!owner(&b, true).unwrap().current);
    }
}
