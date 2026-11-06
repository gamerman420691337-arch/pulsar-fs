fn finite_policy() -> HspFiniteDrainPolicy {
    HspFiniteDrainPolicy {
        maximum_duration_ns: 1_000_000_000,
        maximum_terminal_overrun_ns: 50_000_000,
        stop_budget_ns: 10_000_000,
    }
}

fn finite_window(now_ns: u64, earliest_ms: i64, latest_ms: i64) -> HspFiniteDrainWindow {
    HspFiniteDrainWindow {
        at_ns: now_ns,
        phase: phase(earliest_ms, latest_ms),
        stop_at_ns: now_ns.checked_add(finite_policy().stop_budget_ns).unwrap(),
        stop_phase: phase(earliest_ms + 10, latest_ms + 10),
    }
}

fn seal_running(stream: &mut HspStream) -> HspInputSeal {
    stream
        .seal_input(
            &identity().live,
            sample(2, 200),
            10_000,
            finite_policy(),
            10_000,
            phase(-20, -10),
        )
        .unwrap()
}

fn finite_draining() -> (HspStream, HspInputSeal) {
    let mut stream = running();
    let seal = seal_running(&mut stream);
    assert_eq!(
        stream.begin_finite_drain(&seal, 10_000, finite_window(10_000, -20, -10)),
        Ok(HspDrainProgress::Draining)
    );
    (stream, seal)
}

#[test]
fn finite_zero_and_one_point_never_manufacture_playable_prefill() {
    let mut empty = stream();
    assert_eq!(
        empty.seal_input(
            &identity().live,
            sample(0, 0),
            10_000,
            finite_policy(),
            10_000,
            prefill(),
        ),
        Err(HspStreamError::InvalidTerminal)
    );
    assert_eq!(empty.input_state(), HspInputState::Open);
    let mut one = stream();
    offer(&mut one, 0, 0, prefill());
    let seal = one
        .seal_input(
            &identity().live,
            sample(0, 0),
            10_000,
            finite_policy(),
            10_000,
            prefill(),
        )
        .unwrap();
    assert_eq!(one.claim(10_001, prefill()), Ok(None));
    assert_eq!(
        one.mark_running(10_001, prefill()),
        Err(HspStreamError::NotReady)
    );
    assert_eq!(
        one.begin_finite_drain(&seal, 10_001, finite_window(10_001, -100, -50)),
        Err(HspStreamError::NotReady)
    );
    assert_eq!(one.pending_points(), 1);
    assert_eq!(one.total_added(), 0);
}

#[test]
fn finite_seal_replay_preserves_original_deadline_and_blocks_all_offers() {
    let mut stream = running();
    let seal = seal_running(&mut stream);
    let before = (
        stream.buffered_points(),
        stream.total_added(),
        stream.last_context,
    );
    let replay = stream
        .seal_input(
            &identity().live,
            seal.terminal(),
            seal.closed_at_ns(),
            seal.policy(),
            20_000,
            phase(0, 10),
        )
        .unwrap();
    assert_eq!(seal, replay);
    assert_eq!(seal.expires_at_ns(), 1_000_010_000);
    assert_eq!(seal.identity(), &identity());
    assert_eq!(stream.last_context, before.2);
    assert_eq!(
        stream.offer(&identity().live, sample(3, 300), 20_000, phase(0, 10)),
        Err(HspStreamError::InputSealed)
    );
    assert_eq!(
        (stream.buffered_points(), stream.total_added()),
        (before.0, before.1)
    );
    assert_eq!(stream.claim(20_000, phase(0, 10)), Ok(None));
    assert_eq!(stream.state(), HspStreamState::Running);
}

