fn preclaim_contended<'a>(progress: HspPreclaimProgress<'a>) -> HspClaimGate<'a> {
    match progress {
        HspPreclaimProgress::Contended(gate) => gate,
        HspPreclaimProgress::Claimed(_) => panic!("held local mutex must retain original gate"),
    }
}

fn preclaim_claim(
    gate: HspClaimGate<'_>,
    owner: &HspNativeOwner,
) -> Result<HspClaimedHandoff, HspDispatchFailure> {
    match gate.try_apply(owner, |now| Ok(Some(facts(now, 1_000_000_000))))? {
        HspPreclaimProgress::Claimed(handoff) => Ok(handoff),
        HspPreclaimProgress::Contended(_) => panic!("released local mutexes must allow claim"),
    }
}

fn preclaim_puts(peer: &Arc<Mutex<PeerState>>) -> usize {
    peer.lock()
        .unwrap()
        .requests
        .iter()
        .filter(|r| r.0 == "PUT")
        .count()
}

#[test]
fn native_preclaim_journal_then_runtime_contention_preserves_bytes_and_claims_once() {
    let mut rig = Rig::new();
    rig.setup();
    rig.offer(&[0, 100], 1);
    let effect = rig.next();
    let bytes = effect.request().body().unwrap().to_vec();
    let request = rig.request(effect.clone());
    let duplicate = HspDispatchRequest {
        effect: effect.clone(),
        receipt: request.receipt.clone(),
    };
    let receipt_id = request.receipt.id().to_string();
    let journal = rig.journal.clone();
    let runtime = rig.runtime.clone();
    let owner = rig.owner.clone();
    let peer = rig.peer.state.clone();
    let before_gets = peer
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|r| r.0 == "GET")
        .count();
    let original_bytes = bytes.clone();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let journal_guard = journal.lock().unwrap();
                let mut gate = preclaim_contended(
                    gate.try_apply(&owner, |_| panic!("no facts before journal"))?,
                );
                assert_eq!(
                    journal_guard.hsp_effect(&receipt_id).unwrap().state,
                    HspJournalState::Prepared
                );
                assert_eq!(preclaim_puts(&peer), 1);
                assert!(
                    runtime.try_lock().is_ok(),
                    "journal contention retains no runtime guard"
                );
                assert_eq!(
                    gate.request.effect.request().body(),
                    Some(original_bytes.as_slice())
                );
                drop(journal_guard);
                assert!(gate.preclaim_retry_delay(&owner)? <= Duration::from_millis(1));

                let runtime_guard = runtime.lock().unwrap();
                let mut gate = preclaim_contended(
                    gate.try_apply(&owner, |_| panic!("no facts before runtime"))?,
                );
                let journal_guard = journal
                    .try_lock()
                    .expect("runtime contention releases journal before yielding");
                assert_eq!(
                    journal_guard.hsp_effect(&receipt_id).unwrap().state,
                    HspJournalState::Prepared
                );
                assert_eq!(preclaim_puts(&peer), 1);
                assert_eq!(
                    gate.request.effect.request().body(),
                    Some(original_bytes.as_slice())
                );
                drop(journal_guard);
                drop(runtime_guard);
                assert!(gate.preclaim_retry_delay(&owner)? <= Duration::from_millis(1));
                preclaim_claim(gate, &owner)
            },
        )
        .unwrap_or_else(|_| panic!("native dispatch start"));
    let completion = rig.wait();
    assert!(completion.claim.is_some());
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: Some(_) }
    ));
    assert_eq!(completion.request.effect, effect);
    assert_eq!(
        completion.request.effect.request().body(),
        Some(bytes.as_slice())
    );
    assert_eq!(rig.peer.puts(), 2);
    let peer = rig.peer.state.lock().unwrap();
    assert_eq!(
        peer.requests.iter().filter(|r| r.0 == "GET").count(),
        before_gets + 3
    );
    assert_eq!(
        peer.requests
            .iter()
            .filter(|r| r.0 == "PUT" && r.1 == "/hsp/add")
            .count(),
        1
    );
    assert_eq!(
        peer.requests
            .iter()
            .find(|r| r.0 == "PUT" && r.1 == "/hsp/add")
            .unwrap()
            .2,
        bytes
    );
    drop(peer);
    let owner = rig.owner.clone();
    let refused = rig.dispatcher.start(
        duplicate,
        Instant::now() + Duration::from_secs(2),
        move |_, _, gate| preclaim_claim(gate, &owner),
    );
    assert!(
        matches!(refused, Err(ref error) if error.reason == HspDispatchFailure::Authority(HspRuntimeError::NativeDispatchBusy))
    );
    assert_eq!(rig.peer.puts(), 2);
}

