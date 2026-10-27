// Include inside continuity_native_tests to reuse the actual CPU/native fixture.
// The obsolete timing copy mutex's failing runs remain in the evidence packet.
// This successor holds the real timing publication/claim registry guard; no
// Busy outcome, source stamp, phase, clock or authority is injected.
mod timing_copy_native_tests {
    use super::*;

    #[test]
    #[ignore = "requires pinned actual CPU/runtime/neutral fixture and delegated confinement; run serially"]
    fn native_cpu_timing_copy_contention_preserves_original_playing_stream() {
        let fixture_path = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        let output = env_path("PULSAR_HSP_TIMING_COPY_NATIVE_OUTPUT").join("timing-copy");
        fs::create_dir(&output).expect("fresh case directory");
        let input: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();
        let media_path = input_path(&input, "media");
        let pixels = fs::read(input_path(&input, "rgb")).unwrap();
        let model = input_path(&input, "model");
        assert_eq!(artifact(&model).sha256, MODEL_SHA);
        let runtime_before = artifact(&runtime);
        assert_eq!(runtime_before.sha256, ORT_SHA);
        save(
            &output,
            "subjects.json",
            &json!({"fixture":artifact(&fixture_path),"worker":artifact(&worker),
            "runtime":runtime_before,"case":"timing-copy","actual_confined_cpu":true,"browser_capture":false,
            "physical_device":false,"qualified":false}),
        );
        let f = native_fixture(worker);
        let media = prepare_media(&f, media_path);
        let runtime_identity = f
            .engine
            .tools
            .as_ref()
            .unwrap()
            .onnx_runtime
            .as_ref()
            .unwrap()
            .identity
            .clone();
        let profile = profile(&model, runtime_identity);
        let (x, peer, base_io, device_permit) = finite_device_fixture(f);
        let io = Arc::new(ContinuityIo {
            base: base_io,
            reads: AtomicU64::new(0),
            initial_server_received_ns: AtomicU64::new(0),
            hold: AtomicBool::new(false),
            entered: AtomicBool::new(false),
            release: AtomicBool::new(false),
        });
        let mut shutdown = ShutdownGuard(Some(x.f.engine.clone()));
        let (source_qualification, pending_source) =
            finite_source_dossier(&x, &device_permit, &media, &profile);
        save(&output, "device-dossier-pending.json", &x.accepted);
        save(&output, "source-dossier-pending.json", &pending_source);
        let finite_hash = x
            .accepted
            .manifest
            .subject
            .hsp_finite_drain_policy_sha256()
            .unwrap()
            .unwrap();
        let mut visible = AuthoredVisiblePlayer::open(&x.f, &media);
        visible.observe(&x.f);
        let mut options = capture_options();
        options.browser_media = Some(media.clone());
        options.browser_analysis = Some(BrowserAnalysisCaptureDeclaration {
            feed_id: ArtifactId::new("continuity-native-authored-analysis").unwrap(),
            player: visible.binding.player.clone(),
            timing_binding_id: visible.binding.binding_id.clone(),
            policy: finite_lookahead(),
        });
        options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        options.source_anchor = SourceTimestamp::new(0, 1).unwrap();
        let exact = media
            .clock
            .project_at(options.source_anchor, &media.selection.placement.spec)
            .unwrap();
        options.project_anchor = exact.quantize_nanoseconds().unwrap().time;
        options.exact_project_anchor = Some(exact);
        options.policy.maximum_analysis_age_ns = AGE_NS;
        options.lease_ms = 30_000;
        options.inference = Some(LiveCaptureInference {
            model_path: model,
            profile,
        });
        let capture = capture_report(
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(1),
                    Command::StartLiveCapture {
                        engine_epoch: media.engine_epoch.clone(),
                        options,
                    },
                ))
                .result
                .unwrap(),
        );
        wait_capture(&x.f, &mut visible, &capture.identity.epoch, None);
        let mut preparation = x.prepare("timing-copy-actual-cpu-preparation");
        preparation.expected_revision = Some(RevisionId::new(1));
        let Command::PrepareHandyHsp { qualification, .. } = &mut preparation.command else {
            unreachable!()
        };
        *qualification = device_permit;
        x.call(&preparation, io.clone()).unwrap();
        let until = Instant::now() + WAIT;
        loop {
            visible.observe(&x.f);
            let r = x.status(&preparation.request_id);
            if r.native.as_ref().is_some_and(|n| {
                n.phase == HandyHspPreparationPhase::Ready
                    && !n.worker_active
                    && !n.metadata_pending
            }) {
                break;
            }
            assert!(Instant::now() < until, "preparation: {r:?}");
            std::thread::park_timeout(Duration::from_millis(2));
        }
        let start = usize::try_from(visible.origin.elapsed().as_micros() / 100_000).unwrap() + 10;
        let mut index = start;
        upload_original(&x.f, &capture, &input, &pixels, index, 0);
        index += 2;
        upload_original(&x.f, &capture, &input, &pixels, index, 1);
        let first = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(1));
        let mut stamps = vec![actual_stamp(
            &x.f,
            &first,
            &input,
            &pixels,
            index - 2,
            index,
        )];
        let bind = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::BindHandyHspLive {
                selection: HandyHspLiveSelection {
                    preparation_request_id: preparation.request_id.clone(),
                    capture_epoch: capture.identity.epoch.clone(),
                    inference_attempt_id: first.inference.as_ref().unwrap().attempt_id.clone(),
                    browser_timing: visible.binding.binding_id.clone(),
                    source_qualification,
                    finite_drain_policy_sha256: Some(finite_hash),
                },
                acknowledge_current_environment: true,
                allow_online_control: true,
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        let bound = wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            let n = native_status(r);
            n.post_setup_bound && !n.native_active && !n.metadata_pending
        });
        let original_origin = bound.origin.clone();
        index += 2;
        let mut frame = 2;
        let mut terminal = upload_original(&x.f, &capture, &input, &pixels, index, frame);
        let second = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(frame));
        stamps.push(actual_stamp(
            &x.f,
            &second,
            &input,
            &pixels,
            index - 2,
            index,
        ));
        wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            let n = native_status(r);
            n.total_added == 2 && !n.native_active && !n.metadata_pending
        });
        let play = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::PlayHandyHspLive {
                stream_request_id: bind.request_id.clone(),
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        live_report(x.f.engine.handle(play.clone()).result.unwrap());
        wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            let n = native_status(r);
            n.runtime_state == HandyHspRuntimeState::Playing
                && !n.native_active
                && !n.metadata_pending
        });
        let enable = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::EnableHandyHspContinuity {
                stream_request_id: bind.request_id.clone(),
                duration_ms: 8_000,
                allow_online_observations: true,
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        let ResponseBody::HandyHspContinuity(enabled) =
            x.f.engine.handle(enable.clone()).result.unwrap()
        else {
            panic!("enable continuity")
        };
        enabled.validate().unwrap();
        assert!(enabled.history.is_some(), "{enabled:?}");
        save(&output, "continuity-enabled.json", &enabled);
        let original_history = enabled.history.clone();
        visible.observe(&x.f);
        let timing_request = request(
            &x.f.session,
            Some(&x.f.project),
            None,
            Command::BrowserTimingStatus {
                binding_id: visible.binding.binding_id.clone(),
                player: visible.binding.player.clone(),
            },
        );
        let timing_pin =
            x.f.engine
                .browser_timing_scope(&timing_request, &visible.binding.binding_id)
                .unwrap();
        let media_pin = {
            let db = x.f.engine.db.lock().unwrap();
            x.f.engine
                .pin_browser_media_locked(&db, &timing_request, &media)
                .unwrap()
        };
        let before_ns = crate::handy_clock::boottime_ns().unwrap();
        let observation = timing_pin.latest(before_ns).unwrap();
        let accepted =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .native_accepted(&x.f.session, &capture.identity)
                .unwrap()
                .cloned()
                .unwrap();
        let original_sample_until = accepted
            .capture_boottime
            .earliest_ns
            .checked_sub(50_000_000)
            .unwrap()
            .checked_add(AGE_NS)
            .unwrap();
        let original_clock_until = original_history
            .as_ref()
            .unwrap()
            .initial_clock_valid_until_boottime_ns;
        let capture_before = capture_status(&x.f, &capture.identity.epoch);
        let remaining_capture_ns = u64::from(capture_before.options.lease_ms)
            .checked_mul(1_000_000)
            .unwrap()
            .checked_sub(capture_before.server_elapsed.unwrap().0)
            .unwrap();
        let proof_margin_ns = before_ns.checked_add(100_000_000).unwrap();
        assert!(observation.expires_at_boottime_ns > proof_margin_ns);
        assert!(original_sample_until > proof_margin_ns);
        assert!(original_clock_until > proof_margin_ns);
        assert!(remaining_capture_ns > 100_000_000);
        assert!(media_pin.covers_duration(Duration::from_millis(100)));
        let probe =
            x.f.engine
                .hsp_live_epoch_probe_for_test(&x.f.session, &bind.request_id)
                .unwrap();
        assert!(!probe.watchdog_observation().unwrap().2);
        let before_hold = live_status(&x.f, &bind.request_id);
        assert_eq!(
            native_status(&before_hold).runtime_state,
            HandyHspRuntimeState::Playing
        );
        assert!(!before_hold.native.as_ref().unwrap().retired);
        let delivered_before = before_hold.native.as_ref().unwrap().delivered_samples;
        let puts_before = peer.puts();
        save(
            &output,
            "before-timing-copy-hold.json",
            &json!({
                "live":before_hold,"clock":enabled,"original_observation":observation,
                "original_sample_until_ns":original_sample_until,"original_clock_until_ns":original_clock_until,
                "capture_remaining_ns":remaining_capture_ns,"media_covers_100ms":true,
                "second_original_frame":terminal,"source_sample_count":stamps.len(),"native_puts":puts_before,
            }),
        );
        let pin_for_hold = timing_pin.clone();
        let output_for_hold = output.clone();
        let observed_for_hold = observation.clone();
        let probe_for_hold = &probe;
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let held = std::thread::scope(|scope| {
            let holder = scope.spawn(move || {
                let held_registry = pin_for_hold.hold_registry_for_test();
                let started_ns = crate::handy_clock::boottime_ns().unwrap();
                assert_eq!(pin_for_hold.try_snapshot(started_ns).unwrap(), observed_for_hold);
                let initial_progress = probe_for_hold.watchdog_observation().unwrap();
                let until = Instant::now() + Duration::from_millis(40);
                entered_tx.send(()).unwrap();
                let mut owner_evaluated = initial_progress.2;
                let mut final_progress = initial_progress;
                while Instant::now() < until {
                    final_progress = probe_for_hold.watchdog_observation().unwrap();
                    owner_evaluated |= final_progress.0 > initial_progress.0
                        || final_progress.1 > initial_progress.1 || final_progress.2;
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                let at_release = probe_for_hold.watchdog_observation().unwrap();
                owner_evaluated |= at_release.0 > initial_progress.0
                    || at_release.1 > initial_progress.1 || at_release.2;
                drop(held_registry);
                let released_ns = crate::handy_clock::boottime_ns().unwrap();
                // Evidence is recorded only after releasing actual publication serialization.
                let evidence = json!({"started_boottime_ns":started_ns,"released_boottime_ns":released_ns,
                    "held_ns":released_ns.checked_sub(started_ns).unwrap(),"requested_hold_ns":40_000_000,
                    "original_contention_cap_ns":100_000_000,"initial_progress":initial_progress,
                    "last_loop_progress":final_progress,"release_progress":at_release,
                    "owner_evaluated_while_held":owner_evaluated,
                    "original_observation_expires_ns":observed_for_hold.expires_at_boottime_ns,
                    "original_sample_until_ns":original_sample_until,"original_clock_until_ns":original_clock_until,
                    "authority_injected":false,"timing_registry_guard_only":true,
                    "removed_copy_mutex_red_retained":true});
                save(&output_for_hold,"timing-copy-hold.json",&evidence);
                (started_ns,released_ns,owner_evaluated,at_release)
            });
            entered_rx
                .recv_timeout(WAIT)
                .expect("actual timing publication registry was not acquired");
            // Keep the ordinary observation cadence. A real Observe call may
            // wait on the held registry; no fake clock or replacement frame is used.
            while !holder.is_finished() {
                visible.observe(&x.f);
                std::thread::park_timeout(Duration::from_millis(2));
            }
            holder.join().unwrap()
        });
        let after_hold = live_status(&x.f, &bind.request_id);
        save(
            &output,
            "after-timing-copy-hold.json",
            &json!({"live":after_hold,"native_puts":peer.puts(),
            "watchdog":probe.watchdog_observation().unwrap()}),
        );
        assert!(held.1 - held.0 >= 40_000_000);
        assert!(
            held.1 - held.0 < 100_000_000,
            "fixture exceeded unchanged contention cap"
        );
        assert!(
            held.2,
            "no autonomous owner evaluation was observed while the actual mutex was held"
        );
        assert!(held.1 < observation.expires_at_boottime_ns);
        assert!(held.1 < original_sample_until && held.1 < original_clock_until);
        assert!(held.1.checked_sub(before_ns).unwrap() < remaining_capture_ns);
        assert!(
            media_pin.current(),
            "original media capability expired during hold"
        );
        assert!(
            !held.3 .2,
            "owner retired while all original authority remained valid"
        );
        assert!(!after_hold.native.as_ref().unwrap().retired);
        assert_eq!(
            native_status(&after_hold).runtime_state,
            HandyHspRuntimeState::Playing
        );
        assert_eq!(after_hold.origin, original_origin);
        assert_eq!(
            after_hold.native.as_ref().unwrap().delivered_samples,
            delivered_before
        );
        assert_eq!(peer.puts(), puts_before, "copy contention issued a command");
        assert!(timing_pin.current(crate::handy_clock::boottime_ns().unwrap()));
        let clock_after = continuity_status(&x.f, &enable.request_id);
        assert_eq!(clock_after.history, original_history);
        assert!(clock_after.native.as_ref().unwrap().current);
        index += 2;
        frame += 1;
        terminal = upload_original(&x.f, &capture, &input, &pixels, index, frame);
        let third = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(frame));
        let third_stamp = actual_stamp(&x.f, &third, &input, &pixels, index - 2, index);
        assert_eq!(third_stamp["source_sequence"], 3);
        assert_eq!(third_stamp["continuous_from_previous"], true);
        stamps.push(third_stamp);
        let delivered = wait_live(&x.f, &mut visible, &bind.request_id, |status| {
            let native = native_status(status);
            status.native.as_ref().unwrap().delivered_samples == 3
                && native.total_added == 3
                && !native.native_active
                && !native.metadata_pending
        });
        assert_eq!(delivered.origin, original_origin);
        assert_eq!(
            peer.puts(),
            ["/hsp/setup", "/hsp/add", "/hsp/play", "/hsp/add"]
        );
        save(
            &output,
            "original-next-cpu-delivered.json",
            &json!({"live":delivered,"samples":stamps}),
        );
        let close = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::CloseBrowserAnalysisInput {
                epoch: capture.identity.epoch.clone(),
                final_frame: Some(terminal.clone()),
            },
        );
        capture_report(x.f.engine.handle(close.clone()).result.unwrap());
        let sealed = wait_input_sealed(&x.f, &mut visible, &capture.identity.epoch);
        assert_eq!(
            sealed.input_close.as_ref().unwrap().final_frame.as_ref(),
            Some(&terminal)
        );
        save(&output, "final-input-sealed.json", &sealed);
        let stopped = wait_live(&x.f, &mut visible, &bind.request_id, |status| {
            let n = native_status(status);
            n.runtime_state == HandyHspRuntimeState::StopAcknowledged
                && !n.native_active
                && !n.metadata_pending
        });
        let finite = native_status(&stopped).finite_drain.as_ref().unwrap();
        assert_eq!(finite.input_state, HandyHspInputState::TailReached);
        assert!(!finite.aborted && !finite.stop_deadline_missed);
        let until = Instant::now() + WAIT;
        let clock_final = loop {
            let status = continuity_status(&x.f, &enable.request_id);
            if status
                .native
                .as_ref()
                .is_some_and(|n| n.worker_joined && !n.worker_active)
            {
                break status;
            }
            assert!(
                Instant::now() < until,
                "clock owner did not join: {status:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        };
        assert_eq!(clock_final.history, original_history);
        save(
            &output,
            "stopped-and-clock-joined.json",
            &json!({"live":stopped,"clock":clock_final}),
        );
        let final_puts = peer.puts();
        assert_eq!(
            final_puts,
            [
                "/hsp/setup",
                "/hsp/add",
                "/hsp/play",
                "/hsp/add",
                "/hsp/stop"
            ]
        );
        assert_eq!(io.base.syncs.load(Ordering::Acquire), 1);
        capture_report(x.f.engine.handle(close).result.unwrap());
        live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        live_report(x.f.engine.handle(play).result.unwrap());
        assert_eq!(peer.puts(), final_puts);
        let ResponseBody::HandyQualification(device) =
            qualification_status(&x.f, &x.accepted.dossier_id).unwrap()
        else {
            panic!("device dossier")
        };
        assert_eq!(device.state, HandyQualificationState::Pending);
        assert_eq!(
            device.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let ResponseBody::HandyHspSourceQualification(source) =
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    None,
                    Command::HandyHspSourceQualificationStatus {
                        dossier_id: pending_source.dossier_id.clone(),
                        owner_proof: x.f.engine.handy_owner_token.clone(),
                    },
                ))
                .result
                .unwrap()
        else {
            panic!("source dossier")
        };
        assert_eq!(source.state, HandyQualificationState::Pending);
        assert_eq!(
            source.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        save(
            &output,
            "dossiers-still-pending.json",
            &json!({"device":device,"source":source}),
        );
        let retired = capture_report(
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(1),
                    Command::StopLiveCapture {
                        epoch: capture.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        assert!(!retired.active);
        drop(probe);
        drop(timing_pin);
        drop(media_pin);
        shutdown.finish();
        save(
            &output,
            "worker-fence-shutdown-joined.json",
            &assert_joined_worker_fence(&x.f),
        );
        assert_eq!(Arc::strong_count(&x.f.engine), 1);
        let PreparationFixture { f, .. } = x;
        let Fixture { engine, dir, .. } = f;
        let config = Engine::retire_for_test(engine);
        let reopened = Engine::open(config).unwrap();
        let replay = live_report(reopened.handle(bind).result.unwrap());
        assert_eq!(replay.origin, original_origin);
        assert!(replay.native.is_none());
        let ResponseBody::HandyHspContinuity(clock_replay) =
            reopened.handle(enable).result.unwrap()
        else {
            panic!("continuity restart")
        };
        assert_eq!(clock_replay.history, original_history);
        assert!(clock_replay.native.is_none());
        assert_eq!(peer.puts(), final_puts);
        save(
            &output,
            "restart-replay.json",
            &json!({"live":replay,"clock":clock_replay}),
        );
        reopened.finish_shutdown().unwrap();
        Engine::retire_for_test(reopened);
        drop(dir);
        assert_eq!(artifact(&runtime), runtime_before);
        save(
            &output,
            "completion.json",
            &json!({"case":"timing-copy","actual_cpu_points":3,
            "held_ns":held.1-held.0,"owner_evaluated_while_held":held.2,"retired_while_held":held.3.2,
            "original_sample_delivered_after_release":true,"original_stream_preserved":true,
            "worker_joined":true,"delivery_settled":true,"clock_worker_joined":true,
            "restart_source_authority":false,"restart_clock_authority":false,"native_puts":final_puts,
            "physical_device":false,"qualified":false}),
        );
    }
}