#[test]
fn finite_final_acceptance_after_input_close_does_not_renew_expiry() {
    let mut stream = running();
    let closed_at_ns = 1_500;
    let terminal = sample(2, 200);
    assert!(terminal.captured_at_ns < closed_at_ns);
    assert!(terminal.accepted_at_ns > closed_at_ns);
    let seal = stream
        .seal_input(
            &identity().live,
            terminal,
            closed_at_ns,
            finite_policy(),
            10_000,
            phase(-20, -10),
        )
        .unwrap();
    assert_eq!(seal.closed_at_ns(), closed_at_ns);
    assert_eq!(
        seal.expires_at_ns(),
        closed_at_ns + finite_policy().maximum_duration_ns
    );
    assert_eq!(seal.terminal(), terminal);
    assert_eq!(
        stream.seal_input(
            &identity().live,
            terminal,
            closed_at_ns,
            finite_policy(),
            seal.expires_at_ns(),
            phase(0, 10),
        ),
        Err(HspStreamError::FiniteDrainExpired)
    );
    assert_eq!(
        stream.state(),
        HspStreamState::StopRequired(HspStreamError::FiniteDrainExpired)
    );
}

#[test]
fn finite_altered_or_foreign_seal_cannot_change_original_stream() {
    let mut stream = running();
    let seal = seal_running(&mut stream);
    for (terminal, closed_at_ns, policy) in [
        (sample(2, 201), seal.closed_at_ns(), seal.policy()),
        (seal.terminal(), seal.closed_at_ns() + 1, seal.policy()),
        (
            seal.terminal(),
            seal.closed_at_ns(),
            HspFiniteDrainPolicy {
                maximum_duration_ns: seal.policy().maximum_duration_ns + 1,
                ..seal.policy()
            },
        ),
    ] {
        assert_eq!(
            stream.seal_input(
                &identity().live,
                terminal,
                closed_at_ns,
                policy,
                20_000,
                phase(0, 10)
            ),
            Err(HspStreamError::ForeignSeal)
        );
    }
    let mut foreign_identity = identity().live;
    foreign_identity.epoch = AttemptId::new("foreign").unwrap();
    assert_eq!(
        stream.seal_input(
            &foreign_identity,
            seal.terminal(),
            10_000,
            seal.policy(),
            20_000,
            phase(0, 10)
        ),
        Err(HspStreamError::ForeignSource)
    );
    let mut same_named_stream = running();
    let foreign = seal_running(&mut same_named_stream);
    assert_ne!(seal, foreign);
    let mut altered = seal.clone();
    altered.terminal.accepted_at_ns += 1;
    for invalid in [foreign, altered] {
        assert_eq!(
            stream.begin_finite_drain(&invalid, 20_000, finite_window(20_000, 0, 10)),
            Err(HspStreamError::ForeignSeal)
        );
    }
    assert_eq!(stream.last_context, Some((10_000, phase(-20, -10))));
    assert_eq!(stream.input_seal.as_ref(), Some(&seal));
}

#[test]
fn finite_pending_terminal_requires_exact_guarded_ack_before_begin() {
    let mut stream = running();
    offer(&mut stream, 3, 300, phase(-20, -10));
    let seal = stream
        .seal_input(
            &identity().live,
            sample(3, 300),
            10_000,
            finite_policy(),
            10_000,
            phase(-20, -10),
        )
        .unwrap();
    assert_eq!(
        stream.begin_finite_drain(&seal, 10_000, finite_window(10_000, -20, -10)),
        Err(HspStreamError::NotReady)
    );
    let chunk = stream.claim(10_000, phase(-20, -10)).unwrap().unwrap();
    assert_eq!(chunk.samples(), &[sample(3, 300)]);
    assert_eq!(
        stream.begin_finite_drain(&seal, 10_001, finite_window(10_001, 0, 10)),
        Err(HspStreamError::NotReady)
    );
    assert_eq!(
        stream.complete_at(
            chunk.token(),
            HspAppendOutcome::Acknowledged {
                stream_id: chunk.stream_id(),
                tail_point_stream_index: chunk.tail_point_stream_index(),
            },
            10_002,
            phase(0, 10)
        ),
        Ok(HspCompletionDisposition::Advanced)
    );
    assert_eq!(
        stream.begin_finite_drain(&seal, 10_003, finite_window(10_003, 0, 10)),
        Ok(HspDrainProgress::Draining)
    );
    assert_eq!(stream.total_added(), 4);
    assert_eq!(stream.pending_points(), 0);
    assert!(stream.in_flight().is_none());
}

