// Included in handy_hsp_dispatch::tests so all fixtures remain localhost-only.
fn issued_stop(rig: &Rig) -> HspEffect {
    let mut runtime = rig.runtime.lock().unwrap();
    runtime.stop();
    runtime.next_effect(100, None).unwrap().unwrap()
}
fn emergency_request(rig: &Rig, effect: HspEffect) -> HspEmergencyStopRequest {
    rig.dispatcher
        .prepare_emergency_stop(effect, &rig.owner)
        .unwrap()
}
fn emergency_start(rig: &mut Rig, request: HspEmergencyStopRequest) {
    assert!(rig
        .dispatcher
        .start_emergency_stop(request, Instant::now() + Duration::from_secs(2),)
        .is_ok());
}
fn emergency_wait(rig: &mut Rig) -> HspEmergencyStopCompletion {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(completion) = rig.dispatcher.poll_any() {
            let HspJoinedCompletion::Emergency(completion) = completion else {
                panic!("expected emergency completion");
            };
            return *completion;
        }
        assert!(
            Instant::now() < deadline,
            "bounded emergency worker did not join"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn second_dispatcher(rig: &Rig) -> HandyHspDispatcher {
    HandyHspDispatcher {
        runtime: rig.runtime.clone(),
        journal: rig.journal.clone(),
        transport: rig.dispatcher.transport.clone(),
        clock: rig.dispatcher.clock.clone(),
        active: None,
    }
}
fn wait_for_puts(rig: &mut Rig, count: usize, phase: &str) {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2);
    loop {
        let observed = rig
            .peer
            .state
            .try_lock()
            .ok()
            .map(|peer| peer.requests.iter().filter(|r| r.0 == "PUT").count());
        if observed.is_some_and(|observed| observed >= count) {
            return;
        }
        let finished = rig
            .dispatcher
            .active
            .as_ref()
            .is_some_and(Active::is_finished);
        let terminal_without_put = if finished {
            // The worker may have completed between the first peer snapshot and
            // is_finished. Resample after completion before reporting rejection.
            match rig.peer.state.try_lock() {
                Ok(peer) if peer.requests.iter().filter(|r| r.0 == "PUT").count() >= count => {
                    return;
                }
                Ok(_) => true,
                Err(_) => false,
            }
        } else {
            false
        };
        if Instant::now() >= deadline || terminal_without_put {
            panic!(
                "localhost write was not observed: {}",
                hsp_wait_diagnostics(rig, phase, count, started.elapsed()),
            );
        }
        thread::sleep(Duration::from_millis(2));
    }
}

// Failure-only bounded diagnostics: neither credential headers, request bodies,
// native identity strings nor arbitrary peer-controlled text are formatted.
fn hsp_wait_diagnostics(rig: &mut Rig, phase: &str, expected: usize, elapsed: Duration) -> String {
    let active = match rig.dispatcher.active.as_ref() {
        Some(Active::Durable(active)) => {
            let custody = match active.custody.try_lock() {
                Ok(custody) => format!(
                    "attempted={},receipt={}",
                    custody.attempted,
                    custody.claim.is_some()
                ),
                Err(_) => "unavailable".into(),
            };
            format!(
                "durable(finished={},cancelled={},custody={custody})",
                active.worker.is_finished(),
                active.cancel.load(Ordering::Acquire)
            )
        }
        Some(Active::Emergency(active)) => format!(
            "emergency(finished={},cancelled={})",
            active.worker.is_finished(),
            active.cancel.load(Ordering::Acquire)
        ),
        Some(Active::PostSetupProbe(active)) => {
            format!("probe(finished={})", active.worker.is_finished())
        }
        None => "none".into(),
    };
    // poll_any only joins an already finished worker; this diagnostic never
    // waits for transport or observes/completes its runtime operation.
    let joined = match rig.dispatcher.poll_any() {
        Some(HspJoinedCompletion::Durable(completion)) => format!(
            "durable(receipt={},attempt={})",
            completion.claim.is_some(),
            hsp_attempt_diagnostic(&completion.attempt)
        ),
        Some(HspJoinedCompletion::Emergency(completion)) => format!(
            "emergency(attempt={})",
            hsp_attempt_diagnostic(completion.attempt())
        ),
        Some(HspJoinedCompletion::PostSetupProbe(_)) => "probe".into(),
        None => "none".into(),
    };
    let peer = match rig.peer.state.try_lock() {
        Ok(peer) => {
            let paths: Vec<_> = peer
                .requests
                .iter()
                .take(16)
                .map(|request| match (request.0.as_str(), request.1.as_str()) {
                    ("GET", "/info") => "GET /info",
                    ("GET", "/mode") => "GET /mode",
                    ("GET", "/hsp/state") => "GET /hsp/state",
                    ("PUT", "/hsp/setup") => "PUT /hsp/setup",
                    ("PUT", "/hsp/add") => "PUT /hsp/add",
                    ("PUT", "/hsp/play") => "PUT /hsp/play",
                    ("PUT", "/hsp/stop") => "PUT /hsp/stop",
                    _ => "unexpected method/path withheld",
                })
                .collect();
            format!(
                "requests={},puts={},first16={paths:?}",
                peer.requests.len(),
                peer.requests.iter().filter(|r| r.0 == "PUT").count()
            )
        }
        Err(_) => "state unavailable".into(),
    };
    let runtime = match rig.runtime.try_lock() {
        Ok(runtime) => format!(
            "state={:?},effect={:?},native_reservation={:?}",
            runtime.state(),
            runtime.in_flight().map(|e| (e.kind(), e.sequence())),
            runtime.require_native_released()
        ),
        Err(_) => "unavailable".into(),
    };
    format!("phase={phase},expected_puts={expected},elapsed_ms={},active={active},joined={joined},peer_worker_finished={},peer_hold={},peer_stop={},peer=({peer}),runtime_after_optional_join=({runtime}),journal_mutex_available={}",
        elapsed.as_millis(), rig.peer.worker.as_ref().is_none_or(|worker| worker.is_finished()),
        rig.peer.hold.load(Ordering::Acquire), rig.peer.stop.load(Ordering::Acquire), rig.journal.try_lock().is_ok())
}
fn hsp_attempt_diagnostic(attempt: &HspDispatchAttempt) -> String {
    match attempt {
        HspDispatchAttempt::Rejected(reason) => format!("rejected:{reason:?}"),
        HspDispatchAttempt::Observed { response: None } => "observed:unknown".into(),
        HspDispatchAttempt::Observed {
            response: Some(state),
        } => format!(
            "observed:stream={},play={:?}",
            state.stream_id, state.play_state
        ),
    }
}

fn emergency_history_snapshot(db: &rusqlite::Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    ["hsp_streams", "hsp_effects", "hsp_census"]
        .into_iter()
        .map(|table| {
            let mut statement = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
                .unwrap();
            let columns = statement.column_count();
            statement
                .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        })
        .collect()
}

#[test]
fn emergency_stop_works_without_prewrite_while_journal_mutex_is_held() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let journal = rig.journal.clone();
    let held = journal.lock().unwrap();
    let db =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    let before = emergency_history_snapshot(&db);
    let count: u64 = db
        .query_row("SELECT count(*) FROM hsp_effects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let stream = held.hsp_stream(rig.stream.id()).unwrap().record;
    let stop_id =
        crate::handy_hsp_records::HspEffectRecord::capture(rig.stream.id(), &stream, &stop)
            .unwrap()
            .id()
            .unwrap();
    let request = emergency_request(&rig, stop.clone());
    emergency_start(&mut rig, request);
    let mut completed = emergency_wait(&mut rig);
    let observation = completed
        .observe(&mut rig.runtime.lock().unwrap(), 100)
        .unwrap()
        .clone();
    assert_eq!(observation.record().effect, stop);
    assert!(matches!(
        observation.record().outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(rig.peer.puts(), 2);
    // Exactly the Setup exists in durable history; no emergency receipt is made.
    assert!(held.hsp_effect(&stop_id).is_err());
    assert_eq!(emergency_history_snapshot(&db), before);
    assert_eq!(observation.context().registration(), &registration());
    assert_eq!(observation.context().device(), &native_identity(&device()));
    assert_eq!(
        observation.context().mode(),
        &HandyDeviceMode {
            mode: 4,
            mode_session_id: 8
        }
    );
    assert_eq!(
        observation.context().route_sha256(),
        rig.dispatcher.transport.route.route_sha256()
    );
    assert_eq!(
        observation.context().route_namespace_sha256(),
        rig.dispatcher.transport.route.namespace_sha256()
    );
    let original = completed.observation().unwrap().clone();
    for _ in 0..3 {
        assert_eq!(
            completed
                .observe(&mut rig.runtime.lock().unwrap(), 200)
                .unwrap(),
            &original
        );
    }
    assert_eq!(rig.peer.puts(), 2);
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopAcknowledged
    );
}

#[test]
fn emergency_stop_after_locked_durable_claim_keeps_prepared_history_unmodified() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let request = rig.request(stop.clone());
    let receipt = request.receipt.clone();
    let db =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    rig.start(request);
    let refused = rig.wait();
    assert!(matches!(
        refused.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::Journal)
    ));
    assert!(refused.claim.is_none());
    assert_eq!(rig.peer.puts(), 1);
    let request = emergency_request(&rig, stop);
    emergency_start(&mut rig, request);
    let mut completion = emergency_wait(&mut rig);
    let observed = completion
        .observe(&mut rig.runtime.lock().unwrap(), 100)
        .unwrap()
        .clone();
    assert!(matches!(
        observed.record().outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(rig.peer.puts(), 2);
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        rig.journal
            .lock()
            .unwrap()
            .hsp_effect(receipt.id())
            .unwrap()
            .state,
        HspJournalState::Prepared
    );
    // No retrospective observation is mislabeled as a durable claim/completion.
    assert!(refused
        .persist(&mut rig.journal.lock().unwrap(), observed.record())
        .is_err());
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn emergency_stop_with_poisoned_journal_retains_native_transport_checks() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let journal = rig.journal.clone();
    assert!(thread::spawn(move || {
        let _held = journal.lock().unwrap();
        panic!("intentional journal owner failure");
    })
    .join()
    .is_err());
    let request = emergency_request(&rig, stop);
    emergency_start(&mut rig, request);
    let mut completion = emergency_wait(&mut rig);
    assert!(matches!(
        completion
            .observe(&mut rig.runtime.lock().unwrap(), 100)
            .unwrap()
            .record()
            .outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn emergency_and_durable_share_native_claim_before_and_after_join() {
    for emergency_first in [false, true] {
        let mut rig = Rig::new();
        rig.setup();
        let stop = issued_stop(&rig);
        let durable = rig.request(stop.clone());
        let emergency = emergency_request(&rig, stop.clone());
        let second_emergency = emergency_request(&rig, stop.clone());
        let mut second = second_dispatcher(&rig);
        rig.peer.hold.store(true, Ordering::Release);
        if emergency_first {
            emergency_start(&mut rig, emergency);
        } else {
            rig.start(durable);
        }
        wait_for_puts(
            &mut rig,
            2,
            if emergency_first {
                "emergency-first duplicate exclusion"
            } else {
                "durable-first duplicate exclusion"
            },
        );
        let failure = second
            .start_emergency_stop(second_emergency, Instant::now() + Duration::from_secs(2))
            .expect_err("one shared native attempt");
        assert_eq!(
            failure.reason,
            HspDispatchFailure::Authority(HspRuntimeError::NativeDispatchBusy)
        );
        let duplicate = rig.request(stop.clone());
        let owner = rig.owner.clone();
        let failure = second
            .start(
                duplicate,
                Instant::now() + Duration::from_secs(2),
                move |_, _, gate| gate.apply(&owner, |_| Ok(None)),
            )
            .expect_err("shared claim refuses durable clone");
        assert_eq!(
            failure.reason,
            HspDispatchFailure::Authority(HspRuntimeError::NativeDispatchBusy)
        );
        // A fabricated rejection cannot cancel another live or settled worker.
        let forged = HspDispatchCompletion {
            request: failure.request,
            claim: None,
            attempt: HspDispatchAttempt::Rejected(HspDispatchFailure::Journal),
        };
        assert_eq!(
            forged.abandon_unclaimed(
                &mut rig.runtime.lock().unwrap(),
                &mut rig.journal.lock().unwrap()
            ),
            Err(HspDispatchFailure::Authority(
                HspRuntimeError::NativeDispatchBusy
            ))
        );
        assert_eq!(
            rig.runtime.lock().unwrap().complete_settled(
                &stop,
                HspSettledTransport::Unknown,
                100,
                None
            ),
            Err(HspRuntimeError::NativeDispatchBusy)
        );
        rig.dispatcher.cancel();
        assert!(rig.dispatcher.poll_any().is_none());
        rig.peer.hold.store(false, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(3);
        let joined = loop {
            if let Some(joined) = rig.dispatcher.poll_any() {
                break joined;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        };
        assert!(rig
            .dispatcher
            .prepare_emergency_stop(stop.clone(), &rig.owner)
            .is_err());
        match joined {
            HspJoinedCompletion::PostSetupProbe(_) => panic!("unexpected probe"),
            HspJoinedCompletion::Durable(completion) => {
                assert!(!emergency_first);
                let record = completion
                    .observe(&mut rig.runtime.lock().unwrap(), 100, None)
                    .unwrap();
                assert_eq!(record.outcome, HspObservedOutcome::Unknown);
                completion
                    .persist(&mut rig.journal.lock().unwrap(), &record)
                    .unwrap();
            }
            HspJoinedCompletion::Emergency(mut completion) => {
                assert!(emergency_first);
                assert_eq!(
                    completion
                        .observe(&mut rig.runtime.lock().unwrap(), 100)
                        .unwrap()
                        .record()
                        .outcome,
                    HspObservedOutcome::Unknown
                );
            }
        }
        assert!(rig
            .dispatcher
            .prepare_emergency_stop(stop, &rig.owner)
            .is_err());
        assert_eq!(
            rig.runtime.lock().unwrap().state(),
            HspRuntimeState::StopUnknown
        );
        assert_eq!(rig.peer.puts(), 2);
    }
}

#[test]
fn emergency_rejects_ordinary_effect_wrong_owner_and_retired_owner() {
    let mut rig = Rig::new();
    let setup = rig.next();
    assert!(matches!(
        rig.dispatcher
            .prepare_emergency_stop(setup.clone(), &rig.owner),
        Err(HspDispatchFailure::BindingMismatch)
    ));
    let request = rig.request(setup);
    rig.start(request);
    rig.finish();
    rig.rebind_after_setup();
    let stop = issued_stop(&rig);
    let foreign = Rig::transport(&rig.peer, 8).owner();
    assert!(matches!(
        rig.dispatcher
            .prepare_emergency_stop(stop.clone(), &foreign),
        Err(HspDispatchFailure::BindingMismatch)
    ));
    let request = emergency_request(&rig, stop.clone());
    rig.owner.retire_owner();
    assert!(matches!(
        rig.dispatcher.prepare_emergency_stop(stop, &rig.owner),
        Err(HspDispatchFailure::BindingMismatch)
    ));
    let failure = rig
        .dispatcher
        .start_emergency_stop(request, Instant::now() + Duration::from_secs(2))
        .err()
        .unwrap();
    assert_eq!(failure.reason, HspDispatchFailure::BindingMismatch);
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn emergency_known_foreign_journal_owner_stays_revoked_when_metadata_disappears() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let request = emergency_request(&rig, stop.clone());
    let db =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    db.execute("UPDATE owner SET boot='replacement'", [])
        .unwrap();
    emergency_start(&mut rig, request);
    let completion = emergency_wait(&mut rig);
    assert!(matches!(
        completion.attempt(),
        HspDispatchAttempt::Rejected(HspDispatchFailure::BindingMismatch)
    ));
    db.execute("DROP TABLE owner", []).unwrap();
    assert!(matches!(
        rig.dispatcher.prepare_emergency_stop(stop, &rig.owner),
        Err(HspDispatchFailure::BindingMismatch)
    ));
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn emergency_remote_replacement_refused_without_journal_access() {
    for replacement in [0, 1, 2] {
        let mut rig = Rig::new();
        rig.setup();
        let stop = issued_stop(&rig);
        let journal = rig.journal.clone();
        let _held = journal.lock().unwrap();
        let request = emergency_request(&rig, stop);
        {
            let mut peer = rig.peer.state.lock().unwrap();
            match replacement {
                0 => peer.stream += 1,
                1 => peer.mode += 1,
                _ => peer.session = "replacement-session".into(),
            }
        }
        emergency_start(&mut rig, request);
        let completion = emergency_wait(&mut rig);
        assert!(matches!(
            completion.attempt(),
            HspDispatchAttempt::Rejected(HspDispatchFailure::RemoteMismatch)
        ));
        assert_eq!(rig.peer.puts(), 1);
        assert_eq!(
            rig.runtime.lock().unwrap().state(),
            HspRuntimeState::StopInFlight
        );
    }
}

#[test]
fn emergency_malformed_reply_unknown_is_frozen_and_cannot_resend() {
    for reply in [
        Reply::Malformed,
        Reply::Error,
        Reply::Redirect,
        Reply::Echo,
        Reply::Oversized,
        Reply::Lost,
    ] {
        let mut rig = Rig::new();
        rig.setup();
        let stop = issued_stop(&rig);
        rig.peer.state.lock().unwrap().reply = reply;
        let request = emergency_request(&rig, stop.clone());
        emergency_start(&mut rig, request);
        let mut completion = emergency_wait(&mut rig);
        let original = completion
            .observe(&mut rig.runtime.lock().unwrap(), 100)
            .unwrap()
            .clone();
        assert_eq!(original.record().outcome, HspObservedOutcome::Unknown);
        assert_eq!(original.record().late_observation, None);
        for now in [101, 200, 500] {
            assert_eq!(
                completion
                    .observe(&mut rig.runtime.lock().unwrap(), now)
                    .unwrap(),
                &original
            );
        }
        assert_eq!(
            rig.runtime
                .lock()
                .unwrap()
                .uncertain_stop()
                .unwrap()
                .late_observation,
            None
        );
        assert!(rig
            .dispatcher
            .prepare_emergency_stop(stop, &rig.owner)
            .is_err());
        assert!(rig
            .runtime
            .lock()
            .unwrap()
            .next_effect(500, None)
            .unwrap()
            .is_none());
        assert_eq!(rig.peer.puts(), 2);
    }
}

#[test]
fn emergency_shutdown_joins_held_reply_without_runtime_or_journal_lock() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let request = emergency_request(&rig, stop);
    rig.peer.hold.store(true, Ordering::Release);
    emergency_start(&mut rig, request);
    wait_for_puts(&mut rig, 2, "emergency shutdown held reply");
    let runtime = rig.runtime.clone();
    let journal = rig.journal.clone();
    let mut runtime = runtime.lock().unwrap();
    let _journal = journal.lock().unwrap();
    let held = rig.peer.hold.clone();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        held.store(false, Ordering::Release);
    });
    let began = Instant::now();
    let HspJoinedCompletion::Emergency(mut completion) = rig.dispatcher.shutdown().unwrap() else {
        panic!("expected emergency completion");
    };
    assert!(began.elapsed() >= Duration::from_millis(50));
    release.join().unwrap();
    assert!(rig.dispatcher.active.is_none());
    assert_eq!(
        completion
            .observe(&mut runtime, 100)
            .unwrap()
            .record()
            .outcome,
        HspObservedOutcome::Unknown
    );
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_panic_before_claim_releases_only_joined_reservation_for_stop_fallback() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let fallback = emergency_request(&rig, stop.clone());
    let durable = rig.request(stop);
    let receipt = durable.receipt.clone();
    assert!(rig
        .dispatcher
        .start(
            durable,
            Instant::now() + Duration::from_secs(2),
            |_, _, _| panic!("synthetic native owner panic before claim")
        )
        .is_ok());
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::WorkerUnavailable)
    ));
    assert!(completion.claim.is_none());
    assert_eq!(rig.peer.puts(), 1);
    emergency_start(&mut rig, fallback);
    let mut completion = emergency_wait(&mut rig);
    assert!(matches!(
        completion
            .observe(&mut rig.runtime.lock().unwrap(), 100)
            .unwrap()
            .record()
            .outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(
        rig.journal
            .lock()
            .unwrap()
            .hsp_effect(receipt.id())
            .unwrap()
            .state,
        HspJournalState::Prepared
    );
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_claim_boundary_unwind_is_unknown_without_fabricating_a_durable_claim() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let durable = rig.request(stop.clone());
    assert!(rig
        .dispatcher
        .start(
            durable,
            Instant::now() + Duration::from_secs(2),
            |_, _, gate| {
                // Fault injection at the claim boundary: unwinding cannot establish
                // whether a journal transaction committed, even without a receipt.
                gate.custody
                    .lock()
                    .unwrap()
                    .reservation
                    .begin_claim()
                    .unwrap();
                panic!("synthetic claim-boundary unwind");
            }
        )
        .is_ok());
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert!(completion.claim.is_none());
    assert!(rig
        .dispatcher
        .prepare_emergency_stop(stop, &rig.owner)
        .is_err());
    let record = completion
        .observe(&mut rig.runtime.lock().unwrap(), 100, None)
        .unwrap();
    assert_eq!(record.outcome, HspObservedOutcome::Unknown);
    assert!(completion
        .persist(&mut rig.journal.lock().unwrap(), &record)
        .is_err());
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn emergency_worker_panic_after_put_keeps_consumed_unknown_without_retry() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let request = emergency_request(&rig, stop.clone());
    let peer = rig.peer.state.clone();
    rig.dispatcher.clock = Arc::new(move || {
        let puts = peer
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r.0 == "PUT")
            .count();
        assert!(puts < 2, "synthetic clock panic after actual Stop PUT");
        Ok(100)
    });
    emergency_start(&mut rig, request);
    let mut completion = emergency_wait(&mut rig);
    assert!(matches!(
        completion.attempt(),
        HspDispatchAttempt::Observed { response: None }
    ));
    assert_eq!(
        completion
            .observe(&mut rig.runtime.lock().unwrap(), 100)
            .unwrap()
            .record()
            .outcome,
        HspObservedOutcome::Unknown
    );
    assert!(rig
        .dispatcher
        .prepare_emergency_stop(stop, &rig.owner)
        .is_err());
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn emergency_capability_and_joined_observation_bind_exact_native_runtime() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let request = emergency_request(&rig, stop.clone());
    let mut other_runtime =
        HandyHspRuntime::new(binding(), rig.runtime.lock().unwrap().policy()).unwrap();
    let current = facts(100, 1_000_000_000);
    let setup = other_runtime
        .next_effect(100, Some(&current))
        .unwrap()
        .unwrap();
    let body = state_bytes(remote_state(42, -1, false));
    other_runtime
        .complete_settled(
            &setup,
            HspSettledTransport::Response {
                status: 200,
                body: &body,
            },
            100,
            Some(&current),
        )
        .unwrap();
    other_runtime.stop();
    let other_stop = other_runtime.next_effect(100, None).unwrap().unwrap();
    assert_eq!(other_stop.binding(), stop.binding());
    assert_eq!(other_stop.sequence(), stop.sequence());
    assert_eq!(other_stop.kind(), stop.kind());
    assert_eq!(other_stop.request(), stop.request());
    assert_ne!(
        other_stop, stop,
        "native identity is separate from wire values"
    );
    let other_runtime = Arc::new(Mutex::new(other_runtime));
    let mut other = second_dispatcher(&rig);
    other.runtime = other_runtime.clone();
    let failure = other
        .start_emergency_stop(request, Instant::now() + Duration::from_secs(2))
        .expect_err("capability must retain exact original runtime");
    assert_eq!(failure.reason, HspDispatchFailure::BindingMismatch);
    assert_eq!(rig.peer.puts(), 1);
    emergency_start(&mut rig, failure.request);
    let mut completion = emergency_wait(&mut rig);
    assert_eq!(
        completion.observe(&mut other_runtime.lock().unwrap(), 100),
        Err(HspDispatchFailure::Authority(
            HspRuntimeError::ForeignEffect
        ))
    );
    assert!(completion.observation().is_none());
    assert_eq!(other_runtime.lock().unwrap().in_flight(), Some(&other_stop));
    assert!(matches!(
        completion
            .observe(&mut rig.runtime.lock().unwrap(), 100)
            .unwrap()
            .record()
            .outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(rig.peer.puts(), 2);
}
