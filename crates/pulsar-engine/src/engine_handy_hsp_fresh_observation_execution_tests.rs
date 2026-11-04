// Authored publication ordering with the real Engine owner and localhost peer.
// No observation here qualifies browser timing, a model, or a physical device.
struct SelectedFixturePublication {
    received_at_ns: u64,
    expires_at_ns: u64,
}

struct FreshSelectingAuthority {
    inner: Arc<SyntheticAuthority>,
    publication: Mutex<Arc<SelectedFixturePublication>>,
    test_thread: thread::ThreadId,
    test_thread_facts: Mutex<Vec<HspDispatchFacts>>,
    current_reads: AtomicU64,
    snapshot_reads: AtomicU64,
    fixed_reads: AtomicU64,
    refuse_projection: AtomicBool,
}

impl FreshSelectingAuthority {
    fn new(inner: Arc<SyntheticAuthority>) -> Self {
        Self {
            publication: Mutex::new(Arc::new(SelectedFixturePublication {
                received_at_ns: inner.anchor,
                expires_at_ns: inner.lease_until.load(Ordering::Acquire),
            })),
            inner,
            test_thread: thread::current().id(),
            test_thread_facts: Mutex::new(Vec::new()),
            current_reads: AtomicU64::new(0),
            snapshot_reads: AtomicU64::new(0),
            fixed_reads: AtomicU64::new(0),
            refuse_projection: AtomicBool::new(false),
        }
    }

    fn observe_selected(
        &self,
        clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
        project: bool,
    ) -> Result<crate::handy_hsp_execution::HspAuthorityEvaluation, HspDispatchFailure> {
        // Hold a fixture publication through the callback, then install a
        // replacement newer than its sampled time. Evaluating latest afterward
        // would reproduce the fixed-time refusal; the selected Arc is unchanged.
        let selected = self.publication.lock().unwrap().clone();
        let sampled_at_ns = clock()?;
        *self.publication.lock().unwrap() = Arc::new(SelectedFixturePublication {
            received_at_ns: sampled_at_ns.checked_add(1).unwrap(),
            expires_at_ns: selected.expires_at_ns,
        });
        if sampled_at_ns < selected.received_at_ns {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        if sampled_at_ns >= selected.expires_at_ns {
            return Err(HspDispatchFailure::InvalidClock);
        }
        if project && self.refuse_projection.load(Ordering::Acquire) {
            return Err(HspDispatchFailure::InvalidClock);
        }
        let snapshot = if project {
            self.inner.current_snapshot(sampled_at_ns)?
        } else {
            HspAuthoritySnapshot {
                facts: self.inner.current_facts(sampled_at_ns)?,
                projected: None,
            }
        };
        if thread::current().id() == self.test_thread {
            if let Some(facts) = &snapshot.facts {
                self.test_thread_facts.lock().unwrap().push(facts.clone());
            }
        }
        Ok(crate::handy_hsp_execution::HspAuthorityEvaluation {
            sampled_at_ns,
            snapshot,
        })
    }
}

impl HspNativeAuthority for FreshSelectingAuthority {
    fn observe_current(
        &self,
        clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
    ) -> Result<crate::handy_hsp_execution::HspAuthorityEvaluation, HspDispatchFailure> {
        self.current_reads.fetch_add(1, Ordering::Relaxed);
        self.observe_selected(clock, false)
    }

    fn observe_snapshot(
        &self,
        clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
    ) -> Result<crate::handy_hsp_execution::HspAuthorityEvaluation, HspDispatchFailure> {
        self.snapshot_reads.fetch_add(1, Ordering::Relaxed);
        self.observe_selected(clock, true)
    }