#[test]
fn finite_unacknowledged_and_unknown_terminal_never_suppress_starvation() {
    for unknown in [false, true] {
        let mut stream = running();
        offer(&mut stream, 3, 300, phase(-20, -10));
        let seal = stream
            .seal_input(
                &identity().live,
                sample(3, 300),
                10_000,
                finite_policy(),
                10_000,
                phase(-20, -10),
            )
            .unwrap();
        if unknown {
            let chunk = stream.claim(10_000, phase(-20, -10)).unwrap().unwrap();
            assert_eq!(
                stream.complete_at(
                    chunk.token(),
                    HspAppendOutcome::Unknown,
                    10_001,
                    phase(0, 10)
                ),
                Ok(HspCompletionDisposition::Retained)
            );
            assert_eq!(stream.in_flight_outcome(), Some(HspAppendOutcome::Unknown));
            assert_eq!(
                stream.state(),
                HspStreamState::StopRequired(HspStreamError::UnknownAppend)
            );
        } else {
            assert_eq!(
                stream.tick(10_001, phase(180, 190)),
                Err(HspStreamError::Starved)
            );
            assert_eq!(stream.pending_points(), 1);
        }
        assert_eq!(
            stream.begin_finite_drain(&seal, 10_002, finite_window(10_002, 180, 190)),
            Err(HspStreamError::Frozen)
        );
        assert_eq!(stream.total_added(), 3);
    }
}

#[test]
fn finite_late_final_ack_checks_old_coverage_and_retains_exact_observation() {
    let mut stream = running();
    offer(&mut stream, 3, 300, phase(-20, -10));
    let seal = stream
        .seal_input(
            &identity().live,
            sample(3, 300),
            10_000,
            finite_policy(),
            10_000,
            phase(-20, -10),
        )
        .unwrap();
    let chunk = stream.claim(10_000, phase(-20, -10)).unwrap().unwrap();
    let outcome = HspAppendOutcome::Acknowledged {
        stream_id: chunk.stream_id(),
        tail_point_stream_index: chunk.tail_point_stream_index(),
    };
    assert_eq!(
        stream.complete_at(chunk.token(), outcome, 10_001, phase(180, 190)),
        Err(HspStreamError::Starved)
    );
    assert_eq!(stream.in_flight_outcome(), Some(outcome));
    assert_eq!(stream.total_added(), 3);
    assert_eq!(
        stream.retained.back().map(|retained| retained.sample),
        Some(sample(2, 200))
    );
    assert_eq!(
        stream.complete_at(chunk.token(), outcome, 10_002, phase(180, 190)),
        Err(HspStreamError::AlreadyCompleted)
    );
    assert_eq!(
        stream.begin_finite_drain(&seal, 10_003, finite_window(10_003, 180, 190)),
        Err(HspStreamError::Frozen)
    );
}

#[test]
fn finite_unguarded_terminal_ack_is_retained_without_advancing() {
    let mut stream = running();
    offer(&mut stream, 3, 300, phase(-20, -10));
    stream
        .seal_input(
            &identity().live,
            sample(3, 300),
            10_000,
            finite_policy(),
            10_000,
            phase(-20, -10),
        )
        .unwrap();
    let chunk = stream.claim(10_000, phase(-20, -10)).unwrap().unwrap();
    let outcome = HspAppendOutcome::Acknowledged {
        stream_id: chunk.stream_id(),
        tail_point_stream_index: chunk.tail_point_stream_index(),
    };
    assert_eq!(
        stream.complete(chunk.token(), outcome),
        Err(HspStreamError::DrainWindowRequired)
    );
    assert_eq!(stream.in_flight_outcome(), Some(outcome));
    assert_eq!(stream.total_added(), 3);
}

#[test]
fn finite_begin_cannot_rescue_already_starved_acknowledged_coverage() {
    let mut stream = running();
    let seal = seal_running(&mut stream);
    assert_eq!(
        stream.begin_finite_drain(&seal, 10_001, finite_window(10_001, 180, 190)),
        Err(HspStreamError::Starved)
    );
    assert_eq!(stream.input_state(), HspInputState::Sealed);
    assert_eq!(
        stream.state(),
        HspStreamState::StopRequired(HspStreamError::Starved)
    );
}

