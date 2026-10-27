// Actual confined CPU source-basis/admission reads with authored browser callbacks.
// No device, HSP effect, manufactured capture timestamp or refreshed test limit.
mod capture_basis_read_order_native_tests {
    use super::*;
    use crate::authority::live_captures::{
        CaptureReadTestHold, CaptureReadTestPhase, CaptureReadTestTarget, NativeCaptureSelection,
        NativeCaptureSourceSnapshot,
    };

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Case {
        BasisPublication,
        BasisExpiry,
        BasisRetirement,
        AdmissionFirst,
        AdmissionFinal,
    }
    impl Case {
        fn label(self) -> &'static str {
            match self {
                Self::BasisPublication => "basis-publication-after-tick",
                Self::BasisExpiry => "basis-original-expiry",
                Self::BasisRetirement => "basis-lifetime-retirement",
                Self::AdmissionFirst => "admission-first-publication-after-tick",
                Self::AdmissionFinal => "admission-final-publication-after-tick",
            }
        }
        fn basis(self) -> bool {
            matches!(
                self,
                Self::BasisPublication | Self::BasisExpiry | Self::BasisRetirement
            )
        }
    }
    enum ReadOutcome {
        Basis(Box<PResult<NativeCaptureSourceSnapshot>>),
        Admission(Box<PResult<ResponseBody>>),
    }
    struct ReleaseOnDrop<'a>(&'a CaptureReadTestHold);
    impl Drop for ReleaseOnDrop<'_> {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    fn timing_pin(
        f: &Fixture,
        visible: &AuthoredVisiblePlayer,
    ) -> crate::authority::browser_timing::BrowserTimingPin {
        let query = request(
            &f.session,
            Some(&f.project),
            None,
            Command::BrowserTimingStatus {
                binding_id: visible.binding.binding_id.clone(),
                player: visible.binding.player.clone(),
            },
        );
        f.engine
            .browser_timing_scope(&query, &visible.binding.binding_id)
            .unwrap()
    }
    fn other_visible(
        f: &Fixture,
        media: &BrowserMediaBinding,
        original: &AuthoredVisiblePlayer,
    ) -> AuthoredVisiblePlayer {
        let mut player = original.binding.player.clone();
        player.player_id = AttemptId::new("basis-read-independent-player").unwrap();
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
            panic!("independent timing open")
        };
        AuthoredVisiblePlayer {
            binding: report.binding,
            origin: original.origin,
            sequence: 0,
            presented_frames: Arc::new(AtomicU64::new(0)),
            last_tick_us: 0,
        }
    }
    fn run(case: Case) {
        let fixture_path = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        let output = env_path("PULSAR_CAPTURE_BASIS_READ_NATIVE_OUTPUT").join(case.label());
        fs::create_dir(&output).expect("fresh case directory");
        let input: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();
        let pixels = fs::read(input_path(&input, "rgb")).unwrap();
        let model = input_path(&input, "model");
        assert_eq!(artifact(&model).sha256, MODEL_SHA);
        let runtime_before = artifact(&runtime);
        assert_eq!(runtime_before.sha256, ORT_SHA);
        save(
            &output,
            "subjects.json",
            &json!({
            "fixture":artifact(&fixture_path),"worker":artifact(&worker),
            "runtime":runtime_before,"case":case.label(),"browser_capture":false,
            "physical_device":false,"qualified":false}),
        );
        let f = native_fixture(worker);
        let mut shutdown = ShutdownGuard(Some(f.engine.clone()));
        let media = prepare_media(&f, input_path(&input, "media"));
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
        let mut visible = AuthoredVisiblePlayer::open(&f, &media);
        visible.observe(&f);
        let mut options = capture_options();
        options.browser_media = Some(media.clone());
        options.browser_analysis = Some(BrowserAnalysisCaptureDeclaration {
            feed_id: ArtifactId::new("native-capture-basis-read-analysis").unwrap(),
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
            model_path: model.clone(),
            profile: profile(&model, runtime_identity),
        });
        // All authenticated requests are built on this fixture thread: request()
        // intentionally keeps its synthetic test credentials thread-local.
        let start = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::StartLiveCapture {
                engine_epoch: media.engine_epoch.clone(),
                options,
            },
        );
        let mut capture = None;
        let mut accepted = None;
        let mut selection = None;
        let mut basis_query = None;
        if case.basis() {
            let first = capture_report(f.engine.handle(start.clone()).result.unwrap());
            wait_capture(&f, &mut visible, &first.identity.epoch, None);
            let index =
                usize::try_from(visible.origin.elapsed().as_micros() / 100_000).unwrap() + 10;
            upload_original(&f, &first, &input, &pixels, index, 0);
            upload_original(&f, &first, &input, &pixels, index + 2, 1);
            let report = wait_capture(&f, &mut visible, &first.identity.epoch, Some(1));
            let info = report.inference.as_ref().unwrap();
            selection = Some(NativeCaptureSelection {
                cpu_manifest_sha256: None,
                owner: f.session.clone(),
                identity: report.identity.clone(),
                attempt_id: info.attempt_id.clone(),
                profile_sha256: info.profile_sha256.clone(),
                browser_media: media.clone(),
            });
            basis_query = Some(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::LiveCaptureStatus {
                    epoch: report.identity.epoch.clone(),
                },
            ));
            accepted = Some(
                f.engine
                    .live_captures
                    .lock()
                    .unwrap()
                    .native_accepted(&f.session, &report.identity)
                    .unwrap()
                    .cloned()
                    .unwrap(),
            );
            capture = Some(report);
        }
        // Open cannot run while the reader holds DB/capture guards. B is an
        // independent lifetime on the same media, prepared before the rendezvous.
        let mut independent = if case == Case::BasisRetirement {
            visible.observe(&f);
            let mut other = other_visible(&f, &media, &visible);
            other.observe(&f);
            Some(other)
        } else {
            None
        };
        let independent_pin = independent.as_ref().map(|other| timing_pin(&f, other));
        let pin = timing_pin(&f, &visible);
        visible.observe(&f);
        let original = pin
            .latest(crate::handy_clock::boottime_ns().unwrap())
            .unwrap();
        let target = match case {
            Case::AdmissionFirst => CaptureReadTestTarget::AdmissionFirst(start.request_id.clone()),
            Case::AdmissionFinal => CaptureReadTestTarget::AdmissionFinal(start.request_id.clone()),
            _ => CaptureReadTestTarget::Basis(selection.as_ref().unwrap().identity.epoch.clone()),
        };
        let phase = if matches!(case, Case::BasisExpiry | Case::BasisRetirement) {
            CaptureReadTestPhase::BeforeSample
        } else {
            CaptureReadTestPhase::AfterSample
        };
        let hold = f.engine.hold_capture_read_for_test(target, phase);
        let mut replacement = None;
        let mut release_at = 0;
        let outcome = std::thread::scope(|scope| {
            // This guard drops before scope waits for a panicking caller's reader.
            let release = ReleaseOnDrop(&hold);
            let reader = scope.spawn(|| {
                if let Some(selected) = &selection {
                    let db = f.engine.db.lock().unwrap();
                    f.engine
                        .authorize(&db, basis_query.as_ref().unwrap(), &f.session)
                        .unwrap();
                    let registry = f.engine.live_captures.lock().unwrap();
                    ReadOutcome::Basis(Box::new(registry.native_source_basis(selected)))
                } else {
                    ReadOutcome::Admission(Box::new(f.engine.handle(start.clone()).result))
                }
            });
            assert!(
                hold.wait_entered(Duration::from_secs(2)),
                "source read missed rendezvous"
            );
            let entered = hold.snapshot();
            match case {
                Case::BasisExpiry => {
                    assert!(entered.sampled_at_ns.is_none());
                    let until = Instant::now() + Duration::from_secs(1);
                    loop {
                        visible.observe(&f);
                        if crate::handy_clock::boottime_ns().unwrap()
                            >= original.expires_at_boottime_ns
                        {
                            break;
                        }
                        assert!(
                            Instant::now() < until,
                            "original expiry rendezvous exceeded bound"
                        );
                        std::thread::park_timeout(Duration::from_millis(20));
                    }
                    replacement = Some(
                        pin.latest(crate::handy_clock::boottime_ns().unwrap())
                            .unwrap(),
                    );
                }
                Case::BasisRetirement => {
                    assert!(entered.sampled_at_ns.is_none());
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            None,
                            Command::ReleaseBrowserTiming {
                                binding_id: visible.binding.binding_id.clone(),
                                player: visible.binding.player.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                    independent.as_mut().unwrap().observe(&f);
                    replacement = Some(
                        independent_pin
                            .as_ref()
                            .unwrap()
                            .latest(crate::handy_clock::boottime_ns().unwrap())
                            .unwrap(),
                    );
                    assert!(!pin.current(crate::handy_clock::boottime_ns().unwrap()));
                }
                _ => {
                    visible.observe(&f);
                    let next = pin
                        .latest(crate::handy_clock::boottime_ns().unwrap())
                        .unwrap();
                    let at = entered.sampled_at_ns.unwrap();
                    assert!(
                        at >= original.bracket.native_receive_ns
                            && at < original.expires_at_boottime_ns
                    );
                    assert!(next.bracket.native_receive_ns > at);
                    // The fixed-at API must still refuse the later publication.
                    assert!(pin.try_snapshot_native(at).is_err());
                    replacement = Some(next);
                }
            }
            release_at = crate::handy_clock::boottime_ns().unwrap();
            assert!(release_at < replacement.as_ref().unwrap().expires_at_boottime_ns);
            if let Some(stamp) = &accepted {
                assert!(release_at - stamp.capture_boottime.earliest_ns < AGE_NS);
            }
            hold.release();
            let result = reader.join().unwrap();
            drop(release);
            result
        });
        let receipt = hold.snapshot();
        let (succeeded, error, returned_tick, stamp_unchanged) =
            match outcome {
                ReadOutcome::Basis(result) => match *result {
                    Ok(value) => (
                        true,
                        None,
                        Some(value.native_now.latest_ns),
                        value.latest.as_ref().zip(accepted.as_ref()).is_some_and(
                            |(got, original)| {
                                got.frame == original.frame
                                    && got.capture_boottime == original.capture_boottime
                                    && got.accepted_boottime == original.accepted_boottime
                            },
                        ),
                    ),
                    Err(error) => (false, Some(error), None, true),
                },
                ReadOutcome::Admission(result) => match *result {
                    Ok(value) => {
                        capture = Some(capture_report(value));
                        (true, None, None, true)
                    }
                    Err(error) => (false, Some(error), None, true),
                },
            };
        // A successful admission must launch the real confined worker and accept
        // the same decoded neutral input fixture, not merely return a DTO.
        if !case.basis() && succeeded {
            let report = capture.as_ref().unwrap();
            wait_capture(&f, &mut visible, &report.identity.epoch, None);
            let index =
                usize::try_from(visible.origin.elapsed().as_micros() / 100_000).unwrap() + 10;
            upload_original(&f, report, &input, &pixels, index, 0);
            upload_original(&f, report, &input, &pixels, index + 2, 1);
            capture = Some(wait_capture(
                &f,
                &mut visible,
                &report.identity.epoch,
                Some(1),
            ));
        }
        let at = receipt.sampled_at_ns.unwrap();
        let replacement = replacement.unwrap();
        let expected = match case {
            Case::BasisExpiry => {
                !succeeded
                    && receipt.observation.is_none()
                    && at >= original.expires_at_boottime_ns
                    && at < replacement.expires_at_boottime_ns
            }
            Case::BasisRetirement => {
                !succeeded
                    && receipt.observation.is_none()
                    && replacement.binding_id != original.binding_id
                    && at < replacement.expires_at_boottime_ns
            }
            _ => succeeded && receipt.observation.as_ref() == Some(&original),
        } && !receipt.timed_out
            && receipt.completed
            && stamp_unchanged
            && returned_tick.is_none_or(|tick| tick == at);
        save(
            &output,
            "capture-read-receipt.json",
            &json!({
            "case":case.label(),"original":original,"replacement":replacement,
            "sampled_at_ns":receipt.sampled_at_ns,"evaluated_observation":receipt.observation,
            "release_at_ns":release_at,"entered":receipt.entered,"completed":receipt.completed,"timed_out":receipt.timed_out,
            "succeeded":succeeded,"error":error,"returned_tick":returned_tick,
            "original_stamp_unchanged":stamp_unchanged,"expected_semantics":expected,
            "actual_cpu_accepted":capture.as_ref().and_then(|r|r.inference.as_ref()).map(|i|i.accepted),
            "capture_identity":capture.as_ref().map(|r|&r.identity),
            "accepted_frame":accepted.as_ref().map(|s|&s.frame),
            "accepted_at_ns":accepted.as_ref().map(|s|s.accepted_boottime.latest_ns),
            "capture_earliest_ns":accepted.as_ref().map(|s|s.capture_boottime.earliest_ns),
            "physical_device":false,"qualified":false}),
        );
        shutdown.finish();
        let fence: Value = serde_json::from_slice(
            &fs::read(f.engine.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        assert!(fence["groups"].as_object().unwrap().is_empty());
        let Fixture {
            engine,
            dir,
            session,
            project,
        } = f;
        drop((engine, session, project, hold, pin, independent_pin));
        let temporary = dir.path().to_owned();
        dir.close().unwrap();
        assert_eq!(artifact(&runtime).sha256, runtime_before.sha256);
        save(
            &output,
            "completion.json",
            &json!({"case":case.label(),"passed":expected,
            "worker_groups_empty":true,"engine_shutdown_joined":true,"scratch_removed":!temporary.exists(),
            "runtime_unchanged":true,"direct_device_effects":false,"qualified":false,"physical_device":false}),
        );
        assert!(expected, "original capture read selected/evaluated the wrong publication or lost original refusal");
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_basis_retains_publication_selected_before_tick() {
        run(Case::BasisPublication)
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_basis_keeps_original_publication_expiry() {
        run(Case::BasisExpiry)
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_basis_refuses_retired_lifetime_with_live_replacement() {
        run(Case::BasisRetirement)
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_admission_first_read_selects_before_tick() {
        run(Case::AdmissionFirst)
    }
    #[test]
    #[ignore = "requires pinned CPU runtime, worker, decoded neutral fixture and private native confinement; run serially"]
    fn native_cpu_admission_final_read_selects_before_tick() {
        run(Case::AdmissionFinal)
    }
}
