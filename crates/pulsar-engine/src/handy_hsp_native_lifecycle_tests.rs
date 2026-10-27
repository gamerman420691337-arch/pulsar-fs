// Included in handy_hsp_dispatch::tests; all network activity is localhost only.
fn setup_without_rebinding(rig: &mut Rig) -> HspEffect {
    let setup = rig.next();
    let request = rig.request(setup.clone());
    rig.start(request);
    rig.finish();
    setup
}
fn wait_probe(rig: &mut Rig) -> HspPostSetupProbeCompletion {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(completion) = rig.dispatcher.poll_any() {
            let HspJoinedCompletion::PostSetupProbe(completion) = completion else {
                panic!("expected joined probe");
            };
            return *completion;
        }
        assert!(Instant::now() < deadline, "probe worker failed to join");
        thread::sleep(Duration::from_millis(2));
    }
}
fn start_probe(rig: &mut Rig, setup: &HspEffect) {
    rig.dispatcher
        .start_post_setup_probe(setup, &rig.owner, Instant::now() + Duration::from_secs(2))
        .unwrap();
}

#[test]
fn native_probe_rejects_changed_journal_owner_on_its_joined_worker() {
    let mut rig = Rig::new();
    let setup = setup_without_rebinding(&mut rig);
    let db =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    db.execute("UPDATE owner SET boot='replacement'", [])
        .unwrap();
    start_probe(&mut rig, &setup);
    let probe = wait_probe(&mut rig);
    assert!(matches!(
        probe.result(),
        Err(HspDispatchFailure::BindingMismatch)
    ));
    assert!(rig.dispatcher.install_post_setup_probe(&probe).is_err());
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn native_probe_rebind_never_bypasses_later_sender_owner_check() {
    let mut rig = Rig::new();
    let setup = setup_without_rebinding(&mut rig);
    start_probe(&mut rig, &setup);
    let probe = wait_probe(&mut rig);
    let db =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    db.execute("UPDATE owner SET boot='replacement'", [])
        .unwrap();
    // Rebinding is nonblocking native state, not a new network operation or a
    // journal grant. The next sender independently detects owner replacement.
    rig.dispatcher.install_post_setup_probe(&probe).unwrap();
    let stop = issued_stop(&rig);
    let request = emergency_request(&rig, stop.clone());
    emergency_start(&mut rig, request);
    let completion = emergency_wait(&mut rig);
    assert!(matches!(
        completion.attempt(),
        HspDispatchAttempt::Rejected(HspDispatchFailure::BindingMismatch)
    ));
    assert!(rig
        .dispatcher
        .prepare_emergency_stop(stop, &rig.owner)
        .is_err());
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn native_probe_rebinds_same_owner_and_runs_setup_prefill_play_append_stop() {
    let mut rig = Rig::new();
    let setup = setup_without_rebinding(&mut rig);
    let before_owner = rig.owner.clone();
    let old_transport = rig.dispatcher.transport.clone();
    start_probe(&mut rig, &setup);
    let probe = wait_probe(&mut rig);
    assert_eq!(probe.setup(), &setup);
    assert_eq!(probe.result().unwrap().mode().mode_session_id, 8);
    assert_eq!(probe.result().unwrap().state().stream_id, 42);
    rig.dispatcher.install_post_setup_probe(&probe).unwrap();
    assert!(Arc::ptr_eq(
        &before_owner.0,
        &rig.dispatcher.transport.owner.0
    ));
    assert!(Arc::ptr_eq(
        &old_transport.credentials,
        &rig.dispatcher.transport.credentials
    ));
    assert!(!Arc::ptr_eq(&old_transport, &rig.dispatcher.transport));
    assert!(rig.dispatcher.install_post_setup_probe(&probe).is_err());
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
    rig.offer(&[200, 300], 3);
    let request = rig.request(rig.next());
    rig.start(request);
    rig.finish();
    rig.owner.retire_ordinary();
    let stop = issued_stop(&rig);
    let request = rig.request(stop);
    rig.start(request);
    rig.finish();
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopAcknowledged
    );
    assert_eq!(rig.peer.puts(), 5);
}

#[test]
fn native_probe_unknown_setup_preserves_unknown_and_retired_ordinary_but_enables_stop() {
    let mut rig = Rig::new();
    rig.peer.state.lock().unwrap().reply = Reply::Lost;
    let setup = setup_without_rebinding(&mut rig);
    rig.peer.state.lock().unwrap().reply = Reply::Normal;
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopRequired
    );
    rig.owner.retire_ordinary();
    let stop = issued_stop(&rig);
    let old_envelope = emergency_request(&rig, stop.clone());
    start_probe(&mut rig, &setup);
    let probe = wait_probe(&mut rig);
    rig.dispatcher.install_post_setup_probe(&probe).unwrap();
    assert!(!rig.owner.0.ordinary.load(Ordering::Acquire));
    assert_eq!(
        rig.runtime
            .lock()
            .unwrap()
            .uncertain_effect()
            .unwrap()
            .outcome,
        HspObservedOutcome::Unknown
    );
    let failure = rig
        .dispatcher
        .start_emergency_stop(old_envelope, Instant::now() + Duration::from_secs(2));
    assert!(matches!(failure, Err(f) if f.reason == HspDispatchFailure::BindingMismatch));
    let request = emergency_request(&rig, stop);
    emergency_start(&mut rig, request);
    let mut completion = emergency_wait(&mut rig);
    completion
        .observe(&mut rig.runtime.lock().unwrap(), 100)
        .unwrap();
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopAcknowledged
    );
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_probe_rejects_unattempted_foreign_replaced_and_late_setup() {
    let mut rig = Rig::new();
    let setup = rig.next();
    assert!(rig
        .dispatcher
        .start_post_setup_probe(&setup, &rig.owner, Instant::now() + Duration::from_secs(2))
        .is_err());
    let request = rig.request(setup.clone());
    rig.start(request);
    rig.finish();
    let transport = Rig::transport(&rig.peer, 7);
    let foreign_owner = transport.owner();
    let mut foreign = HandyHspDispatcher::new(rig.runtime.clone(), rig.journal.clone(), transport);
    assert!(foreign
        .start_post_setup_probe(
            &setup,
            &foreign_owner,
            Instant::now() + Duration::from_secs(2)
        )
        .is_err());
    let mut other = Rig::new();
    let other_setup = setup_without_rebinding(&mut other);
    assert!(rig
        .dispatcher
        .start_post_setup_probe(
            &other_setup,
            &rig.owner,
            Instant::now() + Duration::from_secs(2)
        )
        .is_err());
    start_probe(&mut rig, &setup);
    let probe = wait_probe(&mut rig);
    assert!(other.dispatcher.install_post_setup_probe(&probe).is_err());
    rig.offer(&[0, 100], 1);
    let append = rig.next();
    assert!(rig.dispatcher.install_post_setup_probe(&probe).is_err());
    assert_eq!(rig.runtime.lock().unwrap().in_flight(), Some(&append));
}

