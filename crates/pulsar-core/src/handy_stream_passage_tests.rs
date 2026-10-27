#[test]
fn passage_accepts_recorded_overlap_and_keeps_raw_context() {
    let mut stream = stream();
    let old_now = 102_248_034_046_617;
    let new_now = 102_248_034_099_945;
    stream.tick(old_now, phase(10_999, 11_056)).unwrap();
    let fresh = phase(10_998, 11_056);
    stream.tick(new_now, fresh).unwrap();
    assert_eq!(stream.last_context, Some((new_now, fresh)));
    stream.tick(new_now + 1, phase(10_997, 10_999)).unwrap();
    // The original passage remains known after two weaker observations.
    assert_eq!(
        stream.tick(new_now + 2, phase(10_997, 10_998)),
        Err(HspStreamError::ClockRegression)
    );
}

#[test]
fn passage_preserves_irreversible_ring_predecessor_after_widening() {
    let mut stream = running();
    stream.tick(10_001, phase(100, 150)).unwrap();
    assert_eq!(stream.retained_points(), 3);
    stream.tick(10_002, phase(101, 150)).unwrap();
    assert_eq!(stream.retained_points(), 2);
    assert_eq!(stream.retained.front().unwrap().sample.point.time_ms, 100);
    stream.tick(10_003, phase(90, 150)).unwrap();
    stream.tick(10_004, phase(80, 101)).unwrap();
    assert_eq!(stream.retained_points(), 2);
    assert_eq!(stream.retained.front().unwrap().sample.point.time_ms, 100);
    assert_eq!(stream.last_context, Some((10_004, phase(80, 101))));
    assert_eq!(
        stream.tick(10_005, phase(80, 100)),
        Err(HspStreamError::ClockRegression)
    );
    assert_eq!(
        stream.state(),
        HspStreamState::StopRequired(HspStreamError::ClockRegression)
    );
    assert_eq!(stream.retained_points(), 2);
}

#[test]
fn passage_does_not_hide_native_regression_or_leak_to_new_stream() {
    let mut current = running();
    current.tick(10_001, phase(100, 150)).unwrap();
    current.tick(10_002, phase(90, 150)).unwrap();
    assert_eq!(
        current.tick(10_001, phase(101, 150)),
        Err(HspStreamError::ClockRegression)
    );
    let mut fresh = HspStream::new(
        HspStreamIdentity {
            generation: identity().generation + 1,
            ..identity()
        },
        policy(),
    )
    .unwrap();
    fresh.tick(10_003, phase(-100, -50)).unwrap();
    assert_eq!(fresh.state(), HspStreamState::Priming);
}

#[test]
fn passage_keeps_raw_lead_horizon_and_starvation_checks() {
    let mut lead = stream();
    lead.offer(&identity().live, sample(0, 500), 10_000, phase(100, 150))
        .unwrap();
    assert_eq!(
        lead.offer(&identity().live, sample(1, 510), 10_001, phase(90, 505)),
        Err(HspStreamError::InsufficientLead)
    );
    assert_eq!(lead.pending_points(), 1);

    let mut horizon = stream();
    horizon
        .offer(&identity().live, sample(0, 1000), 10_000, phase(100, 110))
        .unwrap();
    // Using historical passage as the fresh lower endpoint would accept this.
    assert_eq!(
        horizon.offer(&identity().live, sample(1, 1050), 10_001, phase(0, 110)),
        Err(HspStreamError::FutureHorizon)
    );
    assert_eq!(horizon.pending_points(), 1);

    let mut starved = running();
    starved.tick(10_001, phase(100, 150)).unwrap();
    assert_eq!(
        starved.tick(10_002, phase(90, 190)),
        Err(HspStreamError::Starved)
    );
    assert_eq!(
        starved.tick(10_003, phase(100, 150)),
        Err(HspStreamError::Frozen)
    );
}

#[test]
fn passage_preserves_same_coordinate_transfer_and_original_chunk_samples() {
    let (mut stream, original) = transition_running();
    let original_samples = original.samples().to_vec();
    let original_token = original.token().clone();
    stream.tick(10_001, phase(100, 150)).unwrap();
    let first = transition_sample(300, 1, 3_000, false);
    let spec = transition_spec(&stream, "passage-b", "passage-worker-b", first, 30_000);
    let successor = spec.successor.clone();
    let transition = stream
        .prepare_source_transition(spec, 10_002, phase(90, 150))
        .unwrap();
    assert_eq!(transition.first_successor(), first);
    assert_eq!(transition.expires_at_ns(), 30_000);
    stream
        .commit_source_transition(transition, 10_003, phase(80, 150))
        .unwrap();
    assert_eq!(stream.active_source(), Some(&successor));
    let next = stream.claim(10_004, phase(70, 150)).unwrap().unwrap();
    assert_eq!(next.samples(), &[first]);
    assert_eq!(next.source_cursor(), Some(&successor));
    assert_eq!(original.token(), &original_token);
    assert_eq!(original.samples(), original_samples.as_slice());
    assert_eq!(stream.identity(), &identity());
    assert_eq!(
        stream.tick(10_005, phase(70, 99)),
        Err(HspStreamError::ClockRegression)
    );
}

