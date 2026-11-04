// Included inside live_cpu_native_tests. Actual confined CPU work and the
// original native delivery own these samples; only visible-player timing and
// localhost device behavior are authored. Both dossiers stay Pending/Synthetic.
// This is software composition evidence, never physical qualification.
mod finite_tail_native_tests {
    use super::*;

    fn finite_lookahead() -> BrowserAnalysisLookaheadPolicy {
        BrowserAnalysisLookaheadPolicy {
            target_lead_us: 1_000_000,
            minimum_lead_us: 200_000,
            maximum_lead_us: 3_000_000,
        }
    }

    fn finite_device_fixture(
        f: Fixture,
    ) -> (
        PreparationFixture,
        Peer,
        Arc<RealPreparationIo>,
        HandyQualificationBinding,
    ) {
        let (path, mut input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Synthetic);
        let s = &mut input.subject;
        s.profile.maximum_command_age_ns = 10_000_000_000;
        s.profile.maximum_clock_uncertainty_ns = 100_000_000;
        s.server_clock.as_mut().unwrap().timestamp_error_ns = 1_000_000;
        let device = s.device_clock.as_mut().unwrap();
        device.maximum_sync_age_ns = 10_000_000_000;
        device.device_error_ns = 1_000;
        device.maximum_sync_round_trip_ms = 10;
        let p = policy();
        let hsp = s.hsp.as_mut().unwrap();
        hsp.policy = HandyHspPolicy {
            max_buffered_points: p.max_buffered_points as u32,
            device_capacity_points: p.device_capacity_points as u32,
            max_chunk_points: p.max_chunk_points as u32,
            min_lead_ms: p.min_lead_ms,
            max_future_ms: p.max_future_ms,
            max_sample_age_ns: p.max_sample_age_ns,
            max_gap_ms: p.max_gap_ms,
        };
        // Authored before execution; failure never broadens these limits.
        hsp.finite_drain = Some(HandyHspFiniteDrainQualification {
            schema_version: 1,
            model: HandyHspFiniteDrainModel::EarliestPhaseTailThenOwnedStop,
            maximum_drain_duration_ns: 3_000_000_000,
            maximum_terminal_overrun_ns: 250_000_000,
            maximum_stop_dispatch_delay_ns: 100_000_000,
        });
        let evidence_path = f.dir.path().join("authored-finite-tail-device.txt");
        fs::write(&evidence_path, b"AUTHORED SOFTWARE CONTRACT: native final send gate to localhost Stop acknowledgment. No physical stopping bound or human acceptance is established.\n").unwrap();
        let evidence = artifact(&evidence_path);
        input.evidence.push(HandyQualificationInputEvidence {
            kind: HandyEvidenceKind::FiniteTailDrain,
            path: evidence_path,
            sha256: evidence.sha256,
            byte_len: evidence.byte_len,
        });
        input.validate().unwrap();
        let pending = qualification_saved(
            f.engine
                .handle(qualification_import(
                    &f,
                    "finite-native-cpu-device",
                    &path,
                    &input,
                ))
                .result
                .unwrap(),
        );
        assert_eq!(pending.state, HandyQualificationState::Pending);
        assert_eq!(
            pending.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let (_, connection) = hsp_qualification_connection(&f);
        let permit = handy_qualification::install_authored_hsp_device_acceptance_for_test(
            &f.engine.db.lock().unwrap(),
            &pending.dossier_id,
            crate::handy_clock::boottime_ns()
                .unwrap()
                .checked_add(30_000_000_000)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(permit.dossier_id, pending.dossier_id);
        assert_eq!(pending.revision, 1);
        assert_eq!(permit.revision, 2);
        permit.validate().unwrap();
        let peer = Peer::new(false);
        peer.state.lock().unwrap().native_session =
            pending.manifest.subject.identity.session_id.clone();
        let io = Arc::new(RealPreparationIo {
            peer: peer.state.clone(),
            origin: peer.origin.clone(),
            identity: pending.manifest.subject.identity.clone(),
            syncs: AtomicU64::new(0),
        });
        (
            PreparationFixture {
                f,
                accepted: pending,
                connection,
            },
            peer,
            io,
            permit,
        )
    }

    fn finite_source_dossier(
        x: &PreparationFixture,
        device_permit: &HandyQualificationBinding,
        media: &BrowserMediaBinding,
        profile: &DeclaredInferenceProfile,
    ) -> (
        HandyHspSourceQualificationBinding,
        HandyHspSourceQualificationReport,
    ) {
        let mut evidence = Vec::new();
        let mut environment = None;
        for kind in HandyHspSourceEvidenceKind::ALL
            .into_iter()
            .chain([HandyHspSourceEvidenceKind::FiniteInputClosure])
        {
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
                schema_version: 1,
                finite_drain_policy_sha256: x
                    .accepted
                    .manifest
                    .subject
                    .hsp_finite_drain_policy_sha256()
                    .unwrap(),
                origin: HandyEvidenceOrigin::Synthetic,
                context: x.accepted.manifest.subject.context.clone(),
                device_qualification: device_permit.clone(),
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
                lookahead: finite_lookahead(),
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

    struct ReleaseOnDrop<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for ReleaseOnDrop<F> {
        fn drop(&mut self) {
            if let Some(release) = self.0.take() {
                release();
            }
        }
    }

    fn upload_original(
        f: &Fixture,
        capture: &LiveCaptureReport,
        input: &Value,
        pixels: &[u8],
        decoded_index: usize,
        frame: u64,
    ) -> LiveFrameHeader {
        let declared = &input["frames"][decoded_index];
        assert_eq!(declared["index"].as_u64().unwrap(), decoded_index as u64);
        let offset = declared["offset"].as_u64().unwrap() as usize;
        let bytes = &pixels[offset..offset + LIVE_FRAME_BYTES];
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            declared["sha256"].as_str().unwrap()
        );
        let current = capture_status(f, &capture.identity.epoch);
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
        header
    }

    fn wait_input_sealed(
        f: &Fixture,
        visible: &mut AuthoredVisiblePlayer,
        epoch: &AttemptId,
    ) -> LiveCaptureReport {
        let deadline = Instant::now() + WAIT;
        loop {
            visible.observe(f);
            let report = capture_status(f, epoch);
            if let Some(close) = &report.input_close {
                assert_ne!(close.phase, LiveCaptureInputPhase::Aborted, "{report:?}");
                if close.phase == LiveCaptureInputPhase::Sealed {
                    assert!(close.worker_joined && close.delivery_settled, "{report:?}");
                    assert_eq!(
                        close.disposition,
                        Some(LiveCaptureInputFinalDisposition::Accepted)
                    );
                    assert!(
                        report.active,
                        "sealing must preserve the original capture authority"
                    );
                    assert_eq!(
                        report.inference.as_ref().unwrap().state,
                        LiveCaptureInferenceState::InputSealed
                    );
                    return report;
                }
            }
            assert!(
                Instant::now() < deadline,
                "input did not settle: {report:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }

    fn native_status(status: &HandyHspLiveStatus) -> &HandyHspNativeStatus {
        status.stream.as_ref().unwrap().native.as_ref().unwrap()
    }

    fn assert_joined_worker_fence(f: &Fixture) -> Value {
        let value: Value = serde_json::from_slice(
            &fs::read(f.engine.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        for key in ["groups", "retired", "staging"] {
            assert!(
                value[key].as_object().unwrap().is_empty(),
                "unjoined worker {key}: {value}"
            );
        }
        assert!(value["creation"]["records"].as_object().unwrap().is_empty());
        value
    }

    fn actual_position(stamp: &Value) -> u64 {
        (stamp["estimate"]["result"]["payload"]["position"]
            .as_f64()
            .unwrap()
            * 100.0)
            .round() as u64
    }

    #[derive(Clone, Copy)]
    enum TailCase {
        Reached,
        OriginalTimingExpires,
    }

    fn run_finite_tail(case: TailCase) {
        let fixture_path = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        let output_root = env_path("PULSAR_HSP_TAIL_NATIVE_OUTPUT");
        let label = match case {
            TailCase::Reached => "tail-reached",
            TailCase::OriginalTimingExpires => "original-timing-expires",
        };
        let output = output_root.join(label);
        fs::create_dir(&output).expect("fresh per-case evidence directory required");
        let input: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();
        assert_eq!(input["schema_version"], 1);
        assert_eq!(input["frame_bytes"], LIVE_FRAME_BYTES);
        let media_path = input_path(&input, "media");
        let pixels_path = input_path(&input, "rgb");
        let model = input_path(&input, "model");
        assert_eq!(artifact(&model).sha256, MODEL_SHA);
        let runtime_before = artifact(&runtime);
        assert_eq!(runtime_before.sha256, ORT_SHA);
        save(
            &output,
            "subjects.json",
            &json!({
                "fixture":artifact(&fixture_path), "worker":artifact(&worker), "runtime":runtime_before,
                "input":input, "case":label, "authored_model":true, "browser_capture":false,
                "physical_device":false, "qualified":false,
                "device_dossier":"durable Pending/Synthetic plus cfg(test) TEMP permit",
                "source_dossier":"durable Pending/Synthetic plus cfg(test) TEMP permit",
            }),
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
        let (x, peer, preparation_io, device_permit) = finite_device_fixture(f);
        let mut shutdown = ShutdownGuard(Some(x.f.engine.clone()));
        let (source_qualification, pending_source) =
            finite_source_dossier(&x, &device_permit, &media, &profile);
        save(&output, "device-dossier-pending.json", &x.accepted);
        save(&output, "source-dossier-pending.json", &pending_source);
        save(&output, "profile.json", &profile);
        let finite_policy_hash = x
            .accepted
            .manifest
            .subject
            .hsp_finite_drain_policy_sha256()
            .unwrap()
            .unwrap();
        assert_eq!(
            pending_source
                .manifest
                .subject
                .finite_drain_policy_sha256
                .as_deref(),
            Some(finite_policy_hash.as_str())
        );
        let mut visible = AuthoredVisiblePlayer::open(&x.f, &media);
        visible.observe(&x.f);
        let mut options = capture_options();
        options.browser_media = Some(media.clone());
        options.browser_analysis = Some(BrowserAnalysisCaptureDeclaration {
            feed_id: ArtifactId::new("finite-native-cpu-original-analysis").unwrap(),
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
        let mut preparation = x.prepare("finite-actual-cpu-hsp-preparation");
        preparation.expected_revision = Some(RevisionId::new(1));
        let Command::PrepareHandyHsp { qualification, .. } = &mut preparation.command else {
            unreachable!()
        };
        *qualification = device_permit;
        x.call(&preparation, preparation_io.clone()).unwrap();
        let deadline = Instant::now() + WAIT;
        loop {
            visible.observe(&x.f);
            let status = x.status(&preparation.request_id);
            if status.native.as_ref().is_some_and(|n| {
                n.phase == HandyHspPreparationPhase::Ready
                    && !n.worker_active
                    && !n.metadata_pending
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "preparation did not settle: {status:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
        // Select existing decoded samples using their original presentation
        // times. This neither rewrites a timestamp nor reuses/redraws a frame.
        let decoded_index =
            usize::try_from(visible.origin.elapsed().as_micros() / 100_000).unwrap() + 10;
        assert!(decoded_index + 3 < input["frames"].as_array().unwrap().len());
        save(
            &output,
            "selected-original-frame-window.json",
            &json!({
                "first_index":decoded_index, "last_index":decoded_index+3,
                "visible_elapsed_us":u64::try_from(visible.origin.elapsed().as_micros()).unwrap(),
                "source_times_us":(decoded_index..=decoded_index+3).map(|i| input["frames"][i]["source_time_us"].clone()).collect::<Vec<_>>(),
            }),
        );
        upload_original(&x.f, &capture, &input, &pixels, decoded_index, 0);
        upload_original(&x.f, &capture, &input, &pixels, decoded_index + 1, 1);
        let first = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(1));
        let first_stamp = accepted_stamp(&x.f, &first, &input, &pixels, decoded_index + 1);
        save(&output, "first-actual-acceptance.json", &first_stamp);
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
                    finite_drain_policy_sha256: Some(finite_policy_hash),
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
        assert_eq!(bound.native.as_ref().unwrap().delivered_samples, 1);
        assert_eq!(peer.puts(), ["/hsp/setup"]);
        upload_original(&x.f, &capture, &input, &pixels, decoded_index + 2, 2);
        let second = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(2));
        let second_stamp = accepted_stamp(&x.f, &second, &input, &pixels, decoded_index + 2);
        save(&output, "second-actual-acceptance.json", &second_stamp);
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

        let delivery_hold =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .hold_next_native_delivery_for_test(&capture.identity);
        let _release_delivery = ReleaseOnDrop(Some(|| delivery_hold.release()));
        peer.hold_next("/hsp/add");
        let _release_append = ReleaseOnDrop(Some(|| peer.release()));
        let terminal_header =
            upload_original(&x.f, &capture, &input, &pixels, decoded_index + 3, 3);
        assert!(
            delivery_hold.wait_entered(Duration::from_secs(2)),
            "final actual callback never reached reservation hold"
        );
        let third = capture_status(&x.f, &capture.identity.epoch);
        assert_eq!(third.inference.as_ref().unwrap().accepted, 3);
        let third_stamp = accepted_stamp(&x.f, &third, &input, &pixels, decoded_index + 3);
        save(&output, "third-actual-acceptance.json", &third_stamp);
        let active_fence: Value = serde_json::from_slice(
            &fs::read(x.f.engine.config.state_dir.join("worker-fence.json")).unwrap(),
        )
        .unwrap();
        assert!(!active_fence["groups"].as_object().unwrap().is_empty());
        save(&output, "worker-fence-before-close.json", &active_fence);
        let close = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::CloseBrowserAnalysisInput {
                epoch: capture.identity.epoch.clone(),
                final_frame: Some(terminal_header.clone()),
            },
        );
        let mut mismatched = close.clone();
        mismatched.request_id = RequestId::new("finite-wrong-original-frame").unwrap();
        let Command::CloseBrowserAnalysisInput {
            final_frame: Some(header),
            ..
        } = &mut mismatched.command
        else {
            unreachable!()
        };
        header.capture_tick = LiveClockTick(header.capture_tick.0.checked_add(1).unwrap());
        let refused = x.f.engine.handle(mismatched).result.unwrap_err();
        save(&output, "wrong-full-header-refusal.json", &refused);
        assert!(
            capture_status(&x.f, &capture.identity.epoch)
                .input_close
                .is_none(),
            "wrong full header froze native ingress"
        );
        let close_before = crate::handy_clock::boottime_ns().unwrap();
        let closing = capture_report(x.f.engine.handle(close.clone()).result.unwrap());
        let close_after = crate::handy_clock::boottime_ns().unwrap();
        let pending = closing.input_close.as_ref().unwrap();
        assert_eq!(pending.original_request_id, close.request_id);
        assert_eq!(pending.final_frame.as_ref(), Some(&terminal_header));
        assert!(
            !pending.delivery_settled,
            "reserved final direct callback must block input sealing"
        );
        assert_ne!(pending.phase, LiveCaptureInputPhase::Sealed);
        assert!(!delivery_hold.settled());
        assert_eq!(
            peer.puts(),
            ["/hsp/setup", "/hsp/add", "/hsp/play"],
            "analysis EOF cannot Stop before final callback"
        );
        save(&output, "close-held-original-callback.json", &closing);
        let replay = capture_report(x.f.engine.handle(close.clone()).result.unwrap());
        assert_eq!(
            replay.input_close.as_ref().unwrap().original_request_id,
            close.request_id
        );
        let handshake = live_handshake(&x.f, &closing);
        let ready_refusal =
            x.f.engine
                .live_bulk_ready(&handshake, Some(&terminal_header))
                .unwrap_err();
        let bytes_offset = input["frames"][decoded_index + 3]["offset"]
            .as_u64()
            .unwrap() as usize;
        let upload_refusal =
            x.f.engine
                .live_bulk_upload(
                    &handshake,
                    &terminal_header,
                    &pixels[bytes_offset..bytes_offset + LIVE_FRAME_BYTES],
                )
                .unwrap_err();
        save(
            &output,
            "ingress-after-close-refusals.json",
            &json!({
                "exact_final_frame":terminal_header, "ready":ready_refusal, "upload":upload_refusal,
            }),
        );
        delivery_hold.release();
        let append_deadline = Instant::now() + WAIT;
        loop {
            let (reached, timed_out) = {
                let held = peer.hold.0.lock().unwrap();
                (held.reached, held.timed_out)
            };
            assert!(
                !timed_out,
                "final Append hold exceeded its original deadline"
            );
            if reached {
                break;
            }
            visible.observe(&x.f);
            let status = live_status(&x.f, &bind.request_id);
            if status.native.as_ref().is_some_and(|native| native.retired)
                || Instant::now() >= append_deadline
            {
                save(&output, "final-append-not-held-status.json", &status);
                save(
                    &output,
                    "final-append-not-held-capture.json",
                    &capture_status(&x.f, &capture.identity.epoch),
                );
                save(&output, "final-append-not-held-puts.json", &peer.puts());
                panic!("original final Append did not reach its hold; see retained status");
            }
            std::thread::park_timeout(Duration::from_millis(2));
        }
        let sealed = wait_input_sealed(&x.f, &mut visible, &capture.identity.epoch);
        assert!(delivery_hold.settled());
        assert_eq!(
            sealed.input_close.as_ref().unwrap().final_frame.as_ref(),
            Some(&terminal_header)
        );
        assert_eq!(sealed.inference.as_ref().unwrap().accepted, 3);
        assert_eq!(
            serde_json::to_value(&sealed.options).unwrap(),
            serde_json::to_value(&capture.options).unwrap(),
            "input close cannot renew the capture lease/options"
        );
        save(&output, "input-sealed-final-append-held.json", &sealed);
        save(
            &output,
            "worker-fence-input-joined.json",
            &assert_joined_worker_fence(&x.f),
        );
        let held = live_status(&x.f, &bind.request_id);
        let finite = native_status(&held).finite_drain.as_ref().unwrap();
        assert_eq!(finite.input_state, HandyHspInputState::Sealed);
        assert_eq!(finite.terminal_time_ms, 200);
        assert_eq!(finite.terminal_source_sequence, 3);
        assert_eq!(
            u64::from(finite.terminal_position_percent),
            actual_position(&third_stamp)
        );
        assert_eq!(
            finite.captured_at_ns,
            third_stamp["capture_boottime"]["earliest_ns"]
                .as_u64()
                .unwrap()
                - 50_000_000
        );
        assert_eq!(
            finite.accepted_at_ns,
            third_stamp["accepted_boottime"]["latest_ns"]
                .as_u64()
                .unwrap()
        );
        assert!(
            (close_before..=close_after).contains(&finite.closed_at_boottime_ns),
            "seal must retain original close time, not worker join or status time"
        );
        assert_eq!(
            finite.expires_at_boottime_ns,
            finite.closed_at_boottime_ns + 3_000_000_000
        );
        assert!(finite.tail_reached_at_boottime_ns.is_none());
        assert_eq!(
            native_status(&held).total_added,
            2,
            "held final Append is not acknowledged coverage"
        );
        assert_eq!(
            peer.puts(),
            ["/hsp/setup", "/hsp/add", "/hsp/play", "/hsp/add"],
            "analysis EOF cannot send an early Stop"
        );
        save(&output, "finite-seal-before-final-ack.json", &held);
        let original_close_ns = finite.closed_at_boottime_ns;
        let original_deadline_ns = finite.expires_at_boottime_ns;
        peer.release();
        let draining = wait_live(&x.f, &mut visible, &bind.request_id, |r| {
            let n = native_status(r);
            n.total_added == 3 && !n.native_active && !n.metadata_pending
        });
        let finite = native_status(&draining).finite_drain.as_ref().unwrap();
        assert_eq!(finite.input_state, HandyHspInputState::Draining);
        assert!(!finite.aborted);
        assert!(finite.tail_reached_at_boottime_ns.is_none());
        save(&output, "final-ack-draining.json", &draining);
        let last_observation_tick = visible.last_tick_us;
        let deadline = Instant::now() + WAIT;
        let stopped = loop {
            if matches!(case, TailCase::Reached) {
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
                Instant::now() < deadline,
                "finite source did not resolve owned Stop: {status:?}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        };
        let finite = native_status(&stopped).finite_drain.as_ref().unwrap();
        assert_eq!(finite.closed_at_boottime_ns, original_close_ns);
        assert_eq!(finite.expires_at_boottime_ns, original_deadline_ns);
        match case {
            TailCase::Reached => {
                assert_eq!(finite.input_state, HandyHspInputState::TailReached);
                let reached = finite
                    .tail_reached_at_boottime_ns
                    .expect("actual passage marker required");
                let deadline = finite
                    .stop_dispatch_deadline_boottime_ns
                    .expect("original Stop deadline required");
                let sent = finite
                    .stop_dispatched_at_boottime_ns
                    .expect("actual native final send gate required");
                assert_eq!(deadline, reached + 100_000_000);
                assert!(sent >= reached && sent <= deadline);
                assert!(!finite.stop_deadline_missed && !finite.aborted);
            }
            TailCase::OriginalTimingExpires => {
                assert_eq!(
                    visible.last_tick_us, last_observation_tick,
                    "status polling cannot refresh the original visible observation"
                );
                assert!(finite.aborted);
                assert_ne!(finite.input_state, HandyHspInputState::TailReached);
                assert!(finite.tail_reached_at_boottime_ns.is_none());
                assert!(finite.stop_dispatch_deadline_boottime_ns.is_none());
                let terminal_us = input["frames"][decoded_index + 3]["source_time_us"]
                    .as_u64()
                    .unwrap();
                assert!(
                    u64::try_from(visible.origin.elapsed().as_micros()).unwrap() < terminal_us,
                    "expiry scenario must Stop before the original terminal presentation"
                );
            }
        }
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
        let actual = effects
            .iter()
            .filter(|e| e["record"]["kind"] == "append")
            .flat_map(|e| e["record"]["chunk"]["samples"].as_array().unwrap().iter())
            .collect::<Vec<_>>();
        assert_eq!(
            actual.len(),
            3,
            "finite drain cannot add a repeated terminal point, hold or interpolation"
        );
        for (index, stamp) in [&first_stamp, &second_stamp, &third_stamp]
            .into_iter()
            .enumerate()
        {
            assert_eq!(actual[index]["time_ms"], (index as u64) * 100);
            assert_eq!(actual[index]["source_sequence"], (index as u64) + 1);
            assert_eq!(actual[index]["position_percent"], actual_position(stamp));
            assert_eq!(
                actual[index]["accepted_at_ns"],
                stamp["accepted_boottime"]["latest_ns"]
            );
            assert_eq!(
                actual[index]["captured_at_ns"],
                stamp["capture_boottime"]["earliest_ns"].as_u64().unwrap() - 50_000_000
            );
        }
        assert_eq!(
            effects
                .iter()
                .filter(|e| e["record"]["kind"] == "stop")
                .count(),
            1
        );
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
        let replayed = capture_report(x.f.engine.handle(close.clone()).result.unwrap());
        assert_eq!(
            replayed.input_close.as_ref().unwrap().original_request_id,
            close.request_id
        );
        assert_eq!(
            replayed.input_close.as_ref().unwrap().final_frame.as_ref(),
            Some(&terminal_header)
        );
        live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        live_report(x.f.engine.handle(play.clone()).result.unwrap());
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
        live_report(x.f.engine.handle(stop).result.unwrap());
        assert_eq!(
            peer.puts().len(),
            5,
            "close/Bind/Play/Stop replays cannot create a second command"
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
        let ResponseBody::HandyQualification(still_pending) =
            qualification_status(&x.f, &x.accepted.dossier_id).unwrap()
        else {
            panic!("device dossier")
        };
        assert_eq!(still_pending.state, HandyQualificationState::Pending);
        assert_eq!(
            still_pending.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        save(&output, "device-dossier-still-pending.json", &still_pending);
        let ResponseBody::HandyHspSourceQualification(still_pending_source) =
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
        assert_eq!(still_pending_source.state, HandyQualificationState::Pending);
        assert_eq!(
            still_pending_source.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        assert_eq!(still_pending_source.revision, pending_source.revision);
        save(
            &output,
            "source-dossier-still-pending.json",
            &still_pending_source,
        );
        let live_temp_tables: u64 = x.f.engine.db.lock().unwrap().query_row(
            "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name IN ('pulsar_test_hsp_device_acceptance','pulsar_test_hsp_source_acceptance')", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            live_temp_tables, 2,
            "positive fixture must use both explicitly scoped TEMP permits"
        );
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
        save(
            &output,
            "worker-fence-shutdown-joined.json",
            &assert_joined_worker_fence(&x.f),
        );
        drop(_release_delivery);
        drop(delivery_hold);
        assert_eq!(Arc::strong_count(&x.f.engine), 1);
        let PreparationFixture { f, .. } = x;
        let Fixture {
            engine,
            session: _,
            project: _,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let reopened = Engine::open(config).unwrap();
        let replay = live_report(reopened.handle(bind).result.unwrap());
        assert_eq!(replay.origin, stopped.origin);
        assert!(
            replay.native.is_none(),
            "restart cannot restore source or finite-drain authority"
        );
        assert_eq!(peer.puts().len(), 5);
        save(&output, "restart-replay.json", &replay);
        let temp_tables: u64 = reopened.db.lock().unwrap().query_row(
            "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name IN ('pulsar_test_hsp_device_acceptance','pulsar_test_hsp_source_acceptance')", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            temp_tables, 0,
            "test-only device/source permits must not survive restart"
        );
        reopened.finish_shutdown().unwrap();
        Engine::retire_for_test(reopened);
        drop(dir);
        assert_eq!(
            artifact(&runtime),
            runtime_before,
            "borrowed ORT must remain byte-identical"
        );
        save(
            &output,
            "completion.json",
            &json!({
                "case":label,"actual_cpu_points":3,"original_terminal_ms":200,"native_puts":peer.puts(),
                "worker_joined":true,"delivery_settled":true,"restart_source_authority":false,
                "borrowed_runtime_unchanged":true,"physical_device":false,"qualified":false,
            }),
        );
    }

    #[test]
    #[ignore = "requires pinned native CPU fixture/runtime, rebuilt worker, fresh PULSAR_HSP_TAIL_NATIVE_OUTPUT and delegated confinement; run serially"]
    fn native_cpu_finite_tail_waits_for_original_callback_append_and_owned_stop() {
        run_finite_tail(TailCase::Reached);
    }

    #[test]
    #[ignore = "requires pinned native CPU fixture/runtime, rebuilt worker, fresh PULSAR_HSP_TAIL_NATIVE_OUTPUT and delegated confinement; run serially"]
    fn native_cpu_finite_tail_original_timing_expiry_preempts_tail_passage() {
        run_finite_tail(TailCase::OriginalTimingExpires);
    }
    include!("engine_handy_hsp_live_cpu_preclaim_tests.rs");
    include!("engine_handy_hsp_live_cpu_continuity_tests.rs");
    include!("engine_handy_hsp_browser_continuity_fixture.rs");
}