#[test]
fn native_probe_remote_replacement_and_expired_result_cannot_install() {
    for replacement in [true, false] {
        let mut rig = Rig::new();
        let setup = setup_without_rebinding(&mut rig);
        if replacement {
            rig.peer.state.lock().unwrap().stream = 99;
        }
        start_probe(&mut rig, &setup);
        let probe = wait_probe(&mut rig);
        if replacement {
            assert_eq!(
                probe.result().unwrap_err(),
                &HspDispatchFailure::RemoteMismatch
            );
        } else {
            rig.tick.store(3_000_000_000, Ordering::Release);
        }
        assert!(rig.dispatcher.install_post_setup_probe(&probe).is_err());
        assert_eq!(rig.dispatcher.transport.binding.mode.mode_session_id, 7);
        assert_eq!(rig.peer.puts(), 1);
    }
}

#[test]
fn native_probe_retired_owner_and_original_transport_replacement_cannot_install() {
    for replace in [true, false] {
        let mut rig = Rig::new();
        let setup = setup_without_rebinding(&mut rig);
        start_probe(&mut rig, &setup);
        let probe = wait_probe(&mut rig);
        if replace {
            rig.rebind_after_setup();
        } else {
            rig.owner.retire_owner();
        }
        assert!(rig.dispatcher.install_post_setup_probe(&probe).is_err());
        assert_eq!(rig.peer.puts(), 1);
    }
}