#[test]
fn finite_earliest_terminal_crossing_returns_stable_typed_observation() {
    let (mut stream, seal) = finite_draining();
    assert_eq!(
        stream.tick_finite_drain(&seal, 20_000, finite_window(20_000, 189, 199)),
        Ok(HspDrainProgress::Draining)
    );
    assert_eq!(
        stream.tick_finite_drain(&seal, 30_000, finite_window(30_000, 199, 210)),
        Ok(HspDrainProgress::Draining)
    );
    let reached_window = finite_window(40_000, 200, 210);
    let HspDrainProgress::TailReached(observation) = stream
        .tick_finite_drain(&seal, 40_000, reached_window)
        .unwrap()
    else {
        panic!("earliest phase crossed original terminal")
    };
    assert_eq!(observation.seal(), &seal);
    assert_eq!(observation.terminal(), sample(2, 200));
    assert_eq!(observation.reached_at_ns(), 40_000);
    assert_eq!(observation.window(), reached_window);
    assert_eq!(stream.input_state(), HspInputState::TailReached);
    assert_eq!(
        stream.tick_finite_drain(&seal, 50_000, finite_window(50_000, 201, 211)),
        Ok(HspDrainProgress::TailReached(observation.clone()))
    );
    assert_eq!(stream.total_added(), 3);
    assert_eq!(stream.last_offered, Some(sample(2, 200)));
    assert!(stream.in_flight().is_none());
    assert_eq!(stream.pending_points(), 0);
    stream.cancel();
    assert_eq!(stream.input_seal.as_ref(), Some(&seal));
    assert_eq!(stream.tail_observation.as_ref(), Some(&observation));
    assert_eq!(
        stream.tick_finite_drain(&seal, 60_000, finite_window(60_000, 202, 212)),
        Err(HspStreamError::Frozen)
    );
}

#[test]
fn finite_projected_stop_bound_is_inclusive_and_checked_before_passage() {
    let (mut edge, seal) = finite_draining();
    assert!(matches!(
        edge.tick_finite_drain(&seal, 20_000, finite_window(20_000, 200, 240)),
        Ok(HspDrainProgress::TailReached(_))
    ));
    let (mut overrun, seal) = finite_draining();
    assert_eq!(
        overrun.tick_finite_drain(&seal, 20_000, finite_window(20_000, 200, 241)),
        Err(HspStreamError::TerminalOverrun)
    );
    assert_eq!(
        overrun.state(),
        HspStreamState::StopRequired(HspStreamError::TerminalOverrun)
    );
    assert!(overrun.tail_observation.is_none());
}

#[test]
fn finite_expiry_wins_at_terminal_crossing_and_cannot_be_replayed() {
    let (mut stream, seal) = finite_draining();
    let expiry = seal.expires_at_ns();
    assert_eq!(
        stream.tick_finite_drain(&seal, expiry, finite_window(expiry, 200, 200)),
        Err(HspStreamError::FiniteDrainExpired)
    );
    assert!(stream.tail_observation.is_none());
    assert_eq!(
        stream.seal_input(
            &identity().live,
            seal.terminal(),
            seal.closed_at_ns(),
            seal.policy(),
            expiry + 1,
            phase(200, 200)
        ),
        Err(HspStreamError::Frozen)
    );

    let (mut before, seal) = finite_draining();
    assert!(matches!(
        before.tick_finite_drain(
            &seal,
            seal.expires_at_ns() - 1,
            finite_window(seal.expires_at_ns() - 1, 200, 200)
        ),
        Ok(HspDrainProgress::TailReached(_))
    ));
}

