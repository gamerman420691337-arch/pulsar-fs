fn transition_cursor(epoch: &str, attempt: &str) -> HspSourceCursor {
    let mut live = identity().live;
    live.epoch = AttemptId::new(epoch).unwrap();
    HspSourceCursor {
        identity: live,
        attempt_id: AttemptId::new(attempt).unwrap(),
    }
}

fn transition_initial_cursor() -> HspSourceCursor {
    HspSourceCursor {
        identity: identity().live,
        attempt_id: AttemptId::new("worker-a").unwrap(),
    }
}

fn transition_sample(
    time_ms: u32,
    sequence: u64,
    captured_at_ns: u64,
    continuous: bool,
) -> HspSample {
    HspSample {
        point: HspPoint::new(time_ms, 50).unwrap(),
        source_sequence: sequence,
        captured_at_ns,
        accepted_at_ns: captured_at_ns + 100,
        continuous_from_previous: continuous,
    }
}

fn transition_running() -> (HspStream, Arc<HspChunk>) {
    let mut stream =
        HspStream::new_with_source(identity(), policy(), transition_initial_cursor()).unwrap();
    for index in 0..3 {
        offer(&mut stream, index, index as u32 * 100, prefill());
    }
    let chunk = stream.claim(10_000, prefill()).unwrap().unwrap();
    acknowledge(&mut stream, &chunk);
    stream.mark_running(10_000, phase(-20, -10)).unwrap();
    (stream, chunk)
}

fn transition_spec(
    stream: &HspStream,
    epoch: &str,
    attempt: &str,
    first: HspSample,
    expires: u64,
) -> HspSourceTransitionSpec {
    HspSourceTransitionSpec {
        predecessor: stream.active_source().unwrap().clone(),
        successor: transition_cursor(epoch, attempt),
        predecessor_tail: stream.last_offered.unwrap(),
        first_successor: first,
        expires_at_ns: expires,
    }
}

#[test]
fn source_transition_two_epochs_keep_original_local_sequences_chunks_and_final_seal() {
    let (mut stream, original) = transition_running();
    let original_samples = original.samples().to_vec();
    let source_a = transition_initial_cursor();
    let first_b = transition_sample(300, 1, 3_000, false);
    let next_b = transition_sample(400, 2, 3_001, true);
    let spec = transition_spec(&stream, "epoch-b", "worker-b", first_b, 30_000);
    let source_b = spec.successor.clone();
    let token = stream
        .prepare_source_transition(spec, 10_001, phase(-20, -10))
        .unwrap();
    assert_eq!(token.predecessor(), &source_a);
    assert_eq!(token.first_successor(), first_b);
    assert_eq!(token.expires_at_ns(), 30_000);
    assert_eq!(stream.claim(10_002, phase(-20, -10)), Ok(None));
    assert_eq!(stream.buffered_points(), 4);
    assert_eq!(
        stream.offer(&source_a.identity, sample(3, 300), 10_002, phase(-20, -10)),
        Err(HspStreamError::SourceTransitionPending)
    );
    stream
        .commit_source_transition(token, 10_003, phase(-20, -10))
        .unwrap();
    assert_eq!(stream.active_source(), Some(&source_b));
    assert_eq!(
        stream.offer(&source_a.identity, sample(3, 300), 0, prefill()),
        Err(HspStreamError::ForeignSource)
    );
    assert_eq!(
        stream.offer(&source_b.identity, next_b, 10_004, phase(-20, -10)),
        Ok(HspOfferOutcome::Accepted)
    );
    let second = stream.claim(10_004, phase(-20, -10)).unwrap().unwrap();
    assert_eq!(second.samples(), &[first_b, next_b]);
    assert_eq!(second.source_cursor(), Some(&source_b));
    assert_eq!(second.tail_point_stream_index(), 4);
    acknowledge(&mut stream, &second);

    let first_c = transition_sample(500, 1, 4_000, false);
    let next_c = transition_sample(600, 2, 4_001, true);
    let spec = transition_spec(&stream, "epoch-c", "worker-c", first_c, 30_000);
    let source_c = spec.successor.clone();
    let token = stream
        .prepare_source_transition(spec, 10_005, phase(-20, -10))
        .unwrap();
    stream
        .commit_source_transition(token, 10_006, phase(-20, -10))
        .unwrap();
    stream
        .offer(&source_c.identity, next_c, 10_007, phase(-20, -10))
        .unwrap();
    let third = stream.claim(10_007, phase(-20, -10)).unwrap().unwrap();
    assert_eq!(third.samples(), &[first_c, next_c]);
    assert_eq!(third.source_cursor(), Some(&source_c));
    assert_eq!(third.tail_point_stream_index(), 6);
    acknowledge(&mut stream, &third);
    assert_eq!(stream.identity(), &identity());
    assert_eq!(stream.total_added(), 7);
    assert_eq!(original.source_cursor(), Some(&source_a));
    assert_eq!(original.samples(), original_samples.as_slice());
    for (index, chunk) in [original, second, third].iter().enumerate() {
        assert_eq!(chunk.token().identity(), &identity());
        assert_eq!(chunk.token().sequence(), index as u64 + 1);
    }
    assert_eq!(stream.retained[0].source.as_deref(), Some(&source_a));
    assert_eq!(stream.retained[3].source.as_deref(), Some(&source_b));
    assert_eq!(stream.retained[5].source.as_deref(), Some(&source_c));
    assert_eq!(
        stream.seal_input(
            &source_a.identity,
            next_c,
            10_008,
            finite_policy(),
            10_008,
            phase(-20, -10)
        ),
        Err(HspStreamError::ForeignSource)
    );
    let seal = stream
        .seal_input(
            &source_c.identity,
            next_c,
            10_008,
            finite_policy(),
            10_008,
            phase(-20, -10),
        )
        .unwrap();
    assert_eq!(seal.identity(), &identity());
    assert_eq!(seal.source_cursor(), Some(&source_c));
    assert_eq!(seal.terminal(), next_c);
    stream
        .begin_finite_drain(&seal, 10_009, finite_window(10_009, -20, -10))
        .unwrap();
    assert!(matches!(
        stream.tick_finite_drain(&seal, 20_000, finite_window(20_000, 600, 600)),
        Ok(HspDrainProgress::TailReached(_))
    ));
}

