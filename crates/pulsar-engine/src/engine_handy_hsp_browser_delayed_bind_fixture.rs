// Included in the rendered public-UI fault fixture. All HSP mutations come from
// the ordinary browser owner. This observer releases only the original reply.
// An original Stop ID alone cannot identify the public HSP Stop route: preview
// retirement also creates one. The combined UI contract verifies that route.
struct DelayedBindRun<'a> {
    f: &'a Fixture,
    root: &'a std::path::Path,
    output: &'a std::path::Path,
    deadline: Instant,
    gate: &'a crate::daemon::TestResponseGateHandle,
    peer: &'a SustainedPeer,
    first: &'a LiveCaptureReport,
    originals: &'a mut UiOriginals,
    cancel_id: &'a mut Option<RequestId>,
}
struct DelayedBindOutcome {
    proof: Value,
    chain_ids: Value,
    source_generations: Value,
}
fn delayed_bind_first_stop(f: &Fixture, output: &std::path::Path, id: &RequestId) -> Value {
    let handle = f
        .engine
        .handy_hsp_execution
        .try_lock()
        .ok()
        .and_then(|pump| pump.as_ref().map(|pump| pump.handle()));
    let Some(handle) = handle else {
        return json!({"available":false,"reason":"execution_handle_unavailable"});
    };
    match handle.first_stop_for_session(&f.session, &f.project, id) {
        Ok(observation) => {
            let result = json!({"available":true,"stream_request_id":id,
                "first_stop":observation.as_ref().map(|value|value.diagnostic())});
            if observation.is_some() && !output.join("first-native-stop.json").exists() {
                record(output, "first-native-stop.json", &result);
            }
            result
        }
        Err(error) => json!({"available":false,"reason":format!("{error:?}")}),
    }
}
fn check_original_bind_gate(snapshot: &crate::daemon::TestResponseGateSnapshot, id: &RequestId) {
    assert_eq!(snapshot.request_id.as_ref(), Some(id));
    assert_eq!(snapshot.captures, 1);
    assert_eq!(
        snapshot.matching_handled_responses, 1,
        "original Bind must not be reissued"
    );
    assert_eq!(
        snapshot.response_succeeded,
        Some(true),
        "gated outer error is not a successful Bind"
    );
    assert_eq!(
        snapshot.bind_response_classification,
        Some("admitted_live"),
        "gated body error or historical description is not native Bind admission"
    );
    assert!(snapshot
        .response_sha256
        .as_ref()
        .is_some_and(|h| h.len() == 64));
}
fn check_delayed_bind_quiet(
    f: &Fixture,
    output: &std::path::Path,
    gate: &crate::daemon::TestResponseGateHandle,
    ids: &UiOriginals,
    proof: &Value,
) {
    let id = ids.stream.as_ref().unwrap();
    let snapshot = gate.snapshot();
    let live = live_status(f, id);
    let preparation = public_preparation(f, ids.preparation.as_ref().unwrap());
    record(
        output,
        "delayed-bind-after-release.json",
        &json!({"gate":snapshot,"live":live,"preparation":preparation}),
    );
    check_original_bind_gate(&snapshot, id);
    assert!(!snapshot.waiting);
    assert!(snapshot.captured_write_attempted);
    assert_eq!(
        snapshot.captured_write_succeeded,
        Some(true),
        "original framed reply write failed"
    );
    assert_eq!(
        snapshot.release,
        Some(crate::daemon::TestResponseGateRelease::Explicit)
    );
    assert_eq!(
        json!(snapshot.response_sha256),
        proof["held_response_sha256"]
    );
    assert!(ids.play.is_none() && ids.continuity.is_none());
    assert_eq!(
        serde_json::to_value(live.origin.as_ref().unwrap()).unwrap(),
        proof["origin"]
    );
    assert_eq!(
        serde_json::to_value(&live.selection).unwrap(),
        proof["original_selection"]
    );
    assert_eq!(
        serde_json::to_value(live.source_subject.as_ref().unwrap()).unwrap(),
        proof["original_source_subject"]
    );
    assert_eq!(live.stream_request_id, *id);
    assert_eq!(
        Some(&live.selection.preparation_request_id),
        ids.preparation.as_ref()
    );
    let native = live.native.as_ref().unwrap();
    assert!(
        native.retired,
        "late original Bind response must not restore native readiness"
    );
    assert_eq!(native.original_stop_request_id.as_ref(), ids.stop.as_ref());
    assert!(native.original_play_request_id.is_none());
    assert!(
        native.active_source.is_none(),
        "no continuity cursor is admitted by this case"
    );
    let runtime = live
        .stream
        .as_ref()
        .and_then(|s| s.native.as_ref())
        .unwrap();
    assert_eq!(
        runtime.runtime_state,
        HandyHspRuntimeState::StopAcknowledged
    );
    assert!(!runtime.native_active && !runtime.metadata_pending);
    assert_eq!(
        json!(preparation.history.as_ref().unwrap().deadline_boottime_ns),
        proof["original_preparation_deadline_boottime_ns"]
    );
}
impl DelayedBindRun<'_> {
    fn run(self) -> DelayedBindOutcome {
        fault_stage(
            self.output,
            FaultCase::DelayedBind,
            "awaiting-public-bind",
            self.originals,
            self.cancel_id,
        );
        let held = loop {
            observe_ids(self.f, self.originals, self.cancel_id);
            let snapshot = self.gate.snapshot();
            if snapshot.request_id.is_some() {
                record(self.output, "delayed-bind-gate-held.json", &snapshot);
                let id = self
                    .originals
                    .stream
                    .as_ref()
                    .expect("actual gate must match retained public Bind");
                check_original_bind_gate(&snapshot, id);
                assert!(
                    snapshot.waiting && snapshot.release.is_none(),
                    "original Bind gate expired before original Stop evidence"
                );
                assert!(!snapshot.captured_write_attempted);
                assert_eq!(snapshot.captured_write_succeeded, None);
                break snapshot;
            }
            assert!(self.originals.play.is_none() && self.originals.continuity.is_none());
            bounded_wait(self.root, self.deadline);
        };
        let bind_id = self.originals.stream.as_ref().unwrap().clone();
        let live = live_status(self.f, &bind_id);
        let preparation = public_preparation(self.f, self.originals.preparation.as_ref().unwrap());
        record(
            self.output,
            "delayed-bind-native-admission.json",
            &json!({"gate":held,"live":live,"preparation":preparation,"originals":ids_value(self.originals,self.cancel_id)}),
        );
        assert!(live.error.is_none() && live.source_subject.is_some());
        assert_eq!(live.stream_request_id, bind_id);
        assert_eq!(
            Some(&live.selection.preparation_request_id),
            self.originals.preparation.as_ref()
        );
        let origin = live
            .origin
            .as_ref()
            .expect("actual native Bind origin")
            .clone();
        let selection = live.selection.clone();
        let source_subject = live.source_subject.as_ref().unwrap().clone();
        let source_subject_value = serde_json::to_value(&source_subject).unwrap();
        let native = live.native.as_ref().expect("actual native Bind controller");
        assert!(
            native.active_source.is_none(),
            "continuity was never enabled"
        );
        assert!(native.original_play_request_id.is_none());
        // A no-continuity Bind owns this exact original selection. The optional
        // continuity cursor is deliberately absent, even after actual admission.
        assert_eq!(selection.capture_epoch, self.first.identity.epoch);
        assert_eq!(
            selection.inference_attempt_id,
            self.first.inference.as_ref().unwrap().attempt_id
        );
        let analysis = self.first.options.browser_analysis.as_ref().unwrap();
        let media = self.first.options.browser_media.as_ref().unwrap();
        assert_eq!(selection.browser_timing, analysis.timing_binding_id);
        assert_eq!(source_subject.project_id, self.first.identity.project);
        assert_eq!(source_subject.revision, self.first.identity.revision);
        assert_eq!(source_subject.selection, media.selection);
        assert_eq!(source_subject.clock, media.clock);
        assert_eq!(source_subject.lookahead, analysis.policy);
        assert_eq!(
            source_subject.source_time_basis,
            self.first.options.source_time_basis
        );
        assert_eq!(
            source_subject.maximum_analysis_age_ns,
            self.first.options.policy.maximum_analysis_age_ns
        );
        assert_eq!(
            serde_json::to_value(&source_subject.inference_profile).unwrap(),
            serde_json::to_value(&self.first.options.inference.as_ref().unwrap().profile).unwrap()
        );
        record(
            self.output,
            "delayed-bind-original-source.json",
            &json!({"selection":selection,"origin":origin,"source_subject":source_subject,"first_capture":self.first.identity,"first_inference_attempt":self.first.inference.as_ref().unwrap().attempt_id,"browser_analysis":analysis,"continuity_enabled":false,"active_source":null}),
        );
        let original_preparation_deadline = preparation
            .history
            .as_ref()
            .expect("actual synchronization history")
            .deadline_boottime_ns;
        assert!(self.originals.play.is_none() && self.originals.continuity.is_none());
        fault_stage(
            self.output,
            FaultCase::DelayedBind,
            "bind-response-held",
            self.originals,
            self.cancel_id,
        );
        let stop_while_held = loop {
            observe_ids(self.f, self.originals, self.cancel_id);
            let native = live_status(self.f, &bind_id);
            let gate = self.gate.snapshot();
            record(
                self.output,
                "delayed-bind-awaiting-stop.json",
                &json!({"gate":gate,"live":native,"peer_effects":effect_counts(self.peer),"originals":ids_value(self.originals,self.cancel_id),"first_native_stop":delayed_bind_first_stop(self.f,self.output,&bind_id)}),
            );
            check_original_bind_gate(&gate, &bind_id);
            assert!(
                gate.waiting && gate.release.is_none(),
                "original Stop must be observed before the original Bind response leaves its gate"
            );
            assert!(!gate.captured_write_attempted);
            assert_eq!(gate.captured_write_succeeded, None);
            assert_eq!(native.origin.as_ref(), Some(&origin));
            assert_eq!(native.selection, selection);
            assert_eq!(
                serde_json::to_value(native.source_subject.as_ref().unwrap()).unwrap(),
                source_subject_value
            );
            assert!(
                native.native.as_ref().unwrap().active_source.is_none(),
                "continuity cursor appeared without consent"
            );
            assert!(self.originals.play.is_none() && self.originals.continuity.is_none());
            let counts = effect_counts(self.peer);
            assert_eq!(counts["play"], 0);
            assert!(
                counts["setup"].as_u64().unwrap() <= 1 && counts["stop"].as_u64().unwrap() <= 1
            );
            if self.originals.stop.is_some()
                && native.native.as_ref().is_some_and(|n| {
                    n.retired && n.original_stop_request_id.as_ref() == self.originals.stop.as_ref()
                })
            {
                assert_eq!(
                    counts["setup"], 1,
                    "original Setup must precede the original Stop"
                );
                record(
                    self.output,
                    "original-stop-while-bind-held.json",
                    &json!({"gate":gate,"live":native,"peer_effects":counts,"originals":ids_value(self.originals,self.cancel_id),"original_stop_observed":true,"direct_fixture_stop":false,"original_response_still_held":true}),
                );
                break gate;
            }
            bounded_wait(self.root, self.deadline);
        };
        self.gate.release();
        let released = loop {
            let snapshot = self.gate.snapshot();
            check_original_bind_gate(&snapshot, &bind_id);
            assert_eq!(
                snapshot.release,
                Some(crate::daemon::TestResponseGateRelease::Explicit)
            );
            if !snapshot.waiting && snapshot.captured_write_attempted {
                record(self.output, "delayed-bind-original-write.json", &snapshot);
                assert_eq!(
                    snapshot.captured_write_succeeded,
                    Some(true),
                    "captured original Bind frame was not written successfully"
                );
                break snapshot;
            }
            bounded_wait(self.root, self.deadline);
        };
        assert_eq!(released.response_sha256, held.response_sha256);
        record(self.output, "delayed-bind-gate-released.json", &released);
        fault_stage(
            self.output,
            FaultCase::DelayedBind,
            "bind-reply-released-after-original-stop",
            self.originals,
            self.cancel_id,
        );
        let final_live = loop {
            observe_ids(self.f, self.originals, self.cancel_id);
            let native = live_status(self.f, &bind_id);
            record(
                self.output,
                "delayed-bind-cleanup.json",
                &json!({"live":native,"originals":ids_value(self.originals,self.cancel_id),"first_native_stop":delayed_bind_first_stop(self.f,self.output,&bind_id)}),
            );
            assert!(self.originals.play.is_none() && self.originals.continuity.is_none());
            assert_eq!(native.origin.as_ref(), Some(&origin));
            assert_eq!(native.selection, selection);
            assert_eq!(
                serde_json::to_value(native.source_subject.as_ref().unwrap()).unwrap(),
                source_subject_value
            );
            assert!(
                native.native.as_ref().unwrap().active_source.is_none(),
                "continuity cursor appeared without consent"
            );
            if native.native.as_ref().is_some_and(|n| {
                n.retired && n.original_stop_request_id.as_ref() == self.originals.stop.as_ref()
            }) && native
                .stream
                .as_ref()
                .and_then(|s| s.native.as_ref())
                .is_some_and(|n| {
                    n.runtime_state == HandyHspRuntimeState::StopAcknowledged
                        && !n.native_active
                        && !n.metadata_pending
                })
            {
                break native;
            }
            bounded_wait(self.root, self.deadline);
        };
        record(self.output, "final-stream.json", &final_live);
        let (chain_ids, source_generations) = loop {
            // The driver also stops preview explicitly. A still-live analysis
            // worker cannot be credited as cleanup merely by Engine teardown.
            let chain = interruption_chain(self.f, self.output, "delayed-bind-chain-latest.json");
            let order = validate_interruption_chain(&chain, self.first);
            if chain.settled() {
                // Every original, advanced and prepared analysis capture must
                // retire. Only the original Bind selection describes this HSP
                // source; no continuity generation is invented from the chain.
                let bound = chain
                    .captures
                    .get(selection.capture_epoch.as_str())
                    .expect("original bound capture remains in its chain");
                assert_eq!(bound.identity, self.first.identity);
                assert_eq!(
                    bound.inference.as_ref().unwrap().attempt_id,
                    selection.inference_attempt_id
                );
                assert_eq!(
                    bound
                        .options
                        .browser_analysis
                        .as_ref()
                        .unwrap()
                        .timing_binding_id,
                    selection.browser_timing
                );
                assert!(!bound.active && bound.input_close.is_none());
                assert_eq!(final_live.selection, selection);
                assert_eq!(final_live.origin.as_ref(), Some(&origin));
                assert_eq!(
                    serde_json::to_value(final_live.source_subject.as_ref().unwrap()).unwrap(),
                    source_subject_value
                );
                let retired = final_live.native.as_ref().unwrap();
                assert!(
                    retired.retired
                        && retired.active_source.is_none()
                        && retired.original_play_request_id.is_none()
                );
                assert_eq!(
                    retired.original_stop_request_id.as_ref(),
                    self.originals.stop.as_ref()
                );
                let runtime = final_live
                    .stream
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .unwrap();
                assert_eq!(
                    runtime.runtime_state,
                    HandyHspRuntimeState::StopAcknowledged
                );
                assert!(
                    !runtime.native_active
                        && !runtime.metadata_pending
                        && runtime.finite_drain.is_none()
                );
                assert!(!self.f.engine.is_shutting_down());
                let generations = json!({"mode":"original-bind-without-continuity","selection_at_bind":selection,"selection_at_final_stop":final_live.selection,"active_source_at_bind":null,"active_source_at_final_stop":null});
                record(
                    self.output,
                    "delayed-bind-chain-settled.json",
                    &json!({"chain":chain,"ordered_epochs":order,"source_generations":generations,"before_engine_teardown":true,"no_finite_input_substitute":true}),
                );
                break (chain.identities(), generations);
            }
            bounded_wait(self.root, self.deadline);
        };
        check_effect_counts(self.peer, FaultCase::DelayedBind);
        let proof = json!({"actual_handled_bind_succeeded":true,"actual_bind_body_classification":held.bind_response_classification,"native_bind_origin_observed":true,"original_stop_observed_while_reply_held":true,"original_stop_request_id":self.originals.stop,"held_bind_request_id":held.request_id,"held_response_sha256":held.response_sha256,"held_response":held,"stop_observation":stop_while_held,"released_response":released,"original_response_released_unchanged":true,"original_response_written_after_original_stop":true,"reply_receipt_claimed":false,"client_consumption_claimed":false,"maximum_gate_hold_ms":3000,"gate_deadline_basis":"original-native-frame-ingress","gate_release":"Explicit","origin":origin,"original_selection":selection,"original_source_subject":source_subject,"continuity_active_source_absent":true,"original_preparation_deadline_boottime_ns":original_preparation_deadline,"original_runtime_retired":true,"play_never_requested":true,"continuity_never_requested":true,"all_source_custody_settled_before_teardown":true,"late_bind_cannot_rearm_native":true,"rendered_control_state_requires_cua_observation":true});
        check_delayed_bind_quiet(self.f, self.output, self.gate, self.originals, &proof);
        DelayedBindOutcome {
            proof,
            chain_ids,
            source_generations,
        }
    }
}
