use super::*;
use pulsar_core::{AttemptId, HspPoint, ProjectId, RevisionId, SourceVersionId};

fn binding(generation: u64) -> HspBinding {
    HspBinding::new(
        HspStreamIdentity {
            generation,
            stream_id: 42,
            live: LiveIdentity {
                epoch: AttemptId::new("epoch").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(1),
                source: SourceVersionId::new("source").unwrap(),
            },
        },
        ClientId::new("owner").unwrap(),
        DeviceSessionId::new("session").unwrap(),
        ArtifactId::new("native-clock").unwrap(),
    )
}
fn policy() -> HspPolicy {
    HspPolicy {
        max_buffered_points: 8,
        device_capacity_points: 8,
        max_chunk_points: 2,
        min_lead_ms: 10,
        max_future_ms: 1000,
        max_sample_age_ns: 1000,
        max_gap_ms: 200,
    }
}
fn facts_for(binding: HspBinding, now: u64, phase: i32, until: u64) -> HspDispatchFacts {
    HspDispatchFacts::new(
        binding,
        HspPlayReference::new(
            now,
            HspPhaseWindow {
                earliest_ms: i64::from(phase),
                latest_ms: i64::from(phase),
            },
            phase,
            10_000 + now,
        )
        .unwrap(),
        HspAuthorityWindow::new(until, until, until),
    )
    .unwrap()
}
fn facts(now: u64, phase: i32) -> HspDispatchFacts {
    facts_for(binding(1), now, phase, 2000)
}
fn runtime() -> HandyHspRuntime {
    HandyHspRuntime::new(binding(1), policy()).unwrap()
}
fn response(stream: u32, tail: i32, state: u8) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"result": {
        "play_state":state, "points":(tail + 1).max(0), "max_points":8,
        "current_point":if tail < 0 { -1 } else { 0 }, "current_time":-100,
        "loop":false, "playback_rate":1, "first_point_time":0,
        "last_point_time":(tail.max(0) as u32) * 100, "stream_id":stream,
        "tail_point_stream_index":tail, "tail_point_stream_index_threshold":1,
        "pause_on_starving":false
    }}))
    .unwrap()
}
fn complete(
    runtime: &mut HandyHspRuntime,
    effect: &HspEffect,
    tail: i32,
    state: u8,
    now: u64,
    facts: Option<&HspDispatchFacts>,
) -> HspCompletionRecord {
    let body = response(42, tail, state);
    runtime
        .complete_settled(
            effect,
            HspSettledTransport::Response {
                status: 200,
                body: &body,
            },
            now,
            facts,
        )
        .unwrap()
}
fn setup(runtime: &mut HandyHspRuntime, facts: &HspDispatchFacts) {
    let effect = runtime
        .next_effect(facts.reference.at_ns, Some(facts))
        .unwrap()
        .unwrap();
    assert_eq!(effect.kind(), HspEffectKind::Setup);
    assert_eq!(
        effect.request().body(),
        Some(br#"{"stream_id":42}"#.as_slice())
    );
    complete(runtime, &effect, -1, 2, facts.reference.at_ns, Some(facts));
    assert_eq!(runtime.state(), HspRuntimeState::Prefilling);
}
fn offer(
    runtime: &mut HandyHspRuntime,
    times: &[u32],
    first_sequence: u64,
    facts: &HspDispatchFacts,
) {
    for (i, time) in times.iter().enumerate() {
        runtime
            .offer(
                &facts.binding.identity.live,
                HspSample {
                    point: HspPoint::new(*time, (i * 100) as u8).unwrap(),
                    source_sequence: first_sequence + i as u64,
                    captured_at_ns: facts.reference.at_ns,
                    accepted_at_ns: facts.reference.at_ns,
                    continuous_from_previous: true,
                },
                facts.reference.at_ns,
                facts,
            )
            .unwrap();
    }
}
fn prefill(runtime: &mut HandyHspRuntime, facts: &HspDispatchFacts) {
    setup(runtime, facts);
    offer(runtime, &[0, 100], 1, facts);
    let effect = runtime
        .next_effect(facts.reference.at_ns, Some(facts))
        .unwrap()
        .unwrap();
    assert_eq!(effect.kind(), HspEffectKind::Append);
    assert_eq!(effect.chunk().unwrap().tail_point_stream_index(), 1);
    complete(runtime, &effect, 1, 2, facts.reference.at_ns, Some(facts));
}
fn play(runtime: &mut HandyHspRuntime, facts: &HspDispatchFacts) {
    runtime.request_play(facts.reference.at_ns, facts).unwrap();
    let effect = runtime
        .next_effect(facts.reference.at_ns, Some(facts))
        .unwrap()
        .unwrap();
    assert_eq!(effect.kind(), HspEffectKind::Play);
    complete(runtime, &effect, 1, 1, facts.reference.at_ns, Some(facts));
    assert_eq!(runtime.state(), HspRuntimeState::Playing);
}

#[test]
fn ordered_setup_prefill_explicit_play_ongoing_append_and_bound_stop() {
    let mut runtime = runtime();
    let initial = facts(100, -100);
    prefill(&mut runtime, &initial);
    play(&mut runtime, &initial);
    let current = facts(200, 0);
    offer(&mut runtime, &[200, 300], 3, &current);
    let append = runtime.next_effect(200, Some(&current)).unwrap().unwrap();
    assert_eq!(append.chunk().unwrap().tail_point_stream_index(), 3);
    assert_eq!(append.request().body(), Some(br#"{"points":[{"t":200,"x":0},{"t":300,"x":100}],"tail_point_stream_index":3,"flush":false}"#.as_slice()));
    complete(&mut runtime, &append, 3, 1, 200, Some(&current));
    assert_eq!(runtime.stream().total_added(), 4);
    runtime.stop();
    let stop = runtime.next_effect(3000, None).unwrap().unwrap();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    assert_eq!(stop.binding(), &binding(1));
    assert_eq!(stop.request().path(), "/hsp/stop");
    assert_eq!(runtime.validate_dispatch(&stop, 3000, None), Ok(()));
    complete(&mut runtime, &stop, 3, 2, 3000, None);
    assert_eq!(runtime.state(), HspRuntimeState::StopAcknowledged);
    assert_eq!(runtime.next_effect(3001, None), Ok(None));
}

#[test]
fn data_and_polling_never_create_play_or_duplicate_effects() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    let setup_effect = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    for _ in 0..3 {
        assert_eq!(runtime.next_effect(100, Some(&facts)), Ok(None));
    }
    complete(&mut runtime, &setup_effect, -1, 2, 100, Some(&facts));
    offer(&mut runtime, &[0, 100], 1, &facts);
    let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    for _ in 0..3 {
        assert_eq!(runtime.next_effect(100, Some(&facts)), Ok(None));
    }
    complete(&mut runtime, &append, 1, 2, 100, Some(&facts));
    for _ in 0..3 {
        assert_eq!(runtime.next_effect(100, Some(&facts)), Ok(None));
    }
    assert_eq!(runtime.state(), HspRuntimeState::Prefilling);
}

#[test]
fn stop_latches_during_blocked_append_and_late_ack_cannot_rearm() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    prefill(&mut runtime, &facts);
    play(&mut runtime, &facts);
    offer(&mut runtime, &[200, 300], 3, &facts);
    let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    runtime.stop();
    assert_eq!(runtime.next_effect(100, None), Ok(None));
    assert_eq!(
        runtime.validate_dispatch(&append, 100, Some(&facts)),
        Err(HspRuntimeError::Retired)
    );
    let late = complete(&mut runtime, &append, 3, 1, 100, Some(&facts));
    assert!(matches!(late.outcome, HspObservedOutcome::Acknowledged(_)));
    assert_eq!(runtime.stream().total_added(), 2);
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(
        runtime.next_effect(100, None).unwrap().unwrap().kind(),
        HspEffectKind::Stop
    );
}

#[test]
fn unknown_append_is_never_resent_and_late_reply_only_annotates_custody() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    setup(&mut runtime, &facts);
    offer(&mut runtime, &[0, 100], 1, &facts);
    let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    let original_bytes = append.request().body().unwrap().to_vec();
    runtime
        .complete_settled(&append, HspSettledTransport::Unknown, 100, Some(&facts))
        .unwrap();
    assert_eq!(
        runtime.uncertain_effect().unwrap().effect.request().body(),
        Some(original_bytes.as_slice())
    );
    let stop = runtime.next_effect(100, None).unwrap().unwrap();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    complete(&mut runtime, &stop, 1, 2, 100, None);
    let late = complete(&mut runtime, &append, 1, 2, 100, Some(&facts));
    assert_eq!(late.outcome, HspObservedOutcome::Unknown);
    assert!(matches!(
        late.late_observation,
        Some(HspObservedOutcome::Acknowledged(_))
    ));
    assert_eq!(runtime.stream().total_added(), 0);
    assert_eq!(runtime.state(), HspRuntimeState::StopAcknowledged);
    assert_eq!(runtime.next_effect(100, Some(&facts)), Ok(None));
    assert_eq!(
        runtime.complete_settled(&append, HspSettledTransport::Unknown, 100, None),
        Err(HspRuntimeError::AlreadyObserved)
    );
}