#[test]
fn native_preclaim_first_contention_cap_survives_multiple_lock_sites() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let receipt_id = request.receipt.id().to_string();
    let journal = rig.journal.clone();
    let runtime = rig.runtime.clone();
    let owner = rig.owner.clone();
    let tick = rig.tick.clone();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let held = journal.lock().unwrap();
                let gate = preclaim_contended(gate.try_apply(&owner, |_| panic!("journal held"))?);
                drop(held);
                tick.store(50_000_100, Ordering::Release);
                let held = runtime.lock().unwrap();
                let mut gate =
                    preclaim_contended(gate.try_apply(&owner, |_| panic!("runtime held"))?);
                drop(held);
                assert!(gate.preclaim_retry_delay(&owner)? <= Duration::from_millis(1));
                // Exactly 100 ms after the FIRST wait, not after the second site.
                tick.store(100_000_100, Ordering::Release);
                assert_eq!(
                    gate.preclaim_retry_delay(&owner),
                    Err(HspDispatchFailure::InvalidBudget)
                );
                preclaim_claim(gate, &owner)
            },
        )
        .unwrap_or_else(|_| panic!("native dispatch start"));
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::InvalidBudget)
    ));
    assert!(completion.claim.is_none());
    assert_eq!(
        rig.journal
            .lock()
            .unwrap()
            .hsp_effect(&receipt_id)
            .unwrap()
            .state,
        HspJournalState::Prepared
    );
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_preclaim_original_expiry_cancel_and_retirement_end_the_same_wait() {
    for cause in 0..4 {
        let mut rig = Rig::new();
        let effect = rig
            .runtime
            .lock()
            .unwrap()
            .next_effect(100, Some(&facts(100, 200)))
            .unwrap()
            .unwrap();
        let request = rig.request(effect);
        let journal = rig.journal.clone();
        let owner = rig.owner.clone();
        let tick = rig.tick.clone();
        rig.dispatcher
            .start(
                request,
                Instant::now() + Duration::from_secs(2),
                move |_, cancel, gate| {
                    let shutdown = Arc::new(AtomicBool::new(false));
                    let gate = gate
                        .with_retirement_flags(Arc::new(AtomicBool::new(false)), shutdown.clone());
                    let held = journal.lock().unwrap();
                    let mut gate =
                        preclaim_contended(gate.try_apply(&owner, |_| panic!("journal held"))?);
                    drop(held);
                    assert!(gate.preclaim_retry_delay(&owner)? <= Duration::from_nanos(100));
                    match cause {
                        0 => tick.store(200, Ordering::Release),
                        1 => cancel.store(true, Ordering::Release),
                        2 => owner.retire_ordinary(),
                        _ => shutdown.store(true, Ordering::Release),
                    }
                    preclaim_claim(gate, &owner)
                },
            )
            .unwrap_or_else(|_| panic!("native dispatch start"));
        let completion = rig.wait();
        let expected = match cause {
            0 => HspDispatchFailure::Authority(HspRuntimeError::ExpiredAuthority),
            1 | 3 => HspDispatchFailure::Cancelled,
            _ => HspDispatchFailure::BindingMismatch,
        };
        assert!(
            matches!(completion.attempt, HspDispatchAttempt::Rejected(ref reason) if reason == &expected)
        );
        assert!(completion.claim.is_none());
        assert_eq!(rig.peer.puts(), 0);
    }
}

