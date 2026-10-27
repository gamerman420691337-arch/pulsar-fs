// Included in the rendered fixture. Only the ordinary browser can retire A,
// start B, or request either inspection. The observer releases one native reply.
struct StaleInspectRun<'a> {
    f: &'a Fixture,
    root: &'a std::path::Path,
    output: &'a std::path::Path,
    deadline: Instant,
    gate: &'a crate::daemon::TestResponseGateHandle,
    peer: &'a SustainedPeer,
    io: &'a SustainedIo,
    first: &'a LiveCaptureReport,
    dossier: &'a HandyHspSourceQualificationReport,
}

fn stale_inspect_no_device(run: &StaleInspectRun<'_>) {
    let mut ids = UiOriginals::default();
    discover_ui(run.f, &mut ids);
    assert_eq!(
        ids_value(&ids, &None),
        json!({"preparation":null,"stream":null,"play":null,"continuity":null,"stop":null,"cancel":null}),
        "inspection must not create an HSP mutation"
    );
    check_effect_counts(run.peer, FaultCase::StaleInspect);
    assert_eq!(run.io.syncs.load(Ordering::Acquire), 0);
}

fn stale_inspect_stage(run: &StaleInspectRun<'_>, stage: &str, read: &RequestId) {
    record(
        run.output,
        "fault-stage.json",
        &json!({
            "case":"stale-inspection","stage":stage,"original_read_request_id":read,
            "capture_a":run.first.identity.epoch,
            "originals":ids_value(&UiOriginals::default(),&None),
            "required_ui":FaultCase::StaleInspect.required_ui(),"qualified":false,"physical_device":false
        }),
    );
}

fn stale_inspect_gate(
    gate: &crate::daemon::TestResponseGateHandle,
    original: Option<&RequestId>,
    held: bool,
) -> crate::daemon::TestResponseGateSnapshot {
    let value = gate.snapshot();
    assert!(value.request_id.is_some());
    if let Some(id) = original {
        assert_eq!(value.request_id.as_ref(), Some(id));
    }
    assert_eq!(value.captures, 1);
    assert_eq!(value.response_succeeded, Some(true));
    assert_eq!(value.source_qualification_response_matched, Some(true));
    assert!(value.bind_response_classification.is_none());
    assert!(value
        .response_sha256
        .as_ref()
        .is_some_and(|h| h.len() == 64));
    assert_eq!(value.waiting, held);
    assert_eq!(
        value.release,
        if held {
            None
        } else {
            Some(crate::daemon::TestResponseGateRelease::Explicit)
        }
    );
    if held {
        assert_eq!(value.matching_handled_responses, 1);
        assert!(!value.captured_write_attempted);
        assert!(value.captured_write_succeeded.is_none());
    }
    value
}

fn stale_inspect_chain(run: &StaleInspectRun<'_>, name: &str) -> InterruptionChain {
    let chain = interruption_chain(run.f, run.output, name);
    assert!(chain
        .captures
        .contains_key(&run.first.identity.epoch.to_string()));
    assert!(
        chain.captures.len() <= 2,
        "only explicit players A and B are permitted"
    );
    assert!(
        chain.preparations.is_empty() && chain.transfers.is_empty(),
        "this fault must not become a source-continuation scenario"
    );
    chain
}

fn stale_inspect_replacement(first: &LiveCaptureReport, next: &LiveCaptureReport) {
    first.validate().unwrap();
    next.validate().unwrap();
    assert_ne!(first.identity.epoch, next.identity.epoch);
    assert_eq!(first.identity.project, next.identity.project);
    assert_eq!(first.identity.revision, next.identity.revision);
    assert_eq!(first.identity.source, next.identity.source);
    assert_eq!(first.engine_epoch, next.engine_epoch);
    let a = first.options.browser_analysis.as_ref().unwrap();
    let b = next.options.browser_analysis.as_ref().unwrap();
    assert_ne!(a.player.player_id, b.player.player_id);
    assert_eq!(a.player.launch_id, b.player.launch_id);
    assert_eq!(a.player.page_id, b.player.page_id);
    assert_eq!(a.player.source_id, b.player.source_id);
    assert!(b.player.generation > a.player.generation);
    assert_eq!(a.player.time_basis, b.player.time_basis);
    assert_eq!(a.policy, b.policy);
    let media_a = first.options.browser_media.as_ref().unwrap();
    let media_b = next.options.browser_media.as_ref().unwrap();
    assert_eq!(media_a.selection, media_b.selection);
    assert_eq!(media_a.clock, media_b.clock);
    assert_eq!(media_a.lease_ms, 30_000);
    assert_eq!(media_a.lease_ms, media_b.lease_ms);
    assert_eq!(first.options.lease_ms, next.options.lease_ms);
    assert_eq!(first.options.lease_ms, 30_000);
    assert_eq!(
        serde_json::to_value(&first.options.inference).unwrap(),
        serde_json::to_value(&next.options.inference).unwrap()
    );
    assert_eq!(
        serde_json::to_value(first.options.policy).unwrap(),
        serde_json::to_value(next.options.policy).unwrap()
    );
}

