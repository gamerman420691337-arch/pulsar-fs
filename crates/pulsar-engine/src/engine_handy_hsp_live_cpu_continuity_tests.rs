// Included inside finite_tail_native_tests. Actual confined CPU workers consume
// original decoded neutral frames; the localhost peer and visible callbacks are
// authored. Durable dossiers remain Pending/Synthetic. No physical qualification.
mod continuity_native_tests {
    use super::*;
    use crate::authority::handy_hsp_preparation::HspPreparationIo;
    use crate::device_effect_journal::HspEffectObservation;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Case {
        TwoTransfers,
        HeldClockExpiry,
        HeldAppendExpiry,
        StopDuringClock,
        StopDuringTransfer,
    }
    impl Case {
        fn label(self) -> &'static str {
            match self {
                Self::TwoTransfers => "two-transfers",
                Self::HeldClockExpiry => "held-clock-expiry",
                Self::HeldAppendExpiry => "held-append-expiry",
                Self::StopDuringClock => "stop-during-clock",
                Self::StopDuringTransfer => "stop-during-transfer",
            }
        }
    }

    // The read hold lives in IO, outside registry/runtime locks. Calling the
    // supplied native clock throughout retains cancellation and original expiry.
    struct ContinuityIo {
        base: Arc<RealPreparationIo>,
        reads: AtomicU64,
        initial_server_received_ns: AtomicU64,
        hold: AtomicBool,
        entered: AtomicBool,
        release: AtomicBool,
    }
    impl ContinuityIo {
        fn observation(&self, stream: u32) -> HandySetupDeviceObservation {
            let s = self.base.peer.lock().unwrap();
            assert_eq!(s.stream, stream);
            let mut sample = hsp_qualification_sample();
            sample.identity = self.base.identity.clone();
            sample.mode = HandyDeviceMode {
                mode: 4,
                mode_session_id: s.mode,
            };
            let remote = sample.remote_hsp.as_mut().unwrap();
            remote.play_state = if s.playing {
                HandyRemotePlayState::Playing
            } else {
                HandyRemotePlayState::Stopped
            };
            remote.stream_id = s.stream;
            remote.current_time_ms = -10_000;
            remote.points = u32::try_from((s.tail + 1).max(0)).unwrap();
            remote.max_points = 32;
            remote.current_point = if s.tail < 0 { -1 } else { 0 };
            remote.playback_rate = 1.0;
            remote.looping = false;
            remote.pause_on_starving = false;
            remote.first_point_time_ms = s.first_time.try_into().unwrap();
            remote.last_point_time_ms = s.last_time.try_into().unwrap();
            remote.tail_point_stream_index = s.tail.try_into().unwrap();
            remote.tail_point_stream_index_threshold = 1;
            sample.validate().unwrap();
            sample
        }
    }
    impl HspPreparationIo for ContinuityIo {
        fn now(&self) -> PResult<u64> {
            self.base.now()
        }
        fn inspect(
            &self,
            credentials: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            self.base.inspect(credentials, deadline, clock)
        }
        fn synchronize(
            &self,
            credentials: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            self.base.synchronize(credentials, deadline, clock)
        }
        fn server_sample(
            &self,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample> {
            let sample = self.base.server_sample(deadline, clock)?;
            let _ = self.initial_server_received_ns.compare_exchange(
                0,
                sample.received_ns,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            Ok(sample)
        }
        fn hsp_origin(&self) -> Option<String> {
            self.base.hsp_origin()
        }
        fn inspect_hsp_stream(
            &self,
            _credentials: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
            stream: u32,
        ) -> PResult<crate::handy_online::PlayingTimingRead> {
            let started_boottime_ns = clock()?;
            self.reads.fetch_add(1, Ordering::AcqRel);
            let before = self.observation(stream);
            if self.hold.load(Ordering::Acquire) {
                self.entered.store(true, Ordering::Release);
                while !self.release.load(Ordering::Acquire) {
                    clock()?;
                    if Instant::now() >= deadline {
                        return Err(ProtocolError::new(
                            ErrorCode::Unavailable,
                            "original clock read deadline elapsed",
                        ));
                    }
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            }
            let mut sample = || -> PResult<HandyDeviceTimingSample> {
                let sent = clock()?;
                Ok(HandyDeviceTimingSample {
                    info: AuthoredIo::info(sent),
                    user_offset: HandyUserTimeOffset { offset_ms: 0 },
                    read_started_boottime_ns: sent,
                    read_finished_boottime_ns: clock()?,
                })
            };
            let first = sample()?;
            let second = sample()?;
            let after = self.observation(stream);
            Ok(crate::handy_online::PlayingTimingRead {
                device: crate::handy_online::DeviceTimingRead {
                    before,
                    first,
                    second,
                    after,
                },
                started_boottime_ns,
                finished_boottime_ns: clock()?,
            })
        }
    }
    fn initial_media(f: &Fixture, media_path: PathBuf) -> BrowserMediaBinding {
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path: media_path },
            ))
            .result
            .unwrap()
        else {
            panic!("source import");
        };
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::SetSourcePlacement {
                    spec: SourcePlacementSpec {
                        source_version,
                        source_start: SourceTimestamp::new(0, 1).unwrap(),
                        source_end: SourceTimestamp::new(30, 1).unwrap(),
                        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                        project_anchor: ProjectTime::ZERO,
                        rate_numerator: 1,
                        rate_denominator: 1,
                    },
                    replace: None,
                },
            ))
            .result
            .unwrap();
        let ResponseBody::BrowserMediaCatalog(catalog) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::BrowserMediaCatalog,
            ))
            .result
            .unwrap()
        else {
            panic!("browser media catalog");
        };
        assert_eq!(catalog.entries.len(), 1);
        let intent = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::PrepareBrowserMedia {
                engine_epoch: catalog.engine_epoch,
                selection: catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
                },
                lease_ms: 2_000,
                allow_declared_clock: true,
            },
        );
        f.engine.handle(intent.clone()).result.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let ResponseBody::BrowserMediaPreparation(report) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: intent.request_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("media preparation status");
            };
            if report.state == BrowserMediaPreparationState::Ready {
                return report.binding.unwrap();
            }
            assert_eq!(
                report.state,
                BrowserMediaPreparationState::Pending,
                "{report:?}"
            );
            assert!(
                Instant::now() < deadline,
                "media preparation did not finish: {report:?}"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        }
    }
    fn continuity_status(f: &Fixture, enable: &RequestId) -> HandyHspContinuityStatus {
        let ResponseBody::HandyHspContinuity(report) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::HandyHspContinuityStatus {
                    original_request_id: enable.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("continuity status")
        };
        report.validate().unwrap();
        assert!(!report.qualified);
        *report
    }
    fn transfer_status(f: &Fixture, intent: &Request) -> BrowserAnalysisTransferReport {
        let ResponseBody::BrowserAnalysisTransfer(report) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::BrowserAnalysisTransferStatus {
                    transfer_request_id: intent.request_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("transfer status")
        };
        report.validate().unwrap();
        *report
    }
    fn observe(
        f: &Fixture,
        active: &mut AuthoredVisiblePlayer,
        pending: &mut Option<AuthoredVisiblePlayer>,
    ) {
        active.observe(f);
        if let Some(pending) = pending {
            pending.observe(f);
        }
    }
    fn actual_stamp(
        f: &Fixture,
        capture: &LiveCaptureReport,
        input: &Value,
        pixels: &[u8],
        previous: usize,
        current: usize,
    ) -> Value {
        let registry = f.engine.live_captures.lock().unwrap();
        let stamp = registry
            .native_accepted(&f.session, &capture.identity)
            .unwrap()
            .unwrap();
        let LiveInferenceReply::Estimated {
            input: pair,
            position,
            ..
        } = &stamp.estimate.result
        else {
            panic!("actual CPU pair")
        };
        assert_eq!(
            pair.previous.sha256,
            input["frames"][previous]["sha256"].as_str().unwrap()
        );
        assert_eq!(
            pair.current.sha256,
            input["frames"][current]["sha256"].as_str().unwrap()
        );
        assert_eq!(
            stamp.frame.source_time,
            SourceTimestamp::new(
                input["frames"][current]["source_time_us"].as_i64().unwrap(),
                1_000_000
            )
            .unwrap()
        );
        let sum: u64 = [previous, current]
            .into_iter()
            .flat_map(|i| pixels[i * LIVE_FRAME_BYTES..(i + 1) * LIVE_FRAME_BYTES].iter())
            .map(|b| u64::from(*b))
            .sum();
        let expected = 0.25 + 0.5 * (sum as f64 / (2 * LIVE_FRAME_BYTES) as f64 / 255.0);
        assert!((position.value() - expected).abs() < 0.0001);
        json!({ "previous_frame":stamp.previous_frame, "frame":stamp.frame, "estimate":stamp.estimate,
            "source_sequence":pair.sequence,"continuous_from_previous":stamp.continuous_from_previous,
            "continuity_generation":stamp.continuity_generation,"decoded_previous":previous,"decoded_current":current,
            "expected_authored_model_position":expected })
    }
    fn stage_successor(
        f: &Fixture,
        media: &BrowserMediaBinding,
        capture: &LiveCaptureReport,
        visible: &mut AuthoredVisiblePlayer,
        lease_ms: u32,
    ) -> (
        BrowserMediaBinding,
        AuthoredVisiblePlayer,
        Request,
        LiveCaptureReport,
    ) {
        visible.observe(f);
        let media_intent = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::PrepareBrowserMediaSuccessor {
                predecessor: media.clone(),
                lease_ms,
            },
        );
        f.engine.handle(media_intent.clone()).result.unwrap();
        let until = Instant::now() + WAIT;
        let child_media = loop {
            visible.observe(f);
            let ResponseBody::BrowserMediaPreparation(r) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: media_intent.request_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("successor media")
            };
            if r.state == BrowserMediaPreparationState::Ready {
                break r.binding.unwrap();
            }
            assert_eq!(r.state, BrowserMediaPreparationState::Pending, "{r:?}");
            assert!(Instant::now() < until, "media successor readiness");
            std::thread::park_timeout(Duration::from_millis(2));
        };
        assert_eq!(child_media.selection, media.selection);
        assert_eq!(child_media.clock, media.clock);
        let ResponseBody::BrowserTiming(r) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::PrepareBrowserTimingSuccessor {
                    predecessor_binding_id: visible.binding.binding_id.clone(),
                    player: visible.binding.player.clone(),
                    browser_media: child_media.clone(),
                    policy: BrowserTimingPolicy {
                        probe_timeout_ms: 100,
                        observation_timeout_ms: 500,
                    },
                },
            ))
            .result
            .unwrap()
        else {
            panic!("successor timing")
        };
        let mut child_visible = AuthoredVisiblePlayer {
            binding: r.binding,
            origin: visible.origin,
            sequence: 0,
            presented_frames: visible.presented_frames.clone(),
            last_tick_us: visible.last_tick_us,
        };
        child_visible.observe(f);
        let mut declaration = capture.options.browser_analysis.clone().unwrap();
        // Each successor owns a fresh analysis feed; exact Prepare retries keep it.
        declaration.feed_id =
            ArtifactId::new(format!("native-cpu-analysis-{}", uuid::Uuid::new_v4())).unwrap();
        declaration.timing_binding_id = child_visible.binding.binding_id.clone();
        let prepare = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::PrepareBrowserAnalysisSuccessor {
                engine_epoch: child_media.engine_epoch.clone(),
                predecessor: capture.identity.epoch.clone(),
                options: BrowserAnalysisSuccessorOptions {
                    browser_media: child_media.clone(),
                    browser_analysis: declaration,
                    lease_ms: 30_000,
                },
            },
        );
        let until = Instant::now() + WAIT;
        let child = loop {
            visible.observe(f);
            child_visible.observe(f);
            let ResponseBody::BrowserAnalysisSuccessor(r) =
                f.engine.handle(prepare.clone()).result.unwrap()
            else {
                panic!("successor preparation")
            };
            if r.phase == BrowserAnalysisSuccessorPhase::Prepared {
                break r.successor;
            }
            assert_eq!(r.phase, BrowserAnalysisSuccessorPhase::Preparing, "{r:?}");
            assert!(Instant::now() < until, "actual successor worker readiness");
            std::thread::park_timeout(Duration::from_millis(2));
        };
        (child_media, child_visible, prepare, child)
    }
    fn stop(f: &Fixture, bind: &RequestId) -> Request {
        let intent = request(
            &f.session,
            Some(&f.project),
            None,
            Command::StopHandyHspLive {
                stream_request_id: bind.clone(),
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        live_report(f.engine.handle(intent.clone()).result.unwrap());
        intent
    }

    fn assert_original_pair_gap(previous: &LiveFrameHeader, current: &LiveFrameHeader) {
        let capture_gap_ns = current
            .capture_tick
            .0
            .checked_sub(previous.capture_tick.0)
            .expect("actual capture tick regressed in continuity fixture");
        let source_gap = ExactSeconds::from_source(current.source_time)
            .checked_sub(ExactSeconds::from_source(previous.source_time))
            .unwrap();
        assert!(capture_gap_ns <= 500_000_000
            && source_gap.checked_cmp(ExactSeconds::new(0, 1).unwrap()).unwrap().is_gt()
            && !source_gap.checked_cmp(ExactSeconds::new(1, 2).unwrap()).unwrap().is_gt(),
            "actual fixture pair would reseed instead of emitting CPU work: previous={previous:?}, current={current:?}, capture_gap_ns={capture_gap_ns}, source_gap={source_gap:?}");
    }
    fn latest_append(f: &Fixture, stream: &str) -> HspEffectObservation {
        let journal = f.engine._device_effect_journal.lock().unwrap();
        let stream = journal.hsp_stream(stream).unwrap();
        stream
            .effect_ids
            .iter()
            .rev()
            .find_map(|id| {
                let effect = journal.hsp_effect(id).unwrap();
                (effect.record.kind == HspEffectRecordKind::Append).then_some(effect)
            })
            .expect("actual native Append required")
    }
    fn effect_value(effect: &HspEffectObservation) -> Value {
        json!({"id":effect.id,"state":format!("{:?}",effect.state),"record":effect.record,
            "outcome":effect.outcome,"late_observation":effect.late_observation})
    }
    fn effect_expiry(effect: &HspEffectObservation) -> u64 {
        let facts = effect.record.original_facts.as_ref().unwrap();
        facts
            .lease_until_ns
            .min(facts.source_until_ns)
            .min(facts.clock_until_ns)
    }
    fn run_continuity(case: Case) {
        let fixture_path = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        let output = env_path("PULSAR_HSP_CONTINUITY_NATIVE_OUTPUT").join(case.label());
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
            "runtime":runtime_before,"case":case.label(),"actual_confined_cpu":true,"browser_capture":false,
            "physical_device":false,"qualified":false}),
        );
        let f = native_fixture(worker);
        let mut media = initial_media(&f, media_path);
        let original_media = media.clone();
        let media_opened_at = Instant::now();
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
        let mut capture = capture_report(
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
        let mut preparation = x.prepare("continuity-actual-cpu-preparation");
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
        if matches!(case, Case::HeldClockExpiry | Case::StopDuringClock) {
            io.hold.store(true, Ordering::Release);
        }
        let mut all_epochs = vec![capture.identity.epoch.clone()];
        let mut transfers = Vec::new();
        let mut predecessor_epochs = Vec::new();
        let mut explicit_stop = None;
        // A scheduling anchor only. Each upload still captures its own original
        // native header tick; commit/status waits cannot restart this cadence.
        let mut last_upload_started = None;
        for round in 0..2 {
            let (child_media, child_visible, prepare, child) = stage_successor(
                &x.f,
                &media,
                &capture,
                &mut visible,
                if round == 0 { 2_000 } else { 8_000 },
            );
            all_epochs.push(child.identity.epoch.clone());
            let mut pending_visible = Some(child_visible);
            observe(&x.f, &mut visible, &mut pending_visible);
            index += 2;
            frame += 1;
            let previous_header = terminal.clone();
            terminal = upload_original(&x.f, &capture, &input, &pixels, index, frame);
            assert_original_pair_gap(&previous_header, &terminal);
            // Settle the real F result, with both actual timing leases alive.
            let until = Instant::now() + WAIT;
            let f_report = loop {
                observe(&x.f, &mut visible, &mut pending_visible);
                let r = capture_status(&x.f, &capture.identity.epoch);
                if r.inference.as_ref().unwrap().accepted == frame {
                    break r;
                }
                assert!(r.active && Instant::now() < until, "F result: {r:?}");
                std::thread::park_timeout(Duration::from_millis(2));
            };
            stamps.push(actual_stamp(
                &x.f,
                &f_report,
                &input,
                &pixels,
                index - 2,
                index,
            ));
            let delivery_until = Instant::now() + WAIT;
            loop {
                observe(&x.f, &mut visible, &mut pending_visible);
                let status = live_status(&x.f, &bind.request_id);
                let native = native_status(&status);
                if status.native.as_ref().unwrap().delivered_samples == stamps.len() as u64
                    && !native.native_active
                    && !native.metadata_pending
                {
                    break;
                }
                assert!(
                    Instant::now() < delivery_until,
                    "actual F delivery did not settle: {status:?}"
                );
                std::thread::park_timeout(Duration::from_millis(2));
            }
            let old_sink =
                x.f.engine
                    .hsp_live_epoch_probe_for_test(&x.f.session, &bind.request_id)
                    .unwrap();
            let transfer = request(
                &x.f.session,
                Some(&x.f.project),
                Some(1),
                Command::TransferBrowserAnalysisInput {
                    predecessor: capture.identity.epoch.clone(),
                    successor: child.identity.epoch.clone(),
                    boundary: terminal.clone(),
                },
            );
            let ResponseBody::BrowserAnalysisTransfer(reserved) =
                x.f.engine.handle(transfer.clone()).result.unwrap()
            else {
                panic!("reserve native F")
            };
            assert_eq!(reserved.boundary, terminal);
            save(
                &output,
                &format!("transfer-{round}-reserved.json"),
                &reserved,
            );
            if case == Case::StopDuringTransfer {
                explicit_stop = Some(stop(&x.f, &bind.request_id));
                transfers.push(transfer);
                break;
            }
            index += 2;
            let original_g_upload_started = Instant::now();
            terminal = upload_original(&x.f, &child, &input, &pixels, index, 1);
            last_upload_started = Some(original_g_upload_started);
            let until = Instant::now() + WAIT;
            let transferred = loop {
                observe(&x.f, &mut visible, &mut pending_visible);
                let r = transfer_status(&x.f, &transfer);
                if r.phase == BrowserAnalysisTransferPhase::Transferred {
                    break r;
                }
                assert_ne!(r.phase, BrowserAnalysisTransferPhase::Aborted, "{r:?}");
                assert!(
                    Instant::now() < until,
                    "native transfer did not commit: {r:?}"
                );
                std::thread::park_timeout(Duration::from_millis(2));
            };
            assert!(
                transferred.predecessor_worker_joined
                    && transferred.predecessor_delivery_settled
                    && transferred.successor_pair_accepted
            );
            assert_eq!(transferred.preparation_request_id, prepare.request_id);
            let child_current = capture_status(&x.f, &child.identity.epoch);
            let stamp = actual_stamp(&x.f, &child_current, &input, &pixels, index - 2, index);
            assert_eq!(stamp["source_sequence"], 1);
            assert_eq!(stamp["continuous_from_previous"], false);
            stamps.push(stamp);
            save(
                &output,
                &format!("transfer-{round}-committed.json"),
                &transferred,
            );
            let before_late = live_status(&x.f, &bind.request_id);
            let delivered_before_late = before_late.native.as_ref().unwrap().delivered_samples;
            old_sink.replay_offer().unwrap();
            old_sink.retire();
            let after_late = live_status(&x.f, &bind.request_id);
            assert_eq!(
                after_late.native.as_ref().unwrap().delivered_samples,
                delivered_before_late
            );
            assert!(!after_late.native.as_ref().unwrap().retired);
            assert_eq!(after_late.origin, before_late.origin);
            assert_eq!(
                native_status(&after_late).runtime_state,
                HandyHspRuntimeState::Playing
            );
            assert!(capture_status(&x.f, &child.identity.epoch).active);
            assert!(!peer.puts().iter().any(|path| path == "/hsp/stop"));
            save(
                &output,
                &format!("transfer-{round}-saved-old-sink-replay.json"),
                &json!({"before":before_late,"after":after_late,"real_saved_callback":true,"real_saved_sink_retired":true}),
            );
            drop(old_sink);
            let old_epoch = capture.identity.epoch.clone();
            let stopped_old = capture_report(
                x.f.engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        Some(1),
                        Command::StopLiveCapture {
                            epoch: old_epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            );
            assert!(!stopped_old.active);
            // Old ingress, retirement and replay are descriptive; none may stop B.
            let old_handshake = live_handshake(&x.f, &capture);
            assert!(x
                .f
                .engine
                .live_bulk_ready(&old_handshake, Some(&transferred.boundary))
                .is_err());
            let replay = transfer_status(&x.f, &transfer);
            assert_eq!(replay.phase, BrowserAnalysisTransferPhase::Transferred);
            predecessor_epochs.push(old_epoch);
            transfers.push(transfer);
            media = child_media;
            visible = pending_visible.take().unwrap();
            capture = child_current;
            frame = 1;
            let status = live_status(&x.f, &bind.request_id);
            assert_eq!(status.origin, original_origin);
            assert!(!status.native.as_ref().unwrap().retired);
            assert_eq!(
                peer.puts()
                    .iter()
                    .filter(|p| p.as_str() == "/hsp/setup")
                    .count(),
                1
            );
            assert_eq!(
                peer.puts()
                    .iter()
                    .filter(|p| p.as_str() == "/hsp/play")
                    .count(),
                1
            );
        }
        let mut clocks = vec![serde_json::to_value(&enabled).unwrap()];
        let mut held_append: Option<HspEffectObservation> = None;
        let mut append_hold_armed = false;
        let mut append_hold_released = false;
        let _release_append = ReleaseOnDrop(Some(|| peer.release()));
        let initial_received = io.initial_server_received_ns.load(Ordering::Acquire);
        assert!(initial_received > 0);
        let first_refresh_due = initial_received
            .checked_add(
                original_history
                    .as_ref()
                    .unwrap()
                    .initial_clock_valid_until_boottime_ns
                    .checked_sub(initial_received)
                    .unwrap()
                    / 2,
            )
            .unwrap();
        let arm_append_at = first_refresh_due.checked_sub(300_000_000).unwrap();
        let mut generation = 1;
        let until = Instant::now() + Duration::from_secs(8);
        while explicit_stop.is_none() {
            visible.observe(&x.f);
            let clock = continuity_status(&x.f, &enable.request_id);
            assert_eq!(clock.history, original_history);
            let n = clock.native.as_ref().unwrap();
            if n.generation != generation {
                if case == Case::HeldAppendExpiry {
                    let original = held_append
                        .as_ref()
                        .expect("fresh publication must follow the real held original claim");
                    assert!(
                        n.current && n.observed_at_boottime_ns < effect_expiry(original),
                        "fresh clock was not observed before the unchanged effect expired"
                    );
                }
                generation = n.generation;
                clocks.push(serde_json::to_value(&clock).unwrap());
            }
            if case == Case::HeldAppendExpiry {
                let now = crate::handy_clock::boottime_ns().unwrap();
                if !append_hold_armed && now >= arm_append_at {
                    assert_eq!(
                        generation, 1,
                        "held Append missed the first original refresh window"
                    );
                    let status = live_status(&x.f, &bind.request_id);
                    let native = native_status(&status);
                    if !native.native_active && !native.metadata_pending {
                        peer.hold_next("/hsp/add");
                        append_hold_armed = true;
                    }
                }
                let reached = peer.hold.0.lock().unwrap().reached;
                if append_hold_armed && reached && held_append.is_none() {
                    let effect = latest_append(
                        &x.f,
                        original_origin.as_ref().unwrap().stream_record_id.as_str(),
                    );
                    save(
                        &output,
                        "held-append-original-claim.json",
                        &effect_value(&effect),
                    );
                    assert_eq!(effect.state, HspJournalState::Claimed);
                    let facts = effect.record.original_facts.as_ref().unwrap();
                    let original_clock = facts.clock_observation.as_ref().expect(
                        "claimed continuity Append must retain its actual native clock observation",
                    );
                    let history = original_history.as_ref().unwrap();
                    assert_eq!(original_clock.generation, 1);
                    assert_eq!(original_clock.owner_id, history.clock_owner_id);
                    assert_eq!(
                        original_clock.valid_until_ns,
                        history.initial_clock_valid_until_boottime_ns
                    );
                    // Effective phase validity also includes the original
                    // browser alignment; it may expire before the native pin.
                    assert!(facts.clock_until_ns <= original_clock.valid_until_ns);
                    assert!(
                        facts.clock_until_ns <= original_clock.reference_mapping.valid_until_ns
                    );
                    assert!(facts.at_ns < first_refresh_due && effect_expiry(&effect) > first_refresh_due,
                        "the actual original effect must straddle refresh without widening its window");
                    held_append = Some(effect);
                }
                if let Some(original) = &held_append {
                    let current = latest_append(
                        &x.f,
                        original_origin.as_ref().unwrap().stream_record_id.as_str(),
                    );
                    assert_eq!(current.id, original.id);
                    assert_eq!(
                        current.record, original.record,
                        "fresh clock changed an original claimed effect"
                    );
                    if generation >= 2 && now >= effect_expiry(original) {
                        let reply_still_held = !peer.hold.0.lock().unwrap().release;
                        save(
                            &output,
                            "held-append-expired-after-clock-refresh.json",
                            &json!({
                            "clock":clock,"effect":effect_value(&current),"original_expiry_ns":effect_expiry(original),
                            "observed_at_ns":now,"first_refresh_due_ns":first_refresh_due,
                            "peer_reply_still_held":reply_still_held}),
                        );
                        peer.release();
                        append_hold_released = true;
                        break;
                    }
                }
            }
            if case == Case::StopDuringClock && io.entered.load(Ordering::Acquire) {
                explicit_stop = Some(stop(&x.f, &bind.request_id));
                break;
            }
            if case == Case::HeldClockExpiry && n.retired {
                assert_eq!(generation, 1, "late read renewed an expired predecessor");
                assert!(io.entered.load(Ordering::Acquire));
                break;
            }
            assert!(!n.retired, "continuity retired: {clock:?}");
            if case == Case::TwoTransfers && generation >= 3 {
                break;
            }
            assert!(
                Instant::now() < until,
                "fresh clock generations did not arrive: {clock:?}"
            );
            let next = index + 2;
            let next_us = input["frames"][next]["source_time_us"].as_u64().unwrap();
            // Held-Append keeps enough actual future frames buffered across
            // the original command's expiry. Other cases continue
            // real 200ms source pairs at ~5Hz from the actual G upload. Waiting
            // to return to exactly 1s lead after a transfer burst can instead
            // cross the unchanged 500ms native pair-gap limit and only reseed.
            let upload_due = if case == Case::HeldAppendExpiry {
                visible.origin.elapsed().as_micros() + 2_500_000 >= u128::from(next_us)
            } else {
                last_upload_started
                    .expect("actual successor G was uploaded")
                    .elapsed()
                    >= Duration::from_millis(200)
            };
            if held_append.is_none() && upload_due {
                assert!(
                    stamps.len() < 32,
                    "authored peer ring exhausted before required evidence"
                );
                index = next;
                frame += 1;
                let before_upload = capture_status(&x.f, &capture.identity.epoch);
                let lookahead = &capture.options.browser_analysis.as_ref().unwrap().policy;
                let scheduling_lead_us = i128::from(next_us)
                    - i128::try_from(visible.origin.elapsed().as_micros()).unwrap();
                assert!(scheduling_lead_us >= i128::from(lookahead.minimum_lead_us)
                    && scheduling_lead_us <= i128::from(lookahead.maximum_lead_us),
                    "actual next frame left unchanged scheduling lookahead: lead_us={scheduling_lead_us}, policy={lookahead:?}");
                let upload_started = Instant::now();
                terminal = upload_original(&x.f, &capture, &input, &pixels, index, frame);
                last_upload_started = Some(upload_started);
                let after_upload = capture_status(&x.f, &capture.identity.epoch);
                save(
                    &output,
                    &format!("post-transfer-upload-{frame}.json"),
                    &json!({
                        "epoch": capture.identity.epoch,
                        "expected_accepted": frame,
                        "previous_header": before_upload.last_frame,
                        "uploaded_header": terminal,
                        "capture_tick_gap_ns": before_upload.last_frame.as_ref().map(|previous|
                            terminal.capture_tick.0.saturating_sub(previous.capture_tick.0)),
                        "before_received": before_upload.received_frames,
                        "before_inference": before_upload.inference,
                        "after_received": after_upload.received_frames,
                        "after_inference": after_upload.inference,
                        "after_queue": after_upload.queue,
                    }),
                );
                assert_original_pair_gap(
                    before_upload
                        .last_frame
                        .as_ref()
                        .expect("original accepted predecessor header"),
                    &terminal,
                );
                let result = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(frame));
                stamps.push(actual_stamp(
                    &x.f,
                    &result,
                    &input,
                    &pixels,
                    index - 2,
                    index,
                ));
            }
            std::thread::park_timeout(Duration::from_millis(2));
        }
        save(&output, "clock-generations.json", &clocks);
        save(&output, "actual-cpu-samples.json", &stamps);
        save(&output, "original-transfer-requests.json", &transfers);
        if case == Case::TwoTransfers {
            assert_eq!(transfers.len(), 2);
            assert!(generation >= 3);
            assert!(io.reads.load(Ordering::Acquire) >= 2);
            assert!(
                media_opened_at.elapsed()
                    > Duration::from_millis(u64::from(original_media.lease_ms)),
                "original media lease was never crossed"
            );
            let before_close = live_status(&x.f, &bind.request_id);
            assert_eq!(before_close.origin, original_origin);
            assert!(!before_close.native.as_ref().unwrap().retired);
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
            capture_report(x.f.engine.handle(close).result.unwrap());
        }
        let until = Instant::now() + WAIT;
        let stopped = loop {
            if case == Case::TwoTransfers {
                visible.observe(&x.f);
            }
            let status = live_status(&x.f, &bind.request_id);
            let n = native_status(&status);
            if n.runtime_state == HandyHspRuntimeState::StopAcknowledged
                && !n.native_active
                && !n.metadata_pending
            {
                break status;
            }
            assert!(
                Instant::now() < until,
                "owned Stop did not settle: {status:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        };
        if case == Case::TwoTransfers {
            let finite = native_status(&stopped).finite_drain.as_ref().unwrap();
            assert_eq!(finite.input_state, HandyHspInputState::TailReached);
            assert!(!finite.aborted && !finite.stop_deadline_missed);
        }
        if case == Case::StopDuringTransfer {
            let until = Instant::now() + WAIT;
            let aborted = loop {
                let report = transfer_status(&x.f, transfers.last().unwrap());
                assert_ne!(report.phase, BrowserAnalysisTransferPhase::Transferred);
                if report.phase == BrowserAnalysisTransferPhase::Aborted {
                    break report;
                }
                assert!(
                    Instant::now() < until,
                    "Stop did not abort pending transfer: {report:?}"
                );
                std::thread::park_timeout(Duration::from_millis(2));
            };
            assert!(!aborted.successor_pair_accepted);
            save(&output, "transfer-stopped-without-promotion.json", &aborted);
        }
        if case == Case::HeldAppendExpiry {
            assert!(append_hold_armed && append_hold_released && generation >= 2);
            let original = held_append.as_ref().unwrap();
            let final_effect = latest_append(
                &x.f,
                original_origin.as_ref().unwrap().stream_record_id.as_str(),
            );
            assert_eq!(final_effect.id, original.id);
            assert_eq!(final_effect.record, original.record);
            assert_eq!(
                final_effect.state,
                HspJournalState::Unknown,
                "late held acknowledgment cannot restore the original effect"
            );
            let journal = x.f.engine._device_effect_journal.lock().unwrap();
            let stream = journal
                .hsp_stream(original_origin.as_ref().unwrap().stream_record_id.as_str())
                .unwrap();
            assert_eq!(stream.state, HspStreamJournalState::Custody);
            assert_eq!(
                stream
                    .effect_ids
                    .iter()
                    .filter(|id| **id == original.id)
                    .count(),
                1
            );
            drop(journal);
            save(
                &output,
                "held-append-unknown-after-stop.json",
                &effect_value(&final_effect),
            );
        }
        io.release.store(true, Ordering::Release);
        let until = Instant::now() + WAIT;
        let final_clock = loop {
            let status = continuity_status(&x.f, &enable.request_id);
            let n = status.native.as_ref().unwrap();
            if n.worker_joined && !n.worker_active {
                break status;
            }
            assert!(
                Instant::now() < until,
                "owned clock read did not join: {status:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        };
        assert_eq!(final_clock.history, original_history);
        if matches!(case, Case::HeldClockExpiry | Case::StopDuringClock) {
            assert!(io.entered.load(Ordering::Acquire));
            assert_eq!(final_clock.native.as_ref().unwrap().generation, 1);
        }
        if case == Case::HeldClockExpiry {
            assert!(
                crate::handy_clock::boottime_ns().unwrap()
                    >= original_history
                        .as_ref()
                        .unwrap()
                        .initial_clock_valid_until_boottime_ns
            );
            assert!(
                final_clock.native.as_ref().unwrap().error.is_some() || final_clock.error.is_some()
            );
        }

        if case == Case::HeldClockExpiry {
            assert_eq!(final_clock.native.as_ref().unwrap().generation, 1);
        }
        save(&output, "clock-final-joined.json", &final_clock);
        save(&output, "stopped.json", &stopped);
        let remote_requests = peer.state.lock().unwrap().requests.clone();
        let added_points: Vec<Value> = remote_requests
            .iter()
            .filter(|(method, path, _)| method == "PUT" && path == "/hsp/add")
            .flat_map(|(_, _, body)| {
                serde_json::from_slice::<Value>(body).unwrap()["points"]
                    .as_array()
                    .unwrap()
                    .clone()
            })
            .collect();
        if case == Case::TwoTransfers {
            assert_eq!(
                added_points.len(),
                stamps.len(),
                "every original CPU result appears exactly once on the remote stream"
            );
            let source_origin = input["frames"][start + 2]["source_time_us"]
                .as_u64()
                .unwrap();
            for (point, stamp) in added_points.iter().zip(&stamps) {
                let decoded = stamp["decoded_current"].as_u64().unwrap() as usize;
                let original_time = input["frames"][decoded]["source_time_us"].as_u64().unwrap();
                assert_eq!(
                    point["t"].as_u64().unwrap(),
                    (original_time - source_origin) / 1000
                );
                assert_eq!(point["x"].as_u64().unwrap(), actual_position(stamp));
            }
        }
        save(&output, "remote-requests.json", &remote_requests);
        save(&output, "remote-original-points.json", &added_points);
        if let Some(original) = &held_append {
            let request = &original.record.request;
            let matching_puts = remote_requests
                .iter()
                .filter(|(method, path, body)| {
                    method == &request.method
                        && path == &request.path
                        && Some(body) == request.body.as_ref()
                })
                .count();
            assert_eq!(
                matching_puts, 1,
                "original expired Append must never retry or rebase"
            );
        }
        let puts_before_replay = peer.puts();
        assert_eq!(
            puts_before_replay
                .iter()
                .filter(|p| p.as_str() == "/hsp/setup")
                .count(),
            1
        );
        assert_eq!(
            puts_before_replay
                .iter()
                .filter(|p| p.as_str() == "/hsp/play")
                .count(),
            1
        );
        assert_eq!(
            puts_before_replay
                .iter()
                .filter(|p| p.as_str() == "/hsp/stop")
                .count(),
            1
        );
        assert_eq!(
            io.base.syncs.load(Ordering::Acquire),
            1,
            "refresh may not synchronize the device clock"
        );
        live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        live_report(x.f.engine.handle(play).result.unwrap());
        let ResponseBody::HandyHspContinuity(replayed_enable) =
            x.f.engine.handle(enable.clone()).result.unwrap()
        else {
            panic!("enable replay")
        };
        assert_eq!(replayed_enable.history, original_history);
        if let Some(stop) = explicit_stop {
            live_report(x.f.engine.handle(stop).result.unwrap());
        }
        assert_eq!(peer.puts(), puts_before_replay);
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
        save(&output, "device-dossier-still-pending.json", &device);
        save(&output, "source-dossier-still-pending.json", &source);
        for epoch in all_epochs {
            let result =
                x.f.engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        Some(1),
                        Command::StopLiveCapture { epoch },
                    ))
                    .result;
            if let Ok(body) = result {
                assert!(!capture_report(body).active);
            }
        }
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
            panic!("clock history restart")
        };
        assert_eq!(clock_replay.history, original_history);
        assert!(clock_replay.native.is_none());
        assert_eq!(peer.puts(), puts_before_replay);
        save(
            &output,
            "restart-replay.json",
            &json!({"live":replay,"continuity":clock_replay}),
        );
        let temp_tables:u64=reopened.db.lock().unwrap().query_row("SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name IN ('pulsar_test_hsp_device_acceptance','pulsar_test_hsp_source_acceptance')",[],|row|row.get(0)).unwrap();
        assert_eq!(temp_tables, 0);
        reopened.finish_shutdown().unwrap();
        Engine::retire_for_test(reopened);
        drop(dir);
        assert_eq!(artifact(&runtime), runtime_before);
        save(
            &output,
            "completion.json",
            &json!({"case":case.label(),"actual_cpu_points":stamps.len(),
            "committed_transfers":predecessor_epochs.len(),"clock_generation":generation,"clock_reads":io.reads.load(Ordering::Acquire),
            "saved_old_sink_replays":predecessor_epochs.len(),"expired_original_append":held_append.is_some(),"original_append_retried":false,
            "clock_read_entered":io.entered.load(Ordering::Acquire),"clock_worker_joined":true,
            "worker_joined":true,"delivery_settled":true,"restart_source_authority":false,
            "restart_clock_authority":false,"native_puts":puts_before_replay,
            "physical_device":false,"qualified":false}),
        );
    }

    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_continuity_two_actual_transfers_preserve_one_stream_and_final_tail() {
        run_continuity(Case::TwoTransfers)
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_continuity_held_clock_read_cannot_cross_original_expiry() {
        run_continuity(Case::HeldClockExpiry)
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_continuity_held_append_preserves_original_expiry_across_fresh_clock() {
        run_continuity(Case::HeldAppendExpiry)
    }

    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_continuity_stop_during_clock_read_joins_original_owner() {
        run_continuity(Case::StopDuringClock)
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_continuity_stop_during_transfer_never_promotes_successor() {
        run_continuity(Case::StopDuringTransfer)
    }
    include!("engine_handy_hsp_timing_copy_native_tests.rs");
    include!("engine_handy_hsp_producer_read_order_native_tests.rs");
    include!("engine_live_capture_basis_read_order_native_tests.rs");
}
