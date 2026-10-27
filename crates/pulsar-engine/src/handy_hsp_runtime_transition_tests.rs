fn runtime_source_cursor(epoch: &str, attempt: &str) -> HspSourceCursor {
    let mut identity = binding(1).identity.live;
    identity.epoch = AttemptId::new(epoch).unwrap();
    HspSourceCursor {
        identity,
        attempt_id: AttemptId::new(attempt).unwrap(),
    }
}

fn runtime_transfer_sample(time: u32, captured_at: u64) -> HspSample {
    HspSample {
        point: HspPoint::new(time, 50).unwrap(),
        source_sequence: 1,
        captured_at_ns: captured_at,
        accepted_at_ns: captured_at + 10,
        continuous_from_previous: false,
    }
}

fn runtime_transfer_spec(
    runtime: &HandyHspRuntime,
    epoch: &str,
    attempt: &str,
    predecessor_tail: HspSample,
    first: HspSample,
    expires: u64,
) -> HspSourceTransitionSpec {
    HspSourceTransitionSpec {
        predecessor: runtime.active_source().unwrap().clone(),
        successor: runtime_source_cursor(epoch, attempt),
        predecessor_tail,
        first_successor: first,
        expires_at_ns: expires,
    }
}

fn runtime_source_playing() -> HandyHspRuntime {
    let mut runtime = HandyHspRuntime::new_with_source(
        binding(1),
        policy(),
        runtime_source_cursor("epoch", "worker-a"),
    )
    .unwrap();
    let original = facts(100, -100);
    prefill(&mut runtime, &original);
    play(&mut runtime, &original);
    runtime
}

#[test]
fn source_transition_runtime_two_epochs_keep_setup_play_stream_and_original_effects() {
    let mut runtime = runtime_source_playing();
    let original_play = runtime.last_completion().unwrap().effect.clone();
    let original_play_bytes = original_play.request().body().unwrap().to_vec();
    let original_play_facts = original_play.original_facts().unwrap().clone();
    let first_b = runtime_transfer_sample(200, 150);
    let spec = runtime_transfer_spec(
        &runtime,
        "epoch-b",
        "worker-b",
        terminal_sample(100, 100, 2),
        first_b,
        1000,
    );
    let source_b = spec.successor.clone();
    let at = facts(200, -50);
    let token = runtime.prepare_source_transition(spec, 200, &at).unwrap();
    assert_eq!(runtime.next_effect(201, Some(&facts(201, -50))), Ok(None));
    assert_eq!(
        runtime.request_play(201, &facts(201, -50)),
        Err(HspStreamError::SourceTransitionPending.into())
    );
    runtime
        .commit_source_transition(token, 202, &facts(202, -50))
        .unwrap();
    let append_b = runtime
        .next_effect(203, Some(&facts(203, -50)))
        .unwrap()
        .unwrap();
    assert_eq!(append_b.kind(), HspEffectKind::Append);
    assert_eq!(append_b.sequence(), original_play.sequence() + 1);
    assert_eq!(append_b.binding(), &binding(1));
    assert_eq!(append_b.chunk().unwrap().source_cursor(), Some(&source_b));
    assert_eq!(append_b.chunk().unwrap().samples(), &[first_b]);
    assert_eq!(append_b.chunk().unwrap().tail_point_stream_index(), 2);
    complete(&mut runtime, &append_b, 2, 1, 204, Some(&facts(204, -50)));
    let first_c = runtime_transfer_sample(300, 250);
    let spec = runtime_transfer_spec(&runtime, "epoch-c", "worker-c", first_b, first_c, 1000);
    let source_c = spec.successor.clone();
    let token = runtime
        .prepare_source_transition(spec, 300, &facts(300, 0))
        .unwrap();
    runtime
        .commit_source_transition(token, 301, &facts(301, 0))
        .unwrap();
    let append_c = runtime
        .next_effect(302, Some(&facts(302, 0)))
        .unwrap()
        .unwrap();
    assert_eq!(append_c.kind(), HspEffectKind::Append);
    assert_eq!(append_c.sequence(), append_b.sequence() + 1);
    assert_eq!(append_c.binding(), &binding(1));
    assert_eq!(append_c.chunk().unwrap().source_cursor(), Some(&source_c));
    assert_eq!(append_c.chunk().unwrap().samples(), &[first_c]);
    assert_eq!(append_c.chunk().unwrap().tail_point_stream_index(), 3);
    complete(&mut runtime, &append_c, 3, 1, 303, Some(&facts(303, 0)));
    assert_eq!(runtime.active_source(), Some(&source_c));
    assert_eq!(runtime.stream().identity(), binding(1).identity());
    assert_eq!(
        original_play.request().body(),
        Some(original_play_bytes.as_slice())
    );
    assert_eq!(original_play.original_facts(), Some(&original_play_facts));
    assert_eq!(append_b.chunk().unwrap().source_cursor(), Some(&source_b));
    assert_eq!(
        runtime.offer(
            &binding(1).identity.live,
            terminal_sample(400, 50, 3),
            0,
            &facts(0, -100)
        ),
        Err(HspStreamError::ForeignSource.into())
    );

    let seal = runtime
        .seal_input(
            &source_c.identity,
            first_c,
            310,
            finite_config(),
            310,
            &facts(310, 0),
        )
        .unwrap();
    assert_eq!(seal.source_cursor(), Some(&source_c));
    assert_eq!(seal.identity(), binding(1).identity());
    assert_eq!(seal.terminal(), first_c);
    let ready = facts(311, 0);
    runtime
        .tick_snapshot(311, Some(&ready), Some(finite_window(&ready)), true)
        .unwrap();
    let crossing = facts(400, 300);
    assert!(matches!(
        runtime.tick_snapshot(400, Some(&crossing), Some(finite_window(&crossing)), true),
        Ok(HspRuntimeTick::TailReached(_))
    ));
    let stop = runtime.next_effect(401, None).unwrap().unwrap();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    assert_eq!(stop.sequence(), append_c.sequence() + 1);
    assert_eq!(stop.binding(), &binding(1));
}