fn run_stale_inspect(run: StaleInspectRun<'_>) -> Value {
    let ids = UiOriginals::default();
    let held = loop {
        stale_inspect_no_device(&run);
        if run.gate.snapshot().request_id.is_some() {
            break stale_inspect_gate(run.gate, None, true);
        }
        bounded_wait(run.root, run.deadline);
    };
    let original = held.request_id.as_ref().unwrap().clone();
    let first_player = run
        .first
        .options
        .browser_analysis
        .as_ref()
        .unwrap()
        .player
        .clone();
    record(
        run.output,
        "stale-inspection-held.json",
        &json!({
            "gate":held,"original_player":first_player,"original_capture":run.first.identity,
            "maximum_gate_hold_ms":3000,"gate_deadline_basis":"original-native-frame-ingress",
            "source_dossier_response_matched":true,"client_consumption_claimed":false
        }),
    );
    stale_inspect_stage(&run, "inspection-reply-held", &original);

    // CUA must first use HSP Stop, capture its route origin, and Stop preview A.
    // A's actual worker/storage fence is empty BEFORE the explicit replacement.
    let first_settled = loop {
        stale_inspect_no_device(&run);
        stale_inspect_gate(run.gate, Some(&original), true);
        let chain = stale_inspect_chain(&run, "stale-inspection-a-cleanup-latest.json");
        assert_eq!(
            chain.captures.len(),
            1,
            "B started before A custody settled"
        );
        if chain.settled() {
            assert!(!run.f.engine.is_shutting_down());
            break chain;
        }
        bounded_wait(run.root, run.deadline);
    };
    record(
        run.output,
        "stale-inspection-source-a-settled.json",
        &json!({
            "chain":first_settled,"before_original_reply_release":true,"before_player_b":true,
            "before_engine_teardown":true,"actual_cua_public_stop_evidence_required":true
        }),
    );
    stale_inspect_stage(&run, "source-a-settled", &original);

    let second = loop {
        stale_inspect_no_device(&run);
        stale_inspect_gate(run.gate, Some(&original), true);
        let chain = stale_inspect_chain(&run, "stale-inspection-replacement-latest.json");
        if let Some(next) = chain
            .captures
            .values()
            .find(|c| c.identity.epoch != run.first.identity.epoch)
        {
            stale_inspect_replacement(run.first, next);
            assert!(
                next.active,
                "replacement must be live before original reply release"
            );
            break next.clone();
        }
        bounded_wait(run.root, run.deadline);
    };
    let before_release = stale_inspect_gate(run.gate, Some(&original), true);
    let second_player = second
        .options
        .browser_analysis
        .as_ref()
        .unwrap()
        .player
        .clone();
    record(
        run.output,
        "stale-inspection-replacement-before-release.json",
        &json!({
            "gate":before_release,"player_a":first_player,"player_b":second_player,
            "capture_a":run.first.identity,"capture_b":second.identity,
            "first_source_custody_settled":true,"native_replacement_before_release":true
        }),
    );
    run.gate.release();
    assert_eq!(run.gate.wait_written(Duration::from_secs(3)), Some(true));
    let written = stale_inspect_gate(run.gate, Some(&original), false);
    assert!(written.captured_write_attempted);
    assert_eq!(written.captured_write_succeeded, Some(true));
    assert_eq!(written.response_sha256, held.response_sha256);
    record(
        run.output,
        "stale-inspection-original-written.json",
        &written,
    );
    let second_cpu = loop {
        stale_inspect_no_device(&run);
        let current = snapshot(run.f, &second.identity);
        stale_inspect_replacement(run.first, &current);
        assert!(current.active, "B retired before its CPU observation");
        if let Some(cpu) = accepted_observation(run.f, &current) {
            break cpu;
        }
        bounded_wait(run.root, run.deadline);
    };
    record(
        run.output,
        "stale-inspection-replacement-cpu.json",
        &second_cpu,
    );
    stale_inspect_stage(&run, "replacement-reply-written", &original);

    // This is a later EXPLICIT Inspect requested by CUA, never an automatic
    // replacement. The one-shot gate must retain its first response identity.
    let fresh_read = loop {
        stale_inspect_no_device(&run);
        let gate = stale_inspect_gate(run.gate, Some(&original), false);
        assert!(
            gate.matching_handled_responses <= 2,
            "inspection was retried or replaced again"
        );
        if gate.matching_handled_responses == 2 {
            assert_ne!(gate.last_matching_request_id.as_ref(), Some(&original));
            assert!(gate.last_matching_request_id.is_some());
            break gate;
        }
        bounded_wait(run.root, run.deadline);
    };
    record(
        run.output,
        "stale-inspection-explicit-fresh-read.json",
        &json!({
            "gate":fresh_read,"replacement_cpu":second_cpu,
            "actual_cua_fresh_inspect_evidence_required":true,"client_consumption_claimed":false
        }),
    );

    // The driver captures new inspection and consent reset before stopping B.
    let settled = loop {
        stale_inspect_no_device(&run);
        let chain = stale_inspect_chain(&run, "stale-inspection-final-chain-latest.json");
        assert_eq!(chain.captures.len(), 2);
        if chain.settled() {
            assert!(!run.f.engine.is_shutting_down());
            break chain;
        }
        bounded_wait(run.root, run.deadline);
    };
    let final_ids = settled.identities();
    record(
        run.output,
        "stale-inspection-final-chain-settled.json",
        &json!({
            "chain":settled,"before_engine_teardown":true,"both_explicit_players_retired":true
        }),
    );
    let quiet_until = Instant::now() + Duration::from_millis(QUIET_MS);
    loop {
        stale_inspect_no_device(&run);
        let gate = stale_inspect_gate(run.gate, Some(&original), false);
        assert_eq!(gate.matching_handled_responses, 2);
        assert_eq!(
            gate.last_matching_request_id,
            fresh_read.last_matching_request_id
        );
        let chain = stale_inspect_chain(&run, "stale-inspection-quiet-chain.json");
        assert!(chain.settled());
        assert_eq!(chain.identities(), final_ids);
        if Instant::now() >= quiet_until {
            break;
        }
        bounded_wait(run.root, run.deadline);
    }
    let proof = json!({
        "case":"stale-inspection","native_scenario_passed":true,
        "actual_rendered_browser":true,"actual_confined_cpu":true,"one_engine":true,
        "public_ui_controls":false,"public_ui_controls_require_cua":true,
        "direct_fixture_hsp_actions":false,"originals":ids_value(&ids,&None),
        "peer_effects":effect_counts(run.peer),"synchronizations":0,
        "original_ids_retained":true,"no_automatic_readmission":true,
        "no_automatic_readmission_observation_ms":QUIET_MS,
        "coverage_scope":"native-inspection-reply-after-explicit-player-replacement",
        "stale_inspection":{
            "source_dossier_id":run.dossier.dossier_id,
            "source_environment_sha256":run.dossier.manifest.subject.browser_environment.sha256,
            "source_model_sha256":run.dossier.manifest.subject.inference_profile.model.sha256,
            "original_read_request_id":original,"fresh_read_request_id":fresh_read.last_matching_request_id,
            "held_response_sha256":held.response_sha256,"held_response":held,
            "replacement_before_release":before_release,"released_response":written,
            "fresh_read_observation":fresh_read,"player_a":first_player,"player_b":second_player,
            "capture_a":run.first.identity.epoch,"capture_b":second.identity.epoch,
            "first_source_custody_settled_before_replacement_and_release":true,
            "both_source_chains_settled_before_teardown":true,
            "maximum_gate_hold_ms":3000,"gate_deadline_basis":"original-native-frame-ingress",
            "original_response_released_unchanged":true,
            "reply_receipt_claimed":false,"client_consumption_claimed":false,
            "rendered_readiness_and_fresh_inspect_require_cua":true
        },
        "interruption_chain_identities":final_ids,"history_only_restart":false,
        "awaiting_supervisor_stop":true,"physical_device":false,"qualified":false
    });
    record(run.output, "result-ready.json", &proof);
    stale_inspect_stage(&run, "awaiting-supervisor-stop", &original);
    while !stopped(run.root) {
        assert!(
            Instant::now() < run.deadline,
            "original campaign cleanup deadline elapsed"
        );
        stale_inspect_no_device(&run);
        let gate = stale_inspect_gate(run.gate, Some(&original), false);
        assert_eq!(gate.matching_handled_responses, 2);
        let chain = stale_inspect_chain(&run, "stale-inspection-quiet-chain.json");
        assert!(chain.settled());
        assert_eq!(chain.identities(), final_ids);
        std::thread::sleep(Duration::from_millis(50));
    }
    proof
}