#[test]
fn source_transition_foreign_token_cannot_mutate_same_named_kernel() {
    let (mut stream, _) = transition_running();
    let (mut foreign, _) = transition_running();
    let first = transition_sample(300, 1, 3_000, false);
    let spec = transition_spec(&stream, "epoch-b", "worker-b", first, 30_000);
    let token = stream
        .prepare_source_transition(spec.clone(), 10_001, phase(-20, -10))
        .unwrap();
    let foreign_token = foreign
        .prepare_source_transition(spec.clone(), 10_001, phase(-20, -10))
        .unwrap();
    assert!(!stream.owns_source_transition(&foreign_token));
    assert_eq!(
        stream.commit_source_transition(foreign_token, 0, prefill()),
        Err(HspStreamError::ForeignSourceTransition)
    );
    assert_eq!(stream.state(), HspStreamState::Running);
    assert!(stream.owns_source_transition(&token));
    assert!(matches!(
        stream.prepare_source_transition(spec, 20_000, phase(-20, -10)),
        Err(HspStreamError::SourceTransitionPending)
    ));
    assert_eq!(token.expires_at_ns(), 30_000);
    stream
        .commit_source_transition(token, 10_002, phase(-20, -10))
        .unwrap();
}

#[test]
fn source_transition_stale_expired_and_cancelled_tokens_never_reopen_input() {
    for cause in 0..3 {
        let (mut stream, _) = transition_running();
        let spec = transition_spec(
            &stream,
            "epoch-b",
            "worker-b",
            transition_sample(300, 1, 3_000, false),
            if cause == 0 { 10_010 } else { 2_000_000 },
        );
        let token = stream
            .prepare_source_transition(spec, 10_001, phase(-20, -10))
            .unwrap();
        let (at, error) = match cause {
            0 => (10_010, HspStreamError::SourceTransitionExpired),
            1 => (1_003_001, HspStreamError::StaleSample),
            _ => {
                stream.cancel();
                (10_002, HspStreamError::Frozen)
            }
        };
        assert_eq!(
            stream.commit_source_transition(token, at, phase(-20, -10)),
            Err(error)
        );
        assert_eq!(stream.active_source(), Some(&transition_initial_cursor()));
        assert_eq!(stream.total_added(), 3);
        assert_eq!(stream.pending_points(), 0);
    }
}