#[test]
fn native_preclaim_wall_cap_expires_even_when_native_clock_does_not_advance() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let owner = rig.owner.clone();
    let journal = rig.journal.clone();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let held = journal.lock().unwrap();
                let mut gate =
                    preclaim_contended(gate.try_apply(&owner, |_| panic!("journal held"))?);
                drop(held);
                assert!(journal.try_lock().is_ok());
                let until = Instant::now() + Duration::from_millis(110);
                while let Some(remaining) = until.checked_duration_since(Instant::now()) {
                    if remaining.is_zero() {
                        break;
                    }
                    thread::park_timeout(remaining);
                }
                assert_eq!(
                    gate.check_preclaim_wait(&owner),
                    Err(HspDispatchFailure::InvalidBudget)
                );
                preclaim_claim(gate, &owner)
            },
        )
        .unwrap_or_else(|_| panic!("native dispatch start"));
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::InvalidBudget)
    ));
    assert!(completion.claim.is_none());
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_preclaim_poison_and_provider_busy_never_return_retry_custody() {
    for cause in 0..3 {
        let mut rig = Rig::new();
        let request = rig.request(rig.next());
        let journal = rig.journal.clone();
        let runtime = rig.runtime.clone();
        let owner = rig.owner.clone();
        rig.dispatcher
            .start(
                request,
                Instant::now() + Duration::from_secs(2),
                move |_, _, gate| {
                    if cause < 2 {
                        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if cause == 0 {
                                let _held = journal.lock().unwrap();
                                panic!("deliberate journal poison");
                            } else {
                                let _held = runtime.lock().unwrap();
                                panic!("deliberate runtime poison");
                            }
                        }))
                        .is_err());
                    }
                    let result = gate.try_apply(&owner, |_| Err(HspDispatchFailure::Busy));
                    assert!(
                        result.is_err(),
                        "poison and provider errors are terminal, not contention"
                    );
                    Err(result.err().expect("checked terminal refusal"))
                },
            )
            .unwrap_or_else(|_| panic!("native dispatch start"));
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
fn native_preclaim_postclaim_runtime_contention_is_unknown_without_second_put() {
    let mut rig = Rig::new();
    let effect = rig.next();
    let request = rig.request(effect.clone());
    let duplicate = HspDispatchRequest {
        effect: effect.clone(),
        receipt: request.receipt.clone(),
    };
    let runtime = rig.runtime.clone();
    let owner = rig.owner.clone();
    let (start_hold, begin_hold) = std::sync::mpsc::channel();
    let (held, holding) = std::sync::mpsc::channel();
    let (release, finish_hold) = std::sync::mpsc::channel();
    let holder = thread::spawn(move || {
        begin_hold.recv_timeout(Duration::from_secs(2)).unwrap();
        let _guard = runtime.lock().unwrap();
        held.send(()).unwrap();
        finish_hold.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let gate = gate.with_after_claim_hook(Box::new(move || {
                    start_hold.send(()).unwrap();
                    holding.recv_timeout(Duration::from_secs(2)).unwrap();
                }));
                let result = gate.try_apply(&owner, |now| Ok(Some(facts(now, 1_000_000_000))));
                assert!(matches!(result, Err(HspDispatchFailure::Busy)));
                Err(result.err().expect("claimed runtime refusal is terminal"))
            },
        )
        .unwrap_or_else(|_| panic!("native dispatch start"));
    let completion = rig.wait();
    assert!(completion.claim.is_some());
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Observed { response: None }
    ));
    assert_eq!(
        rig.journal
            .try_lock()
            .unwrap()
            .hsp_effect(completion.request.receipt.id())
            .unwrap()
            .state,
        HspJournalState::Claimed
    );
    assert_eq!(rig.peer.puts(), 0);
    release.send(()).unwrap();
    holder.join().unwrap();
    let owner = rig.owner.clone();
    let refused = rig.dispatcher.start(
        duplicate,
        Instant::now() + Duration::from_secs(2),
        move |_, _, gate| preclaim_claim(gate, &owner),
    );
    assert!(
        matches!(refused, Err(ref error) if error.reason == HspDispatchFailure::Authority(HspRuntimeError::NativeDispatchBusy))
    );
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_preclaim_legacy_apply_maps_only_typed_contention_to_busy() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let owner = rig.owner.clone();
    let journal = rig.journal.clone();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let _held = journal.lock().unwrap();
                gate.apply(&owner, |_| {
                    panic!("legacy apply never loops through contention")
                })
            },
        )
        .unwrap_or_else(|_| panic!("native dispatch start"));
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::Busy)
    ));
    assert!(completion.claim.is_none());
    assert_eq!(rig.peer.puts(), 0);
}

#[test]
fn native_preclaim_sqlite_busy_is_terminal_without_returning_gate() {
    let mut rig = Rig::new();
    let request = rig.request(rig.next());
    let writer =
        rusqlite::Connection::open(rig.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let owner = rig.owner.clone();
    rig.dispatcher
        .start(
            request,
            Instant::now() + Duration::from_secs(2),
            move |_, _, gate| {
                let result = gate.try_apply(&owner, |now| Ok(Some(facts(now, 1_000_000_000))));
                assert!(matches!(result, Err(HspDispatchFailure::Journal)));
                Err(result.err().expect("durable I/O failure consumed the gate"))
            },
        )
        .unwrap_or_else(|_| panic!("native dispatch start"));
    let completion = rig.wait();
    assert!(matches!(
        completion.attempt,
        HspDispatchAttempt::Rejected(HspDispatchFailure::Journal)
    ));
    assert!(completion.claim.is_none());
    assert_eq!(rig.peer.puts(), 0);
    writer.execute_batch("ROLLBACK").unwrap();
}
