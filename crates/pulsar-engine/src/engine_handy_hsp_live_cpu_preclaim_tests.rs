// Included inside finite_tail_native_tests. Every motion point is delivered
// by the actual confined CPU worker from the original sealed RGB fixture.
// Mutex holds, localhost device responses and visible timing are authored test
// inputs. Neither dossier becomes physical acceptance.
mod preclaim_native_tests {
    use super::*;
    use crate::device_effect_journal::HspEffectObservation;
    use handy_hsp_preparation::{HspPreclaimTestHold, HspPreclaimTestSite};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum PreclaimCase {
        DbRelease,
        CaptureRelease,
        TimingRelease,
        Cancel,
        Cap,
    }
    impl PreclaimCase {
        fn label(self) -> &'static str {
            match self {
                Self::DbRelease => "preclaim-db-release",
                Self::CaptureRelease => "preclaim-capture-release",
                Self::TimingRelease => "preclaim-timing-release",
                Self::Cancel => "preclaim-cancel",
                Self::Cap => "preclaim-cap",
            }
        }
        fn released(self) -> bool {
            matches!(
                self,
                Self::DbRelease | Self::CaptureRelease | Self::TimingRelease
            )
        }
    }

    fn final_append(f: &Fixture, stream: &str) -> Option<HspEffectObservation> {
        let journal = f.engine._device_effect_journal.try_lock().ok()?;
        let stream = journal.hsp_stream(stream).ok()?;
        stream.effect_ids.iter().rev().find_map(|id| {
            let effect = journal.hsp_effect(id).ok()?;
            (effect.record.kind == HspEffectRecordKind::Append
                && effect.record.chunk.as_ref().is_some_and(|chunk| {
                    chunk
                        .samples
                        .last()
                        .is_some_and(|sample| sample.source_sequence == 3)
                }))
            .then_some(effect)
        })
    }
    fn effect_json(effect: &HspEffectObservation) -> Value {
        json!({"id":effect.id,"state":format!("{:?}",effect.state),"record":effect.record,
            "outcome":effect.outcome,"late_observation":effect.late_observation})
    }
    fn before_claim(
        f: &Fixture,
        visible: &mut AuthoredVisiblePlayer,
        request: &RequestId,
        hold: &HspPreclaimTestHold,
        output: &std::path::Path,
    ) -> HandyHspLiveStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            visible.observe(f);
            let status = live_status(f, request);
            if hold.wait_entered(Duration::ZERO)
                && status.native.as_ref().is_some_and(|native| {
                    native.delivered_samples == 3 && !native.retired && native.error.is_none()
                })
            {
                return status;
            }
            if status
                .native
                .as_ref()
                .is_some_and(|native| native.retired || native.error.is_some())
                || Instant::now() >= deadline
            {
                save(output, "failed-before-preclaim.json", &status);
                panic!("original final Append did not enter preclaim rendezvous: {status:?}");
            }
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }

    fn run_preclaim(case: PreclaimCase) {
        let fixture_path = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        let output_root = env_path("PULSAR_HSP_PRECLAIM_NATIVE_OUTPUT");
        let label = case.label();
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
            feed_id: ArtifactId::new("preclaim-native-cpu-original-analysis").unwrap(),
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
        let mut preparation = x.prepare("preclaim-actual-cpu-hsp-preparation");
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

        let hold =
            x.f.engine
                .hold_next_handy_hsp_preclaim_for_test(&x.f.session, &preparation.request_id)
                .unwrap();
        let _release_hold = ReleaseOnDrop(Some(|| hold.release()));
        let source_probe =
            x.f.engine
                .hsp_live_preclaim_probe_for_test(&x.f.session, &bind.request_id);
        let source_counts_before = [
            source_probe.capture_contentions(),
            source_probe.timing_contentions(),
        ];
        let terminal_header =
            upload_original(&x.f, &capture, &input, &pixels, decoded_index + 3, 3);
        let third = wait_capture(&x.f, &mut visible, &capture.identity.epoch, Some(3));
        let third_stamp = accepted_stamp(&x.f, &third, &input, &pixels, decoded_index + 3);
        save(&output, "third-actual-acceptance.json", &third_stamp);
        let paused = before_claim(&x.f, &mut visible, &bind.request_id, &hold, &output);
        assert_eq!(paused.native.as_ref().unwrap().delivered_samples, 3);
        assert_eq!(hold.claimed(), 0);
        assert_eq!(peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/play"]);
        let stream_id = paused.stream.as_ref().unwrap().stream_record_id.clone();
        let prepared = final_append(&x.f, stream_id.as_str())
            .expect("original final Append must be journal-prepared before preclaim");
        assert_eq!(prepared.state, HspJournalState::Prepared);
        assert_eq!(prepared.record.sequence, 4);
        save(
            &output,
            "original-prepared-append.json",
            &effect_json(&prepared),
        );
        // Join the original CPU input before imposing a source-registry lock:
        // its normal worker loop must not become an unrelated DB waiter.
        let close = request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::CloseBrowserAnalysisInput {
                epoch: capture.identity.epoch.clone(),
                final_frame: Some(terminal_header.clone()),
            },
        );
        capture_report(x.f.engine.handle(close.clone()).result.unwrap());
        let sealed = wait_input_sealed(&x.f, &mut visible, &capture.identity.epoch);
        assert_eq!(
            sealed.input_close.as_ref().unwrap().final_frame.as_ref(),
            Some(&terminal_header)
        );
        assert_eq!(sealed.inference.as_ref().unwrap().accepted, 3);
        save(&output, "input-sealed-before-preclaim-lock.json", &sealed);
        save(
            &output,
            "worker-fence-input-joined.json",
            &assert_joined_worker_fence(&x.f),
        );
        visible.observe(&x.f);
        let execution = x.f.engine.ensure_handy_hsp_execution().unwrap();
        let before_network = peer.state.lock().unwrap().requests.len();
        let observation_attempts = AtomicU64::new(0);
        let observation_completions = AtomicU64::new(0);
        let stop_observer = AtomicBool::new(false);
        // request() reads its authentication from test-thread-local storage;
        // transfer only this existing fixture session token to the observer.
        let observer_auth =
            TOKENS.with(|tokens| tokens.borrow().get(x.f.session.as_str()).unwrap().clone());
        let critical = std::thread::scope(|scope| {
            let _stop = ReleaseOnDrop(Some(|| stop_observer.store(true, Ordering::Release)));
            let (db_guard, capture_guard, timing_guard) = match case {
                PreclaimCase::CaptureRelease => {
                    (None, Some(x.f.engine.live_captures.lock().unwrap()), None)
                }
                PreclaimCase::TimingRelease => {
                    (None, None, Some(x.f.engine.browser_timing.lock().unwrap()))
                }
                _ => (Some(x.f.engine.db.lock().unwrap()), None, None),
            };
            let completions_at_lock = observation_completions.load(Ordering::Acquire);
            let observer = scope.spawn(|| {
                TOKENS.with(|tokens| {
                    tokens
                        .borrow_mut()
                        .insert(x.f.session.to_string(), observer_auth.clone())
                });
                while !stop_observer.load(Ordering::Acquire) {
                    observation_attempts.fetch_add(1, Ordering::AcqRel);
                    // This exact original Issue/Observe path is DB-free. A
                    // deliberately held timing registry naturally blocks it;
                    // completion is counted only after the real call returns.
                    visible.observe(&x.f);
                    observation_completions.fetch_add(1, Ordering::AcqRel);
                    std::thread::park_timeout(Duration::from_millis(2));
                }
            });
            let released_at = Instant::now();
            hold.release();
            let contention_deadline = Instant::now() + Duration::from_millis(50);
            let observed = loop {
                let seen = match case {
                    PreclaimCase::CaptureRelease => {
                        source_probe.capture_contentions() > source_counts_before[0]
                    }
                    PreclaimCase::TimingRelease => {
                        source_probe.timing_contentions() > source_counts_before[1]
                    }
                    _ => hold.wait_contended(HspPreclaimTestSite::Db, Duration::ZERO),
                };
                if seen || Instant::now() >= contention_deadline {
                    break seen;
                }
                std::thread::park_timeout(Duration::from_millis(1));
            };
            let contended_at = Instant::now();
            let mut available = [false; 5]; // DB, sessions, captures, timing, journal.
            let mut native_snapshot = None;
            let mut cancelled = None;
            if case == PreclaimCase::Cancel {
                cancelled = Some(hold.cancel_original_for_test());
            }
            let after_contention_deadline = if case.released() {
                contended_at + Duration::from_millis(15)
            } else {
                // This is only the observer's bound. The original native
                // preclaim cap remains exactly 100ms and is never rewritten.
                released_at + Duration::from_millis(250)
            };
            let mut terminal_while_held = false;
            loop {
                if db_guard.is_none() {
                    available[0] |= x.f.engine.db.try_lock().is_ok();
                }
                available[1] |= x.f.engine.handy_sessions.try_lock().is_ok();
                if capture_guard.is_none() {
                    available[2] |= x.f.engine.live_captures.try_lock().is_ok();
                }
                if timing_guard.is_none() {
                    available[3] |= x.f.engine.browser_timing.try_lock().is_ok();
                }
                available[4] |= x.f.engine._device_effect_journal.try_lock().is_ok();
                if let Ok(snapshot) =
                    execution.snapshot_for_session(&x.f.session, &x.f.project, &bind.request_id)
                {
                    native_snapshot = Some(format!("{snapshot:?}"));
                }
                if !case.released()
                    && final_append(&x.f, stream_id.as_str())
                        .is_some_and(|effect| effect.state == HspJournalState::Cancelled)
                {
                    terminal_while_held = true;
                    break;
                }
                if Instant::now() >= after_contention_deadline {
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(2));
            }
            let effect_while_held =
                final_append(&x.f, stream_id.as_str()).map(|effect| effect_json(&effect));
            let claims_while_held = hold.claimed();
            let puts_while_held = peer.puts();
            let network_while_held = peer.state.lock().unwrap().requests.len();
            let completions_before_release = observation_completions.load(Ordering::Acquire);
            let attempts_before_release = observation_attempts.load(Ordering::Acquire);
            let held_ns = released_at.elapsed().as_nanos();
            // All assertions and artifact writes happen after dropping the
            // deliberately held guard, so failures cannot poison it or hold a
            // guard across joined fixture shutdown.
            drop((db_guard, capture_guard, timing_guard));
            let after_contention_hold_ns = contended_at.elapsed().as_nanos();
            let resumed_deadline = Instant::now() + Duration::from_millis(250);
            while observation_completions.load(Ordering::Acquire) <= completions_before_release
                && Instant::now() < resumed_deadline
            {
                std::thread::park_timeout(Duration::from_millis(1));
            }
            stop_observer.store(true, Ordering::Release);
            let observer_joined = observer.join().is_ok();
            json!({"actual_contention_observed":observed,"outer_counts":hold.counts(),
                "source_counts":[source_probe.capture_contentions(),source_probe.timing_contentions()],
                "claims_while_held":claims_while_held,"puts_while_held":puts_while_held,
                "network_requests_before":before_network,"network_requests_while_held":network_while_held,
                "guard_availability":available,"native_snapshot":native_snapshot,"effect_while_held":effect_while_held,
                "original_cancel":cancelled.map(|result| match result { Ok(value)=>json!({"changed":value}),Err(error)=>json!({"error":error}) }),
                "terminal_while_held":terminal_while_held,"held_ns":held_ns,"after_contention_hold_ns":after_contention_hold_ns,
                "observation_attempts_before_release":attempts_before_release,
                "observation_completions_at_lock":completions_at_lock,
                "observation_completions_before_release":completions_before_release,
                "observation_completions_after_release":observation_completions.load(Ordering::Acquire),
                "observer_joined":observer_joined})
        });
        save(&output, "preclaim-held-observations.json", &critical);
        assert_eq!(
            critical["actual_contention_observed"], true,
            "exact real lock site was not observed: {critical}"
        );
        assert_eq!(
            critical["claims_while_held"], 0,
            "original command claimed while selected lock was held"
        );
        assert_eq!(critical["observer_joined"], true);
        assert!(
            !hold.timed_out(),
            "preclaim rendezvous expired instead of explicit release"
        );
        let puts_while_held = critical["puts_while_held"].as_array().unwrap();
        assert_eq!(
            puts_while_held
                .iter()
                .filter(|path| path.as_str() == Some("/hsp/add"))
                .count(),
            1
        );
        assert_eq!(
            puts_while_held
                .iter()
                .filter(|path| path.as_str() == Some("/hsp/setup"))
                .count(),
            1
        );
        assert_eq!(
            puts_while_held
                .iter()
                .filter(|path| path.as_str() == Some("/hsp/play"))
                .count(),
            1
        );
        for (index, name) in ["db", "sessions", "capture", "timing", "journal"]
            .into_iter()
            .enumerate()
        {
            let deliberately_held = match case {
                PreclaimCase::CaptureRelease => 2,
                PreclaimCase::TimingRelease => 3,
                _ => 0,
            };
            if index != deliberately_held {
                assert_eq!(
                    critical["guard_availability"][index], true,
                    "{name} guard was not observable free during retry"
                );
            }
        }
        let before_release = critical["observation_completions_before_release"]
            .as_u64()
            .unwrap();
        assert!(
            critical["observation_completions_after_release"]
                .as_u64()
                .unwrap()
                > before_release,
            "the original observation driver did not resume after lock release"
        );
        if case == PreclaimCase::TimingRelease {
            assert_eq!(
                critical["observation_completions_at_lock"],
                critical["observation_completions_before_release"],
                "a held timing registry cannot complete a timing callback"
            );
            assert!(
                critical["observation_attempts_before_release"]
                    .as_u64()
                    .unwrap()
                    > before_release,
                "timing observation backpressure was not exercised"
            );
        } else {
            assert!(
                before_release
                    > critical["observation_completions_at_lock"]
                        .as_u64()
                        .unwrap(),
                "DB-free visible observation must continue during the selected hold"
            );
        }
        if case.released() {
            assert!(
                critical["after_contention_hold_ns"].as_u64().unwrap() <= 20_000_000,
                "imposed positive hold exceeded its unchanged 20ms test bound: {critical}"
            );
            assert_eq!(critical["effect_while_held"]["state"], "Prepared");
            assert_eq!(
                critical["network_requests_before"], critical["network_requests_while_held"],
                "local preclaim retries must not rerun network preflight"
            );
        } else {
            assert_eq!(
                critical["terminal_while_held"], true,
                "unclaimed command did not cancel before lock release"
            );
            if case == PreclaimCase::Cancel {
                assert_eq!(critical["original_cancel"]["changed"], true);
            } else {
                assert!(
                    critical["held_ns"].as_u64().unwrap() >= 100_000_000,
                    "case ended before the unchanged 100ms native contention cap"
                );
            }
        }
        let expected_claims = u64::from(case.released());
        if case.released() {
            wait_live(&x.f, &mut visible, &bind.request_id, |status| {
                let native = native_status(status);
                native.total_added == 3 && !native.native_active && !native.metadata_pending
            });
            assert_eq!(hold.claimed(), 1);
            assert_eq!(
                peer.puts(),
                ["/hsp/setup", "/hsp/add", "/hsp/play", "/hsp/add"]
            );
            let stop = request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::StopHandyHspLive {
                    stream_request_id: bind.request_id.clone(),
                    owner_proof: x.f.engine.handy_owner_token.clone(),
                },
            );
            live_report(x.f.engine.handle(stop).result.unwrap());
        }
        let stopped = wait_live(&x.f, &mut visible, &bind.request_id, |status| {
            let native = native_status(status);
            native.runtime_state == HandyHspRuntimeState::StopAcknowledged
                && !native.native_active
                && !native.metadata_pending
        });
        save(&output, "stopped.json", &stopped);
        assert_eq!(hold.claimed(), expected_claims);
        let final_effect = final_append(&x.f, stream_id.as_str()).unwrap();
        assert_eq!(final_effect.id, prepared.id);
        assert_eq!(
            final_effect.record, prepared.record,
            "preclaim wait changed original sample/effect bytes or deadlines"
        );
        assert_eq!(
            final_effect.state,
            if case.released() {
                HspJournalState::Acknowledged
            } else {
                HspJournalState::Cancelled
            }
        );
        let expected_puts = if case.released() {
            vec![
                "/hsp/setup",
                "/hsp/add",
                "/hsp/play",
                "/hsp/add",
                "/hsp/stop",
            ]
        } else {
            vec!["/hsp/setup", "/hsp/add", "/hsp/play", "/hsp/stop"]
        };
        assert_eq!(peer.puts(), expected_puts);
        assert_eq!(preparation_io.syncs.load(Ordering::Acquire), 1);
        let effects = {
            let journal = x.f.engine._device_effect_journal.lock().unwrap();
            let stream = journal.hsp_stream(stream_id.as_str()).unwrap();
            stream
                .effect_ids
                .iter()
                .map(|id| effect_json(&journal.hsp_effect(id).unwrap()))
                .collect::<Vec<_>>()
        };
        let samples = effects
            .iter()
            .filter(|effect| effect["record"]["kind"] == "append")
            .flat_map(|effect| effect["record"]["chunk"]["samples"].as_array().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(samples.len(), 3);
        for (index, stamp) in [&first_stamp, &second_stamp, &third_stamp]
            .into_iter()
            .enumerate()
        {
            assert_eq!(samples[index]["source_sequence"], index as u64 + 1);
            assert_eq!(samples[index]["time_ms"], index as u64 * 100);
            assert_eq!(samples[index]["position_percent"], actual_position(stamp));
            assert_eq!(
                samples[index]["accepted_at_ns"],
                stamp["accepted_boottime"]["latest_ns"]
            );
            assert_eq!(
                samples[index]["captured_at_ns"],
                stamp["capture_boottime"]["earliest_ns"].as_u64().unwrap() - 50_000_000
            );
        }
        save(&output, "journal-history.json", &effects);
        save(
            &output,
            "transport.json",
            &peer.state.lock().unwrap().requests,
        );
        live_report(x.f.engine.handle(bind.clone()).result.unwrap());
        live_report(x.f.engine.handle(play.clone()).result.unwrap());
        capture_report(x.f.engine.handle(close).result.unwrap());
        assert_eq!(
            peer.puts(),
            expected_puts,
            "replay emitted additional native traffic"
        );
        assert_eq!(hold.claimed(), expected_claims);
        let ResponseBody::HandyQualification(device_pending) =
            qualification_status(&x.f, &x.accepted.dossier_id).unwrap()
        else {
            panic!("device dossier")
        };
        assert_eq!(device_pending.state, HandyQualificationState::Pending);
        assert_eq!(
            device_pending.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let ResponseBody::HandyHspSourceQualification(source_pending) =
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
        assert_eq!(source_pending.state, HandyQualificationState::Pending);
        assert_eq!(
            source_pending.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        save(
            &output,
            "dossiers-still-pending.json",
            &json!({"device":device_pending,"source":source_pending}),
        );
        let capture_stopped = capture_report(
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
        assert!(!capture_stopped.active);
        shutdown.finish();
        save(
            &output,
            "worker-fence-shutdown-joined.json",
            &assert_joined_worker_fence(&x.f),
        );
        let actual_contentions = hold.counts().iter().sum::<u64>();
        let counts = hold.counts();
        drop(_release_hold);
        drop(hold);
        drop(execution);
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
        assert!(
            replay.native.is_none(),
            "restart restored live source authority"
        );
        assert_eq!(replay.origin, stopped.origin);
        assert_eq!(peer.puts(), expected_puts);
        save(&output, "restart-replay.json", &replay);
        reopened.finish_shutdown().unwrap();
        Engine::retire_for_test(reopened);
        drop(dir);
        assert_eq!(artifact(&runtime), runtime_before);
        save(
            &output,
            "completion.json",
            &json!({
                "case":label,"actual_cpu_points":3,"original_append_claims":expected_claims,
                "original_append_puts":peer.puts().iter().filter(|path|path.as_str()=="/hsp/add").count()-1,
                "actual_preclaim_contentions":actual_contentions,"outer_contentions":counts,
                "source_contentions":[source_probe.capture_contentions(),source_probe.timing_contentions()],
                "worker_joined":true,"delivery_settled":true,"restart_source_authority":false,
                "physical_device":false,"qualified":false,"borrowed_runtime_unchanged":true,
            }),
        );
    }

    #[test]
    #[ignore = "requires pinned CPU fixture/runtime, rebuilt worker, RAM evidence and native namespaces; run serially"]
    fn native_cpu_preclaim_db_release_preserves_one_original_append() {
        run_preclaim(PreclaimCase::DbRelease);
    }
    #[test]
    #[ignore = "requires pinned CPU fixture/runtime, rebuilt worker, RAM evidence and native namespaces; run serially"]
    fn native_cpu_preclaim_capture_release_preserves_one_original_append() {
        run_preclaim(PreclaimCase::CaptureRelease);
    }
    #[test]
    #[ignore = "requires pinned CPU fixture/runtime, rebuilt worker, RAM evidence and native namespaces; run serially"]
    fn native_cpu_preclaim_timing_release_preserves_one_original_append() {
        run_preclaim(PreclaimCase::TimingRelease);
    }
    #[test]
    #[ignore = "requires pinned CPU fixture/runtime, rebuilt worker, RAM evidence and native namespaces; run serially"]
    fn native_cpu_preclaim_original_cancel_before_claim_sends_no_append() {
        run_preclaim(PreclaimCase::Cancel);
    }
    #[test]
    #[ignore = "requires pinned CPU fixture/runtime, rebuilt worker, RAM evidence and native namespaces; run serially"]
    fn native_cpu_preclaim_original_cap_before_claim_sends_no_append() {
        run_preclaim(PreclaimCase::Cap);
    }
}