#[test]
fn finite_windows_require_exact_times_and_compatible_passage() {
    for invalid in 0..6 {
        let (mut stream, seal) = finite_draining();
        let mut window = finite_window(20_000, 0, 10);
        let expected = match invalid {
            0 => {
                window.at_ns += 1;
                HspStreamError::InvalidDrainWindow
            }
            1 => {
                window.stop_at_ns += 1;
                HspStreamError::InvalidDrainWindow
            }
            2 => {
                window.stop_phase = phase(-1, 20);
                HspStreamError::InvalidDrainWindow
            }
            3 => {
                window.phase = phase(11, 10);
                HspStreamError::InvalidPhase
            }
            4 => {
                window.stop_phase = phase(21, 20);
                HspStreamError::InvalidPhase
            }
            _ => {
                window.phase = phase(-21, -21);
                HspStreamError::ClockRegression
            }
        };
        assert_eq!(
            stream.tick_finite_drain(&seal, 20_000, window),
            Err(expected)
        );
        assert_eq!(stream.state(), HspStreamState::StopRequired(expected));
    }
    let (mut stream, seal) = finite_draining();
    let mut earlier = finite_window(9_999, 0, 10);
    assert_eq!(
        stream.tick_finite_drain(&seal, 9_999, earlier),
        Err(HspStreamError::ClockRegression)
    );
    let (mut projected, seal) = finite_draining();
    earlier = finite_window(20_000, 0, 10);
    earlier.stop_phase = phase(20, 30);
    projected.tick_finite_drain(&seal, 20_000, earlier).unwrap();
    assert_eq!(
        projected.tick_finite_drain(&seal, 30_000, finite_window(30_000, 0, 9)),
        Err(HspStreamError::ClockRegression)
    );
}

#[test]
fn finite_policy_and_deadline_arithmetic_fail_closed() {
    for policy in [
        HspFiniteDrainPolicy {
            maximum_duration_ns: 0,
            ..finite_policy()
        },
        HspFiniteDrainPolicy {
            maximum_terminal_overrun_ns: 0,
            ..finite_policy()
        },
        HspFiniteDrainPolicy {
            stop_budget_ns: 0,
            ..finite_policy()
        },
        HspFiniteDrainPolicy {
            stop_budget_ns: 50_000_001,
            ..finite_policy()
        },
    ] {
        assert_eq!(policy.validate(), Err(HspStreamError::InvalidPolicy));
    }
    let mut stream = running();
    assert_eq!(
        stream.seal_input(
            &identity().live,
            sample(2, 200),
            u64::MAX - 1,
            finite_policy(),
            u64::MAX - 1,
            phase(-20, -10)
        ),
        Err(HspStreamError::ArithmeticOverflow)
    );
    assert_eq!(
        stream.state(),
        HspStreamState::StopRequired(HspStreamError::ArithmeticOverflow)
    );

    let mut overflow = running();
    let policy = HspFiniteDrainPolicy {
        maximum_duration_ns: u64::MAX - 10_000,
        ..finite_policy()
    };
    let seal = overflow
        .seal_input(
            &identity().live,
            sample(2, 200),
            10_000,
            policy,
            10_000,
            phase(-20, -10),
        )
        .unwrap();
    overflow
        .begin_finite_drain(&seal, 10_000, finite_window(10_000, -20, -10))
        .unwrap();
    let now_ns = u64::MAX - 1;
    let window = HspFiniteDrainWindow {
        at_ns: now_ns,
        phase: phase(200, 200),
        stop_at_ns: u64::MAX,
        stop_phase: phase(210, 210),
    };
    assert_eq!(
        overflow.tick_finite_drain(&seal, now_ns, window),
        Err(HspStreamError::ArithmeticOverflow)
    );
    assert!(overflow.tail_observation.is_none());
}

#[test]
fn finite_drain_never_uses_ordinary_tick_or_another_append() {
    let (mut stream, seal) = finite_draining();
    assert_eq!(
        stream.offer(&identity().live, sample(3, 300), 20_000, phase(0, 10)),
        Err(HspStreamError::InputSealed)
    );
    assert_eq!(
        stream.claim(20_000, phase(0, 10)),
        Err(HspStreamError::DrainWindowRequired)
    );
    assert!(stream.in_flight().is_none());
    assert_eq!(stream.total_added(), 3);
    assert_eq!(
        stream.tick_finite_drain(&seal, 20_000, finite_window(20_000, 0, 10)),
        Err(HspStreamError::Frozen)
    );
    let (mut stream, _) = finite_draining();
    assert_eq!(
        stream.tick(20_000, phase(0, 10)),
        Err(HspStreamError::DrainWindowRequired)
    );
}

