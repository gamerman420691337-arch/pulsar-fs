// Included inside captured_cpu_browser_native_tests. Eligibility is deliberately
// simulated in an owned disposable database; no public acceptance is performed.

#[cfg(all(target_arch = "x86_64", feature = "cuda"))]
struct CudaHandoverInputs {
    policy: ArtifactReference,
    from_file: ArtifactReference,
    to_file: ArtifactReference,
    from: DeclaredInferenceProfile,
    to: DeclaredInferenceProfile,
}

#[cfg(all(target_arch = "x86_64", feature = "cuda"))]
impl CudaHandoverInputs {
    fn load(inputs: &Inputs) -> Result<Self> {
        let load = |path_name: &str, hash_name: &str| -> Result<ArtifactReference> {
            pinned(path_env(path_name)?, &std::env::var(hash_name)?)
        };
        let policy = load(
            "PULSAR_BROWSER_CUDA_POLICY",
            "PULSAR_BROWSER_CUDA_POLICY_SHA256",
        )?;
        let from_file = load(
            "PULSAR_BROWSER_CUDA_PROFILE",
            "PULSAR_BROWSER_CUDA_PROFILE_SHA256",
        )?;
        let to_file = load(
            "PULSAR_BROWSER_CPU_PROFILE",
            "PULSAR_BROWSER_CPU_PROFILE_SHA256",
        )?;
        let from: DeclaredInferenceProfile =
            serde_json::from_slice(&read_bounded(&from_file.path, 1024 * 1024)?)?;
        let to: DeclaredInferenceProfile =
            serde_json::from_slice(&read_bounded(&to_file.path, 1024 * 1024)?)?;
        from.validate()?;
        to.validate()?;
        ensure!(
            from.provider == InferenceProvider::Cuda,
            "original profile must be CUDA"
        );
        ensure!(
            to.provider == InferenceProvider::Cpu,
            "target profile must be CPU"
        );
        ensure!(
            from.model == inputs.model.identity && to.model == inputs.model.identity,
            "profile model differs from pinned fixture"
        );
        ensure!(
            encode(&to)? == encode(&inputs.profile()?)?,
            "target profile differs from captured CPU policy"
        );
        Ok(Self {
            policy,
            from_file,
            to_file,
            from,
            to,
        })
    }

    fn verify_unchanged(&self) -> Result<()> {
        for artifact in [&self.policy, &self.from_file, &self.to_file] {
            pinned(artifact.path.clone(), &artifact.identity.sha256)?;
        }
        Ok(())
    }
}

fn with_timing_observers<T>(
    client: &Client<'_>,
    players: &mut [&mut AuthoredPlayer],
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let stopped = AtomicBool::new(false);
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    std::thread::scope(|scope| {
        let stop = Stop(&stopped);
        let observer = scope.spawn(|| -> Result<()> {
            while !stopped.load(Ordering::Acquire) {
                for player in players.iter_mut() {
                    player.observe(client)?;
                }
                std::thread::park_timeout(Duration::from_millis(5));
            }
            Ok(())
        });
        let result = operation();
        drop(stop);
        let observed = observer
            .join()
            .map_err(|_| anyhow::anyhow!("authored timing observer panicked"))?;
        match (result, observed) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(operation), Err(observer)) => Err(anyhow::anyhow!(
                "operation failed: {operation:#}; timing observer failed: {observer:#}"
            )),
        }
    })
}

#[cfg(target_arch = "x86_64")]
fn browser_fallback_fixture(
    client: &Client<'_>,
    inputs: &Inputs,
    media: &BrowserMediaBinding,
) -> Result<BrowserInferenceFallbackSelection> {
    let from = inputs.profile()?;
    let mut to = from.clone();
    to.cpu_threads = to
        .cpu_threads
        .checked_add(1)
        .context("fixture CPU count overflow")?;
    ensure!(
        to.cpu_threads <= inputs.owner_cpu_threads,
        "fixture alternate profile exceeds explicit owner thread pool"
    );
    browser_fallback_fixture_for_profiles(client, inputs, media, &from, &to)
}

