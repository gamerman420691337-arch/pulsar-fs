// Supplied authored clock phases and a localhost transport exercise owner
// liveness only; this is not source/device or physical timing qualification.
#[test]
fn engine_hsp_finite_tail_stops_while_final_ack_metadata_is_held_and_never_resends() {
    use crate::handy_hsp_execution::HspMetadataTaskKind;
    let rig = Rig::with_finite_budget(false, None, 500_000_000);
    rig.ready();
    rig.offer(1);
    rig.offer(2);
    rig.wait("prefill metadata settled", |s| {
        s.total_added == 2 && !s.metadata_pending && !s.native_active
    });
    rig.session.request_play().unwrap();
    rig.wait("actual original Play ACK joined", |s| {
        s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::Playing
            && !s.metadata_pending
            && !s.native_active
    });
    let hold = rig
        .session
        .hold_next_metadata_for_test(HspMetadataTaskKind::Complete);
    let at = crate::handy_clock::boottime_ns().unwrap();
    let terminal = HspSample {
        point: HspPoint::new(200, 20).unwrap(),
        source_sequence: 3,
        captured_at_ns: at,
        accepted_at_ns: at,
        continuous_from_previous: true,
    };
    let until = Instant::now() + WAIT;
    loop {
        match rig.session.offer(&rig.binding.identity().live, terminal) {
            Ok(HspOfferOutcome::Accepted) => break,
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2))
            }
            other => panic!("original terminal offer failed {other:?}"),
        }
    }
    let task = hold.wait_entered(Instant::now() + WAIT).unwrap();
    assert_eq!(task.sequence, Some(4));
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
            at,
            policy,
            400_000_000,
        ) {
            Ok(seal) => break seal,
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2))
            }
            other => panic!("original input seal failed {other:?}"),
        }
    };
    rig.wait("finite drain must not wait for final ACK metadata", |s| {
        s.metadata_pending
            && s.finite_drain
                .as_ref()
                .is_some_and(|d| d.input_state == HandyHspInputState::Draining)
    });
    assert!(!hold.joined());
    let stopped = rig.stopped();
    let drain = stopped.finite_drain.unwrap();
    assert_eq!(drain.input_state, HandyHspInputState::TailReached);
    assert_eq!(drain.captured_at_ns, terminal.captured_at_ns);
    assert_eq!(drain.accepted_at_ns, terminal.accepted_at_ns);
    assert_eq!(drain.closed_at_boottime_ns, at);
    assert_eq!(drain.expires_at_boottime_ns, seal.expires_at_ns());
    assert!(
        drain.tail_reached_at_boottime_ns.is_some()
            && !drain.aborted
            && !drain.stop_deadline_missed
    );
    assert!(
        drain.stop_dispatched_at_boottime_ns.unwrap()
            <= drain.stop_dispatch_deadline_boottime_ns.unwrap()
    );
    assert!(stopped.metadata_pending && !hold.joined());
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
    hold.release();
    rig.wait("late metadata joins without transport replay", |s| {
        !s.metadata_pending
    });
    assert!(hold.joined());
    assert!(rig.session.request_play().is_err());
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

#[test]
fn engine_hsp_finite_drain_expiry_keeps_original_tail_but_still_dispatches_stop() {
    let rig = Rig::with_finite_budget(false, None, 500_000_000);
    rig.ready();
    let mut terminal = None;
    for sequence in 1..=2 {
        let at = crate::handy_clock::boottime_ns().unwrap();
        let original = HspSample {
            point: HspPoint::new((sequence as u32 - 1) * 100, 20).unwrap(),
            source_sequence: sequence,
            captured_at_ns: at,
            accepted_at_ns: at,
            continuous_from_previous: true,
        };
        let until = Instant::now() + WAIT;
        loop {
            match rig.session.offer(&rig.binding.identity().live, original) {
                Ok(HspOfferOutcome::Accepted) => break,
                Err(HspExecutionError::Busy) if Instant::now() < until => {
                    thread::park_timeout(Duration::from_millis(2))
                }
                other => panic!("original finite sample failed {other:?}"),
            }
        }
        terminal = Some(original);
    }
    rig.wait("original finite prefill joined", |s| {
        s.total_added == 2 && !s.metadata_pending && !s.native_active
    });
    rig.session.request_play().unwrap();
    rig.wait("actual Play ACK joined", |s| {
        s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::Playing && !s.native_active
    });
    let terminal = terminal.unwrap();
    let closed_at = crate::handy_clock::boottime_ns().unwrap();
    let policy = pulsar_core::HspFiniteDrainPolicy {
        maximum_duration_ns: 4_000_000_000,
        maximum_terminal_overrun_ns: 750_000_000,
        stop_budget_ns: 500_000_000,
    };
    let until = Instant::now() + WAIT;
    loop {
        match rig.session.seal_input(
            &rig.binding.identity().live,
            terminal,
            closed_at,
            policy,
            400_000_000,
        ) {
            Ok(_) => break,
            Err(HspExecutionError::Busy) if Instant::now() < until => {
                thread::park_timeout(Duration::from_millis(2))
            }
            other => panic!("original finite seal failed {other:?}"),
        }
    }
    rig.wait("finite input entered actual drain", |s| {
        s.finite_drain
            .as_ref()
            .is_some_and(|d| d.input_state == HandyHspInputState::Draining)
    });
    rig.authority.lease_until.store(
        crate::handy_clock::boottime_ns().unwrap(),
        Ordering::Release,
    );
    let stopped = rig.stopped();
    let drain = stopped.finite_drain.unwrap();
    assert_eq!(drain.input_state, HandyHspInputState::Draining);
    assert!(drain.aborted);
    assert_eq!(drain.closed_at_boottime_ns, closed_at);
    assert_eq!(drain.accepted_at_ns, terminal.accepted_at_ns);
    assert!(drain.tail_reached_at_boottime_ns.is_none());
    assert!(drain.stop_dispatch_deadline_boottime_ns.is_none());
    assert_eq!(
        rig.peer.puts(),
        ["/hsp/setup", "/hsp/add", "/hsp/play", "/hsp/stop"]
    );
    rig.f.engine.finish_shutdown().unwrap();
}