#[test]
fn phase_upper_refinement_preserves_current_append_and_original_sample_times() {
    let mut stream = HspStream::new(
        identity(),
        HspPolicy {
            max_future_ms: 2_000,
            ..policy()
        },
    )
    .unwrap();
    for sequence in 0..3 {
        offer(
            &mut stream,
            sequence,
            sequence as u32 * 100,
            phase(-900, -880),
        );
    }
    let prefill = stream.claim(10_000, phase(-900, -880)).unwrap().unwrap();
    acknowledge(&mut stream, &prefill);
    stream.mark_running(20_000, phase(-876, -859)).unwrap();

    let terminal = sample(3, 300);
    assert_eq!(
        stream.offer(&identity().live, terminal, 20_000, phase(-876, -859)),
        Ok(HspOfferOutcome::Accepted)
    );
    let original_append = stream.claim(20_000, phase(-876, -859)).unwrap().unwrap();
    let original_token = original_append.token().clone();
    assert_eq!(original_append.samples(), &[terminal]);

    // Native repetition 009 advanced 26,554 ns while its independently
    // calculated interval tightened from [-876, -859] to [-875, -860].
    // The upper endpoint is an uncertainty bound, not a progress counter.
    assert_eq!(stream.tick(46_554, phase(-875, -860)), Ok(()));
    assert_eq!(stream.state(), HspStreamState::Running);
    assert_eq!(stream.in_flight(), Some(original_append.as_ref()));
    assert_eq!(original_append.token(), &original_token);
    assert_eq!(original_append.samples(), &[sample(3, 300)]);
    assert_eq!(stream.total_added(), 3);
    assert_eq!(
        stream.complete_at(
            &original_token,
            HspAppendOutcome::Acknowledged {
                stream_id: original_append.stream_id(),
                tail_point_stream_index: original_append.tail_point_stream_index(),
            },
            46_554,
            phase(-875, -860),
        ),
        Ok(HspCompletionDisposition::Advanced)
    );
    let seal = stream
        .seal_input(
            &identity().live,
            terminal,
            20_000,
            finite_policy(),
            47_000,
            phase(-875, -860),
        )
        .unwrap();
    assert_eq!(seal.terminal(), sample(3, 300));
    assert_eq!(seal.closed_at_ns(), 20_000);
    assert_eq!(seal.expires_at_ns(), 1_000_020_000);
    assert_eq!(stream.total_added(), 4);
    assert!(stream.in_flight().is_none());
}

#[test]
fn phase_upper_refinement_preserves_finite_stop_projection_and_overrun_limit() {
    let (mut stream, seal) = finite_draining();
    let first = HspFiniteDrainWindow {
        stop_phase: phase(120, 150),
        ..finite_window(20_000, 100, 110)
    };
    assert_eq!(
        stream.tick_finite_drain(&seal, 20_000, first),
        Ok(HspDrainProgress::Draining)
    );
    let refined = HspFiniteDrainWindow {
        stop_phase: phase(121, 149),
        ..finite_window(46_554, 101, 111)
    };
    // Current bounds and the future evaluation instant both advance; only
    // the fresh projected upper uncertainty bound decreases.
    assert_eq!(
        stream.tick_finite_drain(&seal, 46_554, refined),
        Ok(HspDrainProgress::Draining)
    );
    assert_eq!(stream.state(), HspStreamState::Running);
    assert_eq!(stream.input_state(), HspInputState::Draining);
    assert_eq!(stream.total_added(), 3);
    assert_eq!(stream.pending_points(), 0);
    assert!(stream.in_flight().is_none());
    assert_eq!(seal.terminal(), sample(2, 200));
    assert_eq!(seal.closed_at_ns(), 10_000);
    assert_eq!(seal.expires_at_ns(), 1_000_010_000);
    assert_eq!(
        stream
            .seal_input(
                &identity().live,
                sample(2, 200),
                10_000,
                finite_policy(),
                47_000,
                phase(101, 111),
            )
            .unwrap(),
        seal
    );

    let overrun = HspFiniteDrainWindow {
        stop_phase: phase(210, 251),
        ..finite_window(50_000, 200, 240)
    };
    assert_eq!(
        stream.tick_finite_drain(&seal, 50_000, overrun),
        Err(HspStreamError::TerminalOverrun)
    );
    assert_eq!(stream.input_state(), HspInputState::Draining);
    assert_eq!(
        stream.state(),
        HspStreamState::StopRequired(HspStreamError::TerminalOverrun)
    );
}