fn passage_drain_window(
    now: u64,
    current: HspPhaseWindow,
    projected: HspPhaseWindow,
) -> HspFiniteDrainWindow {
    HspFiniteDrainWindow {
        phase: current,
        stop_phase: projected,
        ..finite_window(now, current.earliest_ms, current.latest_ms)
    }
}

#[test]
fn passage_keeps_future_projection_separate_from_current_tail() {
    let (mut stream, seal) = finite_draining();
    let first = passage_drain_window(20_000, phase(100, 110), phase(200, 210));
    assert_eq!(
        stream.tick_finite_drain(&seal, first.at_ns, first),
        Ok(HspDrainProgress::Draining)
    );
    let wider = passage_drain_window(30_000, phase(99, 110), phase(199, 211));
    assert_eq!(
        stream.tick_finite_drain(&seal, wider.at_ns, wider),
        Ok(HspDrainProgress::Draining)
    );
    assert_eq!(stream.last_context, Some((wider.at_ns, wider.phase)));
    assert_eq!(stream.last_drain_window, Some(wider));
    assert_eq!(stream.retained_points(), 3);
    let near = passage_drain_window(40_000, phase(198, 205), phase(208, 215));
    assert_eq!(
        stream.tick_finite_drain(&seal, near.at_ns, near),
        Ok(HspDrainProgress::Draining)
    );
    let terminal = passage_drain_window(50_000, phase(200, 207), phase(210, 217));
    let HspDrainProgress::TailReached(observation) = stream
        .tick_finite_drain(&seal, terminal.at_ns, terminal)
        .unwrap()
    else {
        panic!("only current passage establishes the terminal");
    };
    assert_eq!(observation.seal(), &seal);
    assert_eq!(observation.window(), terminal);
    assert_eq!(observation.reached_at_ns(), terminal.at_ns);
    assert_eq!(seal.terminal(), sample(2, 200));
    assert_eq!(seal.expires_at_ns(), 1_000_010_000);
}

#[test]
fn passage_rejects_projected_contradiction_after_wider_projections() {
    let (mut stream, seal) = finite_draining();
    for (now, current, projected) in [
        (20_000, phase(100, 110), phase(200, 210)),
        (30_000, phase(99, 110), phase(199, 211)),
    ] {
        let window = passage_drain_window(now, current, projected);
        assert_eq!(
            stream.tick_finite_drain(&seal, now, window),
            Ok(HspDrainProgress::Draining)
        );
    }
    let contradiction = passage_drain_window(40_000, phase(98, 110), phase(198, 199));
    assert_eq!(
        stream.tick_finite_drain(&seal, contradiction.at_ns, contradiction),
        Err(HspStreamError::ClockRegression)
    );
    assert_eq!(
        stream.state(),
        HspStreamState::StopRequired(HspStreamError::ClockRegression)
    );
}

#[test]
fn passage_drain_still_refuses_native_time_and_projected_target_reversal() {
    let (mut stream, seal) = finite_draining();
    let first = passage_drain_window(20_000, phase(100, 110), phase(200, 210));
    stream.tick_finite_drain(&seal, first.at_ns, first).unwrap();
    let earlier = passage_drain_window(19_999, phase(100, 110), phase(200, 210));
    assert!(earlier.stop_at_ns < first.stop_at_ns);
    assert_eq!(
        stream.tick_finite_drain(&seal, earlier.at_ns, earlier),
        Err(HspStreamError::ClockRegression)
    );

    let (mut stream, seal) = finite_draining();
    stream.tick_finite_drain(&seal, first.at_ns, first).unwrap();
    let mut wrong_target = passage_drain_window(30_000, phase(100, 110), phase(200, 210));
    wrong_target.stop_at_ns = first.stop_at_ns - 1;
    assert_eq!(
        stream.tick_finite_drain(&seal, wrong_target.at_ns, wrong_target),
        Err(HspStreamError::InvalidDrainWindow)
    );
}

#[test]
fn passage_does_not_extend_drain_expiry_or_relax_fresh_upper_overrun() {
    for expired in [false, true] {
        let (mut stream, seal) = finite_draining();
        let first = passage_drain_window(20_000, phase(100, 110), phase(200, 210));
        stream.tick_finite_drain(&seal, first.at_ns, first).unwrap();
        let now = if expired {
            seal.expires_at_ns()
        } else {
            30_000
        };
        let next = passage_drain_window(
            now,
            phase(99, 110),
            phase(199, if expired { 211 } else { 251 }),
        );
        let error = if expired {
            HspStreamError::FiniteDrainExpired
        } else {
            HspStreamError::TerminalOverrun
        };
        assert_eq!(stream.tick_finite_drain(&seal, now, next), Err(error));
        assert_eq!(stream.state(), HspStreamState::StopRequired(error));
        assert_eq!(seal.expires_at_ns(), 1_000_010_000);
    }
}