#[test]
fn source_transition_watchdog_starvation_wins_before_first_successor_enqueue() {
    let (mut stream, _) = transition_running();
    let spec = transition_spec(
        &stream,
        "epoch-b",
        "worker-b",
        transition_sample(300, 1, 3_000, false),
        30_000,
    );
    let token = stream
        .prepare_source_transition(spec, 10_001, phase(-20, -10))
        .unwrap();
    assert_eq!(
        stream.tick(10_002, phase(180, 190)),
        Err(HspStreamError::Starved)
    );
    assert_eq!(
        stream.commit_source_transition(token, 10_003, phase(180, 190)),
        Err(HspStreamError::Frozen)
    );
    assert_eq!(stream.total_added(), 3);
    assert_eq!(stream.pending_points(), 0);
}

#[test]
fn source_transition_pending_and_unknown_predecessor_appends_refuse_barrier() {
    let (mut stream, _) = transition_running();
    stream
        .offer(&identity().live, sample(3, 300), 10_001, phase(-20, -10))
        .unwrap();
    let spec = transition_spec(
        &stream,
        "epoch-b",
        "worker-b",
        transition_sample(400, 1, 3_000, false),
        30_000,
    );
    assert!(matches!(
        stream.prepare_source_transition(spec.clone(), 10_001, phase(-20, -10)),
        Err(HspStreamError::NotReady)
    ));
    let append = stream.claim(10_001, phase(-20, -10)).unwrap().unwrap();
    assert!(matches!(
        stream.prepare_source_transition(spec.clone(), 10_002, phase(-20, -10)),
        Err(HspStreamError::NotReady)
    ));
    stream
        .complete(append.token(), HspAppendOutcome::Unknown)
        .unwrap();
    assert!(matches!(
        stream.prepare_source_transition(spec, 10_003, phase(-20, -10)),
        Err(HspStreamError::Frozen)
    ));
    assert_eq!(stream.in_flight_outcome(), Some(HspAppendOutcome::Unknown));
    assert_eq!(stream.total_added(), 3);
}

#[test]
fn source_transition_sequence_reset_is_only_at_the_exact_epoch_boundary() {
    let (mut stream, _) = transition_running();
    let first = transition_sample(300, 1, 3_000, false);
    let spec = transition_spec(&stream, "epoch-b", "worker-b", first, 30_000);
    let successor = spec.successor.clone();
    let token = stream
        .prepare_source_transition(spec, 10_001, phase(-20, -10))
        .unwrap();
    stream
        .commit_source_transition(token, 10_002, phase(-20, -10))
        .unwrap();
    assert_eq!(
        stream.offer(
            &successor.identity,
            transition_sample(400, 1, 3_001, true),
            10_003,
            phase(-20, -10)
        ),
        Err(HspStreamError::SourceOrder)
    );
}

#[test]
fn source_transition_requires_exact_acknowledged_tail_and_matching_stable_source() {
    for cause in 0..4 {
        let (mut stream, _) = transition_running();
        let mut spec = transition_spec(
            &stream,
            "epoch-b",
            "worker-b",
            transition_sample(300, 1, 3_000, false),
            30_000,
        );
        let expected = match cause {
            0 => {
                spec.predecessor_tail.accepted_at_ns += 1;
                HspStreamError::NotReady
            }
            1 => {
                spec.successor.identity.source = SourceVersionId::new("foreign-source").unwrap();
                HspStreamError::ForeignSource
            }
            2 => {
                spec.successor.attempt_id = spec.predecessor.attempt_id.clone();
                HspStreamError::ForeignSource
            }
            _ => {
                spec.first_successor.point.time_ms = 301;
                HspStreamError::CoverageGap
            }
        };
        assert!(
            matches!(stream.prepare_source_transition(spec, 10_001, phase(-20, -10)), Err(error) if error == expected)
        );
        assert_eq!(stream.active_source(), Some(&transition_initial_cursor()));
        assert!(!stream.source_transition_pending());
    }
}