#[test]
fn uncertain_stop_keeps_obligation_without_retry_or_overwriting_unknown_append() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    setup(&mut runtime, &facts);
    offer(&mut runtime, &[0, 100], 1, &facts);
    let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    runtime
        .complete_settled(&append, HspSettledTransport::Unknown, 100, Some(&facts))
        .unwrap();
    let stop = runtime.next_effect(100, None).unwrap().unwrap();
    runtime
        .complete_settled(&stop, HspSettledTransport::Unknown, 100, None)
        .unwrap();
    assert_eq!(runtime.state(), HspRuntimeState::StopUnknown);
    assert_eq!(runtime.uncertain_effect().unwrap().effect, append);
    assert_eq!(runtime.uncertain_stop().unwrap().effect, stop);
    assert_eq!(runtime.next_effect(100, None), Ok(None));
    complete(&mut runtime, &stop, 1, 2, 100, None);
    assert_eq!(runtime.state(), HspRuntimeState::StopUnknown);
}

#[test]
fn malformed_foreign_or_unusable_setup_freezes_and_retains_original_effect() {
    let base = response(42, -1, 2);
    let mut small: serde_json::Value = serde_json::from_slice(&base).unwrap();
    small["result"]["max_points"] = 4.into();
    for body in [
        b"{}".to_vec(),
        response(7, -1, 2),
        response(42, -1, 3),
        response(42, 1, 2),
        serde_json::to_vec(&small).unwrap(),
        br#"{"error":{"message":"secret"}}"#.to_vec(),
    ] {
        let mut runtime = runtime();
        let facts = facts(100, -100);
        let setup = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
        let record = runtime
            .complete_settled(
                &setup,
                HspSettledTransport::Response {
                    status: 200,
                    body: &body,
                },
                100,
                Some(&facts),
            )
            .unwrap();
        assert_eq!(record.outcome, HspObservedOutcome::Unknown);
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
        assert_eq!(runtime.uncertain_effect().unwrap().effect, setup);
    }
}

#[test]
fn changed_stream_or_tail_on_append_requires_stop_without_advancing() {
    for (stream, tail) in [(7, 1), (42, 2)] {
        let mut runtime = runtime();
        let facts = facts(100, -100);
        setup(&mut runtime, &facts);
        offer(&mut runtime, &[0, 100], 1, &facts);
        let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
        let body = response(stream, tail, 2);
        runtime
            .complete_settled(
                &append,
                HspSettledTransport::Response {
                    status: 200,
                    body: &body,
                },
                100,
                Some(&facts),
            )
            .unwrap();
        assert_eq!(runtime.stream().total_added(), 0);
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    }
}

#[test]
fn foreign_identity_authority_and_effect_cannot_mutate_current_generation() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    let setup = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    let foreign_facts = facts_for(binding(2), 100, -100, 2000);
    assert_eq!(
        runtime.next_effect(100, Some(&foreign_facts)),
        Err(HspRuntimeError::ForeignAuthority)
    );
    let mut foreign = HandyHspRuntime::new(binding(2), policy()).unwrap();
    let foreign_effect = foreign
        .next_effect(100, Some(&foreign_facts))
        .unwrap()
        .unwrap();
    assert_eq!(
        runtime.complete_settled(&foreign_effect, HspSettledTransport::Unknown, 100, None),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(runtime.in_flight(), Some(&setup));
    let mut source = facts.binding.identity.live.clone();
    source.revision = RevisionId::new(2);
    assert_eq!(
        runtime.offer(
            &source,
            HspSample {
                point: HspPoint::new(0, 0).unwrap(),
                source_sequence: 0,
                captured_at_ns: 100,
                accepted_at_ns: 100,
                continuous_from_previous: true,
            },
            100,
            &facts
        ),
        Err(HspRuntimeError::Core(HspStreamError::ForeignSource))
    );
    assert_eq!(runtime.state(), HspRuntimeState::AwaitingSetup);
}

