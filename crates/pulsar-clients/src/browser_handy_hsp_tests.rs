//! Native ownership/DTO tests only; no accepted dossiers or device permits.
use super::*;
use crate::browser_live::{
    BrowserAnalysisFeedBinding, BrowserAnalysisFrameObservation, BrowserAnalysisRole,
};

fn config() -> Config {
    Config {
        engine_epoch: DeviceSessionId::new("engine-epoch").unwrap(),
        device_session: DeviceSessionId::new("device-session").unwrap(),
        device_qualification: HandyQualificationBinding {
            dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
            revision: 2,
        },
        source_qualification: HandyHspSourceQualificationBinding {
            dossier_id: ArtifactId::new("c".repeat(64)).unwrap(),
            revision: 2,
        },
        preparation_timeout_ms: 30_000,
        finite_drain_policy_sha256: None,
        continuity_duration_ms: Some(10_000),
    }
}
fn native() -> Native {
    Native {
        directory: PathBuf::from("/unopened-hsp-owner-test"),
        project: ProjectId::new("project").unwrap(),
        revision: RevisionId::new(1),
        config: config(),
    }
}
fn source() -> BrowserAnalysisFeedSnapshot {
    let player = HandyPlayerIdentity {
        launch_id: AttemptId::new("launch").unwrap(),
        page_id: AttemptId::new("page").unwrap(),
        player_id: AttemptId::new("player").unwrap(),
        source_id: AttemptId::new("selection").unwrap(),
        generation: 1,
        time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
    };
    let media = BrowserMediaBinding {
        preparation_request_id: RequestId::new("media-prepare").unwrap(),
        lease_id: ArtifactId::new("lease").unwrap(),
        engine_epoch: AttemptId::new("engine-epoch").unwrap(),
        project_id: ProjectId::new("project").unwrap(),
        revision: RevisionId::new(1),
        lease_ms: 30_000,
        selection: BrowserMediaSelection {
            source_version: SourceVersionId::new("source").unwrap(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("source-bytes").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("placement").unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: SourceVersionId::new("source").unwrap(),
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(10, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: ProjectTime::ZERO,
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            },
        },
        clock: DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
        },
    };
    let timing = BrowserTimingBinding {
        request_id: RequestId::new("timing-open").unwrap(),
        binding_id: ArtifactId::new("timing").unwrap(),
        engine_epoch: media.engine_epoch.clone(),
        project_id: media.project_id.clone(),
        revision: media.revision,
        player: player.clone(),
        browser_media: media.clone(),
        policy: BrowserTimingPolicy {
            probe_timeout_ms: 1000,
            observation_timeout_ms: 3000,
        },
    };
    let artifact = |c: char| ArtifactIdentity {
        artifact_id: ArtifactId::new(c.to_string().repeat(64)).unwrap(),
        sha256: c.to_string().repeat(64),
        byte_len: 32,
    };
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: artifact('d'),
        runtime: artifact('e'),
        preprocessing: InferencePreprocessing {
            input: ModelInputContract {
                width: 64,
                height: 64,
                channels: 6,
                class_count: 1,
                layout: TensorLayout::Nchw,
                decoder: DetectorDecoder::TemporalScriptPosition,
            },
            resize: InferenceResize::NearestStretch,
            color: InferenceColor::RgbFramePair,
            scale: vec![1.0 / 255.0; 6],
            bias: vec![0.0; 6],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes: 1,
        minimum_output_bytes: 1,
        cpu_threads: 1,
        cuda_resources: None,
    };
    BrowserAnalysisFeedSnapshot {
        binding: BrowserAnalysisFeedBinding {
            feed_id: ArtifactId::new("feed").unwrap(),
            role: BrowserAnalysisRole::Lookahead,
            generation: 1,
            player,
            capture_epoch: AttemptId::new("capture").unwrap(),
            policy: BrowserAnalysisLookaheadPolicy {
                minimum_lead_us: 100_000,
                target_lead_us: 500_000,
                maximum_lead_us: 1_000_000,
            },
        },
        browser_timing: timing,
        engine_epoch: media.engine_epoch.clone(),
        capture: LiveIdentity {
            epoch: AttemptId::new("capture").unwrap(),
            project: media.project_id.clone(),
            revision: media.revision,
            source: media.selection.source_version.clone(),
        },
        browser_media: media,
        inference_attempt_id: Some(AttemptId::new("inference").unwrap()),
        inference_profile: Some(profile),
        estimated: true,
        maximum_analysis_age_ns: 500_000_000,
        latest_observation: Some(BrowserAnalysisFrameObservation {
            feed_id: ArtifactId::new("feed").unwrap(),
            role: BrowserAnalysisRole::Lookahead,
            generation: 1,
            visible_media_us: 1_000_000,
            visible_presented_frames: 3,
            visible_tick_us: 100,
            analysis_tick_us: 200,
            presented_frames: 5,
        }),
        measured_lead_us: Some(500_000),
    }
}
fn inspected() -> Inspected {
    Inspected {
        source: source(),
        device: json!({"state":"pending"}),
        dossier: json!({"state":"pending","origin":"synthetic"}),
    }
}
fn owner() -> (Hsp, mpsc::Receiver<Job>, mpsc::Receiver<Job>) {
    let (mutation, mutations) = mpsc::sync_channel(1);
    let (reads, observations) = mpsc::sync_channel(1);
    (
        Hsp {
            native: native(),
            shared: Arc::new(Mutex::new(State::default())),
            mutation: Some(mutation),
            reads: Some(reads),
            shutdown: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
        },
        mutations,
        observations,
    )
}
fn pending_report(selection: HandyHspLiveSelection, stream: RequestId) -> HandyHspLiveStatus {
    HandyHspLiveStatus {
        project_id: native().project,
        stream_request_id: stream,
        selection,
        source_subject: None,
        origin: None,
        native: None,
        stream: None,
        error: None,
        qualified: false,
    }
}

