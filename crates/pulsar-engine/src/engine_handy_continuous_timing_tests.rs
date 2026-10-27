fn continuous_fixture() -> HandyPlayFixture {
    let f = fixture();
    let (mut sync, old) = server_clock_inputs_with_subject(&f, |s| {
        // Authored qualification facts exercise conditional software behavior;
        // they are not physical device measurements or acceptance.
        s.schema_version = 6;
        s.profile.maximum_command_age_ns = 1_000_000_000;
        s.device_clock = Some(HandyDeviceClockQualification {
            model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
            user_offset_ms: -50,
            device_error_ns: 2_000_000,
            maximum_sync_age_ns: 500_000_000,
            maximum_sync_round_trip_ms: 100,
            drift_ppm: 20,
        });
        s.recovery = Some(HandyRecoveryQualification {
            transport: HandyQualificationTransport::Hssp,
            model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
            remote_state: HandyRecoveryRemoteState::Stopped,
            prior_command_settle_ns: 1_000_000,
            stopped_sample_separation_ns: 1_000_000,
            maximum_sample_span_ns: 100_000_000,
        });
        s.continuous_timing = Some(HandyContinuousTimingQualification {
            model: HandyContinuousTimingModel::HsspPlaytimeRequestBracket,
            timestamp_error_ns: 1_000,
            maximum_observation_age_ns: 1_000_000_000,
            maximum_sample_span_ns: 100_000_000,
            drift_ppm: 20,
        });
    });
    sync.request_id = RequestId::new("continuous-original-sync").unwrap();
    sync.command = Command::SynchronizeHandyClock {
        acquisition_request_id: old.acquisition_request_id.clone(),
        controller_id: old.controller_id.clone(),
        expected_generation: old.generation,
        allow_online_synchronization: true,
        owner_proof: f.engine.handy_owner_token.clone(),
    };
    let history = sync_report(sync_call(&f, &sync, |_, _, clock| sync_ack(clock)).unwrap())
        .history
        .unwrap();
    let lease = clock_replacement(&f, &old, "continuous-controller", 1_000_000_010);
    let mut acquire = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::AcquireHandyPlaybackClock {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            controller_id: lease.controller_id.clone(),
            expected_generation: lease.generation,
            synchronization: HandyClockSyncBinding {
                sync_request_id: sync.request_id,
                record_id: history.record_id,
            },
            allow_online_timing: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    acquire.request_id = RequestId::new("continuous-initial-clock").unwrap();
    assert!(
        playback_clock_report(playback_clock_call(&f, &acquire, 1_000_000_020).unwrap()).current
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
            clock_request_id: acquire.request_id,
            start_time_ms: 0,
            allow_play: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    start.request_id = RequestId::new("continuous-original-play").unwrap();
    HandyPlayFixture {
        f,
        lease,
        start,
        runtime,
        pending,
        listener,
    }
}
type ContinuousServer = std::thread::JoinHandle<Vec<(String, Vec<u8>)>>;
fn continuous_started() -> (HandyPlayFixture, ContinuousServer) {
    let x = continuous_fixture();
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    (x, server)
}
fn continuous_request(x: &HandyPlayFixture, id: &str, generation: u64) -> Request {
    let history = handy_play_status(x).history.unwrap();
    let mut r = request(
        &x.f.session,
        Some(&x.f.project),
        Some(1),
        Command::RefreshHandyPlaybackTiming {
            selection: HandyContinuousTimingSelection {
                acquisition_request_id: x.lease.acquisition_request_id.clone(),
                controller_id: x.lease.controller_id.clone(),
                expected_generation: generation,
                play_request_id: x.start.request_id.clone(),
                play_record_id: history.record_id,
            },
            allow_online_inspection: true,
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn continuous_device_read(
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> PResult<crate::handy_online::PlayingTimingRead> {
    let started_boottime_ns = clock()?;
    let mut device = qualified_device_read(clock)?;
    let finished_boottime_ns = clock()?;
    for (sample, now) in [
        (&mut device.before, started_boottime_ns),
        (&mut device.after, finished_boottime_ns),
    ] {
        let hssp = sample.remote_hssp.as_mut().unwrap();
        hssp.play_state = HandyRemotePlayState::Playing;
        hssp.current_time_ms = now.saturating_sub(1_000_000_000) / 1_000_000;
    }
    device.first.info.device_uptime_ms =
        100 + started_boottime_ns.saturating_sub(1_000_000_000) / 1_000_000;
    device.second.info.device_uptime_ms =
        100 + finished_boottime_ns.saturating_sub(1_000_000_000) / 1_000_000;
    Ok(crate::handy_online::PlayingTimingRead {
        device,
        started_boottime_ns,
        finished_boottime_ns,
    })
}
fn continuous_server_sample(
    _: std::time::Instant,
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> PResult<crate::handy_server_clock::Sample> {
    let sent_ns = clock()?;
    let received_ns = clock()?;
    Ok(crate::handy_server_clock::Sample {
        sent_ns,
        received_ns,
        server_ms: 1_000_000 + sent_ns.saturating_sub(1_000_000_000) / 1_000_000,
    })
}
fn continuous_report(body: ResponseBody) -> HandyContinuousTimingObservation {
    let ResponseBody::HandyContinuousTiming(value) = body else {
        panic!("continuous timing response");
    };
    value.validate().unwrap();
    *value
}
fn continuous_call(
    x: &HandyPlayFixture,
    r: &Request,
    now: u64,
) -> PResult<HandyContinuousTimingObservation> {
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    x.f.engine
        .handy_continuous_timing_with(
            r,
            &x.f.session,
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
fn continuous_history(
    x: &HandyPlayFixture,
    original: &RequestId,
    now: u64,
) -> PResult<HandyContinuousTimingObservation> {
    let r = request(
        &x.f.session,
        Some(&x.f.project),
        None,
        Command::HandyContinuousTimingStatus {
            refresh_request_id: original.clone(),
            owner_proof: x.f.engine.handy_owner_token.clone(),
        },
    );
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    x.f.engine
        .handy_continuous_timing_with(
            &r,
            &x.f.session,
            &fingerprint,
            (
                |_, _| panic!("history must not recheck hosting"),
                |_, _, _| panic!("history must not inspect device"),
                |_, _| panic!("history must not sample server"),
            ),
            || Ok(now),
        )
        .map(continuous_report)
}
fn continuous_finish(x: &HandyPlayFixture, server: ContinuousServer) {
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    assert_eq!(server.join().unwrap().len(), 2);
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
    assert!(x.listener.accept().is_err());
}

#[test]
fn handy_continuous_timing_extends_independently_without_rewriting_original_sync_or_play() {
    let (x, server) = continuous_started();
    let original =
        x.f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .play_request_history(&x.f.session, &x.start.request_id)
            .unwrap()
            .unwrap();
    let original_bytes = serde_json::to_vec(&original.record).unwrap();
    let original_sync =
        x.f.engine
            .continuous_lineage_for_test(&x.lease.preparation_id, 1_100_000_000)
            .unwrap()
            .0;
    let r = continuous_request(&x, "continuous-first", 0);
    let first = continuous_call(&x, &r, 1_400_000_000).unwrap();
    assert!(first.current);
    let (sync, old_current) =
        x.f.engine
            .continuous_lineage_for_test(&x.lease.preparation_id, 1_600_000_000)
            .unwrap();
    assert_eq!(sync, original_sync);
    assert!(!old_current);
    assert!(
        continuous_history(&x, &r.request_id, 1_600_000_000)
            .unwrap()
            .current
    );
    assert_eq!(
        x.f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts(),
        (1, 1)
    );
    let renewal = controller_renew_request(&x.f, &x.lease, "continuous-renew");
    let renewed = controller_report(controller_call(&x.f, &renewal, 1_600_000_000).unwrap()).lease;
    assert_eq!(renewed.state, HandyControllerState::Playing);
    let kept = continuous_history(&x, &r.request_id, 1_700_000_000).unwrap();
    assert!(kept.current);
    assert_eq!(kept.observed_at_boottime_ns, first.observed_at_boottime_ns);
    assert_eq!(kept.valid_until_boottime_ns, first.valid_until_boottime_ns);
    assert_eq!(kept.controller.generation, 0);
    let next = continuous_request(&x, "continuous-next", renewed.generation);
    let second = continuous_call(&x, &next, 1_800_000_000).unwrap();
    assert!(second.current && second.valid_until_boottime_ns > first.valid_until_boottime_ns);
    assert_eq!(second.reference, first.reference);
    let after =
        x.f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .play_request_history(&x.f.session, &x.start.request_id)
            .unwrap()
            .unwrap();
    assert_eq!(serde_json::to_vec(&after.record).unwrap(), original_bytes);
    assert_eq!(
        after.state,
        crate::device_effect_journal::PlayOperationState::Acknowledged
    );
    continuous_finish(&x, server);
}

#[test]
fn handy_continuous_timing_replay_never_resamples_and_expired_history_cannot_revive_play() {
    let (x, server) = continuous_started();
    let r = continuous_request(&x, "continuous-replay", 0);
    let first = continuous_call(&x, &r, 1_400_000_000).unwrap();
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    for (now, current) in [(1_600_000_000, true), (2_000_000_011, false)] {
        let replay = continuous_report(
            x.f.engine
                .handy_continuous_timing_with(
                    &r,
                    &x.f.session,
                    &fingerprint,
                    (
                        |_, _| panic!("replay hosted read"),
                        |_, _, _| panic!("replay device read"),
                        |_, _| panic!("replay server read"),
                    ),
                    || Ok(now),
                )
                .unwrap(),
        );
        assert_eq!(replay.current, current);
        assert_eq!(
            replay.observed_at_boottime_ns,
            first.observed_at_boottime_ns
        );
        assert_eq!(
            replay.valid_until_boottime_ns,
            first.valid_until_boottime_ns
        );
        assert_eq!(replay.reference, first.reference);
    }
    let fresh = continuous_request(&x, "continuous-too-late", 0);
    assert!(continuous_call(&x, &fresh, 2_000_000_012).is_err());
    continuous_finish(&x, server);
}

#[test]
fn handy_continuous_timing_cannot_bridge_initial_sync_expiry_or_restore_removed_lineage() {
    for install in [false, true] {
        let (x, server) = continuous_started();
        let r = continuous_request(&x, "continuous-lineage", 0);
        if install {
            assert!(continuous_call(&x, &r, 1_400_000_000).unwrap().current);
            x.f.engine
                .remove_continuous_lineage_for_test(&x.lease.preparation_id);
            assert!(
                !continuous_history(&x, &r.request_id, 1_400_000_001)
                    .unwrap()
                    .current
            );
        }
        let denied = continuous_request(&x, "continuous-expired", 0);
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(&denied).unwrap())
        );
        assert!(x
            .f
            .engine
            .handy_continuous_timing_with(
                &denied,
                &x.f.session,
                &fingerprint,
                (
                    |_, _| panic!("expired hosting"),
                    |_, _, _| panic!("expired device"),
                    |_, _| panic!("expired server")
                ),
                || Ok(1_500_000_001),
            )
            .is_err());
        continuous_finish(&x, server);
    }
}

#[test]
fn handy_continuous_timing_renewal_during_read_preserves_installed_observation_bounds() {
    let (x, server) = continuous_started();
    let first_request = continuous_request(&x, "continuous-before-renewal", 0);
    let first = continuous_call(&x, &first_request, 1_100_000_000).unwrap();
    let r = continuous_request(&x, "continuous-renewal-race", 0);
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    let tick = std::sync::atomic::AtomicU64::new(1_200_000_000);
    let renewed = std::cell::Cell::new(false);
    assert!(x
        .f
        .engine
        .handy_continuous_timing_with(
            &r,
            &x.f.session,
            &fingerprint,
            (
                |h, _| Ok(h.downloaded.clone()),
                |_, _, clock| {
                    assert!(x.f.engine.db.try_lock().is_ok());
                    assert!(x.f.engine.handy_preparations.try_lock().is_ok());
                    tick.store(1_200_000_001, Ordering::Release);
                    let renewal =
                        controller_renew_request(&x.f, &x.lease, "renew-during-continuous-read");
                    assert_eq!(
                        controller_report(controller_call(&x.f, &renewal, 1_200_000_001).unwrap())
                            .lease
                            .generation,
                        1
                    );
                    renewed.set(true);
                    continuous_device_read(clock)
                },
                continuous_server_sample
            ),
            || Ok(tick.load(Ordering::Acquire)),
        )
        .is_err());
    assert!(renewed.get());
    let kept = continuous_history(&x, &first_request.request_id, 1_200_000_001).unwrap();
    assert!(kept.current);
    assert_eq!(kept.observed_at_boottime_ns, first.observed_at_boottime_ns);
    assert_eq!(kept.valid_until_boottime_ns, first.valid_until_boottime_ns);
    assert!(continuous_history(&x, &r.request_id, 1_200_000_001).is_err());
    continuous_finish(&x, server);
}

#[test]
fn handy_continuous_timing_older_attempt_cannot_retire_newer_installed_observation() {
    let (x, server) = continuous_started();
    let first = continuous_request(&x, "continuous-baseline", 0);
    continuous_call(&x, &first, 1_100_000_000).unwrap();
    let old = continuous_request(&x, "continuous-older-attempt", 0);
    let new = continuous_request(&x, "continuous-newer-attempt", 0);
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&old).unwrap()));
    let tick = std::sync::atomic::AtomicU64::new(1_200_000_000);
    assert!(x
        .f
        .engine
        .handy_continuous_timing_with(
            &old,
            &x.f.session,
            &fingerprint,
            (
                |h, _| Ok(h.downloaded.clone()),
                |_, _, clock| {
                    tick.store(1_200_000_001, Ordering::Release);
                    assert!(continuous_call(&x, &new, 1_200_000_001).unwrap().current);
                    continuous_device_read(clock)
                },
                continuous_server_sample
            ),
            || Ok(tick.load(Ordering::Acquire)),
        )
        .is_err());
    let installed = continuous_history(&x, &new.request_id, 1_300_000_000).unwrap();
    assert!(installed.current);
    assert_eq!(installed.sequence, 3);
    assert!(
        !continuous_history(&x, &first.request_id, 1_300_000_000)
            .unwrap()
            .current
    );
    assert!(continuous_history(&x, &old.request_id, 1_300_000_000).is_err());
    continuous_finish(&x, server);
}

#[test]
fn handy_continuous_timing_receipt_failure_installs_no_native_authority() {
    let (x, server) = continuous_started();
    let r = continuous_request(&x, "continuous-receipt-failure", 0);
    x.f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER continuous_receipt_failure BEFORE INSERT ON requests WHEN NEW.request='continuous-receipt-failure' BEGIN SELECT RAISE(ABORT,'synthetic continuous receipt failure'); END;").unwrap();
    assert!(continuous_call(&x, &r, 1_100_000_000).is_err());
    assert_eq!(
        continuous_history(&x, &r.request_id, 1_100_000_001)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        handy_play_status(&x).history.unwrap().state,
        HandyPlaybackOperationState::Acknowledged
    );
    continuous_finish(&x, server);
}

#[test]
fn handy_continuous_timing_stop_progresses_while_refresh_read_is_stalled() {
    let (x, server) = continuous_started();
    let r = continuous_request(&x, "continuous-stalled-read", 0);
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    let (entered, started) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let engine = x.f.engine.clone();
        let actor = x.f.session.clone();
        let handle = scope.spawn(move || {
            engine.handy_continuous_timing_with(
                &r,
                &actor,
                &fingerprint,
                (
                    |h, _| Ok(h.downloaded.clone()),
                    |_, _, clock| {
                        assert!(engine.db.try_lock().is_ok());
                        assert!(engine.handy_preparations.try_lock().is_ok());
                        assert!(engine._device_effect_journal.try_lock().is_ok());
                        entered.send(()).unwrap();
                        gate.recv_timeout(std::time::Duration::from_secs(5))
                            .unwrap();
                        continuous_device_read(clock)
                    },
                    continuous_server_sample,
                ),
                || Ok(1_100_000_000),
            )
        });
        started
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        x.f.engine
            .retire_handy_execution_for_test(&x.lease.preparation_id);
        handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
        assert_eq!(
            x.runtime.lock().unwrap().session().state(),
            DeviceState::StopUnconfirmed
        );
        release.send(()).unwrap();
        assert!(handle.join().unwrap().is_err());
    });
    assert_eq!(server.join().unwrap().len(), 2);
    assert!(x.runtime.lock().unwrap().session().physical_stop_pending());
}

#[test]
fn handy_continuous_timing_refuses_claimed_and_unknown_original_play_before_any_read() {
    for acknowledge in [true, false] {
        let x = continuous_fixture();
        let listener = x.listener.try_clone().unwrap();
        let (entered, started) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let mut stream = handy_play_accept(&listener);
            let play = handy_play_read_request(&mut stream);
            assert_eq!(play.0, "PUT /hssp/play HTTP/1.1");
            entered.send(()).unwrap();
            gate.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            if acknowledge {
                handy_play_reply(&mut stream, 1);
            }
            drop(stream);
            let mut stream = handy_play_accept(&listener);
            let stop = handy_play_read_request(&mut stream);
            assert_eq!(stop.0, "PUT /hssp/stop HTTP/1.1");
            handy_play_reply(&mut stream, 2);
            vec![play, stop]
        });
        handy_play_call(&x, Arc::new(|| Ok(1_000_000_030))).unwrap();
        started
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            handy_play_status(&x).history.unwrap().state,
            HandyPlaybackOperationState::Claimed
        );
        let r = continuous_request(&x, "continuous-claimed-refusal", 0);
        let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
        assert!(x
            .f
            .engine
            .handy_continuous_timing_with(
                &r,
                &x.f.session,
                &fingerprint,
                (
                    |_, _| panic!("claimed hosting read"),
                    |_, _, _| panic!("claimed device read"),
                    |_, _| panic!("claimed server read")
                ),
                || Ok(1_100_000_000),
            )
            .is_err());
        assert_eq!(
            x.runtime.lock().unwrap().session().state(),
            DeviceState::Playing
        );
        release.send(()).unwrap();
        handy_execution_wait(|| {
            handy_play_status(&x).history.is_some_and(|h| {
                h.state
                    == if acknowledge {
                        HandyPlaybackOperationState::Acknowledged
                    } else {
                        HandyPlaybackOperationState::Unknown
                    }
            })
        });
        let after = continuous_request(&x, "continuous-after-initial-completion", 0);
        assert_eq!(
            continuous_call(&x, &after, 1_200_000_000).is_ok(),
            acknowledge
        );
        continuous_finish(&x, server);
    }
}