#[test]
fn starvation_and_missing_clock_stop_independently_of_new_samples() {
    for clock_lost in [false, true] {
        let mut runtime = runtime();
        let initial = facts(100, -100);
        prefill(&mut runtime, &initial);
        play(&mut runtime, &initial);
        let depleted = facts(200, 90);
        let result = runtime.tick(200, if clock_lost { None } else { Some(&depleted) });
        assert!(result.is_err());
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
        assert_eq!(
            runtime.next_effect(200, None).unwrap().unwrap().kind(),
            HspEffectKind::Stop
        );
    }
}

#[test]
fn source_coverage_gap_retires_play_eligibility() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    prefill(&mut runtime, &facts);
    assert_eq!(
        runtime.offer(
            &facts.binding.identity.live,
            HspSample {
                point: HspPoint::new(200, 50).unwrap(),
                source_sequence: 3,
                captured_at_ns: 100,
                accepted_at_ns: 100,
                continuous_from_previous: false,
            },
            100,
            &facts
        ),
        Err(HspRuntimeError::Core(HspStreamError::CoverageGap))
    );
    assert_eq!(
        runtime.request_play(100, &facts),
        Err(HspRuntimeError::Retired)
    );
    assert_eq!(
        runtime.next_effect(100, None).unwrap().unwrap().kind(),
        HspEffectKind::Stop
    );
}

#[test]
fn fresh_observation_cannot_rejuvenate_original_pending_play_reference() {
    let mut runtime = runtime();
    let original = facts_for(binding(1), 100, -100, 110);
    prefill(&mut runtime, &original);
    runtime.request_play(100, &original).unwrap();
    let renewed = facts_for(binding(1), 110, -90, 2000);
    assert_eq!(
        runtime.next_effect(110, Some(&renewed)),
        Err(HspRuntimeError::ExpiredAuthority)
    );
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(
        runtime
            .next_effect(110, Some(&renewed))
            .unwrap()
            .unwrap()
            .kind(),
        HspEffectKind::Stop
    );
}

#[test]
fn play_bytes_preserve_original_reference_and_dispatch_rechecks_original_expiry() {
    let mut runtime = runtime();
    let original = facts_for(binding(1), 100, -100, 110);
    prefill(&mut runtime, &original);
    runtime.request_play(100, &original).unwrap();
    let fresh = facts_for(binding(1), 101, -99, 2000);
    let effect = runtime.next_effect(101, Some(&fresh)).unwrap().unwrap();
    let body: serde_json::Value = serde_json::from_slice(effect.request().body().unwrap()).unwrap();
    assert_eq!(body["start_time"], -100);
    assert_eq!(body["server_time"], 10_100);
    let renewed = facts_for(binding(1), 110, -90, 2000);
    assert_eq!(
        runtime.validate_dispatch(&effect, 110, Some(&renewed)),
        Err(HspRuntimeError::ExpiredAuthority)
    );
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(runtime.next_effect(110, None), Ok(None));
}

#[test]
fn dispatch_rechecks_claimed_append_lead_without_replacing_original_bytes() {
    let mut runtime = runtime();
    let initial = facts(100, -100);
    setup(&mut runtime, &initial);
    offer(&mut runtime, &[0, 100], 1, &initial);
    let effect = runtime.next_effect(100, Some(&initial)).unwrap().unwrap();
    let later = facts(101, -5);
    assert_eq!(
        runtime.validate_dispatch(&effect, 101, Some(&later)),
        Err(HspRuntimeError::Core(HspStreamError::InsufficientLead))
    );
    assert_eq!(runtime.in_flight(), Some(&effect));
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
}