#[test]
fn native_unsent_without_journal_access_does_not_cancel_persisted_intent() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    let append = rig.next();
    let request = rig.request(append.clone());
    let writer =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let observation = rig
        .dispatcher
        .retire_native_unsent(&append, &rig.owner)
        .unwrap();
    assert_eq!(observation.effect(), &append);
    assert_eq!(
        rig.runtime.lock().unwrap().native_unsent_effect(),
        Some(&append)
    );
    assert!(rig.runtime.lock().unwrap().unclaimed_effect().is_none());
    assert_eq!(
        rig.journal
            .lock()
            .unwrap()
            .hsp_effect(request.receipt.id())
            .unwrap()
            .state,
        HspJournalState::Prepared
    );
    let stop = issued_stop(&rig);
    let request = emergency_request(&rig, stop);
    emergency_start(&mut rig, request);
    let mut completion = emergency_wait(&mut rig);
    completion
        .observe(&mut rig.runtime.lock().unwrap(), 100)
        .unwrap();
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_unsent_joined_rejection_works_but_another_reserved_sender_blocks_it() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    let append = rig.next();
    let request = rig.request(append.clone());
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let mut other = second_dispatcher(&rig);
    other
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, _| {
                entered.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
                Err(HspDispatchFailure::Cancelled)
            },
        )
        .unwrap_or_else(|_| panic!("start"));
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(rig
        .dispatcher
        .retire_native_unsent(&append, &rig.owner)
        .is_err());
    other.cancel();
    assert!(rig
        .dispatcher
        .retire_native_unsent(&append, &rig.owner)
        .is_err());
    release.send(()).unwrap();
    let joined = other.shutdown().unwrap();
    assert!(
        matches!(joined,HspJoinedCompletion::Durable(c) if matches!(c.attempt,HspDispatchAttempt::Rejected(_)))
    );
    rig.dispatcher
        .retire_native_unsent(&append, &rig.owner)
        .unwrap();
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopRequired
    );
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn native_probe_cancellation_joins_before_new_worker_and_never_writes() {
    let mut rig = Rig::new();
    let setup = setup_without_rebinding(&mut rig);
    start_probe(&mut rig, &setup);
    assert!(matches!(
        rig.dispatcher.start_post_setup_probe(
            &setup,
            &rig.owner,
            Instant::now() + Duration::from_secs(2)
        ),
        Err(HspDispatchFailure::Busy)
    ));
    rig.dispatcher.cancel();
    assert!(rig.dispatcher.active.is_some());
    let HspJoinedCompletion::PostSetupProbe(completion) = rig.dispatcher.shutdown().unwrap() else {
        panic!("probe");
    };
    assert!(rig.dispatcher.active.is_none());
    // Cancellation cannot install even if all read bytes arrived before cancel.
    assert!(completion.result().is_err());
    assert!(rig
        .dispatcher
        .install_post_setup_probe(&completion)
        .is_err());
    assert_eq!(rig.peer.puts(), 1);
}

#[test]
fn native_retirement_inside_stalled_provider_refuses_claim_and_put() {
    for shutdown_flag in [false, true] {
        let mut rig = Rig::new();
        let setup = rig.next();
        let request = rig.request(setup.clone());
        let receipt = request.receipt.clone();
        let owner = rig.owner.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let stopped_task = stopped.clone();
        let shutdown_task = shutdown.clone();
        let (entered, ready) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        rig.dispatcher
            .start(
                request,
                Instant::now() + Duration::from_secs(2),
                move |_, _, gate| {
                    gate.with_retirement_flags(stopped_task, shutdown_task)
                        .apply(&owner, |now| {
                            entered.send(()).unwrap();
                            wait.recv_timeout(Duration::from_secs(2)).unwrap();
                            Ok(Some(facts(now, 1_000_000_000)))
                        })
                },
            )
            .unwrap_or_else(|_| panic!("start"));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        if shutdown_flag {
            shutdown.store(true, Ordering::Release);
        } else {
            stopped.store(true, Ordering::Release);
        }
        release.send(()).unwrap();
        let completion = rig.wait();
        assert!(matches!(
            completion.attempt,
            HspDispatchAttempt::Rejected(HspDispatchFailure::Cancelled)
        ));
        assert!(completion.claim.is_none());
        assert_eq!(
            rig.journal
                .lock()
                .unwrap()
                .hsp_effect(receipt.id())
                .unwrap()
                .state,
            HspJournalState::Prepared
        );
        rig.dispatcher
            .retire_native_unsent(&setup, &rig.owner)
            .unwrap();
        assert_eq!(rig.peer.puts(), 0);
    }
}