#[test]
fn handy_continuous_timing_changed_original_hstp_or_stream_context_retires_exact_target() {
    for case in 0..5 {
        let (x, server) = continuous_started();
        let r = continuous_request(&x, "continuous-context-change", 0);
        let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
        assert!(x
            .f
            .engine
            .handy_continuous_timing_with(
                &r,
                &x.f.session,
                &fingerprint,
                (
                    |h, _| Ok(h.downloaded.clone()),
                    |_, _, clock| {
                        let mut value = continuous_device_read(clock)?;
                        match case {
                            0 => {
                                value.device.first.info.reported_clock_offset_ms += 1;
                                value.device.second.info.reported_clock_offset_ms += 1;
                            }
                            1 => {
                                value.device.first.user_offset.offset_ms += 1;
                                value.device.second.user_offset.offset_ms += 1;
                            }
                            2 => {
                                value.device.first.info.device_uptime_ms = 99;
                                value.device.second.info.device_uptime_ms = 99;
                            }
                            3 => {
                                value.device.before.remote_hssp.as_mut().unwrap().stream_id += 1;
                                value.device.after.remote_hssp.as_mut().unwrap().stream_id += 1;
                            }
                            _ => {
                                value
                                    .device
                                    .before
                                    .remote_hssp
                                    .as_mut()
                                    .unwrap()
                                    .pause_on_starving = true;
                                value
                                    .device
                                    .after
                                    .remote_hssp
                                    .as_mut()
                                    .unwrap()
                                    .pause_on_starving = true;
                            }
                        }
                        Ok(value)
                    },
                    continuous_server_sample
                ),
                || Ok(1_100_000_000),
            )
            .is_err());
        assert_eq!(
            continuous_history(&x, &r.request_id, 1_100_000_001)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            handy_play_status(&x).history.unwrap().state,
            HandyPlaybackOperationState::Acknowledged
        );
        continuous_finish(&x, server);
    }
}