#[test]
fn expired_original_append_ack_is_history_only_even_with_fresh_valid_facts() {
    let mut runtime = runtime();
    let original = facts_for(binding(1), 100, -100, 110);
    setup(&mut runtime, &original);
    offer(&mut runtime, &[0, 100], 1, &original);
    let append = runtime.next_effect(100, Some(&original)).unwrap().unwrap();
    let renewed = facts_for(binding(1), 110, -90, 2000);
    let record = complete(&mut runtime, &append, 1, 2, 110, Some(&renewed));
    assert!(matches!(
        record.outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(record.effect, append);
    assert_eq!(runtime.stream().total_added(), 0);
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(
        runtime.next_effect(110, None).unwrap().unwrap().kind(),
        HspEffectKind::Stop
    );
}

#[test]
fn empty_setup_accepts_supported_nonplaying_states_without_assuming_initial_tail() {
    for state in [0, 2] {
        let mut runtime = runtime();
        let facts = facts(100, -100);
        let setup = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
        let mut body: serde_json::Value = serde_json::from_slice(&response(42, -1, state)).unwrap();
        body["result"]["tail_point_stream_index"] = 7.into();
        let body = serde_json::to_vec(&body).unwrap();
        let record = runtime
            .complete_settled(
                &setup,
                HspSettledTransport::Response {
                    status: 200,
                    body: &body,
                },
                100,
                Some(&facts),
            )
            .unwrap();
        assert!(matches!(
            record.outcome,
            HspObservedOutcome::Acknowledged(_)
        ));
        offer(&mut runtime, &[0, 100], 1, &facts);
        let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
        assert_eq!(append.chunk().unwrap().tail_point_stream_index(), 1);
    }
}

#[test]
fn missing_dispatch_authority_latches_stop_but_keeps_transport_slot_until_settled() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    let setup = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    assert_eq!(
        runtime.validate_dispatch(&setup, 100, None),
        Err(HspRuntimeError::ExpiredAuthority)
    );
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(runtime.next_effect(100, None), Ok(None));
    runtime
        .complete_settled(&setup, HspSettledTransport::Unknown, 100, None)
        .unwrap();
    assert_eq!(
        runtime.next_effect(100, None).unwrap().unwrap().kind(),
        HspEffectKind::Stop
    );
}

#[test]
fn watchdog_retains_blocked_effect_when_its_original_facts_expire() {
    let mut runtime = runtime();
    let original = facts_for(binding(1), 100, -100, 110);
    let setup = runtime.next_effect(100, Some(&original)).unwrap().unwrap();
    let renewed = facts_for(binding(1), 110, -90, 2000);
    assert_eq!(
        runtime.tick(110, Some(&renewed)),
        Err(HspRuntimeError::ExpiredAuthority)
    );
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(runtime.in_flight(), Some(&setup));
    assert_eq!(runtime.next_effect(110, None), Ok(None));
}

fn response_with_options(tail: i32, state: u8, rate: u32, looping: bool, pause: bool) -> Vec<u8> {
    let mut body: serde_json::Value = serde_json::from_slice(&response(42, tail, state)).unwrap();
    body["result"]["playback_rate"] = rate.into();
    body["result"]["loop"] = looping.into();
    body["result"]["pause_on_starving"] = pause.into();
    serde_json::to_vec(&body).unwrap()
}

#[test]
fn preplay_defaults_allow_prefill_and_only_explicit_play_sets_fixed_options() {
    let mut runtime = runtime();
    let facts = facts(100, -100);
    let setup = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    let body = response_with_options(-1, 2, 2, true, true);
    let observed = runtime
        .complete_settled(
            &setup,
            HspSettledTransport::Response {
                status: 200,
                body: &body,
            },
            100,
            Some(&facts),
        )
        .unwrap();
    assert!(matches!(
        observed.outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(runtime.state(), HspRuntimeState::Prefilling);

    offer(&mut runtime, &[0, 100], 1, &facts);
    let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    let body = response_with_options(1, 2, 2, true, true);
    let observed = runtime
        .complete_settled(
            &append,
            HspSettledTransport::Response {
                status: 200,
                body: &body,
            },
            100,
            Some(&facts),
        )
        .unwrap();
    assert!(matches!(
        observed.outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(runtime.stream().total_added(), 2);
    assert_eq!(runtime.next_effect(100, Some(&facts)), Ok(None));

    runtime.request_play(100, &facts).unwrap();
    let play = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
    assert_eq!(play.kind(), HspEffectKind::Play);
    assert_eq!(play.request().body(), Some(br#"{"start_time":-100,"server_time":10100,"playback_rate":1.0,"pause_on_starving":false,"loop":false}"#.as_slice()));
    let observed = complete(&mut runtime, &play, 1, 1, 100, Some(&facts));
    assert!(matches!(
        observed.outcome,
        HspObservedOutcome::Acknowledged(_)
    ));
    assert_eq!(runtime.state(), HspRuntimeState::Playing);
}

#[test]
fn play_acknowledgment_requires_each_explicit_playback_option() {
    for (rate, looping, pause) in [(2, false, false), (1, true, false), (1, false, true)] {
        let mut runtime = runtime();
        let facts = facts(100, -100);
        prefill(&mut runtime, &facts);
        runtime.request_play(100, &facts).unwrap();
        let play = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
        let body = response_with_options(1, 1, rate, looping, pause);
        let observed = runtime
            .complete_settled(
                &play,
                HspSettledTransport::Response {
                    status: 200,
                    body: &body,
                },
                100,
                Some(&facts),
            )
            .unwrap();
        assert_eq!(observed.outcome, HspObservedOutcome::Unknown);
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
        assert_eq!(runtime.uncertain_effect().unwrap().effect, play);
    }
}

#[test]
fn running_append_changed_playback_options_are_unknown_and_do_not_advance() {
    for (rate, looping, pause) in [(2, false, false), (1, true, false), (1, false, true)] {
        let mut runtime = runtime();
        let facts = facts(100, -100);
        prefill(&mut runtime, &facts);
        play(&mut runtime, &facts);
        offer(&mut runtime, &[200, 300], 3, &facts);
        let append = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
        let body = response_with_options(3, 1, rate, looping, pause);
        let observed = runtime
            .complete_settled(
                &append,
                HspSettledTransport::Response {
                    status: 200,
                    body: &body,
                },
                100,
                Some(&facts),
            )
            .unwrap();
        assert_eq!(observed.outcome, HspObservedOutcome::Unknown);
        assert_eq!(runtime.stream().total_added(), 2);
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
        assert_eq!(runtime.uncertain_effect().unwrap().effect, append);
    }
}

fn unclaimed_journal() -> (
    tempfile::TempDir,
    crate::device_effect_journal::DeviceEffectJournal,
    crate::device_effect_journal::HspStreamReceipt,
) {
    use crate::device_effect_journal::DeviceEffectJournal;
    use crate::handy_hsp_records::{HspBindingRecord, HspStreamRecord};
    use pulsar_core::{DeviceId, RequestId, SessionId};
    use pulsar_protocol::{HandyIdentity, HandyRegistration, HandyRegistrationSettings};
    let dir = tempfile::tempdir().unwrap();
    let boot = DeviceSessionId::new("unclaimed-runtime-boot").unwrap();
    let mut journal = DeviceEffectJournal::open(dir.path(), boot.clone()).unwrap();
    let stream = HspStreamRecord {
        boot,
        actor_session: SessionId::new("actor-session").unwrap(),
        request_id: RequestId::new("stream-request").unwrap(),
        registration: HandyRegistration {
            device_id: DeviceId::new("device").unwrap(),
            revision: 1,
            settings: HandyRegistrationSettings {
                label: "Synthetic unclaimed runtime fixture".into(),
                api_key_env: "HANDY_API_KEY".into(),
                connection_key_env: "HANDY_CONNECTION_KEY".into(),
            },
            disabled: false,
            binding_sha256: "a".repeat(64),
        },
        device: HandyIdentity {
            firmware: "4.0".into(),
            model_number: 2,
            model_name: "Handy".into(),
            model_variant: 0,
            session_id: "native-session".into(),
            firmware_status: 0,
        },
        route_sha256: "b".repeat(64),
        route_namespace_sha256: "c".repeat(64),
        binding: HspBindingRecord::capture(&binding(1)),
        qualification_sha256: "d".repeat(64),
        profile_sha256: "e".repeat(64),
    };
    let receipt = journal.prepare_hsp_stream(&stream).unwrap();
    (dir, journal, receipt)
}

fn setup_with_journal(
    runtime: &mut HandyHspRuntime,
    facts: &HspDispatchFacts,
    journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    stream: &crate::device_effect_journal::HspStreamReceipt,
) {
    let effect = runtime
        .next_effect(facts.reference.at_ns, Some(facts))
        .unwrap()
        .unwrap();
    let prepared = journal.prepare_hsp_effect(stream, &effect).unwrap();
    let claimed = journal.claim_hsp(&prepared, &effect).unwrap();
    let completion = complete(runtime, &effect, -1, 2, facts.reference.at_ns, Some(facts));
    journal.complete_hsp(&claimed, &completion).unwrap();
}

#[test]
fn proved_unclaimed_setup_is_terminal_without_stop_or_network_unknown() {
    let (_dir, mut journal, stream) = unclaimed_journal();
    let mut runtime = runtime();
    let current = facts(100, -100);
    let effect = runtime.next_effect(100, Some(&current)).unwrap().unwrap();
    let prepared = journal.prepare_hsp_effect(&stream, &effect).unwrap();
    let proof = journal.try_cancel_hsp_effect(&prepared, &effect).unwrap();
    runtime.retire_unclaimed(&effect, &proof).unwrap();
    assert_eq!(runtime.state(), HspRuntimeState::SetupCancelled);
    assert_eq!(runtime.unclaimed_effect(), Some(&effect));
    assert!(runtime.in_flight().is_none());
    assert!(runtime.uncertain_effect().is_none());
    assert!(runtime.last_completion().is_none());
    assert_eq!(runtime.next_effect(100, Some(&current)), Ok(None));
    assert_eq!(runtime.retire_unclaimed(&effect, &proof), Ok(()));
    runtime.stop();
    assert_eq!(runtime.state(), HspRuntimeState::SetupCancelled);
    assert_eq!(runtime.next_effect(5000, None), Ok(None));
    assert_eq!(
        runtime.complete_settled(&effect, HspSettledTransport::Unknown, 100, Some(&current)),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(
        runtime.request_play(100, &current),
        Err(HspRuntimeError::Retired)
    );
    assert_eq!(
        journal.hsp_stream(stream.id()).unwrap().state,
        crate::device_effect_journal::HspStreamJournalState::Cancelled
    );
}

#[test]
fn proved_unclaimed_append_releases_only_exact_slot_and_late_reply_never_rearms() {
    let (_dir, mut journal, stream) = unclaimed_journal();
    let mut runtime = runtime();
    let current = facts(100, -100);
    setup_with_journal(&mut runtime, &current, &mut journal, &stream);
    let original_completion = runtime.last_completion().cloned();
    offer(&mut runtime, &[0, 100], 1, &current);
    let append = runtime.next_effect(100, Some(&current)).unwrap().unwrap();
    let prepared = journal.prepare_hsp_effect(&stream, &append).unwrap();
    runtime.stop();
    let proof = journal.try_cancel_hsp_effect(&prepared, &append).unwrap();
    let mut foreign = append.clone();
    foreign.sequence += 1;
    assert_eq!(
        runtime.retire_unclaimed(&foreign, &proof),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(runtime.in_flight(), Some(&append));
    assert_eq!(runtime.stream().in_flight_outcome(), None);
    runtime.retire_unclaimed(&append, &proof).unwrap();
    assert!(runtime.in_flight().is_none());
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(runtime.unclaimed_effect(), Some(&append));
    assert_eq!(
        runtime.stream().in_flight_outcome(),
        Some(HspAppendOutcome::Rejected)
    );
    assert_eq!(runtime.stream().state(), HspStreamState::Cancelled);
    assert_eq!(runtime.last_completion(), original_completion.as_ref());
    assert!(runtime.uncertain_effect().is_none());
    assert_eq!(runtime.retire_unclaimed(&append, &proof), Ok(()));
    let stop = runtime.next_effect(100, None).unwrap().unwrap();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    let prepared_stop = journal.prepare_hsp_effect(&stream, &stop).unwrap();
    let claimed_stop = journal.claim_hsp(&prepared_stop, &stop).unwrap();
    assert!(journal
        .try_cancel_hsp_effect(&prepared_stop, &stop)
        .is_err());
    assert_eq!(
        runtime.retire_unclaimed(&stop, &proof),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(
        runtime.retire_unclaimed(&append, &proof),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(runtime.in_flight(), Some(&stop));
    assert_eq!(runtime.state(), HspRuntimeState::StopInFlight);
    let reply = response(42, 1, 2);
    assert_eq!(
        runtime.complete_settled(
            &append,
            HspSettledTransport::Response {
                status: 200,
                body: &reply
            },
            100,
            Some(&current)
        ),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(runtime.in_flight(), Some(&stop));
    assert_eq!(
        runtime.request_play(100, &current),
        Err(HspRuntimeError::Retired)
    );
    let completed = complete(&mut runtime, &stop, -1, 2, 5000, None);
    journal.complete_hsp(&claimed_stop, &completed).unwrap();
    assert_eq!(runtime.state(), HspRuntimeState::StopAcknowledged);
    assert_eq!(runtime.retire_unclaimed(&append, &proof), Ok(()));
    assert_eq!(runtime.state(), HspRuntimeState::StopAcknowledged);
    assert_eq!(runtime.next_effect(5001, None), Ok(None));
    assert_eq!(
        runtime.complete_settled(&append, HspSettledTransport::Unknown, 5001, None),
        Err(HspRuntimeError::ForeignEffect)
    );
}

#[test]
fn proved_unclaimed_play_keeps_prior_stream_custody_and_requires_stop() {
    let (_dir, mut journal, stream) = unclaimed_journal();
    let mut runtime = runtime();
    let current = facts(100, -100);
    setup_with_journal(&mut runtime, &current, &mut journal, &stream);
    offer(&mut runtime, &[0, 100], 1, &current);
    let append = runtime.next_effect(100, Some(&current)).unwrap().unwrap();
    let prepared = journal.prepare_hsp_effect(&stream, &append).unwrap();
    let claimed = journal.claim_hsp(&prepared, &append).unwrap();
    let completion = complete(&mut runtime, &append, 1, 2, 100, Some(&current));
    journal.complete_hsp(&claimed, &completion).unwrap();
    runtime.request_play(100, &current).unwrap();
    let play = runtime.next_effect(100, Some(&current)).unwrap().unwrap();
    let prepared = journal.prepare_hsp_effect(&stream, &play).unwrap();
    let proof = journal.try_cancel_hsp_effect(&prepared, &play).unwrap();
    runtime.retire_unclaimed(&play, &proof).unwrap();
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
    assert_eq!(runtime.unclaimed_effect(), Some(&play));
    assert_eq!(runtime.stream().state(), HspStreamState::Cancelled);
    assert!(runtime.uncertain_effect().is_none());
    assert_eq!(
        runtime.next_effect(5000, None).unwrap().unwrap().kind(),
        HspEffectKind::Stop
    );
    assert_eq!(
        journal.hsp_stream(stream.id()).unwrap().state,
        crate::device_effect_journal::HspStreamJournalState::Custody
    );
}

#[test]
fn proved_unclaimed_stop_is_terminal_with_stop_obligation_still_open() {
    let (_dir, mut journal, stream) = unclaimed_journal();
    let mut runtime = runtime();
    let current = facts(100, -100);
    setup_with_journal(&mut runtime, &current, &mut journal, &stream);
    runtime.stop();
    let stop = runtime.next_effect(5000, None).unwrap().unwrap();
    let prepared = journal.prepare_hsp_effect(&stream, &stop).unwrap();
    let proof = journal.try_cancel_hsp_effect(&prepared, &stop).unwrap();
    runtime.retire_unclaimed(&stop, &proof).unwrap();
    assert_eq!(runtime.state(), HspRuntimeState::StopUnclaimed);
    assert_eq!(runtime.unclaimed_stop(), Some(&stop));
    assert!(runtime.in_flight().is_none());
    assert!(runtime.uncertain_stop().is_none());
    assert_eq!(runtime.next_effect(5001, None), Ok(None));
    assert_eq!(runtime.retire_unclaimed(&stop, &proof), Ok(()));
    runtime.stop();
    assert_eq!(runtime.state(), HspRuntimeState::StopUnclaimed);
    let reply = response(42, -1, 2);
    assert_eq!(
        runtime.complete_settled(
            &stop,
            HspSettledTransport::Response {
                status: 200,
                body: &reply
            },
            5001,
            None
        ),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(
        journal.hsp_stream(stream.id()).unwrap().state,
        crate::device_effect_journal::HspStreamJournalState::Custody
    );
}

#[test]
fn native_unsent_requires_exact_instance_and_unattempted_joined_reservation() {
    for state in [
        None,
        Some(NATIVE_RESERVED),
        Some(NATIVE_ATTEMPTING),
        Some(NATIVE_CLAIMED),
        Some(NATIVE_SETTLED),
        Some(NATIVE_RELEASED),
    ] {
        let mut runtime = runtime();
        let effect = runtime
            .next_effect(100, Some(&facts(100, -100)))
            .unwrap()
            .unwrap();
        let transport = NativeHspTransportIdentity::new();
        if let Some(state) = state {
            let reservation = runtime.reserve_native(&effect, &transport).unwrap();
            reservation.0.state.store(state, Ordering::Release);
        }
        let allowed = state.is_none() || state == Some(NATIVE_RELEASED);
        assert_eq!(
            runtime.retire_native_unsent(&effect, &transport).is_ok(),
            allowed
        );
        if allowed {
            assert_eq!(runtime.state(), HspRuntimeState::SetupUnsent);
            assert_eq!(runtime.native_unsent_effect(), Some(&effect));
            assert!(runtime.unclaimed_effect().is_none());
            assert!(runtime.next_effect(100, None).unwrap().is_none());
            assert!(runtime.retire_native_unsent(&effect, &transport).is_err());
        } else {
            assert_eq!(runtime.in_flight(), Some(&effect));
            assert!(runtime.native_unsent_effect().is_none());
        }
    }
    let mut first = runtime();
    let mut second = runtime();
    let a = first
        .next_effect(100, Some(&facts(100, -100)))
        .unwrap()
        .unwrap();
    let b = second
        .next_effect(100, Some(&facts(100, -100)))
        .unwrap()
        .unwrap();
    assert_eq!(a.request(), b.request());
    assert_ne!(a, b);
    assert_eq!(
        second.retire_native_unsent(&a, &NativeHspTransportIdentity::new()),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert_eq!(second.in_flight(), Some(&b));
}

#[test]
fn native_unsent_append_and_play_preserve_stop_obligation() {
    for play in [false, true] {
        let mut runtime = runtime();
        let f = facts(100, -100);
        setup(&mut runtime, &f);
        offer(&mut runtime, &[0, 100], 1, &f);
        let append = runtime.next_effect(100, Some(&f)).unwrap().unwrap();
        let effect = if play {
            complete(&mut runtime, &append, 1, 2, 100, Some(&f));
            runtime.request_play(100, &f).unwrap();
            runtime.next_effect(100, Some(&f)).unwrap().unwrap()
        } else {
            append
        };
        let transport = NativeHspTransportIdentity::new();
        runtime.retire_native_unsent(&effect, &transport).unwrap();
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
        assert_eq!(runtime.native_unsent_effect(), Some(&effect));
        let stop = runtime.next_effect(100, None).unwrap().unwrap();
        assert_eq!(stop.kind(), HspEffectKind::Stop);
        assert!(runtime.retire_native_unsent(&stop, &transport).is_err());
        assert_eq!(runtime.in_flight(), Some(&stop));
    }
}

#[test]
fn native_probe_reservation_excludes_senders_and_old_probe_results() {
    let mut runtime = runtime();
    let f = facts(100, -100);
    let setup = runtime.next_effect(100, Some(&f)).unwrap().unwrap();
    let transport = NativeHspTransportIdentity::new();
    let reservation = runtime.reserve_native(&setup, &transport).unwrap();
    reservation.begin_claim().unwrap();
    reservation.claimed();
    assert!(reservation.join());
    complete(&mut runtime, &setup, -1, 2, 100, Some(&f));
    let probe = runtime
        .reserve_post_setup_probe(&setup, &transport)
        .unwrap();
    assert!(runtime
        .reserve_post_setup_probe(&setup, &transport)
        .is_err());
    assert_eq!(
        runtime.validate_post_setup_probe(&probe),
        Err(HspRuntimeError::NativeDispatchBusy)
    );
    probe.join();
    runtime.validate_post_setup_probe(&probe).unwrap();
    let replacement = runtime
        .reserve_post_setup_probe(&setup, &transport)
        .unwrap();
    assert_eq!(
        runtime.validate_post_setup_probe(&probe),
        Err(HspRuntimeError::ForeignEffect)
    );
    runtime.stop();
    let stop = runtime.next_effect(100, None).unwrap().unwrap();
    assert!(runtime.reserve_native(&stop, &transport).is_err());
    replacement.join();
    assert_eq!(
        runtime.validate_post_setup_probe(&replacement),
        Err(HspRuntimeError::ForeignEffect)
    );
    assert!(runtime.reserve_native(&stop, &transport).is_ok());
}

fn finite_policy() -> HspFiniteDrainPolicy {
    HspFiniteDrainPolicy {
        maximum_duration_ns: 1500,
        maximum_terminal_overrun_ns: 200_000_000,
        stop_budget_ns: 100,
    }
}
fn finite_config() -> HspFiniteDrainConfig {
    HspFiniteDrainConfig {
        policy: finite_policy(),
        maximum_stop_dispatch_delay_ns: 50,
    }
}
fn finite_window(facts: &HspDispatchFacts) -> HspFiniteDrainWindow {
    let phase = facts.reference().phase();
    HspFiniteDrainWindow {
        at_ns: facts.reference().at_ns(),
        phase,
        stop_at_ns: facts.reference().at_ns() + finite_policy().stop_budget_ns,
        stop_phase: HspPhaseWindow {
            earliest_ms: phase.earliest_ms + 20,
            latest_ms: phase.latest_ms + 20,
        },
    }
}
fn terminal_sample(time: u32, position: u8, sequence: u64) -> HspSample {
    HspSample {
        point: HspPoint::new(time, position).unwrap(),
        source_sequence: sequence,
        captured_at_ns: 100,
        accepted_at_ns: 100,
        continuous_from_previous: true,
    }
}
#[test]
fn finite_tail_waits_for_final_ack_and_join_then_stops_once_without_retiming() {
    let mut runtime = runtime();
    let original = facts(100, -100);
    prefill(&mut runtime, &original);
    play(&mut runtime, &original);
    offer(&mut runtime, &[200], 3, &original);
    let append = runtime.next_effect(100, Some(&original)).unwrap().unwrap();
    let terminal = terminal_sample(200, 0, 3);
    let seal = runtime
        .seal_input(
            &original.binding.identity.live,
            terminal,
            100,
            finite_config(),
            100,
            &original,
        )
        .unwrap();
    let pending = facts(110, 0);
    assert_eq!(
        runtime
            .tick_snapshot(110, Some(&pending), Some(finite_window(&pending)), true)
            .unwrap(),
        HspRuntimeTick::Continue
    );
    assert_eq!(runtime.stream().input_state(), HspInputState::Sealed);
    complete(&mut runtime, &append, 2, 1, 110, Some(&pending));
    assert_eq!(runtime.stream().total_added(), 3);
    assert_eq!(
        runtime
            .tick_snapshot(110, Some(&pending), Some(finite_window(&pending)), false)
            .unwrap(),
        HspRuntimeTick::Continue
    );
    assert_eq!(runtime.stream().input_state(), HspInputState::Sealed);
    runtime
        .tick_snapshot(110, Some(&pending), Some(finite_window(&pending)), true)
        .unwrap();
    assert_eq!(runtime.stream().input_state(), HspInputState::Draining);
    let late = facts(120, 195);
    assert_eq!(
        runtime
            .tick_snapshot(120, Some(&late), Some(finite_window(&late)), true)
            .unwrap(),
        HspRuntimeTick::Continue
    );
    assert_eq!(
        runtime.offer(
            &original.binding.identity.live,
            terminal_sample(300, 50, 4),
            120,
            &late
        ),
        Err(HspRuntimeError::Core(HspStreamError::InputSealed))
    );
    let crossing = facts(130, 200);
    let HspRuntimeTick::TailReached(reached) = runtime
        .tick_snapshot(130, Some(&crossing), Some(finite_window(&crossing)), true)
        .unwrap()
    else {
        panic!("tail passage missing")
    };
    assert_eq!(reached.terminal(), terminal);
    assert_eq!(reached.seal(), &seal);
    assert_eq!(
        runtime
            .seal_input(
                &original.binding.identity.live,
                terminal,
                100,
                finite_config(),
                140,
                &facts(140, 201)
            )
            .unwrap(),
        seal
    );
    assert_eq!(runtime.finite_tail().unwrap().reached_at_ns(), 130);
    let mut changed = finite_config();
    changed.maximum_stop_dispatch_delay_ns += 1;
    assert_eq!(
        runtime.seal_input(
            &original.binding.identity.live,
            terminal,
            100,
            changed,
            140,
            &facts(140, 201),
        ),
        Err(HspRuntimeError::Core(HspStreamError::ForeignSeal))
    );
    assert_eq!(
        runtime.finite_seal().unwrap().expires_at_ns(),
        seal.expires_at_ns()
    );
    let stop = runtime.next_effect(130, None).unwrap().unwrap();
    assert_eq!(stop.kind(), HspEffectKind::Stop);
    assert!(runtime.next_effect(140, None).unwrap().is_none());
    assert!(!runtime.finite_aborted());
}
#[test]
fn finite_tail_does_not_treat_core_running_as_play_acknowledged() {
    let mut runtime = runtime();
    let original = facts(100, -100);
    prefill(&mut runtime, &original);
    runtime.request_play(100, &original).unwrap();
    let play = runtime.next_effect(100, Some(&original)).unwrap().unwrap();
    runtime
        .seal_input(
            &original.binding.identity.live,
            terminal_sample(100, 100, 2),
            100,
            finite_config(),
            100,
            &original,
        )
        .unwrap();
    runtime
        .tick_snapshot(100, Some(&original), Some(finite_window(&original)), true)
        .unwrap();
    assert_eq!(runtime.stream().input_state(), HspInputState::Sealed);
    complete(&mut runtime, &play, 1, 1, 100, Some(&original));
    runtime
        .tick_snapshot(100, Some(&original), Some(finite_window(&original)), true)
        .unwrap();
    assert_eq!(runtime.stream().input_state(), HspInputState::Draining);
}
#[test]
fn finite_tail_unknown_or_late_final_append_never_establishes_coverage() {
    for unknown in [true, false] {
        let mut runtime = runtime();
        let original = facts(100, -100);
        prefill(&mut runtime, &original);
        play(&mut runtime, &original);
        offer(&mut runtime, &[200], 3, &original);
        let append = runtime.next_effect(100, Some(&original)).unwrap().unwrap();
        runtime
            .seal_input(
                &original.binding.identity.live,
                terminal_sample(200, 0, 3),
                100,
                finite_config(),
                100,
                &original,
            )
            .unwrap();
        if unknown {
            runtime
                .complete_settled(
                    &append,
                    HspSettledTransport::Unknown,
                    110,
                    Some(&facts(110, 0)),
                )
                .unwrap();
            let late = complete(&mut runtime, &append, 2, 1, 120, Some(&facts(120, 1)));
            assert_eq!(late.outcome, HspObservedOutcome::Unknown);
        } else {
            let late = complete(&mut runtime, &append, 2, 1, 120, Some(&facts(120, 100)));
            assert!(matches!(late.outcome, HspObservedOutcome::Acknowledged(_)));
        }
        assert_eq!(runtime.stream().total_added(), 2);
        assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
        assert!(runtime.finite_aborted() && runtime.finite_tail().is_none());
    }
}
#[test]
fn finite_tail_expiry_wins_at_terminal_crossing_and_no_zero_or_one_point_play_is_invented() {
    let mut empty = runtime();
    let original = facts(100, -100);
    setup(&mut empty, &original);
    assert!(empty
        .seal_input(
            &original.binding.identity.live,
            terminal_sample(0, 0, 1),
            100,
            finite_config(),
            100,
            &original
        )
        .is_err());
    let mut one = runtime();
    setup(&mut one, &original);
    offer(&mut one, &[0], 1, &original);
    one.seal_input(
        &original.binding.identity.live,
        terminal_sample(0, 0, 1),
        100,
        finite_config(),
        100,
        &original,
    )
    .unwrap();
    assert!(one.next_effect(100, Some(&original)).unwrap().is_none());
    assert_eq!(one.state(), HspRuntimeState::Prefilling);
    assert!(one.finite_tail().is_none());
    let mut playing = runtime();
    prefill(&mut playing, &original);
    play(&mut playing, &original);
    playing
        .seal_input(
            &original.binding.identity.live,
            terminal_sample(100, 100, 2),
            100,
            finite_config(),
            100,
            &original,
        )
        .unwrap();
    playing
        .tick_snapshot(100, Some(&original), Some(finite_window(&original)), true)
        .unwrap();
    let at_tail = facts_for(binding(1), 120, 100, 150);
    assert!(playing
        .tick_snapshot(120, Some(&at_tail), Some(finite_window(&at_tail)), true)
        .is_err());
    assert!(playing.finite_tail().is_none() && playing.finite_aborted());
    assert_eq!(playing.stream().input_state(), HspInputState::Draining);
    assert!(!playing.is_finite_draining());
    assert_eq!(
        playing.next_effect(120, None).unwrap().unwrap().kind(),
        HspEffectKind::Stop
    );
}

include!("handy_hsp_runtime_transition_tests.rs");

fn overlapping_phase_facts(now: u64, lower: i64, upper: i64) -> HspDispatchFacts {
    HspDispatchFacts::new(
        binding(1),
        HspPlayReference::new(
            now,
            HspPhaseWindow {
                earliest_ms: lower,
                latest_ms: upper,
            },
            -100,
            10_000 + now,
        )
        .unwrap(),
        HspAuthorityWindow::new(2000, 2000, 2000),
    )
    .unwrap()
}

#[test]
fn overlapping_phase_keeps_original_play_bytes_and_expiry() {
    let mut runtime = runtime();
    let original = facts_for(binding(1), 100, -100, 110);
    prefill(&mut runtime, &original);
    runtime.request_play(100, &original).unwrap();
    let wider = overlapping_phase_facts(101, -101, -99);
    let effect = runtime.next_effect(101, Some(&wider)).unwrap().unwrap();
    assert_eq!(effect.kind(), HspEffectKind::Play);
    assert_eq!(effect.request().body(), Some(br#"{"start_time":-100,"server_time":10100,"playback_rate":1.0,"pause_on_starving":false,"loop":false}"#.as_slice()));
    assert_eq!(
        effect.original_facts().unwrap().reference(),
        original.reference()
    );
    let later = overlapping_phase_facts(110, -102, -98);
    assert_eq!(
        runtime.validate_dispatch(&effect, 110, Some(&later)),
        Err(HspRuntimeError::ExpiredAuthority)
    );
    assert_eq!(runtime.state(), HspRuntimeState::StopRequired);
}

#[test]
fn overlapping_phase_producer_offer_keeps_original_sample_and_append_facts() {
    let mut runtime = runtime();
    let initial = facts(100, -100);
    prefill(&mut runtime, &initial);
    play(&mut runtime, &initial);
    let wider = overlapping_phase_facts(101, -101, -99);
    let sample = HspSample {
        point: HspPoint::new(200, 50).unwrap(),
        source_sequence: 3,
        captured_at_ns: 100,
        accepted_at_ns: 101,
        continuous_from_previous: true,
    };
    assert_eq!(
        runtime.offer(&binding(1).identity.live, sample, 101, &wider),
        Ok(HspOfferOutcome::Accepted)
    );
    let append = runtime.next_effect(101, Some(&wider)).unwrap().unwrap();
    assert_eq!(append.kind(), HspEffectKind::Append);
    assert_eq!(append.chunk().unwrap().samples(), &[sample]);
    assert_eq!(
        append.original_facts().unwrap().reference(),
        wider.reference()
    );
    let current = overlapping_phase_facts(102, -102, -98);
    runtime
        .validate_dispatch(&append, 102, Some(&current))
        .unwrap();
    assert_eq!(
        append.original_facts().unwrap().reference(),
        wider.reference()
    );
    assert_eq!(append.chunk().unwrap().samples(), &[sample]);
}