    fn current_facts(&self, at: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure> {
        self.fixed_reads.fetch_add(1, Ordering::Relaxed);
        // The legacy sample-before-selection interleaving deterministically
        // observes a publication received just after that already-taken sample.
        let mut publication = self.publication.lock().unwrap();
        *publication = Arc::new(SelectedFixturePublication {
            received_at_ns: at.checked_add(1).unwrap(),
            expires_at_ns: publication.expires_at_ns,
        });
        assert!(at < publication.received_at_ns);
        Err(HspDispatchFailure::BindingMismatch)
    }

    fn authorize(
        &self,
        deadline: Instant,
        cancel: &AtomicBool,
        gate: HspClaimGate<'_>,
        owner: &HspNativeOwner,
    ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
        // Final claims keep their existing guarded fixed-time path.
        self.inner.authorize(deadline, cancel, gate, owner)
    }
}

fn assert_fresh_original_facts(
    retained: &crate::handy_hsp_records::HspDispatchFactsRecord,
    original: &HspDispatchFacts,
) {
    assert_eq!(
        retained.binding,
        HspBindingRecord::capture(original.binding())
    );
    assert_eq!(retained.at_ns, original.reference().at_ns());
    assert_eq!(
        retained.earliest_ms,
        original.reference().phase().earliest_ms
    );
    assert_eq!(retained.latest_ms, original.reference().phase().latest_ms);
    assert_eq!(retained.start_time_ms, original.reference().start_time_ms());
    assert_eq!(
        retained.server_time_ms,
        original.reference().server_time_ms()
    );
    assert_eq!(retained.lease_until_ns, original.window().lease_until_ns());
    assert_eq!(
        retained.source_until_ns,
        original.window().source_until_ns()
    );
    assert_eq!(retained.clock_until_ns, original.window().clock_until_ns());
}

#[test]
fn engine_hsp_fresh_consumers_keep_original_play_and_effect_while_publications_change() {
    let mut selected = None;
    let rig = Rig::with_authority_wrapper(false, None, 0, |inner| {
        let authority = Arc::new(FreshSelectingAuthority::new(inner));
        selected = Some(authority.clone());
        authority
    });
    let selected = selected.unwrap();
    rig.ready();
    rig.offer(1);
    rig.offer(2);
    rig.wait("fresh prefill and completion must settle", |s| {
        s.total_added == 2 && !s.native_active && !s.metadata_pending
    });
    let runtime = rig.session.runtime_for_test();
    let pause = Arc::new(PreLockFactPause::new(&runtime));
    let release_on_unwind = ReleaseFactPause(pause.clone());
    *rig.authority.pre_lock_pause.lock().unwrap() = Some(pause.clone());
    pause.wait_entered();
    let before = selected.test_thread_facts.lock().unwrap().len();
    let until = Instant::now() + WAIT;
    let intent = loop {
        match rig.session.capture_play_intent() {
            Ok(intent) => break intent,
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2));
            }
            other => panic!("fresh original Play capture failed: {:?}", other.err()),
        }
    };
    let original = {
        let facts = selected.test_thread_facts.lock().unwrap();
        assert_eq!(facts.len(), before + 1);
        facts.last().unwrap().clone()
    };
    // The owner is paused outside its runtime guard while intent is captured
    // and accepted. Hold preparation next to inspect the exact pending effect.
    let hold = rig
        .session
        .hold_next_metadata_for_test(crate::handy_hsp_execution::HspMetadataTaskKind::Prepare);
    rig.peer.hold_next("/hsp/play");
    rig.session.request_original_play(intent).unwrap();
    pause.release();
    let task = hold.wait_entered(Instant::now() + WAIT).unwrap();
    assert_eq!(task.sequence, Some(3));
    let fresh_current = selected
        .test_thread_facts
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert!(fresh_current.reference().at_ns() > original.reference().at_ns());
    let pending = runtime.lock().unwrap().in_flight().unwrap().clone();
    assert_eq!(pending.sequence(), 3);
    assert_eq!(
        pending.kind(),
        crate::handy_hsp_runtime::HspEffectKind::Play
    );
    assert_eq!(pending.original_facts(), Some(&original));
    hold.release();
    rig.peer.wait_held();
    let sent = {
        let journal = rig.f.engine._device_effect_journal.lock().unwrap();
        let stream = journal.hsp_stream(&rig.record.id().unwrap()).unwrap();
        stream
            .effect_ids
            .iter()
            .find_map(|id| {
                let effect = journal.hsp_effect(id).unwrap();
                (effect.record.kind == HspEffectRecordKind::Play).then_some(effect.record)
            })
            .unwrap()
    };
    assert_fresh_original_facts(sent.original_facts.as_ref().unwrap(), &original);
    rig.peer.release();
    rig.wait("fresh completion must settle original Play", |s| {
        s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::Playing
            && !s.native_active
            && !s.metadata_pending
    });
    let before = selected.current_reads.load(Ordering::Acquire);
    let until = Instant::now() + WAIT;
    loop {
        match rig.session.request_play() {
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2));
            }
            Err(HspExecutionError::Busy) => panic!("original Play lock remained held"),
            Err(_) => break,
            Ok(()) => panic!("original Play must remain single use"),
        }
    }
    assert!(selected.current_reads.load(Ordering::Acquire) > before);
    rig.offer(3);
    rig.wait("fresh later Append must settle", |s| {
        s.total_added == 3 && !s.native_active && !s.metadata_pending
    });
    rig.session.cancel();
    rig.stopped();
    rig.wait("original Play metadata retained", |s| !s.metadata_pending);
    let retained = {
        let journal = rig.f.engine._device_effect_journal.lock().unwrap();
        journal.hsp_effect(&sent.id().unwrap()).unwrap().record
    };
    assert_eq!(
        retained, sent,
        "fresh reads must not replace the claimed effect"
    );
    assert!(
        selected.snapshot_reads.load(Ordering::Acquire) > 0,
        "watchdog uses fresh snapshots"
    );
    assert_eq!(
        selected.fixed_reads.load(Ordering::Acquire),
        0,
        "fresh consumers must not sample before selecting"
    );
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
    rig.f.engine.finish_shutdown().unwrap();
    assert!(!pause.state.lock().unwrap().timed_out);
    drop(release_on_unwind);

    // After the owner has joined, remove future projection coverage. Ordinary
    // evaluation still succeeds and does not silently request a tail projection.
    selected.refuse_projection.store(true, Ordering::Release);
    let mut clock_calls = 0;
    let current = selected
        .observe_current(&mut || {
            clock_calls += 1;
            crate::handy_clock::boottime_ns().map_err(|_| HspDispatchFailure::InvalidClock)
        })
        .unwrap();
    assert_eq!(clock_calls, 1);
    assert_eq!(
        current.snapshot.facts.as_ref().unwrap().reference().at_ns(),
        current.sampled_at_ns
    );
    assert!(current.snapshot.projected.is_none());
    let mut clock_calls = 0;
    let projected = selected.observe_snapshot(&mut || {
        clock_calls += 1;
        crate::handy_clock::boottime_ns().map_err(|_| HspDispatchFailure::InvalidClock)
    });
    assert!(matches!(projected, Err(HspDispatchFailure::InvalidClock)));
    assert_eq!(clock_calls, 1);
    let before = selected.current_reads.load(Ordering::Acquire);
    let admission = rig.admission_with_authority(selected.clone());
    assert_eq!(selected.current_reads.load(Ordering::Acquire), before + 1);
    drop(admission);
    assert!(matches!(
        selected.current_facts(current.sampled_at_ns),
        Err(HspDispatchFailure::BindingMismatch)
    ));
}

