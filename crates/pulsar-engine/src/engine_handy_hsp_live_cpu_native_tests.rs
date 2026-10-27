// Included inside hsp_preparation_tests to reuse its localhost peer and native
// preparation IO. This case runs an actual confined CPU inference worker over
// ffmpeg-decoded sterile media. Browser callbacks are authored client inputs;
// this is not browser/RVFC provenance, learned quality or physical qualification.
mod live_cpu_native_tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Write;

    const ORT_SHA: &str = "b5c9d4f124d24707f514dad926dc181820807178855df1c528e3addb2dd0e6f7";
    const MODEL_SHA: &str = "9a10ed332a69f241c880c0ff8b08f8d234371e6702a59f868510fef7550078c8";
    const AGE_NS: u64 = 2_000_000_000;

    fn env_path(name: &str) -> PathBuf {
        let path = PathBuf::from(
            std::env::var_os(name).unwrap_or_else(|| panic!("explicit {name} required")),
        );
        assert!(path.is_absolute(), "{name} must be absolute");
        path
    }
    fn artifact(path: &std::path::Path) -> ArtifactIdentity {
        let bytes = fs::read(path).unwrap();
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }
    fn save(output: &std::path::Path, name: &str, value: &impl serde::Serialize) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))
            .unwrap();
        file.write_all(&serde_json::to_vec_pretty(value).unwrap())
            .unwrap();
        file.write_all(b"\n").unwrap();
        file.sync_all().unwrap();
    }
    fn input_path(input: &Value, name: &str) -> PathBuf {
        let path = PathBuf::from(input[name]["path"].as_str().unwrap());
        assert!(path.is_absolute());
        let pinned = artifact(&path);
        assert_eq!(pinned.sha256, input[name]["sha256"].as_str().unwrap());
        assert_eq!(pinned.byte_len, input[name]["byte_len"].as_u64().unwrap());
        path
    }
    struct ShutdownGuard(Option<Arc<Engine>>);
    impl ShutdownGuard {
        fn finish(&mut self) {
            self.0.as_ref().unwrap().finish_shutdown().unwrap();
            self.0 = None;
        }
    }
    impl Drop for ShutdownGuard {
        fn drop(&mut self) {
            if let Some(engine) = self.0.take() {
                if let Err(error) = engine.finish_shutdown() {
                    eprintln!("native CPU fixture shutdown retains unresolved state: {error}");
                }
            }
        }
    }
    fn native_fixture(worker: PathBuf) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(EngineConfig::new(dir.path().join("state"), worker)).unwrap();
        let session = pair(&engine, "actual CPU HSP integration fixture");
        let ResponseBody::Project(project) = engine
            .handle(request(
                &session,
                None,
                None,
                Command::CreateProject {
                    name: "sterile native CPU HSP integration".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("project creation");
        };
        Fixture {
            engine,
            session,
            project: project.project_id,
            dir,
        }
    }
    fn prepare_media(f: &Fixture, media_path: PathBuf) -> BrowserMediaBinding {
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
                lease_ms: 60_000,
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
    fn native_preparation(f: Fixture) -> (PreparationFixture, Peer, Arc<RealPreparationIo>) {
        let (path, mut input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Physical);
        // Separate authored native-composition contract, exactly as in the
        // existing real-clock preparation tests; no limits change on failure.
        let s = &mut input.subject;
        s.profile.maximum_command_age_ns = 10_000_000_000;
        s.profile.maximum_clock_uncertainty_ns = 100_000_000;
        s.server_clock.as_mut().unwrap().timestamp_error_ns = 1_000_000;
        let device = s.device_clock.as_mut().unwrap();
        device.maximum_sync_age_ns = 10_000_000_000;
        device.device_error_ns = 1_000;
        device.maximum_sync_round_trip_ms = 10;
        let p = policy();
        s.hsp.as_mut().unwrap().policy = HandyHspPolicy {
            max_buffered_points: p.max_buffered_points as u32,
            device_capacity_points: p.device_capacity_points as u32,
            max_chunk_points: p.max_chunk_points as u32,
            min_lead_ms: p.min_lead_ms,
            max_future_ms: p.max_future_ms,
            max_sample_age_ns: p.max_sample_age_ns,
            max_gap_ms: p.max_gap_ms,
        };
        let imported = qualification_saved(
            f.engine
                .handle(qualification_import(
                    &f,
                    "native-cpu-device-dossier",
                    &path,
                    &input,
                ))
                .result
                .unwrap(),
        );
        let (_, connection) = hsp_qualification_connection(&f);
        let accepted = qualification_saved(
            qualification_call(
                &f,
                &qualification_accept(&f, &imported.dossier_id, &connection),
                |_| Ok(hsp_qualification_sample()),
            )
            .unwrap(),
        );
        let peer = Peer::new(false);
        peer.state.lock().unwrap().native_session =
            accepted.manifest.subject.identity.session_id.clone();
        let io = Arc::new(RealPreparationIo {
            peer: peer.state.clone(),
            origin: peer.origin.clone(),
            identity: accepted.manifest.subject.identity.clone(),
            syncs: AtomicU64::new(0),
        });
        (
            PreparationFixture {
                f,
                accepted,
                connection,
            },
            peer,
            io,
        )
    }
    fn profile(model: &std::path::Path, runtime: ArtifactIdentity) -> DeclaredInferenceProfile {
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact(model),
            runtime,
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::NearestStretch,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0f32 / 255.0; 6],
                bias: vec![0.0; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 128 * 1024 * 1024,
            minimum_output_bytes: 1024 * 1024,
            cpu_threads: 2,
            cuda_resources: None,
        };
        profile.validate().unwrap();
        profile
    }
    struct AuthoredVisiblePlayer {
        binding: BrowserTimingBinding,
        origin: Instant,
        // Probe sequences belong to a timing binding; frame presentation
        // belongs to the one visible player shared by overlapping bindings.
        sequence: u64,
        presented_frames: Arc<AtomicU64>,
        last_tick_us: u64,
    }
    impl AuthoredVisiblePlayer {
        fn open(f: &Fixture, media: &BrowserMediaBinding) -> Self {
            let player = HandyPlayerIdentity {
                launch_id: AttemptId::new("native-cpu-authored-launch").unwrap(),
                page_id: AttemptId::new("native-cpu-authored-page").unwrap(),
                player_id: AttemptId::new("native-cpu-authored-visible").unwrap(),
                source_id: AttemptId::new("native-cpu-authored-source").unwrap(),
                generation: 1,
                time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
            };
            let ResponseBody::BrowserTiming(report) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(1),
                    Command::OpenBrowserTiming {
                        player,
                        browser_media: media.clone(),
                        policy: BrowserTimingPolicy {
                            probe_timeout_ms: 100,
                            observation_timeout_ms: 500,
                        },
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("browser timing open");
            };
            Self {
                binding: report.binding,
                origin: Instant::now(),
                sequence: 0,
                presented_frames: Arc::new(AtomicU64::new(0)),
                last_tick_us: 0,
            }
        }
        fn observe(&mut self, f: &Fixture) {
            self.sequence += 1;
            let ResponseBody::BrowserTimingProbe(probe) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::IssueBrowserTimingProbe {
                        binding_id: self.binding.binding_id.clone(),
                        player: self.binding.player.clone(),
                        sequence: self.sequence,
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("timing probe");
            };
            let presented_frames = self
                .presented_frames
                .try_update(Ordering::AcqRel, Ordering::Acquire, |frames| {
                    frames.checked_add(1)
                })
                .expect("authored visible frame counter exhausted")
                + 1;
            let media_us = u64::try_from(self.origin.elapsed().as_micros()).unwrap();
            let tick = 1_000_000 + media_us;
            assert!(tick > self.last_tick_us);
            self.last_tick_us = tick;
            let frame = BrowserTimingFrame {
                sample: HandyPlayerSample {
                    browser_tick_us: tick,
                    media_time_us: media_us,
                    presented_frames,
                    playing: true,
                    seeking: false,
                    ended: false,
                    visible: true,
                    playback_rate_milli: 1000,
                },
                callback_tick_us: tick,
                presentation_time_us: Some(tick),
                expected_display_time_us: None,
            };
            let ResponseBody::BrowserTiming(report) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::ObserveBrowserTiming {
                        binding_id: self.binding.binding_id.clone(),
                        player: self.binding.player.clone(),
                        sequence: probe.sequence,
                        probe_id: probe.probe_id,
                        frame,
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("timing observation");
            };
            assert!(report.current && report.observation.is_some());
        }
    }
    fn lookahead() -> BrowserAnalysisLookaheadPolicy {
        BrowserAnalysisLookaheadPolicy {
            target_lead_us: 10_000_000,
            minimum_lead_us: 5_000_000,
            maximum_lead_us: 15_000_000,
        }
    }
    fn source_dossier(
        x: &PreparationFixture,
        media: &BrowserMediaBinding,
        profile: &DeclaredInferenceProfile,
    ) -> (
        HandyHspSourceQualificationBinding,
        HandyHspSourceQualificationReport,
    ) {
        let mut evidence = Vec::new();
        let mut environment = None;
        for kind in HandyHspSourceEvidenceKind::ALL {
            let path = x.f.dir.path().join(format!("native-source-{kind:?}.txt"));
            fs::write(&path, format!("AUTHORED SOFTWARE FIXTURE: {kind:?}. Direct ingress of actual ffmpeg-decoded sterile RGB; authored visible timing, no browser capture provenance or physical qualification.\n")).unwrap();
            let id = artifact(&path);
            if kind == HandyHspSourceEvidenceKind::EnvironmentConfiguration {
                environment = Some(id.clone());
            }
            evidence.push(HandyHspSourceQualificationInputEvidence {
                kind,
                path,
                sha256: id.sha256,
                byte_len: id.byte_len,
            });
        }
        let input = HandyHspSourceQualificationInput {
            subject: HandyHspSourceSubject {
                cpu_runtime_image: None,
                finite_drain_policy_sha256: None,
                schema_version: 1,
                origin: HandyEvidenceOrigin::Synthetic,
                context: x.accepted.manifest.subject.context.clone(),
                device_qualification: HandyQualificationBinding {
                    dossier_id: x.accepted.dossier_id.clone(),
                    revision: x.accepted.revision,
                },
                browser_environment: environment.unwrap(),
                project_id: x.f.project.clone(),
                revision: RevisionId::new(1),
                selection: media.selection.clone(),
                clock: media.clock,
                inference_profile: profile.clone(),
                source_time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
                recipe: HandyHspSourceRecipe::RvfcRgb64x64NearestStretchV1,
                age_basis: LiveCaptureAgeBasis::ProducerDeclared,
                age_observation: HandyHspSourceAgeObservation::HttpReceiptMonotonic,
                lookahead: lookahead(),
                maximum_analysis_age_ns: AGE_NS,
                maximum_capture_to_engine_receipt_ns: 50_000_000,
                timing: HandyBrowserStartTiming {
                    clock: BrowserClockAssumptions {
                        timestamp_error_ns: 1_000,
                        relative_rate_error_ppm: 20,
                        maximum_age_ns: 1_000_000_000,
                        maximum_interval_ns: 100_000_000,
                    },
                    reference: HandyBrowserStartReference::Callback,
                    display_early_ns: 2_000_000,
                    display_late_ns: 3_000_000,
                    source_correspondence_error_ns: 1_000_000,
                    discrete_media_phase_error_ns: 1_000_000,
                    media_rate_error_ppm: 20,
                    physical_execution_error_ns: 1_000_000,
                    maximum_start_age_ns: 500_000_000,
                    maximum_alignment_error_ns: 50_000_000,
                },
            },
            evidence,
        };
        input.validate().unwrap();
        let path = x.f.dir.path().join("native-source-dossier.json");
        let bytes = serde_json::to_vec(&input).unwrap();
        fs::write(&path, &bytes).unwrap();
        let ResponseBody::HandyHspSourceQualificationSaved(receipt) =
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(1),
                    Command::ImportHandyHspSourceQualification {
                        path,
                        manifest_sha256: format!("{:x}", Sha256::digest(&bytes)),
                        owner_proof: x.f.engine.handy_owner_token.clone(),
                    },
                ))
                .result
                .unwrap()
        else {
            panic!("source import");
        };
        assert_eq!(receipt.report.state, HandyQualificationState::Pending);
        assert_eq!(
            receipt.report.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let binding =
            handy_hsp_source_qualification::install_authored_hsp_source_acceptance_for_test(
                &x.f.engine.db.lock().unwrap(),
                &receipt.report.dossier_id,
            )
            .unwrap();
        (binding, receipt.report)
    }
    fn capture_status(f: &Fixture, epoch: &AttemptId) -> LiveCaptureReport {
        capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(1),
                    Command::LiveCaptureStatus {
                        epoch: epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    }
    fn wait_capture(
        f: &Fixture,
        visible: &mut AuthoredVisiblePlayer,
        epoch: &AttemptId,
        accepted: Option<u64>,
    ) -> LiveCaptureReport {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visible.observe(f);
            let report = capture_status(f, epoch);
            let inference = report.inference.as_ref().unwrap();
            assert!(report.active, "capture retired: {report:?}");
            if inference.state == LiveCaptureInferenceState::Running
                && accepted.is_none_or(|n| inference.accepted == n)
            {
                return report;
            }
            assert!(
                Instant::now() < deadline,
                "actual CPU inference did not reach expected state: {report:?}"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        }
    }
    fn push_decoded(
        f: &Fixture,
        report: &LiveCaptureReport,
        input: &Value,
        pixels: &[u8],
        decoded_index: usize,
        frame: u64,
    ) {
        let declared = &input["frames"][decoded_index];
        assert_eq!(declared["index"].as_u64().unwrap(), decoded_index as u64);
        let offset = declared["offset"].as_u64().unwrap() as usize;
        let bytes = &pixels[offset..offset + LIVE_FRAME_BYTES];
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            declared["sha256"].as_str().unwrap()
        );
        let current = capture_status(f, &report.identity.epoch);
        let header = LiveFrameHeader {
            frame: FrameId::new(frame),
            source_time: SourceTimestamp::new(
                declared["source_time_us"].as_i64().unwrap(),
                1_000_000,
            )
            .unwrap(),
            capture_tick: current.server_elapsed.unwrap(),
            sha256: declared["sha256"].as_str().unwrap().into(),
        };
        let handshake = live_handshake(f, &current);
        f.engine.live_bulk_ready(&handshake, Some(&header)).unwrap();
        let (_, replay) = f
            .engine
            .live_bulk_upload(&handshake, &header, bytes)
            .unwrap();
        assert!(!replay);
    }
    fn live_report(body: ResponseBody) -> HandyHspLiveStatus {
        let ResponseBody::HandyHspLive(report) = body else {
            panic!("live HSP report");
        };
        report.validate().unwrap();
        assert!(!report.qualified);
        *report
    }
    fn live_status(f: &Fixture, id: &RequestId) -> HandyHspLiveStatus {
        live_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::HandyHspLiveStatus {
                        stream_request_id: id.clone(),
                        owner_proof: f.engine.handy_owner_token.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    }
    fn wait_live(
        f: &Fixture,
        visible: &mut AuthoredVisiblePlayer,
        id: &RequestId,
        predicate: impl Fn(&HandyHspLiveStatus) -> bool,
    ) -> HandyHspLiveStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            visible.observe(f);
            let report = live_status(f, id);
            if predicate(&report) {
                return report;
            }
            assert!(
                Instant::now() < deadline,
                "live HSP controller did not settle: {report:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }
    fn accepted_stamp(
        f: &Fixture,
        capture: &LiveCaptureReport,
        declared: &Value,
        pixels: &[u8],
        decoded_index: usize,
    ) -> Value {
        let registry = f.engine.live_captures.lock().unwrap();
        let stamp = registry
            .native_accepted(&f.session, &capture.identity)
            .unwrap()
            .unwrap();
        let LiveInferenceReply::Estimated {
            input, position, ..
        } = &stamp.estimate.result
        else {
            panic!("actual CPU result required");
        };
        assert_eq!(
            input.previous.sha256,
            declared["frames"][decoded_index - 1]["sha256"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            input.current.sha256,
            declared["frames"][decoded_index]["sha256"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            stamp.frame.source_time,
            SourceTimestamp::new(
                declared["frames"][decoded_index]["source_time_us"]
                    .as_i64()
                    .unwrap(),
                1_000_000
            )
            .unwrap()
        );
        let offset = (decoded_index - 1) * LIVE_FRAME_BYTES;
        let pair = &pixels[offset..offset + 2 * LIVE_FRAME_BYTES];
        let sum: u64 = pair.iter().map(|byte| u64::from(*byte)).sum();
        let diagnostic_expected = 0.25 + 0.5 * (sum as f64 / (pair.len() as f64 * 255.0));
        assert!((position.value()-diagnostic_expected).abs() < 0.0001,
            "actual authored graph output differs from its decoded RGB input: {} vs {diagnostic_expected}", position.value());
        let bounds = |value: &live_captures::BootBounds| json!({"earliest_ns":value.earliest_ns,"latest_ns":value.latest_ns});
        json!({"previous_frame":stamp.previous_frame,"frame":stamp.frame,"estimate":stamp.estimate,
            "previous_capture_boottime":bounds(&stamp.previous_capture_boottime),"capture_boottime":bounds(&stamp.capture_boottime),
            "accepted_boottime":bounds(&stamp.accepted_boottime),"continuity_generation":stamp.continuity_generation,
            "continuous_from_previous":stamp.continuous_from_previous,"authored_graph_expected_position":diagnostic_expected,
            "decoded_fixture_index":decoded_index})
    }

    #[test]
    #[ignore = "requires explicit pinned CPU runtime, rebuilt worker, decoded sterile fixture, fresh evidence output and delegated native confinement; run serially"]
    fn native_cpu_decoded_frames_drive_hsp_live_controller_without_injected_source_facts() {
        let fixture_path = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        let output = env_path("PULSAR_HSP_LIVE_NATIVE_OUTPUT");
        if output.exists() {
            assert!(fs::read_dir(&output).unwrap().next().is_none());
        } else {
            fs::create_dir(&output).unwrap();
        }
        let input: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();
        assert_eq!(input["schema_version"], 1);
        assert_eq!(input["frame_bytes"], LIVE_FRAME_BYTES);
        let media_path = input_path(&input, "media");
        let pixels_path = input_path(&input, "rgb");
        let model = input_path(&input, "model");
        assert_eq!(artifact(&model).sha256, MODEL_SHA);
        assert_eq!(artifact(&runtime).sha256, ORT_SHA);
        save(
            &output,
            "subjects.json",
            &json!({"fixture":artifact(&fixture_path),"worker":artifact(&worker),"runtime":artifact(&runtime),
            "input":input,"authored_model":true,"browser_capture":false,"physical_device":false,"qualified":false}),
        );
        let pixels = fs::read(&pixels_path).unwrap();
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
        assert_eq!(runtime_identity.sha256, ORT_SHA);
        let profile = profile(&model, runtime_identity);
        let (x, peer, preparation_io) = native_preparation(f);
        let mut shutdown = ShutdownGuard(Some(x.f.engine.clone()));
        let (source_qualification, pending_source_report) = source_dossier(&x, &media, &profile);
        save(
            &output,
            "source-dossier-pending.json",
            &pending_source_report,
        );
        save(&output, "profile.json", &profile);
        let mut visible = AuthoredVisiblePlayer::open(&x.f, &media);
        visible.observe(&x.f);
        let mut options = capture_options();
        options.browser_media = Some(media.clone());
        options.browser_analysis = Some(BrowserAnalysisCaptureDeclaration {
            feed_id: ArtifactId::new("native-cpu-authored-analysis-feed").unwrap(),
            player: visible.binding.player.clone(),
            timing_binding_id: visible.binding.binding_id.clone(),
            policy: lookahead(),
        });
        options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        options.source_anchor = SourceTimestamp::new(10, 1).unwrap();
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
        let start = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::StartLiveCapture {
                engine_epoch: media.engine_epoch.clone(),
                options,
            },
        );
        let capture = capture_report(x.f.engine.handle(start).result.unwrap());
        wait_capture(&x.f, &mut visible, &capture.identity.epoch, None);
        let mut preparation = x.prepare("actual-cpu-hsp-preparation");
        preparation.expected_revision = Some(RevisionId::new(1));
        x.call(&preparation, preparation_io.clone()).unwrap();
        let preparation_deadline = Instant::now() + WAIT;
        loop {
            visible.observe(&x.f);
            let prepared = x.status(&preparation.request_id);
            if prepared.native.as_ref().is_some_and(|n| {
                n.phase == HandyHspPreparationPhase::Ready
                    && !n.worker_active
                    && !n.metadata_pending
            }) {
                break;
            }
            assert!(
                Instant::now() < preparation_deadline,
                "native preparation did not settle: {prepared:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
        visible.observe(&x.f);
        push_decoded(&x.f, &capture, &input, &pixels, 100, 0);
        push_decoded(&x.f, &capture, &input, &pixels, 101, 1);
        let first = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(1));
        save(
            &output,
            "first-actual-acceptance.json",
            &accepted_stamp(&x.f, &first, &input, &pixels, 101),
        );
        let attempt = first.inference.as_ref().unwrap().attempt_id.clone();
        let bind = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::BindHandyHspLive {
                selection: HandyHspLiveSelection {
                    finite_drain_policy_sha256: None,
                    preparation_request_id: preparation.request_id.clone(),
                    capture_epoch: capture.identity.epoch.clone(),
                    inference_attempt_id: attempt,
                    browser_timing: visible.binding.binding_id.clone(),
                    source_qualification,
                },
                acknowledge_current_environment: true,
                allow_online_control: true,
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        let mut unacknowledged = bind.clone();
        let Command::BindHandyHspLive {
            acknowledge_current_environment,
            ..
        } = &mut unacknowledged.command
        else {
            unreachable!()
        };
        *acknowledge_current_environment = false;
        let refused = x.f.engine.handle(unacknowledged).result.unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden);
        assert!(refused
            .message
            .contains("current environment acknowledgment"));
        assert!(
            peer.puts().is_empty(),
            "unacknowledged applicability sent a native effect"
        );
        let reserved: u64 =
            x.f.engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM requests WHERE session=?1 AND request=?2",
                    params![x.f.session.as_str(), bind.request_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(
            reserved, 0,
            "unacknowledged applicability reserved the original request"
        );
        assert!(
            x.status(&preparation.request_id)
                .history
                .unwrap()
                .linked_stream_id
                .is_none(),
            "unacknowledged applicability consumed preparation readiness"
        );
        save(&output, "bind-environment-refusal.json", &refused);
        let bound = live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        save(&output, "bind-initial.json", &bound);
        let bound = wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            r.stream
                .as_ref()
                .and_then(|s| s.native.as_ref())
                .is_some_and(|n| n.post_setup_bound && !n.native_active && !n.metadata_pending)
        });
        assert_eq!(bound.native.as_ref().unwrap().delivered_samples, 1);
        assert_eq!(peer.puts(), ["/hsp/setup"]);
        let origin = bound.origin.as_ref().unwrap().clone();
        assert_eq!(origin.frame, FrameId::new(1));
        assert_eq!(origin.source_sequence, 1);
        let replay = live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        assert_eq!(replay.origin, bound.origin);
        assert_eq!(peer.puts(), ["/hsp/setup"]);
        push_decoded(&x.f, &capture, &input, &pixels, 102, 2);
        let second = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(2));
        save(
            &output,
            "second-actual-acceptance.json",
            &accepted_stamp(&x.f, &second, &input, &pixels, 102),
        );
        let prefilled = wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            r.stream
                .as_ref()
                .and_then(|s| s.native.as_ref())
                .is_some_and(|n| n.total_added == 2 && !n.native_active && !n.metadata_pending)
        });
        assert_eq!(prefilled.native.as_ref().unwrap().delivered_samples, 2);
        assert_eq!(peer.puts(), ["/hsp/setup", "/hsp/add"]);
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
            r.stream
                .as_ref()
                .and_then(|s| s.native.as_ref())
                .is_some_and(|n| {
                    n.runtime_state == HandyHspRuntimeState::Playing
                        && !n.native_active
                        && !n.metadata_pending
                })
        });
        push_decoded(&x.f, &capture, &input, &pixels, 103, 3);
        let third = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(3));
        save(
            &output,
            "third-actual-acceptance.json",
            &accepted_stamp(&x.f, &third, &input, &pixels, 103),
        );
        let appended = wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            r.stream
                .as_ref()
                .and_then(|s| s.native.as_ref())
                .is_some_and(|n| n.total_added == 3 && !n.native_active && !n.metadata_pending)
        });
        save(&output, "playing-appended.json", &appended);
        let fence_before: Value = serde_json::from_slice(
            &fs::read(x.f.engine.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        assert!(
            !fence_before["groups"].as_object().unwrap().is_empty(),
            "actual worker must own a confined process group"
        );
        save(&output, "worker-fence-active.json", &fence_before);
        let stop = request(
            &x.f.session,
            Some(&x.f.project),
            None,
            Command::StopHandyHspLive {
                stream_request_id: bind.request_id.clone(),
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        live_report(x.f.engine.handle(stop.clone()).result.unwrap());
        let stopped = wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            r.stream
                .as_ref()
                .and_then(|s| s.native.as_ref())
                .is_some_and(|n| {
                    n.runtime_state == HandyHspRuntimeState::StopAcknowledged
                        && !n.native_active
                        && !n.metadata_pending
                })
        });
        assert!(stopped.native.as_ref().unwrap().retired);
        assert_eq!(
            peer.puts(),
            [
                "/hsp/setup",
                "/hsp/add",
                "/hsp/play",
                "/hsp/add",
                "/hsp/stop"
            ]
        );
        assert_eq!(preparation_io.syncs.load(Ordering::Acquire), 1);
        let stream_id = stopped.stream.as_ref().unwrap().stream_record_id.clone();
        let (stream_record, effects, emergency) = {
            let journal = x.f.engine._device_effect_journal.lock().unwrap();
            let stream = journal.hsp_stream(stream_id.as_str()).unwrap();
            let effects = stream.effect_ids.iter().map(|id| {
                let effect = journal.hsp_effect(id).unwrap();
                json!({"id":effect.id,"state":format!("{:?}",effect.state),"record":effect.record,"outcome":effect.outcome,"late_observation":effect.late_observation})
            }).collect::<Vec<_>>();
            let emergency = journal
                .hsp_emergency_for_stream(stream_id.as_str())
                .unwrap();
            (stream.record, effects, emergency)
        };
        let appends = effects
            .iter()
            .filter(|e| e["record"]["kind"] == "append")
            .collect::<Vec<_>>();
        assert_eq!(appends.len(), 2);
        let first_sample = &appends[0]["record"]["chunk"]["samples"][0];
        assert_eq!(first_sample["time_ms"], 0);
        assert_eq!(first_sample["source_sequence"], origin.source_sequence);
        assert_eq!(
            first_sample["position_percent"],
            (origin.first_position.value() * 100.0).round() as u64
        );
        assert_eq!(appends[0]["record"]["chunk"]["samples"][1]["time_ms"], 100);
        assert_eq!(appends[1]["record"]["chunk"]["samples"][0]["time_ms"], 200);
        save(
            &output,
            "journal-history.json",
            &json!({"stream":stream_record,"effects":effects,"emergency":emergency}),
        );
        save(
            &output,
            "transport.json",
            &peer.state.lock().unwrap().requests,
        );
        save(&output, "stopped.json", &stopped);
        live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        live_report(x.f.engine.handle(play.clone()).result.unwrap());
        live_report(x.f.engine.handle(stop.clone()).result.unwrap());
        assert_eq!(
            peer.puts().len(),
            5,
            "exact replays cannot create another command"
        );
        let fresh_play = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::PlayHandyHspLive {
                stream_request_id: bind.request_id.clone(),
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        assert!(x.f.engine.handle(fresh_play).result.is_err());
        let stopped_capture = capture_report(
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
        assert!(!stopped_capture.active);
        shutdown.finish();
        let fence_after: Value = serde_json::from_slice(
            &fs::read(x.f.engine.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        for key in ["groups", "retired", "staging"] {
            assert!(
                fence_after[key].as_object().unwrap().is_empty(),
                "unjoined {key}"
            );
        }
        assert!(fence_after["creation"]["records"]
            .as_object()
            .unwrap()
            .is_empty());
        save(&output, "worker-fence-joined.json", &fence_after);
        assert_eq!(Arc::strong_count(&x.f.engine), 1);
        let PreparationFixture { f, .. } = x;
        let Fixture {
            engine,
            session,
            project,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let reopened = Engine::open(config.clone()).unwrap();
        let replay = live_report(reopened.handle(bind.clone()).result.unwrap());
        assert_eq!(replay.origin, stopped.origin);
        assert!(
            replay.native.is_none(),
            "restart must not restore source controller authority"
        );
        assert_eq!(peer.puts().len(), 5);
        save(&output, "restart-replay.json", &replay);
        reopened.finish_shutdown().unwrap();
        Engine::retire_for_test(reopened);
        let original_response: String = {
            let db = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
            db.query_row(
                "SELECT response FROM requests WHERE session=?1 AND request=?2",
                params![session.as_str(), bind.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap()
        };
        let original_response_sha = format!("{:x}", Sha256::digest(original_response.as_bytes()));
        save(
            &output,
            "startup-original-response.json",
            &serde_json::from_str::<Value>(&original_response).unwrap(),
        );
        let mut journal_bytes = Vec::new();
        for name in ["journal.sqlite3", "journal.sqlite3-wal"] {
            let path = config.state_dir.join("device-effects").join(name);
            match fs::read(&path) {
                Ok(bytes) => {
                    let mut retained = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(output.join(format!("startup-original-{name}")))
                        .unwrap();
                    retained.write_all(&bytes).unwrap();
                    retained.sync_all().unwrap();
                    journal_bytes.push((path, bytes));
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound && name.ends_with("-wal") => {}
                Err(error) => panic!("cannot capture original journal {name}: {error}"),
            }
        }
        let original_journal = journal_bytes.iter().map(|(path, bytes)| json!({
            "path":path,"sha256":format!("{:x}",Sha256::digest(bytes)),"byte_len":bytes.len(),
        })).collect::<Vec<_>>();
        save(&output, "startup-original-journal.json", &original_journal);
        let mut refusals = Vec::new();
        for case in 0..3 {
            let ResponseBody::HandyHspLive(mut altered) =
                serde_json::from_str(&original_response).unwrap()
            else {
                panic!("retained live HSP witness");
            };
            let (label, expected_error) = match case {
                0 => {
                    let origin = altered.origin.as_mut().unwrap();
                    origin.stream_id = origin.stream_id.checked_add(1).unwrap();
                    (
                        "origin-stream-id",
                        "live HSP request/source witness differs from journal history",
                    )
                }
                1 => {
                    altered.origin = None;
                    altered.source_subject = None;
                    (
                        "removed-witnesses",
                        "live HSP journal history lacks its preceding source witness",
                    )
                }
                2 => {
                    let subject = altered.source_subject.as_mut().unwrap();
                    let prior = subject.identity_sha256().unwrap();
                    subject.lookahead.target_lead_us =
                        subject.lookahead.target_lead_us.checked_add(1).unwrap();
                    subject.validate().unwrap();
                    assert_ne!(subject.identity_sha256().unwrap(), prior);
                    (
                        "altered-source-subject",
                        "live HSP source witness differs from retained dossier subject",
                    )
                }
                _ => unreachable!(),
            };
            altered.validate().unwrap();
            let changed = encode(&ResponseBody::HandyHspLive(altered)).unwrap();
            let changed_sha = format!("{:x}", Sha256::digest(changed.as_bytes()));
            {
                let db = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
                let prior: String = db
                    .query_row(
                        "SELECT response FROM requests WHERE session=?1 AND request=?2",
                        params![session.as_str(), bind.request_id.as_str()],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(
                    prior, original_response,
                    "each mutation must start from the exact original response"
                );
                assert_eq!(
                    db.execute(
                        "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                        params![changed, session.as_str(), bind.request_id.as_str()]
                    )
                    .unwrap(),
                    1
                );
            }
            let (refused, message) = match Engine::open(config.clone()) {
                Err(error) => (true, format!("{error:#}")),
                Ok(engine) => {
                    engine.finish_shutdown().unwrap();
                    Engine::retire_for_test(engine);
                    (false, "unexpected successful startup".to_owned())
                }
            };
            let after_journal = journal_bytes.iter().map(|(path, original)| {
                let after = fs::read(path);
                json!({"path":path,"exact_bytes_preserved":after.as_ref().is_ok_and(|bytes| bytes==original),
                    "sha256":after.as_ref().ok().map(|bytes| format!("{:x}",Sha256::digest(bytes))),
                    "byte_len":after.as_ref().ok().map(Vec::len),"read_error":after.as_ref().err().map(ToString::to_string)})
            }).collect::<Vec<_>>();
            // Restore even before assertions, so a failure cannot contaminate
            // the next case or leave an unrelated mutation in this owned store.
            {
                let db = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
                assert_eq!(
                    db.execute(
                        "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                        params![
                            original_response,
                            session.as_str(),
                            bind.request_id.as_str()
                        ]
                    )
                    .unwrap(),
                    1
                );
                let restored: String = db
                    .query_row(
                        "SELECT response FROM requests WHERE session=?1 AND request=?2",
                        params![session.as_str(), bind.request_id.as_str()],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(restored, original_response);
            }
            let receipt = json!({"case":label,"refused":refused,"error":message,"expected_error":expected_error,
                "original_response_sha256":original_response_sha,"altered_response_sha256":changed_sha,
                "original_response_restored":true,"journal":after_journal,"native_puts_unchanged":peer.puts().len()==5,
                "source_dossier":"unchanged durable Pending/Synthetic; no TEMP permit remains after restart"});
            save(&output, &format!("startup-{label}.json"), &receipt);
            assert!(
                refused && message.contains(expected_error),
                "case {label}: {message}"
            );
            assert!(
                after_journal
                    .iter()
                    .all(|row| row["exact_bytes_preserved"] == true),
                "case {label}: {after_journal:?}"
            );
            assert_eq!(peer.puts().len(), 5, "case {label} issued new transport");
            refusals.push(receipt);
        }
        save(&output, "startup-integrity-refusals.json", &refusals);
        save(
            &output,
            "completion.json",
            &json!({"actual_cpu_acceptances":3,"production_delivery":true,"native_source_authority_injected":false,
            "decoded_input_fixture":fixture_path,"capture_route":"production live_bulk_upload completed-ingress API; no browser/HTTP process",
            "browser_timing":"authored public probe/observation inputs","source_dossier":"durable Pending/Synthetic; cfg(test)-only TEMP permit",
            "all_engine_workers_joined":true,"restart_replayed_without_setup":true,"startup_mismatched_origin_refused":true,"startup_missing_witness_refused":true,"startup_altered_source_subject_refused":true,"peer_puts":peer.puts(),"qualified":false}),
        );
        drop((session, project, preparation_io));
        drop(peer);
        let temporary = dir.path().to_owned();
        dir.close().unwrap();
        save(
            &output,
            "cleanup.json",
            &json!({"temporary_state":temporary,"temporary_state_removed":true,"localhost_peer_joined":true,
            "borrowed_runtime_preserved":artifact(&runtime).sha256==ORT_SHA}),
        );
    }
    include!("engine_handy_hsp_live_cpu_tail_tests.rs");
}