#[cfg(target_arch = "x86_64")]
fn browser_fallback_fixture_for_profiles(
    client: &Client<'_>,
    inputs: &Inputs,
    media: &BrowserMediaBinding,
    from: &DeclaredInferenceProfile,
    to: &DeclaredInferenceProfile,
) -> Result<BrowserInferenceFallbackSelection> {
    let source = SourceArtifact {
        source_version: media.selection.source_version.clone(),
        path: inputs.media.path.clone(),
        identity: media.selection.source.clone(),
    };
    let model = SourceArtifact {
        source_version: SourceVersionId::new(inputs.model.identity.sha256.clone())?,
        path: inputs.model.path.clone(),
        identity: inputs.model.identity.clone(),
    };
    let selected = BrowserInferenceFallbackSelection {
        schema_version: 1,
        tier: InferenceEquivalenceTier::Default,
        axis: Axis::Stroke,
        equivalents: vec![InferenceFallbackReference {
            dossier_id: ArtifactId::new("0".repeat(64))?,
            expected_revision: 2,
            expected_report_sha256: "0".repeat(64),
        }],
        maximum_fallbacks: 1,
        source_policy: BrowserInferenceFallbackSourcePolicy {
            schema_version: 1,
            namespace: BrowserInferenceFallbackSourceNamespace::BrowserMediaCapture,
            sources: Some(vec![media.selection.source.clone()]),
        },
        view_policy: BrowserInferenceFallbackViewPolicy {
            schema_version: 1,
            namespace: BrowserInferenceFallbackSourceNamespace::BrowserMediaCapture,
            applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
        },
    };
    super::inference_fallback_native_tests::install_simulated_browser_eligibility(
        client.engine,
        &client.actor,
        &source,
        &model,
        from,
        to,
        &selected,
    )
}

