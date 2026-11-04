// Authored byte and clock fixtures exercise native source custody and provenance.
// They do not establish browser decoding, submitted-pixel origin or Handy timing.
#[cfg(unix)]
mod browser_media_capture_tests {
    use super::*;

    fn prepare(f: &Fixture) -> BrowserMediaBinding {
        let input = f.dir.path().join("browser-source.bin");
        fs::write(&input, b"authored imported bytes for source-clock tests").unwrap();
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::ImportSource { path: input },
            ))
            .result
            .unwrap()
        else {
            panic!("imported source")
        };
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::SetSourcePlacement {
                    spec: SourcePlacementSpec {
                        source_version,
                        source_start: SourceTimestamp::new(1, 3).unwrap(),
                        source_end: SourceTimestamp::new(10, 1).unwrap(),
                        source_anchor: SourceTimestamp::new(1, 3).unwrap(),
                        project_anchor: ProjectTime::from_nanos(-1),
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
            panic!("media catalog")
        };
        assert_eq!(catalog.entries.len(), 1);
        let start = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::PrepareBrowserMedia {
                engine_epoch: catalog.engine_epoch,
                selection: catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(5, 1).unwrap(),
                    elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
                },
                lease_ms: 30_000,
                allow_declared_clock: true,
            },
        );
        f.engine.handle(start.clone()).result.unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let ResponseBody::BrowserMediaPreparation(report) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: start.request_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("preparation report")
            };
            if report.state == BrowserMediaPreparationState::Ready {
                assert!(report.current);
                return report.binding.unwrap();
            }
            assert_eq!(
                report.state,
                BrowserMediaPreparationState::Pending,
                "{report:?}"
            );
            assert!(
                std::time::Instant::now() < end,
                "media preparation did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn start(f: &Fixture, binding: &BrowserMediaBinding) -> Request {
        let mut options = capture_options();
        options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        options.source_anchor = SourceTimestamp::new(17, 3).unwrap();
        let exact = binding
            .clock
            .project_at(options.source_anchor, &binding.selection.placement.spec)
            .unwrap();
        options.project_anchor = exact.quantize_nanoseconds().unwrap().time;
        options.exact_project_anchor = Some(exact);
        options.browser_media = Some(binding.clone());
        request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::StartLiveCapture {
                engine_epoch: binding.engine_epoch.clone(),
                options,
            },
        )
    }

    fn upload(
        f: &Fixture,
        capture: &LiveCaptureReport,
        frame: u64,
        time: SourceTimestamp,
    ) -> PResult<(LiveCaptureReport, bool)> {
        let mut header = live_header(frame, 7);
        header.source_time = time;
        f.engine.live_bulk_upload(
            &live_handshake(f, capture),
            &header,
            &vec![7; LIVE_FRAME_BYTES],
        )
    }

    #[test]
    fn browser_media_capture_preserves_offset_fraction_and_imported_identity() {
        let f = fixture();
        let binding = prepare(&f);
        let capture = capture_report(f.engine.handle(start(&f, &binding)).result.unwrap());
        assert_eq!(capture.identity.source, binding.selection.source_version);
        assert_eq!(capture.options.browser_media.as_ref(), Some(&binding));
        let at = SourceTimestamp::new(17, 3).unwrap();
        let exact = binding
            .clock
            .project_at(at, &binding.selection.placement.spec)
            .unwrap();
        assert_eq!(capture.clock.project_at_source(at).unwrap(), exact);
        let quantized = exact.quantize_nanoseconds().unwrap();
        assert_eq!(quantized.time.as_nanos(), 333_333_332);
        assert_ne!(quantized.remainder_numerator, "0");
        let (accepted, replay) = upload(&f, &capture, 0, at).unwrap();
        assert!(!replay);
        assert_eq!(accepted.received_frames, 1);
        assert!(upload(&f, &capture, 0, at).unwrap().1);
        assert!(upload(&f, &capture, 1, SourceTimestamp::new(15, 1).unwrap()).is_err());
        assert!(upload(&f, &capture, 1, SourceTimestamp::new(5, 1).unwrap()).is_err());
        assert_eq!(
            upload(&f, &capture, 1, SourceTimestamp::new(6, 1).unwrap())
                .unwrap()
                .0
                .received_frames,
            2
        );
    }

    #[test]
    fn browser_media_release_retires_capture_and_replay_cannot_restore_pin() {
        let f = fixture();
        let binding = prepare(&f);
        let original = start(&f, &binding);
        let capture = capture_report(f.engine.handle(original.clone()).result.unwrap());
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ReleaseBrowserMedia {
                    original_request_id: binding.preparation_request_id.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(upload(&f, &capture, 0, capture.options.source_anchor).is_err());
        let replay = capture_report(f.engine.handle(original).result.unwrap());
        assert!(!replay.active);
        assert_eq!(replay.options.browser_media.as_ref(), Some(&binding));
        assert!(f.engine.handle(start(&f, &binding)).result.is_err());
        assert!(f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::ContinueLiveCaptureAt {
                    engine_epoch: binding.engine_epoch,
                    predecessor: capture.identity.epoch,
                    source_anchor: SourceTimestamp::new(6, 1).unwrap(),
                    lease_ms: 1000,
                }
            ))
            .result
            .is_err());
    }

    #[test]
    fn browser_media_capture_refuses_forged_declared_mapping_and_unbound_continuation() {
        let f = fixture();
        let binding = prepare(&f);
        let mut forged = binding.clone();
        forged.clock.elapsed_anchor = SourceTimestamp::new(1, 1).unwrap();
        assert!(f.engine.handle(start(&f, &forged)).result.is_err());
        let capture = capture_report(f.engine.handle(start(&f, &binding)).result.unwrap());
        let anchor = SourceTimestamp::new(6, 1).unwrap();
        let mut options = capture.continuation_options(anchor, 1000).unwrap();
        options.browser_media = None;
        assert!(f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::ContinueLiveCapture {
                    engine_epoch: binding.engine_epoch.clone(),
                    predecessor: capture.identity.epoch.clone(),
                    options,
                }
            ))
            .result
            .is_err());
        let next = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(1),
                    Command::ContinueLiveCaptureAt {
                        engine_epoch: binding.engine_epoch.clone(),
                        predecessor: capture.identity.epoch,
                        source_anchor: anchor,
                        lease_ms: 1000,
                    },
                ))
                .result
                .unwrap(),
        );
        assert_eq!(next.options.browser_media.as_ref(), Some(&binding));
        assert_eq!(
            next.clock.project_at_source(anchor).unwrap(),
            binding
                .clock
                .project_at(anchor, &binding.selection.placement.spec)
                .unwrap()
        );
    }

    #[test]
    fn browser_media_placement_change_retires_capture_before_new_frames() {
        let f = fixture();
        let binding = prepare(&f);
        let capture = capture_report(f.engine.handle(start(&f, &binding)).result.unwrap());
        let mut spec = binding.selection.placement.spec.clone();
        spec.project_anchor = ProjectTime::from_nanos(8_000_000_000);
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::SetSourcePlacement {
                    spec,
                    replace: Some(binding.selection.placement.placement_id),
                },
            ))
            .result
            .unwrap();
        assert!(upload(&f, &capture, 0, capture.options.source_anchor).is_err());
    }
    fn arm_analysis(f: &Fixture, capture: &LiveCaptureReport) -> LiveInferenceManifest {
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 10,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact.clone(),
            runtime: artifact.clone(),
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
        };
        let manifest = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("authored-analysis").unwrap(),
            identity: capture.identity.clone(),
            clock: capture.clock.clone(),
            model: ArtifactReference {
                path: "/authored/model".into(),
                identity: artifact.clone(),
            },
            runtime: ArtifactReference {
                path: "/authored/runtime".into(),
                identity: artifact,
            },
            worker: f.engine.binary_identity.clone(),
            profile,
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
        manifest
    }

    #[test]
    fn browser_media_recording_candidate_retains_exact_source_and_declared_mapping() {
        let f = fixture();
        let binding = prepare(&f);
        let capture = capture_report(f.engine.handle(start(&f, &binding)).result.unwrap());
        let manifest = arm_analysis(&f, &capture);
        let recording = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::StartLiveRecording {
                epoch: capture.identity.epoch.clone(),
                maximum_samples: 4,
            },
        );
        f.engine.handle(recording.clone()).result.unwrap();
        upload(&f, &capture, 0, SourceTimestamp::new(17, 3).unwrap()).unwrap();
        upload(&f, &capture, 1, SourceTimestamp::new(17003, 3000).unwrap()).unwrap();
        let work = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .take_for_test(&capture.identity.epoch);
        let pixels = |bytes: &[u8]| {
            let sha256 = format!("{:x}", Sha256::digest(bytes));
            ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len: bytes.len() as u64,
            }
        };
        let result = LiveInferenceReply::Estimated {
            attempt_id: manifest.attempt_id.clone(),
            identity: manifest.identity.clone(),
            profile_sha256: crate::inference_profiles::profile_identity(&manifest.profile)
                .unwrap()
                .sha256,
            input: LiveInferenceInput {
                sequence: 1,
                pair: work.declaration(),
                previous: pixels(work.previous()),
                current: pixels(work.current()),
            },
            position: NormalizedPosition::new(0.25).unwrap(),
            pair_analysis_elapsed_ns: 1,
        };
        assert!(f
            .engine
            .live_analysis_complete(&capture.identity.epoch, &manifest.attempt_id, work, result)
            .unwrap());
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::StopLiveCapture {
                    epoch: capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ReleaseBrowserMedia {
                    original_request_id: binding.preparation_request_id.clone(),
                },
            ))
            .result
            .unwrap();
        let ResponseBody::LiveRecordingPage(page) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::LiveRecordingPage {
                    epoch: capture.identity.epoch.clone(),
                    after: 0,
                    limit: 64,
                },
            ))
            .result
            .unwrap()
        else {
            panic!("recording page")
        };
        assert_eq!(page.browser_media.as_ref(), Some(&binding));
        assert_eq!(page.identity.source, binding.selection.source_version);
        assert_eq!(page.samples.len(), 1);
        assert!(!page.active && !page.qualified);
        let ResponseBody::Candidate(candidate) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::StageLiveRecording {
                    options: LiveRecordingStageOptions {
                        engine_epoch: capture.engine_epoch,
                        recordings: vec![LiveRecordingSelection {
                            epoch: capture.identity.epoch,
                            recording_request_id: recording.request_id,
                        }],
                        range: TimeRange::new(
                            ProjectTime::ZERO,
                            ProjectTime::from_nanos(2_000_000_000),
                        )
                        .unwrap(),
                        maximum_interpolation_ns: 100_000_000,
                    },
                },
            ))
            .result
            .unwrap()
        else {
            panic!("candidate")
        };
        assert!(candidate
            .review
            .iter()
            .any(|r| r.reason.as_str() == "browser_source_clock_declared"));
        let db = f.engine.db.lock().unwrap();
        let snapshot = project_snapshot(&db, &f.project).unwrap();
        let source = snapshot
            .sources
            .iter()
            .find(|s| s.kind == SourceKind::LiveRecording)
            .unwrap();
        let artifact = f
            .engine
            .source(&db, &f.project, &source.source_version)
            .unwrap();
        let proof: serde_json::Value =
            serde_json::from_slice(&fs::read(artifact.path).unwrap()).unwrap();
        assert_eq!(
            proof["browser_media"],
            serde_json::to_value(&binding).unwrap()
        );
        assert_eq!(proof["pages"][0]["browser_media"], proof["browser_media"]);
        assert_eq!(proof["qualified"], false);
        assert_eq!(snapshot.revision.value(), 1);
    }
}
