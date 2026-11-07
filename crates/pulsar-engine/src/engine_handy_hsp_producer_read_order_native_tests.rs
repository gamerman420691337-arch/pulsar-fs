// Actual confined CPU delivery, authored browser callbacks and localhost only.
// Included inside continuity_native_tests for its existing Pending/Synthetic permits.
mod producer_read_order_native_tests {
    use super::*;
    use crate::authority::handy_hsp_live::HspProducerReadTestPhase;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        PublicationAfterTick,
        OriginalExpiry,
        LifetimeRetirement,
    }
    impl Mode {
        fn label(self) -> &'static str {
            match self {
                Self::PublicationAfterTick => "replacement-after-sample",
                Self::OriginalExpiry => "original-expiry",
                Self::LifetimeRetirement => "original-lifetime-retirement",
            }
        }
    }
    fn run(mode: Mode) {
        let fixture_path = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        let output = env_path("PULSAR_HSP_PRODUCER_READ_NATIVE_OUTPUT").join(mode.label());
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
            "runtime":runtime_before,"case":mode.label(),"actual_confined_cpu":true,"browser_capture":false,
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
        let mut preparation = x.prepare("producer-order-actual-cpu-preparation");
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
        let stamps = vec![actual_stamp(
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
        save(&output, "first-actual-cpu-result.json", &stamps);
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
        let ResponseBody::HandyHspContinuity(enabled) = x.f.engine.handle(enable).result.unwrap()
        else {
            panic!("continuity enable")
        };
        let original_history = enabled.history.as_ref().unwrap().clone();
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
        let original = timing_pin
            .latest(crate::handy_clock::boottime_ns().unwrap())
            .unwrap();
        let hold = x.f.engine.hold_next_hsp_producer_read_for_test(
            &x.f.session,
            &bind.request_id,
            if mode == Mode::PublicationAfterTick {
                HspProducerReadTestPhase::AfterSample
            } else {
                HspProducerReadTestPhase::BeforeSample
            },
        );
        index += 2;
        let header = upload_original(&x.f, &capture, &input, &pixels, index, 2);
        assert!(
            hold.wait_entered(Duration::from_secs(2)),
            "actual producer never reached read rendezvous"
        );
        let entered = hold.snapshot();
        let accepted =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .native_accepted(&x.f.session, &capture.identity)
                .unwrap()
                .cloned()
                .unwrap();
        assert_eq!(accepted.frame.frame, header.frame);
        let selected_expiry = original.expires_at_boottime_ns;
        let replacement = match mode {
            Mode::PublicationAfterTick => {
                visible.observe(&x.f);
                let next = timing_pin
                    .latest(crate::handy_clock::boottime_ns().unwrap())
                    .unwrap();
                let at = entered.sampled_at_ns.unwrap();
                assert!(next.bracket.native_receive_ns > at);
                assert!(at >= original.bracket.native_receive_ns && at < selected_expiry);
                next
            }
            Mode::OriginalExpiry => {
                assert!(entered.sampled_at_ns.is_none());
                let until = Instant::now() + Duration::from_secs(1);
                loop {
                    visible.observe(&x.f);
                    if crate::handy_clock::boottime_ns().unwrap() >= selected_expiry {
                        break;
                    }
                    assert!(
                        Instant::now() < until,
                        "original expiry rendezvous exceeded its unchanged bound"
                    );
                    std::thread::park_timeout(Duration::from_millis(20));
                }
                timing_pin
                    .latest(crate::handy_clock::boottime_ns().unwrap())
                    .unwrap()
            }
            Mode::LifetimeRetirement => {
                assert!(entered.sampled_at_ns.is_none());
                // A player admits only one current ordinary timing binding.
                // Release A through the DB-free fast path before opening B.
                x.f.engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        None,
                        Command::ReleaseBrowserTiming {
                            binding_id: visible.binding.binding_id.clone(),
                            player: visible.binding.player.clone(),
                        },
                    ))
                    .result
                    .unwrap();
                assert!(!timing_pin.current(crate::handy_clock::boottime_ns().unwrap()));
                // Delivery holds no DB/capture guard. Open's capture sweep
                // retires A through atomic cancellation; it neither acquires
                // the producer delivery mutex nor joins this paused callback.
                let opened = AuthoredVisiblePlayer::open(&x.f, &media);
                let mut replacement = AuthoredVisiblePlayer {
                    binding: opened.binding,
                    origin: visible.origin,
                    sequence: 0,
                    presented_frames: visible.presented_frames.clone(),
                    last_tick_us: visible.last_tick_us,
                };
                replacement.observe(&x.f);
                assert_ne!(replacement.binding.binding_id, visible.binding.binding_id);
                assert_eq!(replacement.binding.player, visible.binding.player);
                let replacement_request = request(
                    &x.f.session,
                    Some(&x.f.project),
                    None,
                    Command::BrowserTimingStatus {
                        binding_id: replacement.binding.binding_id.clone(),
                        player: replacement.binding.player.clone(),
                    },
                );
                x.f.engine
                    .browser_timing_scope(&replacement_request, &replacement.binding.binding_id)
                    .unwrap()
                    .latest(crate::handy_clock::boottime_ns().unwrap())
                    .unwrap()
            }
        };
        assert_ne!(replacement.observation_id, original.observation_id);
        let release_at = crate::handy_clock::boottime_ns().unwrap();
        assert!(release_at < replacement.expires_at_boottime_ns);
        assert!(release_at < original_history.initial_clock_valid_until_boottime_ns);
        assert!(
            release_at
                .checked_sub(
                    accepted
                        .capture_boottime
                        .earliest_ns
                        .checked_sub(50_000_000)
                        .unwrap()
                )
                .unwrap()
                < AGE_NS
        );
        hold.release();
        // No further browser publication is allowed until this exact producer
        // has finished its attempted delivery. No Engine/capture/timing guard is held.
        let until = Instant::now() + Duration::from_secs(2);
        let receipt = loop {
            let receipt = hold.snapshot();
            if receipt.completed {
                break receipt;
            }
            assert!(
                Instant::now() < until,
                "producer did not complete after release"
            );
            std::thread::park_timeout(Duration::from_millis(1));
        };
        let sample_json = receipt.sample.map(|s| json!({"time_ms":s.point.time_ms,"position_percent":s.point.position_percent,
            "source_sequence":s.source_sequence,"captured_at_ns":s.captured_at_ns,"accepted_at_ns":s.accepted_at_ns,
            "continuous_from_previous":s.continuous_from_previous}));
        save(
            &output,
            "producer-read-receipt.json",
            &json!({"case":mode.label(),"entered":receipt.entered,
            "sampled_at_ns":receipt.sampled_at_ns,"original":original,"replacement":replacement,
            "evaluated_observation":receipt.observation,"clock_generation":receipt.clock_pin.as_ref().map(|p|p.generation()),
            "clock_valid_until_ns":receipt.clock_pin.as_ref().map(|p|p.valid_until_ns()),
            "original_clock_history":original_history,"release_at_ns":release_at,"sample":sample_json,
            "completed":receipt.completed,"delivered":receipt.delivered,"timed_out":receipt.timed_out,
            "input_identity":accepted.frame,"input_accepted_ns":accepted.accepted_boottime.latest_ns,
            "original_capture_earliest_ns":accepted.capture_boottime.earliest_ns,
            "physical_device":false,"qualified":false}),
        );
        assert!(!receipt.timed_out);
        let at = receipt.sampled_at_ns.unwrap();
        match mode {
            Mode::PublicationAfterTick => {
                assert_eq!(
                    receipt.observation.as_ref(),
                    Some(&original),
                    "producer must evaluate its selected A after B arrives"
                );
                assert!(
                    receipt.delivered,
                    "actual original CPU sample was not delivered"
                );
                let pin = receipt.clock_pin.as_ref().unwrap();
                assert_eq!(pin.generation(), 1);
                assert_eq!(
                    pin.valid_until_ns(),
                    original_history.initial_clock_valid_until_boottime_ns
                );
                let sample = receipt.sample.unwrap();
                assert_eq!(sample.accepted_at_ns, accepted.accepted_boottime.latest_ns);
                assert_eq!(
                    sample.captured_at_ns,
                    accepted.capture_boottime.earliest_ns - 50_000_000
                );
                let LiveInferenceReply::Estimated { input, .. } = &accepted.estimate.result else {
                    unreachable!()
                };
                assert_eq!(sample.source_sequence, input.sequence);
                assert_eq!(
                    sample.continuous_from_previous,
                    accepted.continuous_from_previous
                );
            }
            Mode::OriginalExpiry => {
                assert!(at >= selected_expiry);
                assert!(at < replacement.expires_at_boottime_ns);
                assert!(
                    !receipt.delivered && receipt.observation.is_none(),
                    "B must not rescue expired selected A"
                );
            }
            Mode::LifetimeRetirement => {
                assert_ne!(replacement.binding_id, original.binding_id);
                assert!(
                    !receipt.delivered && receipt.observation.is_none(),
                    "a new B binding must not rescue retired A"
                );
            }
        }
        // Stop remains independently owned even when selected timing is gone.
        stop(&x.f, &bind.request_id);
        let until = Instant::now() + WAIT;
        let stopped = loop {
            let status = live_status(&x.f, &bind.request_id);
            let native = native_status(&status);
            if native.runtime_state == HandyHspRuntimeState::StopAcknowledged
                && !native.native_active
                && !native.metadata_pending
            {
                break status;
            }
            assert!(
                Instant::now() < until,
                "owned Stop did not settle: {status:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        };
        assert_eq!(stopped.origin, original_origin);
        assert_eq!(
            stopped.native.as_ref().unwrap().delivered_samples,
            if mode == Mode::PublicationAfterTick {
                2
            } else {
                1
            }
        );
        let puts = peer.puts();
        assert_eq!(
            puts.iter().filter(|p| p.as_str() == "/hsp/setup").count(),
            1
        );
        assert_eq!(puts.iter().filter(|p| p.as_str() == "/hsp/stop").count(), 1);
        assert!(!puts.iter().any(|p| p == "/hsp/play"));
        shutdown.finish();
        let fence: Value = serde_json::from_slice(
            &fs::read(x.f.engine.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        assert!(fence["groups"].as_object().unwrap().is_empty());
        let Fixture {
            engine,
            dir,
            session,
            project,
        } = x.f;
        drop((engine, session, project, io, hold));
        drop(peer);
        let temporary = dir.path().to_owned();
        dir.close().unwrap();
        assert_eq!(artifact(&runtime).sha256, runtime_before.sha256);
        save(
            &output,
            "completion.json",
            &json!({"case":mode.label(),"actual_cpu_acceptances":2,
            "production_delivery_precheck":true,"original_observation_expiry_preserved":true,
            "all_engine_workers_joined":true,"temporary_state_removed":!temporary.exists(),
            "localhost_peer_joined":true,"borrowed_runtime_preserved":true,"native_puts":puts,
            "physical_device":false,"qualified":false}),
        );
    }
    #[test]
    #[ignore = "requires pinned actual CPU/runtime/neutral fixture and delegated confinement; run serially"]
    fn native_cpu_producer_retains_selected_observation_after_later_publication() {
        run(Mode::PublicationAfterTick);
    }
    #[test]
    #[ignore = "requires pinned actual CPU/runtime/neutral fixture and delegated confinement; run serially"]
    fn native_cpu_producer_cannot_replace_expired_selected_observation() {
        run(Mode::OriginalExpiry);
    }
    #[test]
    #[ignore = "requires pinned actual CPU/runtime/neutral fixture and delegated confinement; run serially"]
    fn native_cpu_producer_cannot_rebind_after_selected_lifetime_retirement() {
        run(Mode::LifetimeRetirement);
    }
}