#[test]
fn source_transition_runtime_requires_actual_play_ack_and_joined_native_custody() {
    let mut runtime = HandyHspRuntime::new_with_source(
        binding(1),
        policy(),
        runtime_source_cursor("epoch", "worker-a"),
    )
    .unwrap();
    let original = facts(100, -100);
    prefill(&mut runtime, &original);
    let spec = runtime_transfer_spec(
        &runtime,
        "epoch-b",
        "worker-b",
        terminal_sample(100, 100, 2),
        runtime_transfer_sample(200, 150),
        1000,
    );
    assert!(matches!(
        runtime.prepare_source_transition(spec.clone(), 200, &facts(200, -100)),
        Err(HspRuntimeError::Core(HspStreamError::NotReady))
    ));
    runtime.request_play(200, &facts(200, -100)).unwrap();
    let play = runtime
        .next_effect(200, Some(&facts(200, -100)))
        .unwrap()
        .unwrap();
    let transport = NativeHspTransportIdentity::new();
    let native = runtime.reserve_native(&play, &transport).unwrap();
    native.begin_claim().unwrap();
    native.claimed();
    assert!(matches!(
        runtime.prepare_source_transition(spec.clone(), 201, &facts(201, -100)),
        Err(HspRuntimeError::Core(HspStreamError::NotReady))
    ));
    assert!(native.join());
    complete(&mut runtime, &play, 1, 1, 202, Some(&facts(202, -100)));
    assert_eq!(
        runtime
            .native_custody
            .as_ref()
            .unwrap()
            .0
            .state
            .load(Ordering::Acquire),
        NATIVE_SETTLED
    );
    let token = runtime
        .prepare_source_transition(spec, 203, &facts(203, -100))
        .unwrap();
    runtime
        .commit_source_transition(token, 204, &facts(204, -100))
        .unwrap();
    assert_eq!(
        runtime.active_source(),
        Some(&runtime_source_cursor("epoch-b", "worker-b"))
    );
}