#[test]
fn engine_hsp_fresh_observations_seal_finite_input_with_original_terminal() {
    let mut selected = None;
    let rig = Rig::with_authority_wrapper(false, None, 500_000_000, |inner| {
        let authority = Arc::new(FreshSelectingAuthority::new(inner));
        selected = Some(authority.clone());
        authority
    });
    let selected = selected.unwrap();
    rig.ready();
    rig.offer(1);
    rig.offer(2);
    rig.wait("fresh finite prefill must settle", |s| {
        s.total_added == 2 && !s.native_active && !s.metadata_pending
    });
    let until = Instant::now() + WAIT;
    loop {
        match rig.session.request_play() {
            Ok(()) => break,
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2));
            }
            other => panic!("fresh immediate Play admission failed: {other:?}"),
        }
    }
    rig.wait("fresh immediate Play must settle", |s| {
        s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::Playing
            && !s.native_active
            && !s.metadata_pending
    });
    let terminal_at = crate::handy_clock::boottime_ns().unwrap();
    let terminal = HspSample {
        point: HspPoint::new(200, 20).unwrap(),
        source_sequence: 3,
        captured_at_ns: terminal_at,
        accepted_at_ns: terminal_at,
        continuous_from_previous: true,
    };
    let until = Instant::now() + WAIT;
    loop {
        match rig.session.offer(&rig.binding.identity().live, terminal) {
            Ok(HspOfferOutcome::Accepted) => break,
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2));
            }
            other => panic!("fresh finite terminal offer failed: {other:?}"),
        }
    }
    let closed_at = crate::handy_clock::boottime_ns().unwrap();
    let policy = pulsar_core::HspFiniteDrainPolicy {
        maximum_duration_ns: 4_000_000_000,
        maximum_terminal_overrun_ns: 750_000_000,
        stop_budget_ns: 500_000_000,
    };
    let until = Instant::now() + WAIT;
    let seal = loop {
        match rig.session.seal_input(
            &rig.binding.identity().live,
            terminal,
            closed_at,
            policy,
            400_000_000,
        ) {
            Ok(seal) => break seal,
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2));
            }
            other => panic!("fresh finite input seal failed: {other:?}"),
        }
    };
    let stopped = rig.stopped();
    let drain = stopped.finite_drain.unwrap();
    assert_eq!(drain.input_state, HandyHspInputState::TailReached);
    assert_eq!(drain.captured_at_ns, terminal.captured_at_ns);
    assert_eq!(drain.accepted_at_ns, terminal.accepted_at_ns);
    assert_eq!(drain.closed_at_boottime_ns, closed_at);
    assert_eq!(drain.expires_at_boottime_ns, seal.expires_at_ns());
    assert!(!drain.aborted && !drain.stop_deadline_missed);
    assert_eq!(selected.fixed_reads.load(Ordering::Acquire), 0);
    assert!(selected.snapshot_reads.load(Ordering::Acquire) > 0);
    rig.f.engine.finish_shutdown().unwrap();
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
}