#[cfg(all(target_arch = "x86_64", feature = "cuda"))]
#[allow(clippy::too_many_arguments)]
fn browser_cuda_handover(
    client: &Client<'_>,
    inputs: &Inputs,
    media: &BrowserMediaBinding,
    player: &mut AuthoredPlayer,
    selected: &BrowserInferenceFallbackSelection,
    profiles: &CudaHandoverInputs,
    record_target: bool,
    discard_recording: &mut Option<Request>,
) -> Result<Value> {
    player.observe(client)?;
    let mut initial_options = options(inputs, media, player)?;
    initial_options
        .inference
        .as_mut()
        .context("inference absent")?
        .profile = profiles.from.clone();
    initial_options.inference_fallback = Some(selected.clone());
    initial_options.validate()?;
    let deadline = Instant::now() + Duration::from_millis(u64::from(LEASE_MS));
    let intent = client.intent(
        Some(1),
        Command::StartLiveCapture {
            engine_epoch: media.engine_epoch.clone(),
            options: initial_options.clone(),
        },
    );
    let original = capture(with_timing_observers(client, &mut [&mut *player], || {
        Ok(client.engine.handle(intent).result?)
    })?)?;
    let (failure, next_epoch) = loop {
        player.observe(client)?;
        let current = client.capture(&original.identity.epoch)?;
        if let Some(transition) = current
            .inference
            .as_ref()
            .and_then(|i| i.fallback_transition.as_ref())
        {
            match transition.phase {
                BrowserInferenceFallbackPhase::Started => {
                    break (
                        transition.failure.clone(),
                        transition
                            .next_epoch
                            .clone()
                            .context("started target absent")?,
                    )
                }
                BrowserInferenceFallbackPhase::Refused => {
                    bail!("automatic target refused: {:?}", transition.error)
                }
                BrowserInferenceFallbackPhase::Preparing => {}
            }
        }
        ensure!(
            Instant::now() < deadline,
            "original capture deadline expired before target publication: active={}, retirement={:?}, inference_state={:?}, inference_error={:?}, transition_phase={:?}",
            current.active,
            current.retirement,
            current.inference.as_ref().map(|inference| inference.state),
            current.inference.as_ref().and_then(|inference| inference.error.as_ref()),
            current.inference.as_ref().and_then(|inference| inference.fallback_transition.as_ref()).map(|transition| transition.phase)
        );
        std::thread::park_timeout(Duration::from_millis(5));
    };
    ensure!(
        failure.observation.provider == InferenceProvider::Cuda,
        "settled failure was not CUDA provider absence"
    );
    ensure!(
        failure.manifest.stream.identity == original.identity,
        "failure manifest lost original identity"
    );
    let target = loop {
        player.observe(client)?;
        let report = client.capture(&next_epoch)?;
        let inference = report
            .inference
            .as_ref()
            .context("target inference absent")?;
        if inference.state == LiveCaptureInferenceState::Running {
            break report;
        }
        ensure!(
            report.active && inference.error.is_none(),
            "target retired before Ready: {:?}",
            inference.error
        );
        ensure!(
            Instant::now() < deadline,
            "original capture deadline expired before target Ready"
        );
        std::thread::park_timeout(Duration::from_millis(5));
    };
    let info = target
        .inference
        .as_ref()
        .context("target inference absent")?;
    let history = info
        .fallback
        .as_ref()
        .context("target fallback history absent")?;
    ensure!(
        target.identity.epoch != original.identity.epoch
            && info.attempt_id != failure.manifest.stream.attempt_id,
        "target reused original attempt or epoch"
    );
    ensure!(
        target.options.browser_media.as_ref() == Some(media),
        "target changed source media binding"
    );
    ensure!(
        target
            .options
            .browser_analysis
            .as_ref()
            .context("target feed absent")?
            .timing_binding_id
            == initial_options
                .browser_analysis
                .as_ref()
                .context("original feed absent")?
                .timing_binding_id,
        "target changed timing owner"
    );
    ensure!(
        target
            .options
            .inference
            .as_ref()
            .context("target profile absent")?
            .profile
            .provider
            == InferenceProvider::Cpu,
        "target was not CPU"
    );
    ensure!(
        history.predecessor.as_ref() == Some(&original.identity) && history.history.len() == 1,
        "target lost original fallback history"
    );
    ensure!(
        target.options.lease_ms <= initial_options.lease_ms,
        "target extended original lease"
    );
    let recording = if record_target {
        let request = client.intent(
            Some(1),
            Command::StartLiveRecording {
                epoch: target.identity.epoch.clone(),
                maximum_samples: 4,
            },
        );
        let ResponseBody::LiveRecordingPage(page) = client.engine.handle(request.clone()).result?
        else {
            bail!("target recording start missing")
        };
        *discard_recording = Some(client.intent(
            Some(1),
            Command::DiscardLiveRecording {
                epoch: target.identity.epoch.clone(),
                recording_request_id: request.request_id.clone(),
            },
        ));
        ensure!(
            page.fallback.is_some() && page.total_samples == 0,
            "target recording omitted fallback history"
        );
        let base = player.frame_base()?;
        let first = upload(client, inputs, &target, player, base, 0, deadline)?;
        let second = upload(client, inputs, &target, player, base + 1, 1, deadline)?;
        let point = estimate(
            client,
            inputs,
            &target,
            player,
            (&first, base),
            (&second, base + 1),
            1,
            deadline,
        )?;
        Some((request.request_id, point))
    } else {
        None
    };
    capture(client.call(
        Some(1),
        Command::StopLiveCapture {
            epoch: original.identity.epoch.clone(),
        },
    )?)?;
    let stopped = loop {
        let report = client.capture(&target.identity.epoch)?;
        if !report.active {
            break report;
        }
        ensure!(
            Instant::now() < deadline,
            "target survived root stop until original deadline"
        );
        std::thread::park_timeout(Duration::from_millis(5));
    };
    ensure!(
        stopped.retirement == Some(LiveCaptureRetirement::Stopped),
        "target did not retire from root stop"
    );
    let mut value = json!({"case":if record_target {"browser_cuda_recording"} else {"browser_cuda_handover"},"passed":true,"simulated_eligibility":true,"actual_provider_absence":true,"actual_target_ready":true,"root_stop_retired_target":true,"physical_device":false,"qualified":false,"failure":failure,"original":original,"target":target,"stopped":stopped});
    if let Some((recording_request_id, point)) = recording {
        let ResponseBody::LiveRecordingPage(page) = client.call(
            Some(1),
            Command::LiveRecordingPage {
                epoch: target.identity.epoch.clone(),
                after: 0,
                limit: 64,
            },
        )?
        else {
            bail!("target recording page missing")
        };
        page.validate()?;
        ensure!(
            !page.active
                && page.recording_request_id == recording_request_id
                && page.total_samples == 1
                && page.samples.len() == 1
                && page.fallback.is_some(),
            "actual target recording lost fallback provenance"
        );
        ensure!(
            matches!(
                page.samples[0].result,
                LiveInferenceReply::CapturedCpuEstimated { .. }
            ),
            "target recording is not captured CPU evidence"
        );
        let LiveInferenceReply::CapturedCpuEstimated { input, .. } = &page.samples[0].result else {
            unreachable!()
        };
        let at = page
            .clock
            .project_at_source(input.pair.current_time)?
            .quantize_nanoseconds()?
            .time
            .as_nanos();
        let range = TimeRange::new(
            ProjectTime::from_nanos(at.saturating_sub(100_000_000).max(0)),
            ProjectTime::from_nanos(
                at.checked_add(100_000_000)
                    .context("recording range overflow")?,
            ),
        )?;
        let ResponseBody::Candidate(candidate) = client.call(
            Some(1),
            Command::StageLiveRecording {
                options: LiveRecordingStageOptions {
                    engine_epoch: target.engine_epoch.clone(),
                    recordings: vec![LiveRecordingSelection {
                        epoch: target.identity.epoch.clone(),
                        recording_request_id: recording_request_id.clone(),
                    }],
                    range,
                    maximum_interpolation_ns: 100_000_000,
                },
            },
        )?
        else {
            bail!("native target recording candidate absent")
        };
        ensure!(
            candidate
                .review
                .iter()
                .any(|item| item.reason.as_str() == "live_recording_unqualified"),
            "native recording omitted unqualified review"
        );
        let snapshot = client.snapshot()?;
        let source = snapshot
            .sources
            .iter()
            .find(|s| s.kind == SourceKind::LiveRecording)
            .context("native recording source absent")?;
        client.call(
            Some(1),
            Command::AllowProjectPackaging {
                owner_proof: client.engine.package_owner_token.clone(),
            },
        )?;
        let ResponseBody::PackageExport(started) =
            client.call(Some(1), Command::StartProjectExport)?
        else {
            bail!("native recording export absent")
        };
        let export_deadline = Instant::now() + Duration::from_secs(60);
        let descriptor = loop {
            let ResponseBody::PackageExport(status) = client.call(
                None,
                Command::ProjectPackageStatus {
                    operation_id: started.operation_id.clone(),
                },
            )?
            else {
                bail!("native recording export status absent")
            };
            if status.state == PackageExportOperationState::Ready {
                break status.artifact.context("native export artifact absent")?;
            }
            ensure!(
                !matches!(
                    status.state,
                    PackageExportOperationState::Failed
                        | PackageExportOperationState::Cancelled
                        | PackageExportOperationState::Interrupted
                ) && Instant::now() < export_deadline,
                "native recording export failed or timed out: {status:?}"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        };
        let bytes = fs::read(
            client
                .engine
                .config
                .state_dir
                .join("project-packages")
                .join(&descriptor.sha256),
        )?;
        ensure!(
            bytes.len() as u64 == descriptor.byte_len
                && format!("{:x}", Sha256::digest(&bytes)) == descriptor.sha256,
            "native recording package identity differs"
        );
        let decoded = crate::project_package_format::read_package(
            &mut std::io::Cursor::new(&bytes),
            |_| Ok(std::io::sink()),
            crate::project_package_format::PackageCodecLimits::default(),
        )?;
        let packaged = decoded
            .manifest
            .sources
            .iter()
            .find(|s| s.source_version == source.source_version)
            .context("native recording missing from package")?;
        let payload = packaged
            .browser_fallback_history
            .clone()
            .context("native recording history missing from package")?;
        crate::authority::project_package_imports::exercise_native_browser_portable_lifecycle(
            &bytes, &payload,
        );
        value["portable_archive_lifecycle_passed"] = json!(true);
        value["native_recording_package_sha256"] = json!(descriptor.sha256);
        value["target_recording"] = json!(page);
        value["target_estimate"] = json!(point);
    }
    Ok(value)
}

#[cfg(target_arch = "x86_64")]
fn browser_fallback_admission(
    client: &Client<'_>,
    inputs: &Inputs,
    media: &BrowserMediaBinding,
    player: &mut AuthoredPlayer,
    selected: &BrowserInferenceFallbackSelection,
) -> Result<Value> {
    player.observe(client)?;
    let mut initial_options = options(inputs, media, player)?;
    initial_options.inference_fallback = Some(selected.clone());
    let initial = client.intent(
        Some(1),
        Command::StartLiveCapture {
            engine_epoch: media.engine_epoch.clone(),
            options: initial_options.clone(),
        },
    );
    let mut stale = initial_options.clone();
    stale.inference_fallback.as_mut().unwrap().equivalents[0].expected_revision += 1;
    let stale_request = client.intent(
        Some(1),
        Command::StartLiveCapture {
            engine_epoch: media.engine_epoch.clone(),
            options: stale,
        },
    );
    let before: i64 =
        client
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let refusal = with_timing_observers(client, &mut [&mut *player], || {
        client
            .engine
            .handle(stale_request)
            .result
            .err()
            .context("stale reference was admitted")
    })?;
    ensure!(
        refusal.code == ErrorCode::RevisionConflict,
        "stale reference refusal differs: {refusal}"
    );
    ensure!(
        client
            .engine
            .live_captures
            .lock()
            .unwrap()
            .retained_count_for_test()
            == 0,
        "refused selection created a capture"
    );
    ensure!(
        client
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?
            == before,
        "refused selection armed its reader barrier"
    );

    eprintln!("fallback-recording-phase: start");
    let end = Instant::now() + Duration::from_millis(u64::from(LEASE_MS));
    let started = capture(with_timing_observers(client, &mut [&mut *player], || {
        Ok(client.engine.handle(initial.clone()).result?)
    })?)?;
    let ready = loop {
        player.observe(client)?;
        let current = client.capture(&started.identity.epoch)?;
        ensure!(
            current.active && current.inference.as_ref().unwrap().error.is_none(),
            "selected capture retired before Ready: active={}, retirement={:?}, state={:?}, error={:?}",
            current.active, current.retirement,
            current.inference.as_ref().map(|i| i.state),
            current.inference.as_ref().and_then(|i| i.error.as_ref())
        );
        if current.inference.as_ref().unwrap().state == LiveCaptureInferenceState::Running {
            break current;
        }
        ensure!(
            Instant::now() < end,
            "selected capture exceeded original observation bound"
        );
        std::thread::park_timeout(Duration::from_millis(5));
    };
    eprintln!("fallback-recording-phase: ready");
    let admission = ready
        .inference
        .as_ref()
        .unwrap()
        .fallback
        .as_ref()
        .context("native fallback admission absent")?;
    ensure!(
        admission.selection == selected.identity()?
            && admission.ordinal == 0
            && admission.predecessor.is_none(),
        "initial selection history differs"
    );
    ensure!(
        client
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?
            == 42,
        "retained browser choice lacks reader42"
    );
    let replay = capture(client.engine.handle(initial.clone()).result?)?;
    ensure!(
        replay.identity == ready.identity
            && replay.inference.as_ref().unwrap().attempt_id
                == ready.inference.as_ref().unwrap().attempt_id,
        "exact initial retry started another worker"
    );
    eprintln!("fallback-recording-phase: recording-start");
    let recording_request = client.intent(
        Some(1),
        Command::StartLiveRecording {
            epoch: ready.identity.epoch.clone(),
            maximum_samples: 4,
        },
    );
    let ResponseBody::LiveRecordingPage(initial_page) =
        client.engine.handle(recording_request.clone()).result?
    else {
        bail!("initial recording page absent")
    };
    ensure!(
        initial_page.fallback.is_some() && initial_page.total_samples == 0,
        "initial recording omitted fallback provenance"
    );

    eprintln!("fallback-recording-phase: recording-started");
    let (child_media, child_player, successor_intent, child) =
        prepare_successor(client, &ready, media, player, end)?;
    eprintln!("fallback-recording-phase: successor-ready");
    let child_admission = child
        .inference
        .as_ref()
        .unwrap()
        .fallback
        .as_ref()
        .context("successor fallback admission absent")?;
    ensure!(
        child_admission.selection == admission.selection
            && child_admission.semantic_identity == admission.semantic_identity
            && child_admission.ordinal == admission.ordinal,
        "ordinary successor lost selected semantics or reset fallback history"
    );
    let base = player.frame_base()?;
    let first = upload(client, inputs, &ready, player, base, 0, end)?;
    let second = upload(client, inputs, &ready, player, base + 1, 1, end)?;
    let point = estimate(
        client,
        inputs,
        &ready,
        player,
        (&first, base),
        (&second, base + 1),
        1,
        end,
    )?;
    eprintln!("fallback-recording-phase: estimated");
    client
        .engine
        .handle(request(
            &client.actor,
            None,
            None,
            Command::WithdrawInferenceEquivalence {
                dossier_id: selected.equivalents[0].dossier_id.clone(),
                expected_record_revision: 2,
                reason: "SOFTWARE SIMULATION fixture withdrawal; no real approval".into(),
                owner_proof: client.engine.model_pack_owner_token.clone(),
            },
        ))
        .result?;
    eprintln!("fallback-recording-phase: withdrawn");
    let retired = client.capture(&ready.identity.epoch)?;
    let child_retired = client.capture(&child.identity.epoch)?;
    ensure!(
        !retired.active
            && !child_retired.active
            && retired.retirement == Some(LiveCaptureRetirement::AuthorityLost)
            && child_retired.retirement == Some(LiveCaptureRetirement::AuthorityLost),
        "withdrawal did not retire selected authority"
    );
    let historical = capture(client.engine.handle(initial).result?)?;
    ensure!(
        !historical.active && historical.identity == ready.identity,
        "initial replay revived withdrawn authority"
    );
    let ResponseBody::BrowserAnalysisSuccessor(historical_child) =
        client.engine.handle(successor_intent).result?
    else {
        bail!("successor history response differs")
    };
    ensure!(
        !historical_child.successor.active,
        "successor replay revived withdrawn authority"
    );
    let ResponseBody::LiveRecordingPage(recording) = client.call(
        Some(1),
        Command::LiveRecordingPage {
            epoch: ready.identity.epoch.clone(),
            after: 0,
            limit: 64,
        },
    )?
    else {
        bail!("retired recording page absent")
    };
    eprintln!("fallback-recording-phase: recording-received");
    recording.validate()?;
    ensure!(
        !recording.active
            && recording.total_samples == 1
            && recording.samples.len() == 1
            && recording.recording_request_id == recording_request.request_id
            && serde_json::to_value(
                &recording
                    .fallback
                    .as_ref()
                    .context("recording fallback lost")?
                    .admission
            )? == serde_json::to_value(admission)?
            && serde_json::to_value(&recording.fallback.as_ref().unwrap().options)?
                == serde_json::to_value(&ready.options)?,
        "recorded estimate lost original fallback intent or admission"
    );
    let LiveInferenceReply::CapturedCpuEstimated { input, .. } = &recording.samples[0].result
    else {
        bail!("recording did not retain actual captured CPU estimate")
    };
    let at = recording
        .clock
        .project_at_source(input.pair.current_time)?
        .quantize_nanoseconds()?
        .time
        .as_nanos();
    let range = TimeRange::new(
        ProjectTime::from_nanos(at.saturating_sub(100_000_000).max(0)),
        ProjectTime::from_nanos(
            at.checked_add(100_000_000)
                .context("recording range overflow")?,
        ),
    )?;
    eprintln!("fallback-recording-phase: candidate-start");
    let ResponseBody::Candidate(candidate) = client.call(
        Some(1),
        Command::StageLiveRecording {
            options: LiveRecordingStageOptions {
                engine_epoch: ready.engine_epoch.clone(),
                recordings: vec![LiveRecordingSelection {
                    epoch: ready.identity.epoch.clone(),
                    recording_request_id: recording_request.request_id.clone(),
                }],
                range,
                maximum_interpolation_ns: 100_000_000,
            },
        },
    )?
    else {
        bail!("recording candidate absent")
    };
    ensure!(
        candidate
            .review
            .iter()
            .any(|r| r.reason.as_str() == "live_recording_unqualified"),
        "recording candidate omitted unqualified review"
    );
    eprintln!("fallback-recording-phase: candidate-ready");
    let snapshot = client.snapshot()?;
    ensure!(
        snapshot.revision.value() == 1,
        "recording staging committed project motion"
    );
    eprintln!("fallback-recording-phase: snapshot-received");
    let source_version = snapshot
        .sources
        .iter()
        .find(|s| s.kind == SourceKind::LiveRecording)
        .context("candidate original recording source absent")?
        .source_version
        .clone();
    let source = {
        let db = client.engine.db.lock().unwrap();
        client
            .engine
            .source(&db, &client.project, &source_version)?
    };
    eprintln!("fallback-recording-phase: source-resolved");
    let source_bytes = fs::read(&source.path)?;
    ensure!(
        source_bytes.len() <= MAX_CONTROL_BYTES
            && source_bytes.len() as u64 == source.identity.byte_len
            && format!("{:x}", Sha256::digest(&source_bytes)) == source.identity.sha256,
        "recording candidate source bytes differ"
    );
    eprintln!("fallback-recording-phase: source-read");
    let provenance: Value = serde_json::from_slice(&source_bytes)?;
    ensure!(
        provenance["qualified"] == false
            && provenance["pages"][0]["fallback"] == serde_json::to_value(&recording.fallback)?,
        "candidate snapshot dropped recording fallback provenance"
    );
    eprintln!("fallback-recording-phase: candidate-verified");
    client.call(
        Some(1),
        Command::DiscardLiveRecording {
            epoch: ready.identity.epoch.clone(),
            recording_request_id: recording_request.request_id.clone(),
        },
    )?;
    eprintln!("fallback-recording-phase: recording-discarded");
    release(client, &child_media, &child_player)?;
    release(client, media, player)?;
    // No qualifying decision, device action or alternate worker is asserted.
    Ok(
        json!({"case":"browser_fallback_admission","simulated_eligibility":true,
        "public_equivalence_acceptance_called":false,"public_withdrawal_called":true,
        "selection":selected,"stale_reference_refused":true,"refusal":refusal,
        "ready":ready,"prepared":child,"estimate":point,"retired":retired,"child_retired":child_retired,
        "original_media":media,"successor_media":child_media,
        "original_timing_binding":player.binding,"successor_timing_binding":child_player.binding,
        "initial_retry_kept_original_attempt":true,"retired_replays_inert":true,
        "recording_provenance_retained":true,"candidate_provenance_retained":true,
        "recording":recording,"candidate":candidate,"candidate_source":source.identity,
        "candidate_source_hex":source_bytes.iter().map(|byte|format!("{byte:02x}")).collect::<String>(),
        "reader_version":42,"original_lease_ms":LEASE_MS,"freshness_limit_ns":MAX_AGE_NS,
        "actual_alternate_execution":false,"physical_device":false,"qualified":false}),
    )
}

#[cfg(target_arch = "x86_64")]
#[test]
#[ignore = "owned sterile input/CPU closure/host supervisor required; simulated eligibility, no actual operator approval or physical qualification"]
fn native_browser_fallback_admission_refuses_stale_and_withdrawn_records() {
    let output = path_env("PULSAR_CAPTURED_CPU_BROWSER_OUTPUT").expect("owned output");
    let inputs = Inputs::load().expect("pinned inputs");
    let result = campaign_case(&inputs, "browser_fallback_admission");
    inputs
        .verify_unchanged()
        .expect("borrowed subjects unchanged");
    let receipt = json!({"schema_version":1,"scope":"Actual original browser CPU admission and withdrawal over simulated eligibility in a disposable DB; no automatic alternate execution or qualification",
        "passed":result["passed"],"simulated_eligibility":true,"qualified":false,"physical_device":false,
        "public_equivalence_acceptance_called":false,"subjects":inputs.subjects(),"cases":[result]});
    save(&output.join("browser-fallback-admission.json"), &receipt).expect("retain scoped receipt");
    assert_eq!(receipt["passed"], true, "{}", receipt["cases"][0]["error"]);
}

#[cfg(all(target_arch = "x86_64", feature = "cuda"))]
#[test]
#[ignore = "requires pinned CUDA and CPU policies/profiles, absent CUDA provider, sterile browser inputs, delegated cgroups and owned supervisor"]
fn native_browser_cuda_provider_absence_starts_captured_cpu_target() {
    let output = path_env("PULSAR_CAPTURED_CPU_BROWSER_OUTPUT").expect("owned output");
    let inputs = Inputs::load().expect("pinned browser inputs");
    let cuda = CudaHandoverInputs::load(&inputs).expect("pinned handover inputs");
    let result = campaign_case(&inputs, "browser_cuda_handover");
    inputs
        .verify_unchanged()
        .expect("borrowed browser subjects unchanged");
    cuda.verify_unchanged()
        .expect("borrowed handover subjects unchanged");
    let receipt = json!({"schema_version":1,"scope":"Actual worker provider absence and captured CPU target with simulated equivalence eligibility and authored browser timing; no measured equivalence or physical qualification", "passed":result["passed"],"simulated_eligibility":true,"physical_device":false,"qualified":false,"subjects":inputs.subjects(),"cuda_policy":cuda.policy,"from_profile":cuda.from_file,"to_profile":cuda.to_file,"cases":[result]});
    save(&output.join("browser-cuda-handover.json"), &receipt).expect("retain scoped receipt");
    assert_eq!(receipt["passed"], true, "{}", receipt["cases"][0]["error"]);
}

#[cfg(all(target_arch = "x86_64", feature = "cuda"))]
#[test]
#[ignore = "requires pinned native CUDA/CPU subjects, sterile browser inputs and owned supervisor"]
fn native_browser_cuda_fallback_records_captured_cpu_target() {
    let output = path_env("PULSAR_CAPTURED_CPU_BROWSER_OUTPUT").expect("owned output");
    let inputs = Inputs::load().expect("pinned browser inputs");
    let cuda = CudaHandoverInputs::load(&inputs).expect("pinned handover inputs");
    let result = campaign_case(&inputs, "browser_cuda_recording");
    inputs
        .verify_unchanged()
        .expect("borrowed browser subjects unchanged");
    cuda.verify_unchanged()
        .expect("borrowed handover subjects unchanged");
    let receipt = json!({"schema_version":1,"scope":"Actual captured CPU fallback target recording with simulated eligibility and authored browser timing; no qualification", "passed":result["passed"],"simulated_eligibility":true,"physical_device":false,"qualified":false,"subjects":inputs.subjects(),"cuda_policy":cuda.policy,"from_profile":cuda.from_file,"to_profile":cuda.to_file,"cases":[result]});
    save(&output.join("browser-cuda-recording.json"), &receipt).expect("retain scoped receipt");
    assert_eq!(receipt["passed"], true, "{}", receipt["cases"][0]["error"]);
}