#[test]
fn native_retirement_after_claim_preserves_unknown_without_put() {
    let mut rig = Rig::new();
    let setup = rig.next();
    let request = rig.request(setup);
    let owner = rig.owner.clone();
    let stopped = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));
    let stopped_task = stopped.clone();
    let shutdown_task = shutdown.clone();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let handoff = gate
                    .with_retirement_flags(stopped_task, shutdown_task)
                    .apply(&owner, |now| Ok(Some(facts(now, 1_000_000_000))))?;
                entered.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(handoff)
            },
        )
        .unwrap_or_else(|_| panic!("start"));
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    shutdown.store(true, Ordering::Release);
    release.send(()).unwrap();
    let completion = rig.wait();
    assert!(completion.claim.is_some());
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    let record = completion
        .observe(
            &mut rig.runtime.lock().unwrap(),
            100,
            Some(&facts(100, 1_000_000_000)),
        )
        .unwrap();
    assert_eq!(record.outcome, HspObservedOutcome::Unknown);
    completion
        .persist(&mut rig.journal.lock().unwrap(), &record)
        .unwrap();
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_retirement_flags_do_not_disable_bound_stop() {
    let mut rig = Rig::new();
    rig.setup();
    let stop = issued_stop(&rig);
    let request = rig.request(stop);
    let owner = rig.owner.clone();
    owner.retire_ordinary();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                gate.with_retirement_flags(
                    Arc::new(AtomicBool::new(true)),
                    Arc::new(AtomicBool::new(true)),
                )
                .apply(&owner, |_| Ok(None))
            },
        )
        .unwrap_or_else(|_| panic!("start"));
    assert!(matches!(
        rig.finish().outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_claim_suspend_releases_runtime_but_preserves_exact_claim_custody() {
    let mut rig = Rig::new();
    let setup = rig.next();
    let request = rig.request(setup.clone());
    let owner = rig.owner.clone();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                gate.with_after_claim_hook(Box::new(move || {
                    entered.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(2)).unwrap();
                }))
                .apply(&owner, |now| Ok(Some(facts(now, 1_000_000_000))))
            },
        )
        .unwrap_or_else(|_| panic!("start"));
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    // The exact native claim, unlike a mutex held across storage, protects the
    // operation while allowing the independent watchdog/Stop latch to run.
    rig.runtime
        .try_lock()
        .expect("claim suspension owns no runtime lock")
        .stop();
    let other = second_dispatcher(&rig);
    assert!(matches!(
        other.retire_native_unsent(&setup, &rig.owner),
        Err(HspDispatchFailure::Authority(
            HspRuntimeError::NativeDispatchBusy
        ))
    ));
    release.send(()).unwrap();
    let completion = rig.wait();
    assert!(completion.claim.is_some());
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    let record = completion
        .observe(
            &mut rig.runtime.lock().unwrap(),
            100,
            Some(&facts(100, 1_000_000_000)),
        )
        .unwrap();
    assert_eq!(record.outcome, HspObservedOutcome::Unknown);
    completion
        .persist(&mut rig.journal.lock().unwrap(), &record)
        .unwrap();
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_claim_completion_rechecks_fresh_facts_and_keeps_original_reference() {
    let mut rig = Rig::new();
    let setup = rig.next();
    let request = rig.request(setup.clone());
    let owner = rig.owner.clone();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let captured = observed.clone();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                gate.with_after_claim_hook(Box::new(move || {
                    entered.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(2)).unwrap();
                }))
                .apply(&owner, |now| {
                    captured.lock().unwrap().push(now);
                    Ok((now == 100).then(|| facts(now, 1_000_000_000)))
                })
            },
        )
        .unwrap_or_else(|_| panic!("start"));
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    rig.tick.store(200, Ordering::Release);
    release.send(()).unwrap();
    let completion = rig.wait();
    assert_eq!(*observed.lock().unwrap(), vec![100, 200]);
    assert_eq!(
        completion
            .request
            .effect
            .original_facts()
            .unwrap()
            .reference()
            .at_ns(),
        100
    );
    assert!(completion.claim.is_some());
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert_eq!(
        rig.runtime.lock().unwrap().state(),
        HspRuntimeState::StopRequired
    );
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_claim_completion_never_renews_original_expiry() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let owner = rig.owner.clone();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let captured = observed.clone();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                gate.with_after_claim_hook(Box::new(move || {
                    entered.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(2)).unwrap();
                }))
                .apply(&owner, |now| {
                    captured.lock().unwrap().push(now);
                    Ok(Some(facts(now, 5_000_000_000)))
                })
            },
        )
        .unwrap_or_else(|_| panic!("start"));
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    rig.tick.store(1_000_000_001, Ordering::Release);
    release.send(()).unwrap();
    let completion = rig.wait();
    assert_eq!(*observed.lock().unwrap(), vec![100]);
    assert!(completion.claim.is_some());
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert_eq!(rig.peer.puts(), 0);
}
