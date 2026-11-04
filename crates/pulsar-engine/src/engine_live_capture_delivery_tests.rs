// Authored CPU replies and real local source pins only. These tests establish
// delivery ownership, not source acquisition, learned quality or device behavior.
#[cfg(target_os = "linux")]
mod live_capture_delivery_tests {
    use super::*;
    use crate::authority::live_captures::{
        NativeAcceptedCapture, NativeCaptureSelection, NativeCaptureSink, NativeCaptureSubscription,
    };
    use std::sync::{atomic::AtomicUsize, Weak};
    use std::time::{Duration, Instant};

    struct DeliveryFixture {
        f: Fixture,
        capture: LiveCaptureReport,
        manifest: LiveInferenceManifest,
        media: BrowserMediaBinding,
        media_request: RequestId,
    }
    fn delivery_fixture() -> DeliveryFixture {
        delivery_fixture_declared(true)
    }
    fn delivery_fixture_declared(declared: bool) -> DeliveryFixture {
        let f = fixture();
        let path = f.dir.path().join("native-delivery-source.bin");
        fs::write(&path, b"authored byte-bound delivery fixture").unwrap();
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path },
            ))
            .result
            .unwrap()
        else {
            panic!("source");
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
                        source_end: SourceTimestamp::new(60, 1).unwrap(),
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
            panic!("catalog");
        };
        let prepare = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::PrepareBrowserMedia {
                engine_epoch: catalog.engine_epoch.clone(),
                selection: catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
                },
                lease_ms: 60_000,
                allow_declared_clock: true,
            },
        );
        f.engine.handle(prepare.clone()).result.unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let media = loop {
            let ResponseBody::BrowserMediaPreparation(report) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: prepare.request_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("preparation");
            };
            if report.state == BrowserMediaPreparationState::Ready {
                assert!(report.current);
                break report.binding.unwrap();
            }
            assert_eq!(
                report.state,
                BrowserMediaPreparationState::Pending,
                "{:?}",
                report.error
            );
            assert!(
                Instant::now() < end,
                "owned media preparation did not settle"
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        let player = HandyPlayerIdentity {
            launch_id: AttemptId::new("delivery-launch").unwrap(),
            page_id: AttemptId::new("delivery-page").unwrap(),
            player_id: AttemptId::new("delivery-player").unwrap(),
            source_id: AttemptId::new("delivery-source").unwrap(),
            generation: 1,
            time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        };
        let ResponseBody::BrowserTiming(timing) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::OpenBrowserTiming {
                    player: player.clone(),
                    browser_media: media.clone(),
                    policy: BrowserTimingPolicy {
                        probe_timeout_ms: 2000,
                        observation_timeout_ms: 5000,
                    },
                },
            ))
            .result
            .unwrap()
        else {
            panic!("timing");
        };
        let ResponseBody::BrowserTimingProbe(probe) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::IssueBrowserTimingProbe {
                    binding_id: timing.binding.binding_id.clone(),
                    player: player.clone(),
                    sequence: 1,
                },
            ))
            .result
            .unwrap()
        else {
            panic!("probe");
        };
        let ResponseBody::BrowserTiming(observed) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ObserveBrowserTiming {
                    binding_id: probe.binding_id.clone(),
                    player: player.clone(),
                    sequence: probe.sequence,
                    probe_id: probe.probe_id.clone(),
                    frame: BrowserTimingFrame {
                        sample: HandyPlayerSample {
                            browser_tick_us: 1_000_000,
                            media_time_us: 0,
                            presented_frames: 1,
                            playing: true,
                            seeking: false,
                            ended: false,
                            visible: true,
                            playback_rate_milli: 1000,
                        },
                        callback_tick_us: 1_000_000,
                        presentation_time_us: None,
                        expected_display_time_us: None,
                    },
                },
            ))
            .result
            .unwrap()
        else {
            panic!("observed timing");
        };
        assert!(observed.current);
        let mut options = capture_options();
        options.browser_media = Some(media.clone());
        options.browser_analysis = declared.then(|| BrowserAnalysisCaptureDeclaration {
            feed_id: ArtifactId::new("delivery-analysis-feed").unwrap(),
            player,
            timing_binding_id: timing.binding.binding_id.clone(),
            policy: BrowserAnalysisLookaheadPolicy {
                minimum_lead_us: 100_000,
                target_lead_us: 500_000,
                maximum_lead_us: 1_000_000,
            },
        });
        options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        options.exact_project_anchor = Some(ExactSeconds::new(0, 1).unwrap());
        // This authored fixture does not measure the live latency policy.
        options.policy.maximum_analysis_age_ns = 3_000_000_000;
        let capture = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(1),
                    Command::StartLiveCapture {
                        engine_epoch: catalog.engine_epoch,
                        options,
                    },
                ))
                .result
                .unwrap(),
        );
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 10,
        };
        let manifest = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("authored-delivery-analysis").unwrap(),
            identity: capture.identity.clone(),
            clock: capture.clock.clone(),
            model: ArtifactReference {
                path: "/authored/model".into(),
                identity: artifact.clone(),
            },
            runtime: ArtifactReference {
                path: "/authored/runtime".into(),
                identity: artifact.clone(),
            },
            worker: f.engine.binary_identity.clone(),
            profile: DeclaredInferenceProfile {
                schema_version: 1,
                model: artifact.clone(),
                runtime: artifact,
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
                minimum_memory_bytes: 134217728,
                minimum_output_bytes: 1048576,
                cpu_threads: 2,
                cuda_resources: None,
            },
            budget: ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: 1048576,
                wall_time_ms: 30000,
                cpu_threads: 2,
            },
        };
        manifest.validate().unwrap();
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .arm_for_test(&capture.identity.epoch, &manifest);
        DeliveryFixture {
            f,
            capture,
            manifest,
            media,
            media_request: prepare.request_id,
        }
    }
    impl DeliveryFixture {
        fn selection(&self) -> NativeCaptureSelection {
            NativeCaptureSelection {
                cpu_manifest_sha256: None,
                owner: self.f.session.clone(),
                identity: self.capture.identity.clone(),
                attempt_id: self.manifest.attempt_id.clone(),
                profile_sha256: crate::inference_profiles::profile_identity(&self.manifest.profile)
                    .unwrap()
                    .sha256,
                browser_media: self.media.clone(),
            }
        }
        fn offer(&self, frame: u64) {
            self.f
                .engine
                .live_bulk_upload(
                    &live_handshake(&self.f, &self.capture),
                    &live_header(frame, frame as u8),
                    &vec![frame as u8; LIVE_FRAME_BYTES],
                )
                .unwrap();
        }
        fn work(&self, sequence: u64) -> (crate::live_frames::LiveRgbWork, LiveInferenceReply) {
            let work = self
                .f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .take_for_test(&self.capture.identity.epoch);
            let artifact = |bytes: &[u8]| {
                let sha256 = format!("{:x}", Sha256::digest(bytes));
                ArtifactIdentity {
                    artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                    sha256,
                    byte_len: bytes.len() as u64,
                }
            };
            let reply = LiveInferenceReply::Estimated {
                attempt_id: self.manifest.attempt_id.clone(),
                identity: self.capture.identity.clone(),
                profile_sha256: self.selection().profile_sha256,
                input: LiveInferenceInput {
                    sequence,
                    pair: work.declaration(),
                    previous: artifact(work.previous()),
                    current: artifact(work.current()),
                },
                position: NormalizedPosition::new(0.25).unwrap(),
                pair_analysis_elapsed_ns: 1,
            };
            (work, reply)
        }
        fn complete(&self, sequence: u64) {
            let (work, reply) = self.work(sequence);
            assert!(self
                .f
                .engine
                .live_analysis_complete(
                    &self.capture.identity.epoch,
                    &self.manifest.attempt_id,
                    work,
                    reply
                )
                .unwrap());
        }
        fn status(&self) -> LiveCaptureReport {
            capture_report(
                self.f
                    .engine
                    .handle(request(
                        &self.f.session,
                        Some(&self.f.project),
                        Some(self.capture.identity.revision.value()),
                        Command::LiveCaptureStatus {
                            epoch: self.capture.identity.epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            )
        }
        fn subscribe(&self, sink: Arc<TestSink>) -> NativeCaptureSubscription {
            self.f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .register_native_delivery(self.selection(), sink)
                .unwrap()
        }
    }
    struct TestSink {
        engine: Weak<Engine>,
        offers: Mutex<Vec<NativeAcceptedCapture>>,
        retired: AtomicUsize,
        fail: bool,
    }
    impl TestSink {
        fn new(x: &DeliveryFixture, fail: bool) -> Arc<Self> {
            Arc::new(Self {
                engine: Arc::downgrade(&x.f.engine),
                offers: Mutex::new(Vec::new()),
                retired: AtomicUsize::new(0),
                fail,
            })
        }
    }
    impl NativeCaptureSink for TestSink {
        fn offer(&self, accepted: NativeAcceptedCapture) -> PResult<()> {
            let engine = self.engine.upgrade().unwrap();
            assert!(
                engine.db.try_lock().is_ok(),
                "delivery retained project DB guard"
            );
            assert!(
                engine.live_captures.try_lock().is_ok(),
                "delivery retained capture guard"
            );
            self.offers.lock().unwrap().push(accepted);
            if self.fail {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "authored consumer refusal",
                ));
            }
            Ok(())
        }
        fn retire(&self) {
            // Deliberately no Engine, capture or consumer-runtime lock.
            self.retired.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[test]
    fn live_native_delivery_is_direct_unlocked_and_preserves_original_pair_without_poll_replay() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        x.offer(0);
        x.offer(1);
        x.complete(1);
        assert_eq!(
            sink.offers.lock().unwrap().len(),
            1,
            "completion must directly call consumer"
        );
        let original = sink.offers.lock().unwrap()[0].clone();
        for _ in 0..3 {
            assert!(x.status().active);
            let db = x.f.engine.db.lock().unwrap();
            let registry = x.f.engine.live_captures.try_lock().unwrap();
            let snapshot = registry.native_source_snapshot(&subscription).unwrap();
            assert!(snapshot.source_media.current());
            assert_eq!(
                snapshot.latest.as_ref().unwrap().capture_boottime,
                original.capture_boottime
            );
            assert_eq!(
                snapshot.latest.as_ref().unwrap().accepted_boottime,
                original.accepted_boottime
            );
            assert_eq!(snapshot.latest.as_ref().unwrap().frame, original.frame);
            assert!(snapshot.lease_deadline_boottime_ns > snapshot.native_now.latest_ns);
            drop(registry);
            drop(db);
        }
        assert_eq!(
            sink.offers.lock().unwrap().len(),
            1,
            "status/snapshot replayed a sample"
        );
        x.offer(2);
        x.complete(2);
        let accepted = sink.offers.lock().unwrap();
        assert_eq!(accepted.len(), 2);
        assert!(accepted[1].continuous_from_previous);
        assert_eq!(accepted[1].previous_frame, accepted[0].frame);
        let LiveInferenceReply::Estimated { input, .. } = &accepted[1].estimate.result else {
            panic!();
        };
        assert_eq!(input.sequence, 2);
    }

    #[test]
    fn live_native_subscription_never_replays_existing_stamp_or_replaces_original_sink() {
        let x = delivery_fixture();
        x.offer(0);
        x.offer(1);
        x.complete(1);
        let sink = TestSink::new(&x, false);
        let selection = x.selection();
        let subscription = {
            let mut registry = x.f.engine.live_captures.lock().unwrap();
            assert!(registry
                .native_source_basis(&selection)
                .unwrap()
                .latest
                .is_some());
            let subscription = registry
                .register_native_delivery(selection.clone(), sink.clone())
                .unwrap();
            assert!(registry
                .register_native_delivery(selection.clone(), sink.clone())
                .unwrap()
                .current());
            assert_eq!(
                registry
                    .register_native_delivery(selection, TestSink::new(&x, false))
                    .unwrap_err()
                    .code,
                ErrorCode::RequestConflict
            );
            subscription
        };
        assert!(subscription.current());
        assert!(sink.offers.lock().unwrap().is_empty());
        x.offer(2);
        x.complete(2);
        assert_eq!(sink.offers.lock().unwrap().len(), 1);
        let LiveInferenceReply::Estimated { input, .. } =
            &sink.offers.lock().unwrap()[0].estimate.result
        else {
            panic!();
        };
        assert_eq!(input.sequence, 2);
    }

    #[test]
    fn live_native_subscription_requires_exact_actor_epoch_attempt_profile_and_browser_binding() {
        let x = delivery_fixture();
        for case in 0..5 {
            let mut selection = x.selection();
            match case {
                0 => selection.owner = SessionId::new("foreign").unwrap(),
                1 => selection.identity.source = SourceVersionId::new("foreign").unwrap(),
                2 => selection.attempt_id = AttemptId::new("foreign").unwrap(),
                3 => selection.profile_sha256 = "f".repeat(64),
                _ => selection.browser_media.lease_id = ArtifactId::new("foreign").unwrap(),
            }
            assert!(x
                .f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .register_native_delivery(selection, TestSink::new(&x, false))
                .is_err());
        }
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        let mut foreign = x.selection();
        foreign.owner = SessionId::new("foreign").unwrap();
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_source_basis(&foreign)
            .is_err());
        assert!(subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 0);
    }

    #[test]
    fn live_native_delivery_failure_retires_stream_consumer_without_poisoning_preview() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, true);
        let subscription = x.subscribe(sink.clone());
        x.offer(0);
        x.offer(1);
        x.complete(1);
        assert!(!subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
        x.offer(2);
        x.complete(2);
        assert_eq!(sink.offers.lock().unwrap().len(), 1);
        let report = x.status();
        assert!(report.active);
        assert_eq!(report.inference.unwrap().accepted, 2);
        subscription.retire();
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
    }

    #[test]
    fn live_native_subscription_does_not_retain_its_consumer_or_deliver_after_consumer_drop() {
        struct OwningSink {
            subscription: std::sync::OnceLock<NativeCaptureSubscription>,
            offers: Arc<AtomicUsize>,
            dropped: Arc<AtomicBool>,
        }
        impl NativeCaptureSink for OwningSink {
            fn offer(&self, _: NativeAcceptedCapture) -> PResult<()> {
                self.offers.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
            fn retire(&self) {}
        }
        impl Drop for OwningSink {
            fn drop(&mut self) {
                self.dropped.store(true, Ordering::Release);
            }
        }
        let x = delivery_fixture();
        let offers = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        let sink = Arc::new(OwningSink {
            subscription: std::sync::OnceLock::new(),
            offers: offers.clone(),
            dropped: dropped.clone(),
        });
        let probe = Arc::downgrade(&sink);
        let subscription =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .register_native_delivery(x.selection(), sink.clone())
                .unwrap();
        sink.subscription.set(subscription.clone()).unwrap();
        assert!(subscription.current());
        // This is the consumer -> subscription -> delivery ownership shape.
        // Both the Registry and this local subscription remain alive.
        drop(sink);
        assert!(probe.upgrade().is_none());
        assert!(dropped.load(Ordering::Acquire));
        assert!(!subscription.current());
        x.offer(0);
        x.offer(1);
        x.complete(1);
        assert_eq!(offers.load(Ordering::Acquire), 0);
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_source_snapshot(&subscription)
            .is_err());
        assert!(x.status().active);
        subscription.retire();
        assert!(probe.upgrade().is_none());
    }

    #[test]
    fn live_native_delivery_gap_retires_before_an_older_inflight_completion() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        x.offer(0);
        x.offer(1);
        let (work, reply) = x.work(1);
        let mut gap = live_header(2, 2);
        gap.source_time = SourceTimestamp::new(1, 1).unwrap();
        x.f.engine
            .live_bulk_upload(
                &live_handshake(&x.f, &x.capture),
                &gap,
                &vec![2; LIVE_FRAME_BYTES],
            )
            .unwrap();
        assert!(!subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
        assert!(x
            .f
            .engine
            .live_analysis_complete(
                &x.capture.identity.epoch,
                &x.manifest.attempt_id,
                work,
                reply
            )
            .unwrap());
        assert!(sink.offers.lock().unwrap().is_empty());
        assert!(x.status().active);
    }

    #[test]
    fn live_native_delivery_stale_completion_retires_without_delivery_or_preview_loss() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        x.offer(0);
        x.offer(1);
        let (work, reply) = x.work(1);
        // Advance both authored clocks consistently, preserving the original
        // origin and frame timestamps while exceeding this fixture's age.
        for _ in 0..16 {
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .age_for_test(&x.capture.identity.epoch);
        }
        assert!(x
            .f
            .engine
            .live_analysis_complete(
                &x.capture.identity.epoch,
                &x.manifest.attempt_id,
                work,
                reply
            )
            .unwrap());
        assert!(!subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
        assert!(sink.offers.lock().unwrap().is_empty());
        let report = x.status();
        assert!(report.active);
        assert_eq!(report.inference.unwrap().stale, 1);
    }

    #[test]
    fn live_native_delivery_stop_failure_clock_loss_and_lease_expiry_retire_original_subscription()
    {
        for case in 0..4 {
            let x = delivery_fixture();
            let sink = TestSink::new(&x, false);
            let subscription = x.subscribe(sink.clone());
            match case {
                0 => {
                    x.f.engine
                        .handle(request(
                            &x.f.session,
                            Some(&x.f.project),
                            Some(x.capture.identity.revision.value()),
                            Command::StopLiveCapture {
                                epoch: x.capture.identity.epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                }
                1 => {
                    x.f.engine
                        .live_analysis_failure_for_test(
                            &x.capture.identity.epoch,
                            &x.manifest.attempt_id,
                        )
                        .unwrap();
                }
                2 => {
                    x.f.engine
                        .live_captures
                        .lock()
                        .unwrap()
                        .suspend_for_test(&x.capture.identity.epoch);
                }
                _ => {
                    x.f.engine
                        .live_captures
                        .lock()
                        .unwrap()
                        .expire_for_test(&x.capture.identity.epoch);
                }
            }
            assert!(x
                .f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .native_source_snapshot(&subscription)
                .is_err());
            assert!(!subscription.current());
            assert_eq!(sink.retired.load(Ordering::Acquire), 1);
        }
    }

    #[test]
    fn live_native_delivery_source_release_and_entry_drop_retire_without_consumer_locks() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        let _consumer = sink.offers.lock().unwrap();
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::ReleaseBrowserMedia {
                    original_request_id: x.media_request.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_source_snapshot(&subscription)
            .is_err());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
        drop(_consumer);
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        drop(x);
        assert!(!subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
    }

    #[test]
    fn live_native_subscription_refuses_provisional_cuda_and_finishing_capture() {
        let x = delivery_fixture();
        let mut selection = x.selection();
        selection.profile_sha256 =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .cuda_for_test(&x.capture.identity.epoch);
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .register_native_delivery(selection, TestSink::new(&x, false))
            .is_err());
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        x.offer(0);
        x.offer(1);
        x.complete(1);
        x.f.engine
            .live_captures
            .lock()
            .unwrap()
            .cuda_for_test(&x.capture.identity.epoch);
        x.f.engine
            .live_captures
            .lock()
            .unwrap()
            .finishing_for_test(&x.capture.identity.epoch);
        assert!(!subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
    }

    #[test]
    fn live_capture_status_preserves_valid_timing_while_publication_registry_is_held() {
        let x = delivery_fixture();
        let declaration = x.capture.options.browser_analysis.as_ref().unwrap();
        let timing_status = request(
            &x.f.session,
            Some(&x.f.project),
            None,
            Command::BrowserTimingStatus {
                binding_id: declaration.timing_binding_id.clone(),
                player: declaration.player.clone(),
            },
        );
        let pin =
            x.f.engine
                .browser_timing_scope(&timing_status, &declaration.timing_binding_id)
                .unwrap();
        let before = pin
            .latest(x.f.engine.browser_timing_clock_for_test())
            .unwrap();
        let original = x.status();
        assert!(original.active);
        let status = request(
            &x.f.session,
            Some(&x.f.project),
            Some(x.capture.identity.revision.value()),
            Command::LiveCaptureStatus {
                epoch: x.capture.identity.epoch.clone(),
            },
        );
        // Passive capture validation and immutable timing reads do not acquire
        // publication serialization. Public handle() also performs a timing
        // sweep before returning, so its reply may correctly wait for this guard.
        let held = pin.hold_registry_for_test();
        let engine = x.f.engine.clone();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let _ = send.send(engine.sweep_live_captures());
            engine.handle(status)
        });
        let during = receive.recv_timeout(Duration::from_secs(1));
        let snapshot = crate::handy_clock::boottime_ns().and_then(|now| pin.try_snapshot(now));
        // Always release before joining or asserting, including timeout/panic.
        // The complete public status path keeps its real maintenance locking.
        drop(held);
        let joined = worker.join();
        assert!(joined.is_ok(), "capture status thread panicked");
        during
            .expect("passive capture validation waited for timing publication")
            .unwrap();
        assert_eq!(snapshot.unwrap(), before);
        let response = joined.unwrap();
        let observed = capture_report(response.result.unwrap());
        assert!(
            observed.active,
            "holding publication serialization retired valid capture authority"
        );
        assert_eq!(observed.retirement, None);
        assert_eq!(observed.identity, original.identity);
        assert_eq!(observed.clock, original.clock);
        assert_eq!(observed.options.lease_ms, original.options.lease_ms);
        assert_eq!(observed.received_frames, original.received_frames);
        let now = x.f.engine.browser_timing_clock_for_test();
        assert!(pin.current(now));
        assert_eq!(
            pin.latest(now).unwrap(),
            before,
            "a status read renewed the original observation or deadline"
        );
        assert!(x.status().active);

        // Stable-lifetime checking must still honor actual revocation.
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::ReleaseBrowserTiming {
                    binding_id: declaration.timing_binding_id.clone(),
                    player: declaration.player.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!pin.current(x.f.engine.browser_timing_clock_for_test()));
        let retired = x.status();
        assert!(!retired.active);
        assert_eq!(
            retired.retirement,
            Some(LiveCaptureRetirement::AuthorityLost)
        );
    }

    #[test]
    fn live_native_subscription_requires_native_analysis_pin_while_ordinary_preview_survives() {
        let x = delivery_fixture_declared(false);
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .register_native_delivery(x.selection(), TestSink::new(&x, false))
            .is_err());
        assert!(x.status().active);
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        let declaration = x.capture.options.browser_analysis.as_ref().unwrap();
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::ReleaseBrowserTiming {
                    binding_id: declaration.timing_binding_id.clone(),
                    player: declaration.player.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_source_snapshot(&subscription)
            .is_err());
        assert!(!subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
    }

    #[test]
    fn live_native_analysis_feed_cannot_continue_rebind_or_reuse_another_epoch() {
        let x = delivery_fixture();
        let continuation = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::ContinueLiveCapture {
                engine_epoch: x.capture.engine_epoch.clone(),
                predecessor: x.capture.identity.epoch.clone(),
                options: x.capture.options.clone(),
            },
        );
        assert_eq!(
            x.f.engine.handle(continuation).result.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(x.status().active);
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                Some(x.capture.identity.revision.value()),
                Command::StopLiveCapture {
                    epoch: x.capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        let start = |options| {
            request(
                &x.f.session,
                Some(&x.f.project),
                Some(1),
                Command::StartLiveCapture {
                    engine_epoch: x.capture.engine_epoch.clone(),
                    options,
                },
            )
        };
        assert_eq!(
            x.f.engine
                .handle(start(x.capture.options.clone()))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        let mut options = x.capture.options.clone();
        let declaration = options.browser_analysis.as_mut().unwrap();
        declaration.feed_id = ArtifactId::new("fresh-but-wrong-player").unwrap();
        declaration.player.player_id = AttemptId::new("foreign-player").unwrap();
        assert_eq!(
            x.f.engine.handle(start(options)).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
    }
    #[test]
    fn live_native_snapshot_survives_publication_guard_but_final_claim_waits() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        x.offer(0);
        x.offer(1);
        x.complete(1);
        let original =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .native_source_snapshot(&subscription)
                .unwrap();
        let timing = original.browser_timing.clone();
        let before = timing
            .try_snapshot(x.f.engine.browser_timing_clock_for_test())
            .unwrap();
        let held_registry = timing.hold_registry_for_test();
        let db = x.f.engine.db.lock().unwrap();
        let mut captures = x.f.engine.live_captures.lock().unwrap();
        captures.sweep(&db).unwrap();
        assert!(captures.native_source_snapshot(&subscription).is_ok());
        assert!(matches!(
            timing.try_claim_native(|| panic!("contended claim sampled clock")),
            Err(crate::authority::browser_timing::BrowserTimingClaimFailure::Contended)
        ));
        assert!(subscription.current());
        drop(held_registry);
        let claim = timing
            .try_claim_native(|| {
                let now = crate::handy_clock::boottime_ns()?;
                x.f.engine
                    .browser_timing_test_tick
                    .store(now, Ordering::Release);
                Ok(now)
            })
            .unwrap_or_else(|_| panic!("released original native timing claim"));
        captures.sweep(&db).unwrap();
        let current = captures
            .native_source_snapshot_claimed(&subscription, &claim)
            .unwrap();
        let observed = claim
            .observation_for(&timing, current.native_now.latest_ns)
            .unwrap();
        assert_eq!(observed, &before);
        assert_eq!(current.selection, original.selection);
        assert_eq!(
            current.lease_deadline_boottime_ns,
            original.lease_deadline_boottime_ns
        );
        assert_eq!(
            current.latest.as_ref().unwrap().capture_boottime,
            original.latest.as_ref().unwrap().capture_boottime
        );
        assert_eq!(
            current.latest.as_ref().unwrap().accepted_boottime,
            original.latest.as_ref().unwrap().accepted_boottime
        );
        assert!(subscription.current());
        assert_eq!(sink.offers.lock().unwrap().len(), 1);
        assert_eq!(sink.retired.load(Ordering::Acquire), 0);
        drop(claim);
        drop(captures);
        drop(db);
        assert!(x.status().active);
    }

    #[test]
    fn live_native_claimed_snapshot_rejects_edited_claim_metadata() {
        for edit_admission in [false, true] {
            let x = delivery_fixture();
            let sink = TestSink::new(&x, false);
            let subscription = x.subscribe(sink.clone());
            let timing =
                x.f.engine
                    .live_captures
                    .lock()
                    .unwrap()
                    .native_source_snapshot(&subscription)
                    .unwrap()
                    .browser_timing;
            let captures = x.f.engine.live_captures.lock().unwrap();
            let mut claim = timing
                .try_claim_native(|| {
                    let now = crate::handy_clock::boottime_ns()?;
                    x.f.engine
                        .browser_timing_test_tick
                        .store(now, Ordering::Release);
                    Ok(now)
                })
                .unwrap_or_else(|_| panic!("original timing claim"));
            if edit_admission {
                claim.admitted_at_ns += 1;
            } else {
                claim.observation.expires_at_boottime_ns += 1;
            }
            assert!(captures
                .native_source_snapshot_claimed(&subscription, &claim)
                .is_err());
            assert!(!subscription.current());
            assert_eq!(sink.retired.load(Ordering::Acquire), 1);
        }
    }

    #[test]
    fn live_native_claimed_snapshot_rejects_foreign_native_registry_claim() {
        let x = delivery_fixture();
        let other = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let other_sink = TestSink::new(&other, false);
        let subscription = x.subscribe(sink.clone());
        let other_subscription = other.subscribe(other_sink.clone());
        let other_timing = other
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_source_snapshot(&other_subscription)
            .unwrap()
            .browser_timing;
        let captures = x.f.engine.live_captures.lock().unwrap();
        let claim = other_timing
            .try_claim_native(|| {
                let now = crate::handy_clock::boottime_ns()?;
                other
                    .f
                    .engine
                    .browser_timing_test_tick
                    .store(now, Ordering::Release);
                Ok(now)
            })
            .unwrap_or_else(|_| panic!("foreign timing claim"));
        assert!(captures
            .native_source_snapshot_claimed(&subscription, &claim)
            .is_err());
        assert!(!subscription.current());
        assert!(other_subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
        assert_eq!(other_sink.retired.load(Ordering::Acquire), 0);
    }

    #[test]
    fn live_native_claimed_snapshot_checks_original_expiry_at_fresh_capture_time() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        let timing =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .native_source_snapshot(&subscription)
                .unwrap()
                .browser_timing;
        let mut captures = x.f.engine.live_captures.lock().unwrap();
        let claim = timing
            .try_claim_native(|| {
                let now = crate::handy_clock::boottime_ns()?;
                x.f.engine
                    .browser_timing_test_tick
                    .store(now, Ordering::Release);
                Ok(now)
            })
            .unwrap_or_else(|_| panic!("original timing claim"));
        let original_expiry = claim.observation.expires_at_boottime_ns;
        // Advance both native and capture fixture clocks equally; source lease
        // is still 30 seconds, but this original observation expires at 5.
        for _ in 0..26 {
            captures.age_for_test(&x.capture.identity.epoch);
        }
        assert!(captures
            .native_source_snapshot_claimed(&subscription, &claim)
            .is_err());
        assert_eq!(claim.observation.expires_at_boottime_ns, original_expiry);
        assert!(!subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
    }

    #[test]
    fn live_native_claim_contention_preserves_original_subscription_and_guard_availability() {
        let x = delivery_fixture();
        let sink = TestSink::new(&x, false);
        let subscription = x.subscribe(sink.clone());
        let timing =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .native_source_snapshot(&subscription)
                .unwrap()
                .browser_timing;
        let held = x.f.engine.browser_timing.lock().unwrap();
        let captures = x.f.engine.live_captures.lock().unwrap();
        let result = timing.try_claim_native(|| {
            let now = crate::handy_clock::boottime_ns()?;
            x.f.engine
                .browser_timing_test_tick
                .store(now, Ordering::Release);
            Ok(now)
        });
        assert!(matches!(
            result,
            Err(crate::authority::browser_timing::BrowserTimingClaimFailure::Contended)
        ));
        drop(result);
        drop(captures);
        assert!(x.f.engine.live_captures.try_lock().is_ok());
        assert!(x.f.engine.db.try_lock().is_ok());
        assert!(subscription.current());
        assert_eq!(sink.retired.load(Ordering::Acquire), 0);
        drop(held);
        let captures = x.f.engine.live_captures.lock().unwrap();
        let claim = timing
            .try_claim_native(|| {
                let now = crate::handy_clock::boottime_ns()?;
                x.f.engine
                    .browser_timing_test_tick
                    .store(now, Ordering::Release);
                Ok(now)
            })
            .unwrap_or_else(|_| panic!("released original timing claim"));
        assert!(captures
            .native_source_snapshot_claimed(&subscription, &claim)
            .is_ok());
    }

    include!("engine_live_capture_input_tests.rs");
    include!("engine_live_capture_successor_tests.rs");
}