#[test]
fn source_transition_runtime_pending_unknown_and_expired_old_effects_cannot_be_rescued() {
    for expired in [false, true] {
        let mut runtime = runtime_source_playing();
        let append_facts = facts_for(binding(1), 200, -50, if expired { 250 } else { 2000 });
        offer(&mut runtime, &[200], 3, &append_facts);
        let predecessor = HspSample {
            point: HspPoint::new(200, 0).unwrap(),
            source_sequence: 3,
            captured_at_ns: 200,
            accepted_at_ns: 200,
            continuous_from_previous: true,
        };
        let spec = runtime_transfer_spec(
            &runtime,
            "epoch-b",
            "worker-b",
            predecessor,
            runtime_transfer_sample(300, 210),
            1000,
        );
        assert!(matches!(
            runtime.prepare_source_transition(spec.clone(), 220, &facts(220, -50)),
            Err(HspRuntimeError::Core(HspStreamError::NotReady))
        ));
        let append = runtime
            .next_effect(
                221,
                Some(&facts_for(
                    binding(1),
                    221,
                    -50,
                    if expired { 250 } else { 2000 },
                )),
            )
            .unwrap()
            .unwrap();
        let original_bytes = append.request().body().unwrap().to_vec();
        assert!(matches!(
            runtime.prepare_source_transition(spec.clone(), 222, &facts(222, -50)),
            Err(HspRuntimeError::NativeDispatchBusy)
        ));
        if expired {
            assert!(matches!(
                runtime.prepare_source_transition(spec, 250, &facts(250, -50)),
                Err(HspRuntimeError::ExpiredAuthority)
            ));
        } else {
            runtime
                .complete_settled(
                    &append,
                    HspSettledTransport::Unknown,
                    223,
                    Some(&facts(223, -50)),
                )
                .unwrap();
            assert!(matches!(
                runtime.prepare_source_transition(spec, 224, &facts(224, -50)),
                Err(HspRuntimeError::Retired)
            ));
            assert_eq!(runtime.uncertain_effect().unwrap().effect, append);
        }
        assert_eq!(append.request().body(), Some(original_bytes.as_slice()));
        assert_eq!(
            runtime.active_source(),
            Some(&runtime_source_cursor("epoch", "worker-a"))
        );
        assert!(!runtime.source_transition_pending());
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    }
}

#[test]
fn source_transition_runtime_foreign_token_cannot_advance_or_stop_current_instance() {
    let mut runtime = runtime_source_playing();
    let mut foreign = runtime_source_playing();
    let spec = runtime_transfer_spec(
        &runtime,
        "epoch-b",
        "worker-b",
        terminal_sample(100, 100, 2),
        runtime_transfer_sample(200, 150),
        1000,
    );
    let own = runtime
        .prepare_source_transition(spec.clone(), 200, &facts(200, -50))
        .unwrap();
    let other = foreign
        .prepare_source_transition(spec, 200, &facts(200, -50))
        .unwrap();
    assert_eq!(
        runtime.commit_source_transition(other, 0, &facts_for(binding(9), 0, -200, 1)),
        Err(HspStreamError::ForeignSourceTransition.into())
    );
    assert_eq!(runtime.state(), HspRuntimeState::Playing);
    assert!(runtime.stream().owns_source_transition(&own));
    runtime
        .commit_source_transition(own, 201, &facts(201, -50))
        .unwrap();
}

#[test]
fn source_transition_runtime_original_barrier_authority_and_expiry_never_renew() {
    for original_authority in [false, true] {
        let mut runtime = runtime_source_playing();
        let spec = runtime_transfer_spec(
            &runtime,
            "epoch-b",
            "worker-b",
            terminal_sample(100, 100, 2),
            runtime_transfer_sample(200, 150),
            if original_authority { 1000 } else { 250 },
        );
        let old = facts_for(
            binding(1),
            200,
            -50,
            if original_authority { 250 } else { 2000 },
        );
        let token = runtime.prepare_source_transition(spec, 200, &old).unwrap();
        assert_eq!(runtime.source_transition_facts.as_ref(), Some(&old));
        let expected = if original_authority {
            HspRuntimeError::ExpiredAuthority
        } else {
            HspStreamError::SourceTransitionExpired.into()
        };
        assert_eq!(
            runtime.commit_source_transition(token, 250, &facts(250, -50)),
            Err(expected)
        );
        assert_eq!(runtime.stream().pending_points(), 0);
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
        assert_eq!(
            runtime.next_effect(251, None).unwrap().unwrap().kind(),
            HspEffectKind::Stop
        );
    }
}

#[test]
fn source_transition_runtime_watchdog_and_stop_remain_live_while_barrier_waits() {
    for cancel in [false, true] {
        let mut runtime = runtime_source_playing();
        let spec = runtime_transfer_spec(
            &runtime,
            "epoch-b",
            "worker-b",
            terminal_sample(100, 100, 2),
            runtime_transfer_sample(200, 150),
            1000,
        );
        let token = runtime
            .prepare_source_transition(spec, 200, &facts(200, -50))
            .unwrap();
        if cancel {
            runtime.stop();
        } else {
            assert_eq!(
                runtime.tick(201, Some(&facts(201, 90))),
                Err(HspStreamError::Starved.into())
            );
        }
        assert_eq!(
            runtime.commit_source_transition(token, 202, &facts(202, 90)),
            Err(HspRuntimeError::Retired)
        );
        assert_eq!(runtime.stream().pending_points(), 0);
        let stop = runtime.next_effect(203, None).unwrap().unwrap();
        assert_eq!(stop.kind(), HspEffectKind::Stop);
        assert_eq!(stop.binding(), &binding(1));
        assert_eq!(runtime.next_effect(204, None), Ok(None));
    }
}