#[test]
fn configuration_rejects_unbounded_duration_hash_and_authority_fields() {
    config().validate().unwrap();
    let mut pending_device = config();
    pending_device.device_qualification.revision = 1;
    assert!(pending_device.validate().is_err());
    let mut pending_source = config();
    pending_source.source_qualification.revision = 1;
    assert!(pending_source.validate().is_err());
    for duration in [0, 600_001, u32::MAX] {
        let mut c = config();
        c.continuity_duration_ms = Some(duration);
        assert!(c.validate().is_err());
    }
    let mut c = config();
    c.preparation_timeout_ms = 60_001;
    assert!(c.validate().is_err());
    c = config();
    c.engine_epoch = c.device_session.clone();
    assert!(c.validate().is_err());
    c = config();
    c.finite_drain_policy_sha256 = Some("bad".into());
    assert!(c.validate().is_err());
    let body = json!({"engine_epoch":"boot","device_session":"session","device_qualification":{"dossier_id":"b".repeat(64),"revision":1},
        "source_qualification":{"dossier_id":"c".repeat(64),"revision":1},"preparation_timeout_ms":1000,"owner_proof":"must-not-be-accepted"});
    assert!(serde_json::from_value::<Config>(body).is_err());
}

#[test]
fn every_consent_and_empty_body_is_checked_before_queueing() {
    let (mut hsp, mutations, reads) = owner();
    let permit = Permit::for_analysis_test(source().binding.player);
    for (path,body) in [
        ("/player/hsp-prepare",br#"{"allow_synchronization":false}"#.as_slice()),
        ("/player/hsp-prepare",br#"{"allow_synchronization":true,"timeout_ms":1}"#),
        ("/player/hsp-bind",br#"{"acknowledge_current_environment":true,"allow_online_control":false}"#),
        ("/player/hsp-bind",br#"{"acknowledge_current_environment":true,"allow_online_control":true,"capture_epoch":"fake"}"#),
        ("/player/hsp-play",br#"{"allow_play":false}"#),
        ("/player/hsp-continuity-enable",br#"{"allow_continuity":true,"duration_ms":600000}"#),
        ("/player/hsp-status",b"{}"),
        ("/player/hsp-stop",br#"{"request_id":"replacement"}"#),
    ] {
        assert!(hsp.handle(path,&permit,Some(source()),body).is_err(),"{path}");
    }
    assert!(mutations.try_recv().is_err());
    assert!(reads.try_recv().is_err());
    let s = state(&hsp.shared);
    assert!(s.prepare.is_none() && s.stream.is_none() && !s.stop_latched);
}

#[test]
fn bind_uses_native_snapshot_and_immutable_scope_ignores_only_progress() {
    let original = source();
    source_scope(
        &original,
        &original.binding.player,
        &native().project,
        native().revision,
    )
    .unwrap();
    let prepare = RequestId::new("original-prepare").unwrap();
    let selection = bind_selection(&config(), prepare.clone(), &original).unwrap();
    assert_eq!(selection.preparation_request_id, prepare);
    assert_eq!(selection.capture_epoch, original.capture.epoch);
    assert_eq!(
        selection.inference_attempt_id,
        original.inference_attempt_id.clone().unwrap()
    );
    assert_eq!(selection.browser_timing, original.browser_timing.binding_id);
    for case in 0..8 {
        let mut changed = original.clone();
        match case {
            0 => changed.binding.generation += 1,
            1 => changed.capture.epoch = AttemptId::new("other-capture").unwrap(),
            2 => changed.inference_attempt_id = Some(AttemptId::new("other-inference").unwrap()),
            3 => changed.browser_timing.binding_id = ArtifactId::new("other-timing").unwrap(),
            4 => changed.browser_media.lease_id = ArtifactId::new("other-lease").unwrap(),
            5 => changed.inference_profile.as_mut().unwrap().cpu_threads += 1,
            6 => changed.maximum_analysis_age_ns += 1,
            _ => changed.binding.policy.maximum_lead_us += 1,
        }
        assert!(!same_scope(&original, &changed), "case {case}");
    }
    let mut progressed = original.clone();
    progressed
        .latest_observation
        .as_mut()
        .unwrap()
        .presented_frames += 1;
    progressed.measured_lead_us = Some(600_000);
    assert!(same_scope(&original, &progressed));
}

#[test]
fn actual_cpu_metadata_and_matching_native_player_are_required() {
    for case in 0..8 {
        let mut snapshot = source();
        let player = snapshot.binding.player.clone();
        match case {
            0 => snapshot.estimated = false,
            1 => snapshot.inference_attempt_id = None,
            2 => snapshot.inference_profile = None,
            3 => snapshot.inference_profile.as_mut().unwrap().provider = InferenceProvider::Cuda,
            4 => snapshot.binding.player.generation += 1,
            5 => snapshot.capture.source = SourceVersionId::new("other").unwrap(),
            6 => snapshot.measured_lead_us = Some(0),
            _ => snapshot.latest_observation = None,
        }
        assert!(
            source_scope(&snapshot, &player, &native().project, native().revision).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn original_prepare_is_reserved_before_dispatch_and_stop_bypasses_full_lanes() {
    let (mut hsp, mutations, _reads) = owner();
    let snapshot = source();
    let permit = Permit::for_analysis_test(snapshot.binding.player.clone());
    state(&hsp.shared).inspected = Some(inspected());
    let first = hsp
        .handle(
            "/player/hsp-prepare",
            &permit,
            None,
            br#"{"allow_synchronization":true}"#,
        )
        .unwrap();
    let original = state(&hsp.shared).prepare.clone().unwrap();
    assert_eq!(first["preparation"]["request_id"], original.as_str());
    assert!(hsp
        .handle(
            "/player/hsp-prepare",
            &permit,
            None,
            br#"{"allow_synchronization":true}"#
        )
        .is_err());
    // Keep mutation queue occupied and retire the live capability. Cleanup still
    // latches immediately and retains its own cancellation identity.
    permit.cancelled.store(true, Ordering::Release);
    hsp.handle("/player/hsp-stop", &permit, None, b"").unwrap();
    let cancel = state(&hsp.shared).cancel.clone().unwrap();
    hsp.handle("/player/hsp-cancel", &permit, None, b"")
        .unwrap();
    let s = state(&hsp.shared);
    assert!(s.stop_latched);
    assert_eq!(s.prepare.as_ref(), Some(&original));
    assert_eq!(s.cancel.as_ref(), Some(&cancel));
    assert_ne!(cancel, original);
    drop(s);
    assert!(mutations.try_recv().is_ok());
}

#[test]
fn public_cleanup_trigger_keeps_original_action_and_request_ids() {
    for (route, expected) in [
        ("/player/hsp-stop", "public_stop"),
        ("/player/hsp-cancel", "public_cancel"),
    ] {
        let (mut hsp, _mutations, _reads) = owner();
        let player = source().binding.player;
        let permit = Permit::for_analysis_test(player.clone());
        {
            let mut s = state(&hsp.shared);
            s.player = Some(player.clone());
            s.stream = Some(RequestId::new("original-stream").unwrap());
            s.prepare = Some(RequestId::new("original-preparation").unwrap());
        }
        let first = hsp.handle(route, &permit, None, b"").unwrap();
        assert_eq!(first["cleanup"]["trigger"], expected);
        assert!(first["cleanup"]["stop_request_id"].is_string());
        assert!(first["cleanup"]["cancel_request_id"].is_string());
        hsp.retire(&player);
        hsp.handle("/player/hsp-stop", &permit, None, b"").unwrap();
        hsp.handle("/player/hsp-cancel", &permit, None, b"")
            .unwrap();
        assert_eq!(hsp.view()["cleanup"], first["cleanup"]);
    }
}

#[test]
fn preview_retirement_cannot_be_relabelled_by_later_public_stop() {
    let (mut hsp, _mutations, _reads) = owner();
    let player = source().binding.player;
    let permit = Permit::for_analysis_test(player.clone());
    {
        let mut s = state(&hsp.shared);
        s.player = Some(player.clone());
        s.stream = Some(RequestId::new("original-stream").unwrap());
    }
    hsp.retire(&player);
    let original = hsp.view()["cleanup"].clone();
    assert_eq!(original["trigger"], "player_retired");
    let result = hsp.handle("/player/hsp-stop", &permit, None, b"").unwrap();
    assert_eq!(result["cleanup"], original);
}

#[test]
fn cleanup_trigger_is_archived_and_does_not_authorize_a_new_lifecycle() {
    let mut s = joined_lifecycle(0);
    s.cleanup_trigger = Some(CleanupTrigger::PublicStop);
    let cancel = s.cancel.clone();
    archive_and_reset(&mut s).unwrap();
    let result = view(&config(), &s);
    assert!(result["cleanup"]["trigger"].is_null());
    assert!(result["cleanup"]["stop_request_id"].is_null());
    assert_eq!(result["can_play"], false);
    assert_eq!(
        result["previous_lifecycles"][0]["cleanup_trigger"],
        "public_stop"
    );
    assert_eq!(
        result["previous_lifecycles"][0]["cancel_request_id"],
        json!(cancel)
    );
}

#[test]
fn late_success_after_stop_cannot_restore_readiness_or_replace_originals() {
    let n = native();
    let prepare = RequestId::new("prepare").unwrap();
    let mut s = State {
        prepare: Some(prepare.clone()),
        sequence: 1,
        mutation_active: true,
        pending: Some("prepare"),
        ..State::default()
    };
    let job = Job {
        action: Action::Prepare,
        sequence: 1,
        source: None,
        deadline: Instant::now() + RPC,
    };
    latch_stop(&mut s, CleanupTrigger::PlayerRetired);
    let cancel = s.cancel.clone();
    finish(
        &n,
        &mut s,
        &job,
        Ok(Reports {
            inspected: Some(inspected()),
            ..Reports::default()
        }),
        true,
    );
    assert!(!s.mutation_active && s.stop_latched);
    assert!(s.inspected.is_none());
    assert_eq!(s.prepare, Some(prepare));
    assert_eq!(s.cancel, cancel);
    assert_eq!(view(&config(), &s)["can_prepare"], false);
}

#[test]
fn failed_operation_retains_originals_and_status_never_reserves_replacements() {
    let (mut hsp, _mutations, reads) = owner();
    let permit = Permit::for_analysis_test(source().binding.player);
    let prepare = RequestId::new("original-prepare").unwrap();
    {
        let mut s = state(&hsp.shared);
        s.prepare = Some(prepare.clone());
        s.player = permit.identity.clone();
        s.mutation_active = true;
    }
    let job = Job {
        action: Action::Prepare,
        sequence: 0,
        source: None,
        deadline: Instant::now() + RPC,
    };
    finish(
        &hsp.native,
        &mut state(&hsp.shared),
        &job,
        Err(anyhow::anyhow!("secret-bearing-native-error")),
        true,
    );
    hsp.handle("/player/hsp-status", &permit, None, b"")
        .unwrap();
    assert!(reads.try_recv().is_ok());
    let s = state(&hsp.shared);
    assert_eq!(s.prepare, Some(prepare));
    assert!(s.stream.is_none());
    assert!(s.stop_latched);
    let encoded = view(&config(), &s).to_string();
    assert!(!encoded.contains("secret-bearing"));
    assert!(encoded.contains("original_operation_unconfirmed"));
}

#[test]
fn altered_original_selection_is_refused_and_history_does_not_enable_play() {
    let n = native();
    let prepare = RequestId::new("prepare").unwrap();
    let stream = RequestId::new("stream").unwrap();
    let selection = bind_selection(&config(), prepare.clone(), &source()).unwrap();
    let mut s = State {
        prepare: Some(prepare),
        stream: Some(stream.clone()),
        selection: Some(selection.clone()),
        ..State::default()
    };
    let mut changed = pending_report(selection.clone(), stream.clone());
    changed.selection.browser_timing = ArtifactId::new("different").unwrap();
    assert!(retain(
        &n,
        &mut s,
        Reports {
            live: Some(changed),
            ..Reports::default()
        }
    )
    .is_err());
    retain(
        &n,
        &mut s,
        Reports {
            live: Some(pending_report(selection, stream)),
            ..Reports::default()
        },
    )
    .unwrap();
    assert!(!stream_playable(&s));
    assert!(!cleanup_complete(&s));
    let view = view(&config(), &s);
    assert_eq!(view["can_play"], false);
    assert_eq!(view["stream"]["phase"], "historical_only");
}

#[test]
fn public_view_never_serializes_native_ticks_errors_or_command_envelopes() {
    let prepare = RequestId::new("prepare").unwrap();
    let s = State {
        prepare: Some(prepare.clone()),
        preparation: Some(HandyHspPreparationStatus {
            project_id: native().project,
            preparation_request_id: prepare,
            history: None,
            native: Some(HandyHspPreparationNativeStatus {
                phase: HandyHspPreparationPhase::Synchronizing,
                worker_active: true,
                cancel_requested: false,
                metadata_pending: true,
                clock_valid_until_boottime_ns: Some(u64::MAX),
                error: Some(ProtocolError::new(
                    ErrorCode::UnknownOutcome,
                    "private-native-error",
                )),
            }),
            armed: false,
            playback_enabled: false,
        }),
        ..State::default()
    };
    let encoded = view(&config(), &s).to_string();
    for forbidden in [
        "18446744073709551615",
        "private-native-error",
        "owner_proof",
        "command",
        "boottime",
        "handy-owner.token",
    ] {
        assert!(!encoded.contains(forbidden), "{forbidden}");
    }
    assert!(encoded.contains("native_refusal_or_uncertainty"));
    assert_eq!(bounded(u64::MAX), (1u64 << 53) - 1);
}

#[test]
fn observation_pending_does_not_disable_an_inspected_action_and_sequence_never_wraps() {
    let mut s = State {
        inspected: Some(inspected()),
        read_pending: true,
        sequence: u64::MAX,
        ..State::default()
    };
    assert_eq!(view(&config(), &s)["can_prepare"], true);
    latch_stop(&mut s, CleanupTrigger::PlayerRetired);
    assert_eq!(s.sequence, u64::MAX);
    assert!(s.stop_latched);
    assert_eq!(s.error, Some("owner_sequence_exhausted"));
    assert_eq!(view(&config(), &s)["can_prepare"], false);
}

#[test]
fn joined_native_cancellation_without_journal_history_can_finish() {
    let prepare = RequestId::new("original").unwrap();
    let mut s = State {
        prepare: Some(prepare.clone()),
        preparation: Some(HandyHspPreparationStatus {
            project_id: native().project,
            preparation_request_id: prepare,
            history: None,
            native: Some(HandyHspPreparationNativeStatus {
                phase: HandyHspPreparationPhase::Retired,
                worker_active: false,
                cancel_requested: true,
                metadata_pending: false,
                clock_valid_until_boottime_ns: None,
                error: None,
            }),
            armed: false,
            playback_enabled: false,
        }),
        ..State::default()
    };
    assert!(cleanup_complete(&s));
    s.preparation
        .as_mut()
        .unwrap()
        .native
        .as_mut()
        .unwrap()
        .metadata_pending = true;
    assert!(!cleanup_complete(&s));
    s.preparation.as_mut().unwrap().native = None;
    assert!(!cleanup_complete(&s));
}

fn joined_lifecycle(index: usize) -> State {
    let prepare = RequestId::new(format!("prepare-{index}")).unwrap();
    State {
        player: Some(source().binding.player),
        prepare: Some(prepare.clone()),
        cancel: Some(RequestId::new(format!("cancel-{index}")).unwrap()),
        sequence: 10,
        preparation: Some(HandyHspPreparationStatus {
            project_id: native().project,
            preparation_request_id: prepare,
            history: None,
            native: Some(HandyHspPreparationNativeStatus {
                phase: HandyHspPreparationPhase::Retired,
                worker_active: false,
                cancel_requested: true,
                metadata_pending: false,
                clock_valid_until_boottime_ns: None,
                error: None,
            }),
            armed: false,
            playback_enabled: false,
        }),
        stop_latched: true,
        cleanup_confirmed: true,
        ..State::default()
    }
}

#[test]
fn completed_cleanup_repeated_retirement_keeps_original_status_settled() {
    for exhausted in [false, true] {
        let mut s = joined_lifecycle(0);
        s.stream = Some(RequestId::new("original-stream").unwrap());
        s.stop = Some(RequestId::new("original-stop").unwrap());
        s.bind_unsent = true;
        if exhausted {
            s.cleanup_confirmed = false;
            s.cleanup_attempts = CLEANUP_ATTEMPTS;
            s.error = Some("original_cleanup_unconfirmed");
        }
        let ids = (
            s.prepare.clone(),
            s.stream.clone(),
            s.cancel.clone(),
            s.stop.clone(),
        );
        let sequence = s.sequence;
        for _ in 0..3 {
            latch_stop(&mut s, CleanupTrigger::PlayerRetired);
            let result = view(&config(), &s);
            assert!(result["pending_action"].is_null());
            assert_eq!(result["can_play"], false);
            assert_eq!(s.sequence, sequence);
            assert_eq!(
                (
                    s.prepare.clone(),
                    s.stream.clone(),
                    s.cancel.clone(),
                    s.stop.clone()
                ),
                ids
            );
            if exhausted {
                assert_eq!(result["error"], "original_cleanup_unconfirmed");
            } else {
                assert_eq!(result["phase"], "cleanup_observed");
            }
        }
    }
}

fn stopped_stream_with_retired_clock() -> State {
    let mut s = joined_lifecycle(0);
    let mut live = prefilling_state().live.unwrap();
    live.selection.preparation_request_id = s.prepare.clone().unwrap();
    live.native.as_mut().unwrap().retired = true;
    let observation = live.stream.as_mut().unwrap().native.as_mut().unwrap();
    observation.runtime_state = HandyHspRuntimeState::StopAcknowledged;
    observation.retired = true;
    s.stream = Some(live.stream_request_id.clone());
    s.live = Some(live);
    s.enable = Some(RequestId::new("original-enable").unwrap());
    s.continuity = Some(serde_json::from_value(json!({
        "project_id":native().project,"stream_request_id":s.stream,
        "enable_request_id":s.enable,"duration_ms":10000,"history":null,
        "native":{"observed_at_boottime_ns":10,"generation":1,
            "clock_valid_until_boottime_ns":20,"current":false,"retired":true,
            "worker_active":false,"worker_joined":true,
            "error":{"code":"cancelled","message":"original HSP preparation retired","retryable":false}},
        "error":null,"qualified":false
    })).unwrap());
    assert!(cleanup_complete(&s));
    s
}

#[test]
fn completed_cleanup_expected_clock_retirement_is_not_uncertain() {
    let s = stopped_stream_with_retired_clock();
    let result = view(&config(), &s);
    assert_eq!(result["phase"], "cleanup_observed");
    assert_eq!(result["stream"]["phase"], "stop_acknowledged");
    assert_eq!(result["continuity"]["phase"], "retired");
    assert!(result["error"].is_null());
    assert_eq!(result["can_play"], false);
}

#[test]
fn completed_cleanup_expected_preparation_cancel_is_not_uncertain() {
    for case in 0..4 {
        let mut s = joined_lifecycle(0);
        let observed = s.preparation.as_mut().unwrap().native.as_mut().unwrap();
        observed.error = Some(ProtocolError::new(
            ErrorCode::Cancelled,
            "original HSP preparation retired",
        ));
        match case {
            0 => observed.worker_active = true,
            1 => observed.metadata_pending = true,
            2 => observed.error.as_mut().unwrap().code = ErrorCode::Unavailable,
            _ => {}
        }
        let result = view(&config(), &s);
        if case == 3 {
            assert!(result["error"].is_null());
            assert_eq!(result["phase"], "cleanup_observed");
        } else {
            assert_eq!(result["error"], "native_refusal_or_uncertainty");
        }
        assert_eq!(result["can_bind"], false);
    }
}

#[test]
fn completed_cleanup_preserves_unjoined_and_genuine_native_errors() {
    for case in 0..5 {
        let mut s = stopped_stream_with_retired_clock();
        match case {
            0 => s.cleanup_confirmed = false,
            1 => {
                s.continuity
                    .as_mut()
                    .unwrap()
                    .native
                    .as_mut()
                    .unwrap()
                    .worker_joined = false
            }
            2 => {
                s.live
                    .as_mut()
                    .unwrap()
                    .stream
                    .as_mut()
                    .unwrap()
                    .native
                    .as_mut()
                    .unwrap()
                    .metadata_pending = true
            }
            3 => {
                s.live
                    .as_mut()
                    .unwrap()
                    .stream
                    .as_mut()
                    .unwrap()
                    .native
                    .as_mut()
                    .unwrap()
                    .runtime_state = HandyHspRuntimeState::StopUnknown
            }
            _ => {
                s.continuity
                    .as_mut()
                    .unwrap()
                    .native
                    .as_mut()
                    .unwrap()
                    .error
                    .as_mut()
                    .unwrap()
                    .code = ErrorCode::Unavailable
            }
        }
        assert_eq!(
            view(&config(), &s)["error"],
            "native_refusal_or_uncertainty",
            "case {case}"
        );
    }
}

#[test]
fn completed_cleanup_explicit_stop_rechecks_original_identity() {
    let (mut hsp, _mutations, _reads) = owner();
    let mut s = stopped_stream_with_retired_clock();
    s.stop = Some(RequestId::new("original-stop").unwrap());
    let original = s.stop.clone();
    *state(&hsp.shared) = s;
    let permit = Permit::for_analysis_test(source().binding.player);
    let result = hsp.handle("/player/hsp-stop", &permit, None, b"").unwrap();
    assert_eq!(result["phase"], "stopping");
    assert_eq!(result["pending_action"], "stop");
    assert_eq!(state(&hsp.shared).stop, original);
    assert!(!state(&hsp.shared).cleanup_confirmed);
}

#[test]
fn explicit_inspect_reuses_joined_owner_but_never_mints_a_mutation_identity() {
    for new_player in [false, true] {
        let (mut hsp, mutations, reads) = owner();
        *state(&hsp.shared) = joined_lifecycle(0);
        let original = state(&hsp.shared).prepare.clone().unwrap();
        let mut snapshot = source();
        if new_player {
            snapshot.binding.player.player_id = AttemptId::new("new-player").unwrap();
            snapshot.browser_timing.player = snapshot.binding.player.clone();
        }
        let permit = Permit::for_analysis_test(snapshot.binding.player.clone());
        let reply = hsp
            .handle("/player/hsp-inspect", &permit, Some(snapshot.clone()), b"")
            .unwrap();
        assert_eq!(
            reply["previous_lifecycles"][0]["prepare_request_id"],
            original.as_str()
        );
        assert_eq!(
            reply["previous_lifecycles"][0]["phase"],
            "cleanup_confirmed"
        );
        assert!(mutations.try_recv().is_err());
        let job = reads.try_recv().unwrap();
        assert!(matches!(job.action, Action::Inspect));
        {
            let s = state(&hsp.shared);
            assert!(
                s.prepare.is_none() && s.stream.is_none() && s.play.is_none() && s.enable.is_none()
            );
            assert!(s.cancel.is_none() && s.stop.is_none() && !s.stop_latched && s.read_pending);
        }
        let selected = Inspected {
            source: snapshot,
            device: json!({"state":"pending"}),
            dossier: json!({"state":"pending"}),
        };
        finish(
            &hsp.native,
            &mut state(&hsp.shared),
            &job,
            Ok(Reports {
                inspected: Some(selected),
                ..Reports::default()
            }),
            false,
        );
        assert!(state(&hsp.shared).prepare.is_none());
        hsp.handle(
            "/player/hsp-prepare",
            &permit,
            None,
            br#"{"allow_synchronization":true}"#,
        )
        .unwrap();
        assert!(matches!(
            mutations.try_recv().unwrap().action,
            Action::Prepare
        ));
        let s = state(&hsp.shared);
        assert_ne!(s.prepare.as_ref(), Some(&original));
        assert_eq!(s.previous_lifecycles.len(), 1);
    }
}

#[test]
fn uncertain_busy_or_failed_lifecycles_cannot_be_archived_or_reset() {
    for case in 0..6 {
        let mut s = joined_lifecycle(0);
        let original = s.prepare.clone();
        match case {
            0 => s.cleanup_confirmed = false,
            1 => s.mutation_active = true,
            2 => s.read_pending = true,
            3 => s.worker_failed = true,
            4 => s.preparation.as_mut().unwrap().native = None,
            _ => {
                s.preparation
                    .as_mut()
                    .unwrap()
                    .native
                    .as_mut()
                    .unwrap()
                    .metadata_pending = true
            }
        }
        assert!(archive_and_reset(&mut s).is_err(), "case {case}");
        assert_eq!(s.prepare, original);
        assert!(s.previous_lifecycles.is_empty());
        assert!(s.stop_latched);
    }
}

#[test]
fn archive_bound_preserves_every_original_and_refuses_ninth_archive() {
    let mut s = joined_lifecycle(0);
    for index in 0..LIFECYCLE_HISTORY_LIMIT {
        archive_and_reset(&mut s).unwrap();
        assert_eq!(s.previous_lifecycles.len(), index + 1);
        let sequence = s.sequence;
        let archive = std::mem::take(&mut s.previous_lifecycles);
        s = joined_lifecycle(index + 1);
        s.sequence = sequence;
        s.previous_lifecycles = archive;
    }
    let retained = s.prepare.clone();
    let sequence = s.sequence;
    assert!(archive_and_reset(&mut s).is_err());
    assert_eq!(s.previous_lifecycles.len(), 8);
    assert_eq!(s.prepare, retained);
    assert_eq!(s.sequence, sequence);
    let projection = view(&config(), &s);
    assert_eq!(
        projection["previous_lifecycles"].as_array().unwrap().len(),
        8
    );
    for (index, item) in projection["previous_lifecycles"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(item["prepare_request_id"], format!("prepare-{index}"));
        assert!(item["cleanup_trigger"].is_null());
        assert_eq!(item.as_object().unwrap().len(), 9);
    }
}

#[test]
fn old_lifecycle_completions_cannot_clear_fresh_pending_or_replace_inspection() {
    let mut s = joined_lifecycle(0);
    let old_sequence = s.sequence;
    archive_and_reset(&mut s).unwrap();
    s.sequence += 1;
    s.read_pending = true;
    s.pending = Some("inspect");
    let old = Job {
        action: Action::Inspect,
        sequence: old_sequence,
        source: None,
        deadline: Instant::now() + RPC,
    };
    finish(
        &native(),
        &mut s,
        &old,
        Ok(Reports {
            inspected: Some(inspected()),
            ..Reports::default()
        }),
        false,
    );
    assert!(s.read_pending && s.inspected.is_none());
    assert_eq!(s.pending, Some("inspect"));
    assert_eq!(s.previous_lifecycles.len(), 1);
}

#[test]
fn status_does_not_reset_a_joined_lifecycle() {
    let (mut hsp, mutations, reads) = owner();
    *state(&hsp.shared) = joined_lifecycle(0);
    let original = state(&hsp.shared).prepare.clone();
    let permit = Permit::for_analysis_test(source().binding.player);
    hsp.handle("/player/hsp-status", &permit, None, b"")
        .unwrap();
    assert!(matches!(reads.try_recv().unwrap().action, Action::Status));
    assert!(mutations.try_recv().is_err());
    let s = state(&hsp.shared);
    assert_eq!(s.prepare, original);
    assert!(s.previous_lifecycles.is_empty() && s.stop_latched);
}

#[test]
fn queued_prepare_skipped_by_worker_needs_no_engine_cleanup_and_allows_explicit_inspect() {
    let (mut hsp, mutations, reads) = owner();
    let snapshot = source();
    let permit = Permit::for_analysis_test(snapshot.binding.player.clone());
    state(&hsp.shared).inspected = Some(inspected());
    hsp.handle(
        "/player/hsp-prepare",
        &permit,
        None,
        br#"{"allow_synchronization":true}"#,
    )
    .unwrap();
    let job = mutations.try_recv().unwrap();
    let original = state(&hsp.shared).prepare.clone().unwrap();
    hsp.handle("/player/hsp-stop", &permit, None, b"").unwrap();
    {
        let mut s = state(&hsp.shared);
        // The cleanup lane does not race a still-queued mutation with a query
        // against a nonexistent Engine original.
        let before = Originals::for_observation(&s);
        assert!(before.prepare.is_none() && before.stream.is_none());
        assert!(!cleanup_complete(&s));
        skipped_before_execute(&mut s, &job);
        assert!(s.prepare_unsent && !s.prepare_dispatched && !s.mutation_active);
        let cleanup = Originals::for_observation(&s);
        assert!(cleanup.prepare.is_none() && cleanup.stream.is_none() && cleanup.enable.is_none());
        assert!(cleanup_complete(&s));
        // Same decision the cleanup lane makes after its empty native work set.
        s.cleanup_confirmed = cleanup_complete(&s);
        assert!(s.preparation.is_none());
        assert_eq!(
            view(&config(), &s)["preparation"]["phase"],
            "not_dispatched"
        );
    }
    hsp.handle("/player/hsp-inspect", &permit, Some(snapshot), b"")
        .unwrap();
    assert!(matches!(reads.try_recv().unwrap().action, Action::Inspect));
    assert!(mutations.try_recv().is_err());
    let s = state(&hsp.shared);
    assert!(s.prepare.is_none() && !s.prepare_unsent);
    assert_eq!(
        s.previous_lifecycles[0].prepare_request_id.as_ref(),
        Some(&original)
    );
}

#[test]
fn dispatched_failure_remains_unknown_and_never_becomes_known_unsent() {
    let prepare = RequestId::new("dispatched-prepare").unwrap();
    let mut s = State {
        prepare: Some(prepare.clone()),
        prepare_dispatched: true,
        mutation_active: true,
        ..State::default()
    };
    let job = Job {
        action: Action::Prepare,
        sequence: 0,
        source: None,
        deadline: Instant::now() + RPC,
    };
    finish(
        &native(),
        &mut s,
        &job,
        Err(anyhow::anyhow!("transport lost after dispatch")),
        true,
    );
    assert!(!s.prepare_unsent && s.stop_latched);
    assert_eq!(
        Originals::for_observation(&s).prepare.as_ref(),
        Some(&prepare)
    );
    assert!(!cleanup_complete(&s));
    assert!(archive_and_reset(&mut s).is_err());
    assert_eq!(s.prepare, Some(prepare));
}

#[test]
fn skipped_bind_cancels_real_preparation_and_skipped_play_still_stops_real_stream() {
    for action in [Action::Bind, Action::Play, Action::Enable] {
        let prepare = RequestId::new("real-prepare").unwrap();
        let stream = RequestId::new("stream").unwrap();
        let mut s = State {
            prepare: Some(prepare.clone()),
            prepare_dispatched: true,
            stream: Some(stream.clone()),
            bind_dispatched: action != Action::Bind,
            enable: (action == Action::Enable).then(|| RequestId::new("enable").unwrap()),
            mutation_active: true,
            ..State::default()
        };
        skipped_before_execute(
            &mut s,
            &Job {
                action,
                sequence: 0,
                source: None,
                deadline: Instant::now() + RPC,
            },
        );
        let cleanup = Originals::for_observation(&s);
        assert_eq!(cleanup.prepare.as_ref(), Some(&prepare));
        assert_eq!(cleanup.stream.is_some(), action != Action::Bind);
        assert!(cleanup.enable.is_none());
        assert_eq!(s.bind_unsent, action == Action::Bind);
        assert_eq!(s.enable_unsent, action == Action::Enable);
        assert!(!cleanup_complete(&s));
    }
}

#[test]
fn known_unsent_is_insufficient_while_worker_is_active_or_failed() {
    let mut s = State {
        prepare: Some(RequestId::new("prepare").unwrap()),
        prepare_unsent: true,
        mutation_active: true,
        ..State::default()
    };
    assert!(!cleanup_complete(&s));
    s.mutation_active = false;
    assert!(cleanup_complete(&s));
    s.worker_failed = true;
    assert!(!cleanup_complete(&s));
}

#[test]
fn conservative_input_end_does_not_select_the_dossiers_optional_finite_drain() {
    let documented = Some("a".repeat(64));
    assert!(finite_selection_matches(&None, &None));
    assert!(finite_selection_matches(&None, &documented));
    assert!(finite_selection_matches(&documented, &documented));
    assert!(!finite_selection_matches(&documented, &None));
    assert!(!finite_selection_matches(
        &Some("b".repeat(64)),
        &documented
    ));
}

#[test]
fn inspection_after_status_installs_the_actual_live_player_cancellation_flag() {
    let (mut hsp, _mutations, reads) = owner();
    let snapshot = source();
    let retained = Permit::for_analysis_test(snapshot.binding.player.clone());
    let live = Permit::for_analysis_test(snapshot.binding.player.clone());
    hsp.handle("/player/hsp-status", &retained, None, b"")
        .unwrap();
    let query = reads.try_recv().unwrap();
    finish(
        &hsp.native,
        &mut state(&hsp.shared),
        &query,
        Ok(Reports::default()),
        false,
    );
    hsp.handle("/player/hsp-inspect", &live, Some(snapshot), b"")
        .unwrap();
    let shared = state(&hsp.shared);
    assert!(Arc::ptr_eq(
        shared.cancelled.as_ref().unwrap(),
        &live.cancelled
    ));
    live.cancelled.store(true, Ordering::Release);
    assert!(shared.cancelled.as_ref().unwrap().load(Ordering::Acquire));
    assert!(!retained.cancelled.load(Ordering::Acquire));
}

// Deliberately partial owner-state fixture: native DTO counters are validated;
// no source/device qualification witness or execution authority is supplied.
fn prefilling_state() -> State {
    let prepare = RequestId::new("original-prepare").unwrap();
    let stream = RequestId::new("original-stream").unwrap();
    let selection = bind_selection(&config(), prepare.clone(), &source()).unwrap();
    let mut live = pending_report(selection.clone(), stream.clone());
    live.native = Some(HandyHspLiveNativeStatus {
        retired: false,
        original_play_request_id: None,
        original_stop_request_id: None,
        delivered_samples: 3,
        active_source: None,
        error: None,
    });
    let observation = HandyHspNativeStatus {
        runtime_state: HandyHspRuntimeState::Prefilling,
        native_active: false,
        current_effect: None,
        accepted_samples: 3,
        pending_points: 1,
        retained_points: 2,
        buffered_points: 3,
        total_added: 2,
        last_joined: None,
        metadata_pending: false,
        stop_pending: false,
        stop_blocked: false,
        emergency_observed: false,
        post_setup_bound: true,
        retired: false,
        finite_drain: None,
    };
    observation.validate().unwrap();
    live.stream = Some(HandyHspStatus {
        project_id: native().project,
        stream_request_id: stream.clone(),
        stream_record_id: ArtifactId::new("f".repeat(64)).unwrap(),
        generation: 1,
        stream_id: 1,
        live: source().capture,
        native: Some(observation),
        qualified: false,
        emergency_history_durable: false,
    });
    State {
        player: Some(source().binding.player),
        inspected: Some(inspected()),
        prepare: Some(prepare),
        stream: Some(stream),
        selection: Some(selection),
        live: Some(live),
        ..State::default()
    }
}

#[test]
fn prefill_append_activity_accepts_one_original_intent_without_waiting_for_idle() {
    for (native_active, metadata_pending) in [(true, false), (false, true), (true, true)] {
        for (route, body, action) in [
            (
                "/player/hsp-play",
                br#"{"allow_play":true}"#.as_slice(),
                Action::Play,
            ),
            (
                "/player/hsp-continuity-enable",
                br#"{"allow_continuity":true}"#.as_slice(),
                Action::Enable,
            ),
        ] {
            let (mut hsp, jobs, _reads) = owner();
            let permit = Permit::for_analysis_test(source().binding.player);
            {
                let mut s = state(&hsp.shared);
                *s = prefilling_state();
                let n = s
                    .live
                    .as_mut()
                    .unwrap()
                    .stream
                    .as_mut()
                    .unwrap()
                    .native
                    .as_mut()
                    .unwrap();
                n.native_active = native_active;
                n.metadata_pending = metadata_pending;
                n.current_effect = Some(HandyHspEffectStatus {
                    sequence: 3,
                    kind: HandyHspEffectKind::Append,
                });
                n.validate().unwrap();
            }
            assert_eq!(hsp.view()["can_play"], true);
            assert_eq!(hsp.view()["can_enable_continuity"], true);
            hsp.handle(route, &permit, None, body).unwrap();
            assert!(jobs.try_recv().unwrap().action == action);
            let original = {
                let s = state(&hsp.shared);
                assert_eq!(s.stream.as_ref().unwrap().as_str(), "original-stream");
                if action == Action::Play {
                    s.play.clone()
                } else {
                    s.enable.clone()
                }
            };
            assert!(original.is_some());
            assert!(hsp.handle(route, &permit, None, body).is_err());
            let s = state(&hsp.shared);
            assert_eq!(
                if action == Action::Play {
                    &s.play
                } else {
                    &s.enable
                },
                &original
            );
        }
    }
}

#[test]
fn prefill_readiness_still_refuses_incomplete_retired_or_stopping_streams() {
    for case in 0..5 {
        let (mut hsp, jobs, _reads) = owner();
        let permit = Permit::for_analysis_test(source().binding.player);
        {
            let mut s = state(&hsp.shared);
            *s = prefilling_state();
            let n = s
                .live
                .as_mut()
                .unwrap()
                .stream
                .as_mut()
                .unwrap()
                .native
                .as_mut()
                .unwrap();
            match case {
                0 => n.post_setup_bound = false,
                1 => n.total_added = 1,
                2 => n.retired = true,
                3 => n.stop_pending = true,
                4 => n.runtime_state = HandyHspRuntimeState::StopRequired,
                _ => unreachable!(),
            }
        }
        assert_eq!(hsp.view()["can_play"], false);
        assert_eq!(hsp.view()["can_enable_continuity"], false);
        assert!(hsp
            .handle("/player/hsp-play", &permit, None, br#"{"allow_play":true}"#)
            .is_err());
        assert!(hsp
            .handle(
                "/player/hsp-continuity-enable",
                &permit,
                None,
                br#"{"allow_continuity":true}"#
            )
            .is_err());
        assert!(jobs.try_recv().is_err());
        let s = state(&hsp.shared);
        assert!(s.play.is_none() && s.enable.is_none());
    }
}
