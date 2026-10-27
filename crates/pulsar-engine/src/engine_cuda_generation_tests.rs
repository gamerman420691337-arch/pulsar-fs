mod native_cuda_generation {
    include!("sqlite_sync_delay_tests.rs");
    use super::*;
    type CaptureEvidenceRow = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);
    use crate::{
        cuda_devices::CudaDeviceExpectation,
        cuda_owner_policy::CudaOwnerPolicy,
        resources::{GpuDeviceBudget, GpuUsage},
    };
    use std::{
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    struct RestoreRuntime(Option<std::ffi::OsString>);
    impl Drop for RestoreRuntime {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("PULSAR_ORT_DYLIB", value),
                None => std::env::remove_var("PULSAR_ORT_DYLIB"),
            }
        }
    }
    fn quiescent(engine: &Engine) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let ledger: serde_json::Value = serde_json::from_slice(
                &fs::read(engine.config.state_dir.join("worker-fence.json")).unwrap(),
            )
            .unwrap();
            let empty = ["groups", "retired", "staging"]
                .iter()
                .all(|field| ledger[field].as_object().unwrap().is_empty())
                && ledger["creation"]["records"]
                    .as_object()
                    .unwrap()
                    .is_empty();
            let gpu = engine
                .pool
                .gpu_usage(
                    &engine
                        .cuda_owner_policy
                        .as_ref()
                        .unwrap()
                        .policy()
                        .device
                        .device_uuid,
                )
                .unwrap();
            if empty
                && gpu == GpuUsage::default()
                && engine.pool.reserved_storage() == 0
                && engine.workers.lock().unwrap().is_empty()
            {
                return;
            }
            assert!(Instant::now() < end, "Engine CUDA ownership did not retire");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn completed(
        engine: &Arc<Engine>,
        session: &SessionId,
        project: &ProjectId,
        job: &JobId,
    ) -> JobSnapshot {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let ResponseBody::Job(status) = engine
                .handle(request(
                    session,
                    Some(project),
                    None,
                    Command::JobStatus {
                        job_id: job.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("job response")
            };
            if status.state == "completed" {
                return status;
            }
            assert!(
                !["failed", "cancelled"].contains(&status.state.as_str()),
                "{status:?}"
            );
            assert!(Instant::now() < end, "job deadline: {status:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn retain(engine: &Engine, status: &JobSnapshot, label: &str) {
        let db = engine.db.lock().unwrap();
        let raw: String = db
            .query_row(
                "SELECT manifest FROM attempts WHERE id=?1",
                [status.attempt_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let manifest: WorkerRequest = decode(&raw).unwrap();
        let raw_evidence: String = db
            .query_row(
                "SELECT artifacts FROM cuda_candidate_evidence WHERE attempt=?1",
                [status.attempt_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let evidence: WorkerCudaArtifacts = decode(&raw_evidence).unwrap();
        crate::cuda_result::verify_files(&manifest, &evidence).unwrap();
        assert!(!manifest
            .cuda_execution
            .as_ref()
            .unwrap()
            .library_directory
            .exists());
        let cuda_receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(&evidence.receipt.path).unwrap()).unwrap();
        assert_eq!(cuda_receipt["provider"], "cuda");
        assert_eq!(cuda_receipt["successful_runs"], 1);
        let motion: MotionProgram =
            serde_json::from_slice(&fs::read(manifest.output_dir.join("program.json")).unwrap())
                .unwrap();
        let actions = motion.tracks()[0].actions();
        assert_eq!(actions.len(), 2);
        assert!((actions[1].position().value() - (0.25 + 0.25 * 128.0 / 255.0)).abs() < 1e-5);
        if let Some(root) = std::env::var_os("PULSAR_ENGINE_CUDA_EVIDENCE") {
            let root = PathBuf::from(root).join(label);
            assert!(!root.exists());
            artifacts::private_directory(&root).unwrap();
            fs::write(
                root.join("request.json"),
                serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();
            let mut saved = Vec::new();
            for name in [
                "program.json",
                "receipt.json",
                "cuda-inference.json",
                "cuda-trace.json",
            ] {
                fs::copy(manifest.output_dir.join(name), root.join(name)).unwrap();
                let (sha256, byte_len) =
                    artifacts::digest_file(&root.join(name), 64 << 20).unwrap();
                saved.push(serde_json::json!({"name":name,"sha256":sha256,"byte_len":byte_len}));
            }
            fs::write(
                root.join("artifacts.json"),
                serde_json::to_vec_pretty(&saved).unwrap(),
            )
            .unwrap();
            fs::write(
                root.join("job.json"),
                serde_json::to_vec_pretty(status).unwrap(),
            )
            .unwrap();
            crate::cuda_result::verify_bytes(
                &manifest,
                &fs::read(root.join("cuda-inference.json")).unwrap(),
                &fs::read(root.join("cuda-trace.json")).unwrap(),
            )
            .unwrap();
        }
    }
    #[test]
    #[ignore = "requires explicit CUDA worker, real native image/NVML and authored model, NVIDIA device and delegated cgroups; run serially"]
    fn native_engine_generates_accepts_reopens_and_explicitly_resumes_with_fresh_cuda_ownership() {
        native_engine_cuda_case(false, false, false, false, None);
    }
    #[test]
    #[ignore = "requires rebuilt CUDA worker, actual native image/NVML and authored model, NVIDIA device and delegated cgroups; serial only"]
    fn native_engine_starts_owned_live_cuda_without_offline_jobs() {
        native_engine_cuda_case(true, false, false, false, None);
    }
    #[test]
    #[ignore = "requires rebuilt CUDA worker, actual image/NVML and authored model, NVIDIA device and delegated cgroups; serial only"]
    fn native_engine_public_live_cuda_start_schedule_finish_and_reopen() {
        native_engine_cuda_case(true, true, false, false, None);
    }
    #[test]
    #[ignore = "requires CUDA worker, actual native image/NVML, authored model and delegated cgroups; serial only"]
    fn native_engine_public_live_cuda_continues_explicit_and_derived_epochs() {
        native_engine_cuda_case(true, true, true, false, None);
    }
    #[test]
    #[ignore = "requires CUDA worker, actual native image/NVML, authored model and delegated cgroups; serial only"]
    fn native_engine_public_live_cuda_restarts_without_rearming_old_epoch() {
        native_engine_cuda_case(true, true, false, true, None);
    }
    #[test]
    #[ignore = "requires CUDA worker, actual image/NVML and delegated cgroups; serial only"]
    fn native_engine_cuda_finalizer_stop_before_worker_finish() {
        native_engine_cuda_case(true, false, false, false, Some("before_finish"));
    }
    #[test]
    #[ignore = "requires CUDA worker, actual image/NVML and delegated cgroups; serial only"]
    fn native_engine_cuda_finalizer_real_lease_expiry_before_commit() {
        native_engine_cuda_case(true, false, false, false, Some("before_commit"));
    }
    #[test]
    #[ignore = "requires CUDA worker, actual image/NVML and delegated cgroups; serial only"]
    fn native_engine_cuda_finalizer_real_lease_expiry_after_commit() {
        native_engine_cuda_case(true, false, false, false, Some("after_commit"));
    }
    #[test]
    #[ignore = "requires CUDA worker, actual image/NVML and delegated cgroups; serial only"]
    fn native_engine_cuda_finalizer_real_lease_expiry_inside_sqlite_sync() {
        native_engine_cuda_case(true, false, false, false, Some("sqlite_sync"));
    }
    #[test]
    #[ignore = "requires CUDA worker, actual image/NVML and owned cgroup freezer; serial only"]
    fn native_engine_cuda_stop_during_blocked_worker_finish() {
        native_engine_cuda_case(true, false, false, false, Some("blocked_finish"));
    }
    #[test]
    #[ignore = "requires CUDA worker, actual image/NVML and owned cgroup freezer; serial only"]
    fn native_engine_public_cuda_stop_during_blocked_worker_finish() {
        native_engine_cuda_case(true, true, false, false, Some("blocked_finish"));
    }
    fn finalizer_fault_case(
        engine: &Arc<Engine>,
        intent: &Request,
        identity: &LiveIdentity,
        attempt: &AttemptId,
        mut live: crate::live_cuda_stream::LiveCudaWorker,
        end: Instant,
        point: &str,
    ) {
        let _frozen = if point == "blocked_finish" {
            Some(live.freeze_for_test().unwrap())
        } else {
            None
        };
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
        let ((result, sync), stop) = std::thread::scope(|scope| {
            let joined = scope.spawn(move || {
                if point == "blocked_finish" {
                    crate::launch_pipes::blocked_read_hook_for_test(Box::new(move || {
                        entered_tx.send(()).unwrap();
                    }));
                    return (
                        engine.live_cuda_finalize(intent, &identity.epoch, attempt, live),
                        None,
                    );
                }
                if point == "sqlite_sync" {
                    let delay = SqliteSyncDelay::install(&engine.db, entered_tx, release_rx);
                    Engine::live_cuda_finalizer_hook_for_test(Box::new(
                        SqliteSyncDelay::checkpoint,
                    ));
                    let result = engine.live_cuda_finalize(intent, &identity.epoch, attempt, live);
                    let summary = delay.summary();
                    drop(delay);
                    return (result, Some(summary));
                }
                let target = point.to_owned();
                Engine::live_cuda_finalizer_hook_for_test(Box::new(move |phase| {
                    if phase == target {
                        entered_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(35)).unwrap();
                    }
                }));
                (
                    engine.live_cuda_finalize(intent, &identity.epoch, attempt, live),
                    None,
                )
            });
            entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            let stop = if matches!(point, "before_finish" | "blocked_finish") {
                if point == "blocked_finish" {
                    // Actual WouldBlock has been observed after Finish was sent;
                    // the finalizer must remain pending until cancellation.
                    std::thread::sleep(Duration::from_millis(100));
                    assert!(!joined.is_finished());
                }
                let session = intent.session.as_ref().unwrap();
                let stopped = capture_report(
                    engine
                        .handle(request(
                            session,
                            Some(&identity.project),
                            Some(0),
                            Command::StopLiveCapture {
                                epoch: identity.epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap(),
                );
                assert_eq!(stopped.retirement, Some(LiveCaptureRetirement::Stopped));
                if point == "blocked_finish" {
                    let cancellation_end = Instant::now() + Duration::from_secs(2);
                    while !joined.is_finished() {
                        assert!(
                            Instant::now() < cancellation_end,
                            "blocked Finish ignored Stop"
                        );
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                Some(stopped)
            } else {
                // Keep the real Engine-local clock and original thirty-second lease.
                std::thread::sleep(
                    (end + Duration::from_millis(50)).saturating_duration_since(Instant::now()),
                );
                None
            };
            if point != "blocked_finish" {
                release_tx.send(()).unwrap();
            }
            (joined.join().unwrap(), stop)
        });
        let error = result.unwrap_err();
        if point == "sqlite_sync" {
            assert_eq!(error.message, "CUDA finalization commit exceeded lease");
        }
        if point == "blocked_finish" {
            assert!(
                error.message.contains("CUDA live final reply failed"),
                "{error:?}"
            );
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert!(Instant::now() < end, "Stop relied on the capture deadline");
        }
        let report = capture_report(
            engine
                .handle(request(
                    intent.session.as_ref().unwrap(),
                    Some(&identity.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        report.validate().unwrap();
        assert!(!report.active);
        assert_eq!(
            report.retirement,
            Some(if matches!(point, "before_finish" | "blocked_finish") {
                LiveCaptureRetirement::Stopped
            } else {
                LiveCaptureRetirement::Expired
            })
        );
        let info = report.inference.as_ref().unwrap();
        assert!(info.cuda_evidence.is_none());
        assert!(info.latest.is_none());
        quiescent(engine);
        let db = engine.db.lock().unwrap();
        let expected = i64::from(matches!(point, "after_commit" | "sqlite_sync"));
        let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='live_cuda_evidence')",[],|r|r.get(0)).unwrap();
        let retained: i64 = if exists {
            db.query_row("SELECT COUNT(*) FROM live_cuda_evidence", [], |r| r.get(0))
                .unwrap()
        } else {
            0
        };
        assert_eq!(retained, expected);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        let base = PathBuf::from(
            std::env::var_os("PULSAR_ENGINE_CUDA_LIVE_EVIDENCE")
                .expect("pin new race evidence folder"),
        );
        assert!(!base.exists());
        artifacts::private_directory(&base).unwrap();
        if retained == 1 {
            let admission =
                crate::live_cuda_evidence::EvidenceAdmission::reserve(&engine.pool).unwrap();
            let raw: (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = db
                .query_row(
                    "SELECT manifest,summary,receipt,trace FROM live_cuda_evidence",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap();
            let evidence = admission.read(raw.0, raw.1, raw.2, raw.3).unwrap();
            assert_eq!(evidence.summary().successful_runs, 3);
            for (name, bytes) in [
                ("manifest.json", evidence.manifest_bytes()),
                ("summary.json", evidence.summary_bytes()),
                ("receipt.json", evidence.receipt()),
                ("original-trace.json", evidence.trace()),
            ] {
                fs::write(base.join(name), bytes).unwrap();
            }
        }
        fs::write(
            base.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        fs::write(base.join("checked.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "fault_point":point,"error":error,"stop":stop,"retained_rows":retained,"sqlite_sync":sync,
            "owned_cgroup_frozen_and_finish_read_would_block":point=="blocked_finish",
            "native_successful_runs":3,"original_lease_ms":30000,"clock_modified":false,
            "test_inserted_registry_entry":true,"test_build_pause_hook":true,
            "all_custody_retired":true,"completed_published":false,"qualified":false
        })).unwrap()).unwrap();
    }
    struct PublicLiveCase<'a> {
        engine: Arc<Engine>,
        session: SessionId,
        project: ProjectId,
        options: LiveCaptureOptions,
        source: SourceVersionId,
        settings: GenerationSettings,
        continuation: bool,
        restart: bool,
        finalizer_fault: Option<&'a str>,
        prepared: Option<crate::PreparedLiveCudaImage>,
    }
    fn public_live_case(case: PublicLiveCase<'_>) {
        let PublicLiveCase {
            mut engine,
            session,
            project,
            options,
            source,
            settings,
            continuation,
            restart,
            finalizer_fault,
            mut prepared,
        } = case;
        let generation = options.inference.as_ref().unwrap().clone();
        let started_at = Instant::now();
        let mut end = started_at + Duration::from_secs(30);
        let sources_before: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))
            .unwrap();
        let storage_before_refusal = engine.pool.reserved_storage();
        let mut bad = options.clone();
        bad.inference
            .as_mut()
            .unwrap()
            .profile
            .cuda_resources
            .as_mut()
            .unwrap()
            .device_ordinal = 99;
        let refused = engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::StartLiveCapture {
                    engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                    options: bad,
                },
            ))
            .result;
        assert!(refused.is_err());
        assert_eq!(engine.pool.reserved_storage(), storage_before_refusal);
        eprintln!(
            "live CUDA refused invalid ordinal after {:?}",
            started_at.elapsed()
        );
        let mut start = request(
            &session,
            Some(&project),
            Some(0),
            Command::StartLiveCapture {
                engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                options,
            },
        );
        let mut initial = capture_report(engine.handle(start.clone()).result.unwrap());
        assert!(initial.active);
        eprintln!(
            "live CUDA admitted after {:?}, inference={:?}",
            started_at.elapsed(),
            initial.inference
        );
        let mut restart_record = None;
        if restart {
            let prior = initial.clone();
            let prior_epoch = prior.identity.epoch.clone();
            loop {
                let report = capture_report(
                    engine
                        .handle(request(
                            &session,
                            Some(&project),
                            Some(0),
                            Command::LiveCaptureStatus {
                                epoch: prior_epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap(),
                );
                assert!(report.active, "{}", serde_json::to_string(&report).unwrap());
                if report.inference.as_ref().unwrap().state == LiveCaptureInferenceState::Running {
                    break;
                }
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(5));
            }
            let old_handshake = LiveBulkHandshake {
                version: PROTOCOL_VERSION,
                channel: LiveBulkChannel::LiveCapture,
                session: session.clone(),
                auth_token: TOKENS.with(|tokens| tokens.borrow()[session.as_str()].clone()),
                engine_epoch: prior.engine_epoch.clone(),
                identity: prior.identity.clone(),
            };
            for frame in 0..=1 {
                let bytes = vec![(frame * 64) as u8; LIVE_FRAME_BYTES];
                let report = capture_report(
                    engine
                        .handle(request(
                            &session,
                            Some(&project),
                            Some(0),
                            Command::LiveCaptureStatus {
                                epoch: prior_epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap(),
                );
                let header = LiveFrameHeader {
                    frame: FrameId::new(frame),
                    source_time: SourceTimestamp::new(frame as i64, 10).unwrap(),
                    capture_tick: report.server_elapsed.unwrap(),
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                };
                engine
                    .live_bulk_upload(&old_handshake, &header, &bytes)
                    .unwrap();
            }
            let running = loop {
                let report = capture_report(
                    engine
                        .handle(request(
                            &session,
                            Some(&project),
                            Some(0),
                            Command::LiveCaptureStatus {
                                epoch: prior_epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap(),
                );
                assert!(report.active, "{}", serde_json::to_string(&report).unwrap());
                if report.inference.as_ref().unwrap().completed == 1 {
                    break report;
                }
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(2));
            };
            let config = engine.config.clone();
            drop(engine);
            engine = reopen_after_transient_leases(config).unwrap();
            assert_ne!(engine.engine_epoch, prior.engine_epoch.as_str());
            quiescent(&engine);
            let retired = capture_report(engine.handle(start.clone()).result.unwrap());
            assert!(!retired.active);
            assert_eq!(
                retired.retirement,
                Some(LiveCaptureRetirement::EngineRestarted)
            );
            assert!(retired.inference.as_ref().unwrap().latest.is_none());
            assert!(retired.inference.as_ref().unwrap().cuda_evidence.is_none());
            assert!(engine.live_bulk_ready(&old_handshake, None).is_err());
            assert!(engine
                .handle(request(
                    &session,
                    Some(&project),
                    Some(0),
                    Command::FinishLiveCapture { epoch: prior_epoch }
                ))
                .result
                .is_err());
            let mut stale = start.clone();
            stale.request_id = id!(RequestId);
            assert_eq!(
                engine.handle(stale).result.unwrap_err().code,
                ErrorCode::DependencyMismatch
            );
            start = request(
                &session,
                Some(&project),
                Some(0),
                Command::StartLiveCapture {
                    engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                    options: prior.options.clone(),
                },
            );
            initial = capture_report(engine.handle(start.clone()).result.unwrap());
            assert!(initial.active);
            assert_ne!(initial.identity.epoch, prior.identity.epoch);
            assert_ne!(initial.identity.source, prior.identity.source);
            restart_record = Some(
                serde_json::json!({"prior_running":running,"replayed_retired":retired,"fresh_start":initial,"automatic_reactivation":false,"old_intake_refused":true,"old_finish_refused":true,"unknown_old_boot_refused":true,"ownership_retired_before_fresh_start":true}),
            );
            end = Instant::now() + Duration::from_secs(30);
        }
        let mut handoffs = Vec::new();
        if continuation {
            for derived in [false, true] {
                let prior = initial.clone();
                let prior_epoch = prior.identity.epoch.clone();
                loop {
                    let report = capture_report(
                        engine
                            .handle(request(
                                &session,
                                Some(&project),
                                Some(0),
                                Command::LiveCaptureStatus {
                                    epoch: prior_epoch.clone(),
                                },
                            ))
                            .result
                            .unwrap(),
                    );
                    assert!(report.active, "{}", serde_json::to_string(&report).unwrap());
                    if report.inference.as_ref().unwrap().state
                        == LiveCaptureInferenceState::Running
                    {
                        break;
                    }
                    assert!(Instant::now() < end);
                    std::thread::sleep(Duration::from_millis(5));
                }
                let old_handshake = LiveBulkHandshake {
                    version: PROTOCOL_VERSION,
                    channel: LiveBulkChannel::LiveCapture,
                    session: session.clone(),
                    auth_token: TOKENS.with(|tokens| tokens.borrow()[session.as_str()].clone()),
                    engine_epoch: prior.engine_epoch.clone(),
                    identity: prior.identity.clone(),
                };
                for frame in 0..=1 {
                    let bytes = vec![(frame * 64) as u8; LIVE_FRAME_BYTES];
                    let current = capture_report(
                        engine
                            .handle(request(
                                &session,
                                Some(&project),
                                Some(0),
                                Command::LiveCaptureStatus {
                                    epoch: prior_epoch.clone(),
                                },
                            ))
                            .result
                            .unwrap(),
                    );
                    let offset = if derived { 2 } else { 0 };
                    let header = LiveFrameHeader {
                        frame: FrameId::new(frame),
                        source_time: SourceTimestamp::new(offset + frame as i64, 10).unwrap(),
                        capture_tick: current.server_elapsed.unwrap(),
                        sha256: format!("{:x}", Sha256::digest(&bytes)),
                    };
                    engine
                        .live_bulk_upload(&old_handshake, &header, &bytes)
                        .unwrap();
                }
                let prior_running = loop {
                    let report = capture_report(
                        engine
                            .handle(request(
                                &session,
                                Some(&project),
                                Some(0),
                                Command::LiveCaptureStatus {
                                    epoch: prior_epoch.clone(),
                                },
                            ))
                            .result
                            .unwrap(),
                    );
                    assert!(report.active, "{}", serde_json::to_string(&report).unwrap());
                    if report.inference.as_ref().unwrap().completed == 1 {
                        break report;
                    }
                    assert!(Instant::now() < end);
                    std::thread::sleep(Duration::from_millis(2));
                };
                let anchor = SourceTimestamp::new(if derived { 4 } else { 2 }, 10).unwrap();
                let command = if derived {
                    Command::ContinueLiveCaptureAt {
                        engine_epoch: prior.engine_epoch.clone(),
                        predecessor: prior_epoch.clone(),
                        source_anchor: anchor,
                        lease_ms: 30000,
                    }
                } else {
                    Command::ContinueLiveCapture {
                        engine_epoch: prior.engine_epoch.clone(),
                        predecessor: prior_epoch.clone(),
                        options: prior_running.continuation_options(anchor, 30000).unwrap(),
                    }
                };
                start = request(&session, Some(&project), Some(0), command);
                initial = capture_report(engine.handle(start.clone()).result.unwrap());
                assert_ne!(initial.identity.epoch, prior_epoch);
                assert_eq!(initial.identity.source, prior.identity.source);
                assert_eq!(
                    initial.continuation.as_ref().unwrap().predecessor,
                    prior.identity
                );
                assert_eq!(initial.options.source_anchor, anchor);
                assert_eq!(initial.received_frames, 0);
                assert!(engine.live_bulk_ready(&old_handshake, None).is_err());
                assert!(engine
                    .handle(request(
                        &session,
                        Some(&project),
                        Some(0),
                        Command::FinishLiveCapture {
                            epoch: prior_epoch.clone()
                        }
                    ))
                    .result
                    .is_err());
                let retired = capture_report(
                    engine
                        .handle(request(
                            &session,
                            Some(&project),
                            Some(0),
                            Command::LiveCaptureStatus { epoch: prior_epoch },
                        ))
                        .result
                        .unwrap(),
                );
                assert_eq!(retired.retirement, Some(LiveCaptureRetirement::Superseded));
                assert!(retired.inference.as_ref().unwrap().latest.is_none());
                handoffs.push(serde_json::json!({"derived":derived,"prior_running":prior_running,"retired":retired,"successor":initial}));
                end = Instant::now() + Duration::from_secs(30);
            }
        }
        let epoch = initial.identity.epoch.clone();
        let status = |engine: &Arc<Engine>| {
            capture_report(
                engine
                    .handle(request(
                        &session,
                        Some(&project),
                        Some(0),
                        Command::LiveCaptureStatus {
                            epoch: epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            )
        };
        loop {
            let report = status(&engine);
            assert!(report.active, "{}", serde_json::to_string(&report).unwrap());
            if report.inference.as_ref().unwrap().state == LiveCaptureInferenceState::Running {
                break;
            }
            assert!(Instant::now()<end, "live CUDA readiness exceeded the issued test deadline: elapsed={:?}, inference={:?}, capture_elapsed={:?}", started_at.elapsed(), report.inference, report.server_elapsed);
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(handle) = prepared.take() {
            assert!(handle.release().unwrap());
            assert!(
                engine.pool.reserved_storage() > 0,
                "running stream must retain native storage after preparation release"
            );
        }
        assert!(engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::FinishLiveCapture {
                    epoch: epoch.clone()
                }
            ))
            .result
            .is_err());
        let handshake = LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveCapture,
            session: session.clone(),
            auth_token: TOKENS.with(|tokens| tokens.borrow()[session.as_str()].clone()),
            engine_epoch: initial.engine_epoch.clone(),
            identity: initial.identity.clone(),
        };
        let mut reports = Vec::new();
        for frame in 0..=3 {
            let bytes = vec![(frame * 64) as u8; LIVE_FRAME_BYTES];
            let current = status(&engine);
            let header = LiveFrameHeader {
                frame: FrameId::new(frame),
                source_time: SourceTimestamp::new(
                    frame as i64 + if continuation { 4 } else { 0 },
                    10,
                )
                .unwrap(),
                capture_tick: current.server_elapsed.unwrap(),
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            };
            engine
                .live_bulk_upload(&handshake, &header, &bytes)
                .unwrap();
            if frame == 0 {
                continue;
            }
            loop {
                let report = status(&engine);
                assert!(report.active, "{}", serde_json::to_string(&report).unwrap());
                if report.inference.as_ref().unwrap().completed == frame {
                    reports.push(report);
                    break;
                }
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
        let _hook = if finalizer_fault == Some("blocked_finish") {
            Some(engine.public_cuda_finish_hook_for_test(
                &epoch,
                Box::new(move |worker| {
                    let frozen = worker.freeze_for_test().unwrap();
                    crate::launch_pipes::blocked_read_hook_for_test(Box::new(move || {
                        entered_tx.send(()).unwrap();
                    }));
                    frozen
                }),
            ))
        } else {
            None
        };
        let finish = request(
            &session,
            Some(&project),
            Some(0),
            Command::FinishLiveCapture {
                epoch: epoch.clone(),
            },
        );
        let requested = capture_report(engine.handle(finish.clone()).result.unwrap());
        assert_eq!(
            requested.inference.as_ref().unwrap().state,
            LiveCaptureInferenceState::FinishRequested
        );
        assert!(engine.live_bulk_ready(&handshake, None).is_err());
        if finalizer_fault == Some("blocked_finish") {
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let blocked = status(&engine);
            blocked.validate().unwrap();
            assert!(blocked.active);
            assert_eq!(
                blocked.inference.as_ref().unwrap().state,
                LiveCaptureInferenceState::Finishing
            );
            assert!(blocked.inference.as_ref().unwrap().latest.is_none());
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(
                status(&engine).inference.as_ref().unwrap().state,
                LiveCaptureInferenceState::Finishing
            );
            let stopped = capture_report(
                engine
                    .handle(request(
                        &session,
                        Some(&project),
                        Some(0),
                        Command::StopLiveCapture {
                            epoch: epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            );
            assert_eq!(stopped.retirement, Some(LiveCaptureRetirement::Stopped));
            let cleanup = Instant::now();
            quiescent(&engine);
            let cleanup_ns = cleanup.elapsed().as_nanos();
            assert!(cleanup_ns < 2_000_000_000);
            assert!(Instant::now() < end);
            let terminal = status(&engine);
            assert_eq!(
                serde_json::to_vec(&terminal).unwrap(),
                serde_json::to_vec(&stopped).unwrap()
            );
            assert!(terminal.inference.as_ref().unwrap().cuda_evidence.is_none());
            assert!(engine.live_bulk_ready(&handshake, None).is_err());
            let finish_replay = capture_report(engine.handle(finish).result.unwrap());
            let start_replay = capture_report(engine.handle(start).result.unwrap());
            assert_eq!(
                serde_json::to_vec(&finish_replay).unwrap(),
                serde_json::to_vec(&terminal).unwrap()
            );
            assert_eq!(
                serde_json::to_vec(&start_replay).unwrap(),
                serde_json::to_vec(&terminal).unwrap()
            );
            let db = engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                sources_before
            );
            assert!(!db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2)",
                    params![project.as_str(), initial.identity.source.as_str()],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap());
            for table in ["live_cuda_capture_evidence", "live_cuda_evidence"] {
                let exists: bool = db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                        [table],
                        |r| r.get(0),
                    )
                    .unwrap();
                if exists {
                    assert_eq!(
                        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                            .get::<_, i64>(0))
                            .unwrap(),
                        0
                    );
                }
            }
            let base = PathBuf::from(
                std::env::var_os("PULSAR_PUBLIC_LIVE_CUDA_EVIDENCE")
                    .expect("pin public blocked evidence folder"),
            );
            assert!(!base.exists());
            artifacts::private_directory(&base).unwrap();
            for (name, report) in [
                ("initial", &initial),
                ("requested", &requested),
                ("blocked", &blocked),
                ("stopped", &stopped),
                ("terminal", &terminal),
                ("finish-replay", &finish_replay),
                ("start-replay", &start_replay),
            ] {
                fs::write(
                    base.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(report).unwrap(),
                )
                .unwrap();
            }
            fs::write(
                base.join("provisional-reports.json"),
                serde_json::to_vec_pretty(&reports).unwrap(),
            )
            .unwrap();
            fs::write(base.join("checked.json"),serde_json::to_vec_pretty(&serde_json::json!({
                "authenticated_public_start":true,"automatic_scheduler":true,"public_finish_and_stop":true,
                "test_inserted_registry_entry":false,"owned_freezer_and_observed_reply_would_block":true,
                "original_lease_ms":30000,"clock_modified":false,"completed_provisional_estimates":3,
                "cleanup_after_stop_ns":cleanup_ns,"all_custody_retired":true,
                "no_evidence_rows":true,"offline_jobs":0,"source_library_unchanged":true,
                "exact_terminal_start_finish_replay":true,"qualified":false,"physical_dispatch":false
            })).unwrap()).unwrap();
            return;
        }
        let completed = loop {
            let report = status(&engine);
            if !report.active {
                break report;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(2));
        };
        completed.validate().unwrap();
        assert_eq!(completed.retirement, Some(LiveCaptureRetirement::Completed));
        let record = completed
            .inference
            .as_ref()
            .unwrap()
            .cuda_evidence
            .as_ref()
            .unwrap();
        assert_eq!(record.summary.successful_runs, 3);
        assert!(!record.qualified);
        assert_eq!(
            serde_json::to_vec(&capture_report(engine.handle(finish).result.unwrap())).unwrap(),
            serde_json::to_vec(&completed).unwrap()
        );
        assert_eq!(
            capture_report(engine.handle(start).result.unwrap()).identity,
            initial.identity
        );
        quiescent(&engine);
        assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_ok());
        let admission =
            crate::live_cuda_evidence::EvidenceAdmission::reserve(&engine.pool).unwrap();
        let (evidence, origin) = {
            let db = engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                22
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                sources_before
            );
            assert!(!db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2)",
                    params![project.as_str(), initial.identity.source.as_str()],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap());
            let raw: CaptureEvidenceRow = db
                .query_row(
                    "SELECT origin,manifest,summary,receipt,trace FROM live_cuda_capture_evidence",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .unwrap();
            (admission.read(raw.1, raw.2, raw.3, raw.4).unwrap(), raw.0)
        };
        assert_eq!(&evidence.report_record().unwrap(), record);
        let base = PathBuf::from(
            std::env::var_os("PULSAR_PUBLIC_LIVE_CUDA_EVIDENCE")
                .expect("pin new public evidence folder"),
        );
        assert!(!base.exists());
        artifacts::private_directory(&base).unwrap();
        fs::write(
            base.join("restart.json"),
            serde_json::to_vec_pretty(&restart_record).unwrap(),
        )
        .unwrap();
        fs::write(
            base.join("handoffs.json"),
            serde_json::to_vec_pretty(&handoffs).unwrap(),
        )
        .unwrap();
        for (name, bytes) in [
            (
                "initial-report.json",
                serde_json::to_vec_pretty(&initial).unwrap(),
            ),
            (
                "requested-report.json",
                serde_json::to_vec_pretty(&requested).unwrap(),
            ),
            (
                "completed-report.json",
                serde_json::to_vec_pretty(&completed).unwrap(),
            ),
            ("reports.json", serde_json::to_vec_pretty(&reports).unwrap()),
            ("origin.json", origin),
            ("manifest.json", evidence.manifest_bytes().to_vec()),
            ("summary.json", evidence.summary_bytes().to_vec()),
            ("receipt.json", evidence.receipt().to_vec()),
            ("original-trace.json", evidence.trace().to_vec()),
        ] {
            fs::write(base.join(name), bytes).unwrap();
        }
        fs::write(base.join("checked.json"),serde_json::to_vec_pretty(&serde_json::json!({"authenticated_public_start":true,"automatic_gpu_scheduler":true,"public_finish":true,"test_inserted_registry_entry":false,"source_library_unchanged":true,"engine_elapsed_ns":completed.server_elapsed.unwrap().0,"original_lease_ms":30000,"successful_runs":3,"all_custody_retired":true,"authored_rgb_and_source_timestamps":true,"physical_dispatch":false,"qualified":false})).unwrap()).unwrap();
        drop(evidence);
        let ResponseBody::Job(job) = engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::Generate {
                    source_version: source,
                    preset: "fast".into(),
                    settings: Some(settings),
                    model_path: Some(generation.model_path),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("schema22 generation");
        };
        let job = self::completed(&engine, &session, &project, &job.job_id);
        quiescent(&engine);
        retain(&engine, &job, "schema22-generation");
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            22
        );
        let config = engine.config.clone();
        drop(engine);
        engine = reopen_after_transient_leases(config).unwrap();
        let db = engine.db.lock().unwrap();
        crate::live_cuda_evidence_db::validate(&db, 22, &engine.pool).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM live_cuda_capture_evidence", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(db);
        let config = engine.config.clone();
        drop(engine);
        let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;UPDATE projects SET name=name || ' schema22';").unwrap();
        let paths = [
            config.state_dir.join("projects.sqlite3"),
            config.state_dir.join("projects.sqlite3-wal"),
            config.state_dir.join("worker-fence.json"),
        ];
        let before = paths
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>();
        assert!(before[1].len() > 32);
        let reader = std::env::var_os("PULSAR_PUBLIC_CUDA_PRECEDING_READER")
            .expect("pin preceding schema21 reader");
        let result = std::process::Command::new("/usr/bin/timeout")
            .args(["--signal=TERM", "5"])
            .arg(reader)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PULSAR_STATE_DIR", &config.state_dir)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&result.stderr).contains("explicit recoverable migration"));
        assert_eq!(
            paths
                .iter()
                .map(|p| fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before
        );
        assert!(!config.state_dir.join("engine.sock").exists());
        fs::write(base.join("preceding-reader-stderr.txt"), &result.stderr).unwrap();
        fs::write(base.join("reader-barrier.json"),serde_json::to_vec_pretty(&serde_json::json!({"preceding_schema21_exit_code":1,"database_wal_ledger_unchanged":true,"wal_bytes":before[1].len(),"current_engine_reopened":true})).unwrap()).unwrap();
        fs::write(base.join("schema22-generation.json"),serde_json::to_vec_pretty(&serde_json::json!({"normal_cuda_generation_after_live_finish":true,"schema22_preserved":true,"job_id":job.job_id,"attempt_id":job.attempt_id,"state":job.state,"qualified":false})).unwrap()).unwrap();
        engine = reopen_after_transient_leases(config).unwrap();
        let db = engine.db.lock().unwrap();
        crate::live_cuda_evidence_db::validate(&db, 22, &engine.pool).unwrap();
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        fs::copy(
            engine.config.state_dir.join("projects.sqlite3"),
            base.join("projects.sqlite3"),
        )
        .unwrap();
        fs::copy(
            engine.config.state_dir.join("worker-fence.json"),
            base.join("worker-fence.json"),
        )
        .unwrap();
    }
    fn native_engine_cuda_case(
        live_only: bool,
        public_live: bool,
        continuation: bool,
        restart: bool,
        finalizer_fault: Option<&str>,
    ) {
        native_engine_cuda_case_prepared(
            live_only,
            public_live,
            continuation,
            restart,
            finalizer_fault,
            false,
        );
    }
    #[test]
    #[ignore = "requires CUDA worker, native image/NVML, authored model and delegated cgroups; serial only"]
    fn native_engine_owned_preparation_before_public_live_capture() {
        native_engine_cuda_case_prepared(true, true, false, false, None, true);
    }
    fn native_engine_cuda_case_prepared(
        live_only: bool,
        public_live: bool,
        continuation: bool,
        restart: bool,
        finalizer_fault: Option<&str>,
        prepare: bool,
    ) {
        let worker = PathBuf::from(
            std::env::var_os("PULSAR_ENGINE_CUDA_WORKER").expect("pin CUDA-enabled worker"),
        );
        let mut spec: NativeDependencySpec = serde_json::from_slice(
            &fs::read(std::env::var_os("PULSAR_NATIVE_LIBRARY_SPEC").expect("pin native spec"))
                .unwrap(),
        )
        .unwrap();
        let nvml = PathBuf::from(std::env::var_os("PULSAR_NVML_LIBRARY").expect("pin NVML"));
        let (sha256, byte_len) = artifacts::digest_file(&nvml, 64 << 20).unwrap();
        spec.libraries.push(NativeLibrarySpec {
            path: nvml,
            sha256,
            byte_len,
            names: vec!["libnvidia-ml.so.1".into()],
        });
        let runtime = spec
            .libraries
            .iter()
            .find(|library| {
                library
                    .names
                    .iter()
                    .any(|name| name == "libonnxruntime.so.1")
            })
            .unwrap()
            .path
            .clone();
        let _restore = RestoreRuntime(std::env::var_os("PULSAR_ORT_DYLIB"));
        std::env::set_var("PULSAR_ORT_DYLIB", &runtime);
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut example: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let execution = example.cuda_execution.take().unwrap();
        let policy = CudaOwnerPolicy {
            schema_version: 1,
            device: CudaDeviceExpectation {
                device_uuid: execution.device_uuid.clone(),
                pci_bus_address: execution.pci_bus_address,
                device_minor: execution.device_minor,
            },
            accounting: GpuDeviceBudget {
                device_uuid: execution.device_uuid,
                device_ordinal: execution.device_ordinal,
                maximum_reserved_bytes: 128 << 20,
                interactive_reserved_bytes: 64 << 20,
                control_reserve_bytes: 256 << 20,
            },
            native_dependencies: spec,
        };
        let policy_path = dir.path().join("policy.json");
        fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
        fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = EngineConfig::new(dir.path().join("state"), worker);
        config.cuda_owner_policy = Some(policy_path);
        config.worker_timeout = Duration::from_secs(30);
        let mut engine = Engine::open(config.clone()).unwrap();
        let session = pair(&engine, "authored CUDA generation");
        let ResponseBody::Project(project) = engine
            .handle(request(
                &session,
                None,
                None,
                Command::CreateProject {
                    name: "Authored CUDA".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("project")
        };
        let project = project.project_id;
        let source_path = dir.path().join("pair.rgbpair");
        let mut bytes = vec![0; 64 * 64 * 3];
        bytes.extend(vec![128; 64 * 64 * 3]);
        fs::write(&source_path, bytes).unwrap();
        let ResponseBody::Source {
            source_version: source,
        } = engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::ImportSource { path: source_path },
            ))
            .result
            .unwrap()
        else {
            panic!("import")
        };
        let model_path = dir.path().join("authored.onnx");
        fs::copy(
            std::env::var_os("PULSAR_AUTHORED_CUDA_MODEL").expect("pin authored model"),
            &model_path,
        )
        .unwrap();
        let model = worker::pin_artifact(&model_path).unwrap();
        let WorkerOperation::Generate { mut settings, .. } = example.operation else {
            panic!("fixture generate")
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.model = model.identity;
        profile.runtime = policy.runtime_identity().unwrap();
        settings.raw_frame_pair = Some(RawFramePair {
            source_epoch: AttemptId::new("authored-pair").unwrap(),
            previous_frame: FrameId::new(0),
            current_frame: FrameId::new(1),
            previous_time: SourceTimestamp::new(0, 1).unwrap(),
            current_time: SourceTimestamp::new(1, 10).unwrap(),
            presentation_clock: None,
        });
        if live_only {
            let mut stream: LiveInferenceManifest = serde_json::from_str(include_str!(
                "../../pulsar-protocol/tests/fixtures/live-cpu-manifest.json"
            ))
            .unwrap();
            let preprocessing = stream.profile.preprocessing;
            stream.profile = settings.inference_profile.as_ref().unwrap().clone();
            stream.profile.preprocessing = preprocessing;
            stream
                .profile
                .cuda_resources
                .as_mut()
                .unwrap()
                .device_reservation_bytes = 96 << 20;
            stream.attempt_id = id!(AttemptId);
            stream.identity = LiveIdentity {
                epoch: id!(AttemptId),
                project: project.clone(),
                revision: RevisionId::new(0),
                source: source.clone(),
            };
            let options = LiveCaptureOptions {
                browser_analysis: None,
                browser_media: None,
                model_pack: None,
                source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
                continuation_mode: LiveContinuationMode::Immediate,
                encoded_source: None,
                inference: Some(LiveCaptureInference {
                    model_path: model_path.clone(),
                    profile: stream.profile.clone(),
                }),
                policy: LivePolicy {
                    maximum_analysis_age_ns: 200_000_000,
                    added_buffer_ns: 0,
                },
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::from_nanos(0),
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                lease_ms: 30_000,
            };
            if public_live {
                let prepared = if prepare {
                    let handle = engine
                        .prepare_live_cuda_image(
                            crate::LiveCudaImagePreparationOptions {
                                preparation_ms: 180_000,
                                retention_ms: 300_000,
                            },
                            &AtomicBool::new(false),
                        )
                        .unwrap();
                    assert!(handle.available());
                    assert_eq!(handle.engine_epoch().as_str(), engine.engine_epoch);
                    Some(handle)
                } else {
                    None
                };
                public_live_case(PublicLiveCase {
                    engine,
                    session,
                    project,
                    options,
                    source,
                    settings: *settings,
                    continuation,
                    restart,
                    finalizer_fault,
                    prepared,
                });
                return;
            }
            stream.clock = options.clock(stream.identity.epoch.clone());
            stream.model = ArtifactReference {
                path: model_path.clone(),
                identity: stream.profile.model.clone(),
            };
            stream.runtime = engine
                .tools
                .as_ref()
                .unwrap()
                .onnx_runtime
                .as_ref()
                .unwrap()
                .clone();
            stream.worker = engine.binary_identity.clone();
            stream.budget = ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: OUTPUT_LIMIT,
                wall_time_ms: 30_000,
                cpu_threads: 2,
            };
            let intent = request(
                &session,
                Some(&project),
                Some(0),
                Command::StartLiveCapture {
                    engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                    options: options.clone(),
                },
            );
            let runtime = crate::live_frames::LiveRgbRuntime::new(
                &engine.pool,
                stream.identity.clone(),
                options.policy,
                Some(stream.clock.clone()),
                LiveClockTick(0),
            )
            .unwrap();
            let origin = Instant::now();
            let end = origin + Duration::from_secs(30);
            let before_ledger =
                fs::read(engine.config.state_dir.join("worker-fence.json")).unwrap();
            for mismatch in ["clock", "boot"] {
                let mut altered = intent.clone();
                let Command::StartLiveCapture {
                    engine_epoch,
                    options,
                } = &mut altered.command
                else {
                    unreachable!()
                };
                if mismatch == "clock" {
                    options.project_anchor = ProjectTime::from_nanos(1);
                } else {
                    *engine_epoch = AttemptId::new("retired-boot").unwrap();
                }
                assert!(engine
                    .start_live_cuda(
                        &altered,
                        stream.clone(),
                        runtime.inference_scope(),
                        Arc::new(AtomicBool::new(false)),
                        origin,
                        end
                    )
                    .is_err());
                assert_eq!(engine.pool.reserved_storage(), 0);
                assert_eq!(
                    fs::read(engine.config.state_dir.join("worker-fence.json")).unwrap(),
                    before_ledger
                );
            }
            let cancelled = engine
                .start_live_cuda(
                    &intent,
                    stream.clone(),
                    runtime.inference_scope(),
                    Arc::new(AtomicBool::new(true)),
                    origin,
                    end,
                )
                .err()
                .unwrap();
            assert_eq!(cancelled.code, ErrorCode::Cancelled, "{cancelled:?}");
            assert_eq!(engine.pool.reserved_storage(), 0);
            assert_eq!(
                fs::read(engine.config.state_dir.join("worker-fence.json")).unwrap(),
                before_ledger
            );
            let control = engine.pool.reserve(1 << 20).unwrap();
            let scope = runtime.inference_scope();
            let identity = stream.identity.clone();
            let attempt = stream.attempt_id.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            engine.live_captures.lock().unwrap().insert_cuda_for_test(
                live_captures::TestCaptureSession {
                    owner: session.clone(),
                    created: origin,
                    engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                },
                &stream,
                options.clone(),
                runtime,
                cancel.clone(),
            );
            let (manifest, mut live) = engine
                .start_live_cuda(&intent, stream, scope, cancel, origin, end)
                .unwrap();
            let foreign = crate::live_frames::LiveRgbRuntime::new(
                &engine.pool,
                identity.clone(),
                options.policy,
                Some(manifest.stream.clock.clone()),
                LiveClockTick(0),
            )
            .unwrap();
            assert!(live.ready_manifest(&foreign.inference_scope()).is_err());
            drop(foreign);
            assert!(engine
                .live_cuda_analysis_ready(&identity.epoch, &live, &attempt)
                .unwrap());
            let handshake = LiveBulkHandshake {
                version: PROTOCOL_VERSION,
                channel: LiveBulkChannel::LiveCapture,
                session: session.clone(),
                auth_token: TOKENS.with(|tokens| tokens.borrow()[session.as_str()].clone()),
                engine_epoch: AttemptId::new(engine.engine_epoch.clone()).unwrap(),
                identity: identity.clone(),
            };
            let mut replies = Vec::new();
            let mut reports = Vec::new();
            for frame in 0..=3 {
                let bytes = vec![(frame * 64) as u8; LIVE_FRAME_BYTES];
                let header = LiveFrameHeader {
                    frame: FrameId::new(frame),
                    source_time: SourceTimestamp::new(frame as i64, 10).unwrap(),
                    capture_tick: LiveClockTick(
                        u64::try_from(origin.elapsed().as_nanos()).unwrap(),
                    ),
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                };
                engine
                    .live_bulk_upload(&handshake, &header, &bytes)
                    .unwrap();
                if frame == 0 {
                    continue;
                }
                let work = engine
                    .live_analysis_take(&identity.epoch, &attempt)
                    .unwrap()
                    .unwrap();
                let reply = live.estimate(&work).unwrap();
                let LiveCudaStreamReply::Estimated { position, .. } = &reply else {
                    panic!("estimate");
                };
                assert!(
                    (position.value()
                        - (0.25 + 0.25 * ((frame - 1) * 64 + frame * 64) as f64 / 255.0))
                        .abs()
                        < 1e-5
                );
                assert!(engine
                    .live_cuda_analysis_complete(
                        &identity.epoch,
                        &attempt,
                        &live,
                        work,
                        reply.clone()
                    )
                    .unwrap());
                let report = capture_report(
                    engine
                        .handle(request(
                            &session,
                            Some(&project),
                            Some(0),
                            Command::LiveCaptureStatus {
                                epoch: identity.epoch.clone(),
                            },
                        ))
                        .result
                        .unwrap(),
                );
                report.validate().unwrap();
                let info = report.inference.as_ref().unwrap();
                assert_eq!(info.completed, frame);
                assert_eq!(info.accepted + info.stale, frame);
                assert_eq!(
                    info.cuda_manifest_sha256,
                    Some(manifest.identity_sha256().unwrap())
                );
                if let Some(latest) = &info.latest {
                    assert!(!latest.qualified);
                    assert!(matches!(
                        latest.result,
                        LiveInferenceReply::CudaProvisional { .. }
                    ));
                }
                reports.push(report);
                replies.push(reply);
            }
            if let Some(point) = finalizer_fault {
                finalizer_fault_case(&engine, &intent, &identity, &attempt, live, end, point);
                drop(control);
                assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_ok());
                return;
            }
            let completed = engine
                .live_cuda_finalize(&intent, &identity.epoch, &attempt, live)
                .unwrap();
            completed.validate().unwrap();
            assert_eq!(completed.retirement, Some(LiveCaptureRetirement::Completed));
            assert!(!completed.active);
            let record = completed
                .inference
                .as_ref()
                .unwrap()
                .cuda_evidence
                .as_ref()
                .unwrap();
            assert!(!record.qualified);
            assert_eq!(record.summary.successful_runs, 3);
            assert!(Instant::now() < end);
            let stopped = capture_report(
                engine
                    .handle(request(
                        &session,
                        Some(&project),
                        Some(0),
                        Command::StopLiveCapture {
                            epoch: identity.epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap(),
            );
            assert_eq!(
                serde_json::to_vec(&stopped).unwrap(),
                serde_json::to_vec(&completed).unwrap()
            );
            quiescent(&engine);
            // Finish retires its admission; only the other worker slot remains held.
            assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_err());
            drop(control);
            assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_ok());
            assert_eq!(engine.pool.reserved_storage(), 0);
            // Acquire readback memory before SQLite copies any retained BLOB.
            let admission =
                crate::live_cuda_evidence::EvidenceAdmission::reserve(&engine.pool).unwrap();
            let evidence = {
                let db = engine.db.lock().unwrap();
                assert_eq!(
                    db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    0
                );
                assert_eq!(
                    db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    21
                );
                let originals: (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = db
                    .query_row(
                        "SELECT manifest,summary,receipt,trace FROM live_cuda_evidence",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .unwrap();
                admission
                    .read(originals.0, originals.1, originals.2, originals.3)
                    .unwrap()
            };
            assert_eq!(&evidence.report_record().unwrap(), record);
            assert_eq!(
                evidence.manifest_bytes(),
                serde_json::to_vec(&manifest).unwrap()
            );
            assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_err());
            assert!(engine
                .pool
                .reserve_memory(
                    engine.config.max_ram_bytes - crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES
                )
                .is_ok());
            if let Some(base) = std::env::var_os("PULSAR_ENGINE_CUDA_LIVE_EVIDENCE") {
                let base = PathBuf::from(base);
                assert!(!base.exists());
                artifacts::private_directory(&base).unwrap();
                fs::write(
                    base.join("manifest.json"),
                    serde_json::to_vec_pretty(&manifest).unwrap(),
                )
                .unwrap();
                fs::write(
                    base.join("exchanges.json"),
                    serde_json::to_vec_pretty(&replies).unwrap(),
                )
                .unwrap();
                fs::write(
                    base.join("reports.json"),
                    serde_json::to_vec_pretty(&reports).unwrap(),
                )
                .unwrap();
                fs::write(
                    base.join("completed-report.json"),
                    serde_json::to_vec_pretty(&completed).unwrap(),
                )
                .unwrap();
                fs::write(
                    base.join("stopped-report.json"),
                    serde_json::to_vec_pretty(&stopped).unwrap(),
                )
                .unwrap();
                fs::write(
                    base.join("summary.json"),
                    serde_json::to_vec_pretty(evidence.summary()).unwrap(),
                )
                .unwrap();
                fs::write(base.join("receipt.json"), evidence.receipt()).unwrap();
                fs::write(base.join("original-trace.json"), evidence.trace()).unwrap();
                fs::write(base.join("checked.json"), serde_json::to_vec_pretty(&serde_json::json!({
                    "engine_owner_policy":true,"original_lease_ms":30000,"elapsed_ms":origin.elapsed().as_millis(),
                    "successful_runs":3,"offline_jobs":0,"all_custody_retired":true,"engine_consuming_finalization":true,"completed_report_matches_retained_bytes":true,
                    "default_worker_slots":engine.config.max_jobs,"other_worker_slot_held_throughout":true,"capture_completion_clock":"engine_local_monotonic","authored_source_timestamps":true,"test_inserted_registry_entry":true,
                    "public_gpu_live_enabled":false,"physical_dispatch":false,"real_media":false})).unwrap()).unwrap();
            }
            drop(evidence);
            assert!(engine.pool.reserve(engine.config.max_ram_bytes).is_ok());
            assert_eq!(engine.pool.reserved_storage(), 0);
            let reopened_config = engine.config.clone();
            drop(engine);
            engine = reopen_after_transient_leases(reopened_config).unwrap();
            let db = engine.db.lock().unwrap();
            crate::live_cuda_evidence_db::validate(&db, 21, &engine.pool).unwrap();
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM live_cuda_evidence", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
            if let Some(base) = std::env::var_os("PULSAR_ENGINE_CUDA_LIVE_EVIDENCE") {
                fs::copy(
                    engine.config.state_dir.join("projects.sqlite3"),
                    PathBuf::from(base).join("projects.sqlite3"),
                )
                .unwrap();
            }
            return;
        }
        let generate = |session: &SessionId| {
            request(
                session,
                Some(&project),
                Some(0),
                Command::Generate {
                    source_version: source.clone(),
                    preset: "fast".into(),
                    settings: Some(*settings.clone()),
                    model_path: Some(model_path.clone()),
                },
            )
        };
        let ResponseBody::Job(first) = engine.handle(generate(&session)).result.unwrap() else {
            panic!("generate")
        };
        let first = completed(&engine, &session, &project, &first.job_id);
        quiescent(&engine);
        retain(&engine, &first, "first");
        // A committed queued intent with no launched worker is interrupted on reopen.
        let queued_request = generate(&session);
        let captured = engine
            .prepare_cuda_generation(&queued_request, None)
            .unwrap()
            .unwrap();
        let staged_model = engine.snapshot_model(&model_path).unwrap();
        let (queued, reservation, _) = {
            let mut db = engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            let work = engine
                .prepare_operation_with_capture(
                    &tx,
                    &queued_request,
                    &source,
                    WorkerOperation::Generate {
                        preset: "fast".into(),
                        settings: (settings.clone()),
                        model: Some(staged_model),
                    },
                    OperationDependencies {
                        origin: AttemptOrigin::New,
                        extra: &[],
                        cuda: Some(&captured),
                        cpu: None,
                        fallback: None,
                    },
                )
                .unwrap();
            tx.commit().unwrap();
            work
        };
        let previous_attempt = queued.attempt_id.clone();
        let old_epoch = engine.engine_epoch.clone();
        drop(reservation);
        drop(captured);
        quiescent(&engine);
        drop(engine);
        engine = reopen_after_transient_leases(config.clone()).unwrap();
        assert_ne!(engine.engine_epoch, old_epoch);
        {
            let db = engine.db.lock().unwrap();
            let status = job_snapshot(&db, &queued.job_id, &project).unwrap();
            assert_eq!(status.state, "interrupted");
            assert_eq!(status.attempt_id, previous_attempt);
            crate::cuda_acceptance::validate(&db, 20).unwrap();
        }
        let ResponseBody::Job(resumed) = engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::ResumeJob {
                    job_id: queued.job_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("resume")
        };
        assert_ne!(resumed.attempt_id, previous_attempt);
        assert_eq!(resumed.job_id, queued.job_id);
        let resumed = completed(&engine, &session, &project, &resumed.job_id);
        quiescent(&engine);
        retain(&engine, &resumed, "resumed");
        let ResponseBody::Job(cancel) = engine.handle(generate(&session)).result.unwrap() else {
            panic!("cancel generate")
        };
        let ResponseBody::Job(cancelled) = engine
            .handle(request(
                &session,
                Some(&project),
                None,
                Command::CancelJob {
                    job_id: cancel.job_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("cancel")
        };
        assert_eq!(cancelled.state, "cancelled");
        quiescent(&engine);
        {
            let db = engine.db.lock().unwrap();
            let version: i64 = db
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(version, 20);
            crate::cuda_attempts::validate(&db, version).unwrap();
            crate::cuda_acceptance::validate(&db, version).unwrap();
            let accepted: i64 = db
                .query_row("SELECT COUNT(*) FROM cuda_candidate_evidence", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(accepted, 2);
            let current_epoch: String = db
                .query_row(
                    "SELECT engine_epoch FROM cuda_attempt_intents WHERE attempt=?1",
                    [resumed.attempt_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(current_epoch, engine.engine_epoch);
            let original_epoch: String = db
                .query_row(
                    "SELECT engine_epoch FROM cuda_attempt_intents WHERE attempt=?1",
                    [previous_attempt.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(original_epoch, old_epoch);
        }
        let final_epoch = engine.engine_epoch.clone();
        drop(engine);
        let reopened = reopen_after_transient_leases(config).unwrap();
        assert_ne!(reopened.engine_epoch, final_epoch);
        quiescent(&reopened);
        retain(&reopened, &first, "first-after-reopen");
        retain(&reopened, &resumed, "resumed-after-reopen");
        eprintln!("Engine CUDA integration: public generation and acceptance, queued-intent reopen without automatic GPU dispatch, fresh explicit resume/new attempt/current epoch, cancellation without candidate, retained original receipts/traces and empty ownership journals passed");
    }

    #[test]
    #[ignore = "requires explicit CUDA worker, authored detector, real image/NVML and delegated cgroups; serial"]
    fn native_engine_cuda_detector_preview_uses_interactive_admission_and_original_trace() {
        let worker = PathBuf::from(
            std::env::var_os("PULSAR_ENGINE_CUDA_WORKER").expect("pin CUDA-enabled worker"),
        );
        let mut spec: NativeDependencySpec = serde_json::from_slice(
            &fs::read(std::env::var_os("PULSAR_NATIVE_LIBRARY_SPEC").expect("pin native spec"))
                .unwrap(),
        )
        .unwrap();
        let nvml = PathBuf::from(std::env::var_os("PULSAR_NVML_LIBRARY").expect("pin NVML"));
        let (sha256, byte_len) = artifacts::digest_file(&nvml, 64 << 20).unwrap();
        spec.libraries.push(NativeLibrarySpec {
            path: nvml,
            sha256,
            byte_len,
            names: vec!["libnvidia-ml.so.1".into()],
        });
        let runtime = spec
            .libraries
            .iter()
            .find(|library| {
                library
                    .names
                    .iter()
                    .any(|name| name == "libonnxruntime.so.1")
            })
            .unwrap()
            .path
            .clone();
        let _restore = RestoreRuntime(std::env::var_os("PULSAR_ORT_DYLIB"));
        std::env::set_var("PULSAR_ORT_DYLIB", &runtime);
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut example: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let execution = example.cuda_execution.take().unwrap();
        let policy = CudaOwnerPolicy {
            schema_version: 1,
            device: CudaDeviceExpectation {
                device_uuid: execution.device_uuid.clone(),
                pci_bus_address: execution.pci_bus_address,
                device_minor: execution.device_minor,
            },
            accounting: GpuDeviceBudget {
                device_uuid: execution.device_uuid,
                device_ordinal: execution.device_ordinal,
                maximum_reserved_bytes: 128 << 20,
                interactive_reserved_bytes: 64 << 20,
                control_reserve_bytes: 256 << 20,
            },
            native_dependencies: spec,
        };
        let policy_path = dir.path().join("policy.json");
        fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
        fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = EngineConfig::new(dir.path().join("state"), worker);
        config.cuda_owner_policy = Some(policy_path);
        config.worker_timeout = Duration::from_secs(30);
        let engine = Engine::open(config.clone()).unwrap();
        let session = pair(&engine, "authored CUDA preview");
        let ResponseBody::Project(project) = engine
            .handle(request(
                &session,
                None,
                None,
                Command::CreateProject {
                    name: "Authored CUDA".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("project")
        };
        let project = project.project_id;

        let source_path = dir.path().join("frame.y4m");
        let mut video = b"YUV4MPEG2 W64 H64 F1:1 Ip A1:1 Cmono\nFRAME\n".to_vec();
        video.extend(vec![128u8; 64 * 64]);
        fs::write(&source_path, video).unwrap();
        let ResponseBody::Source {
            source_version: source,
        } = engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::ImportSource { path: source_path },
            ))
            .result
            .unwrap()
        else {
            panic!("source")
        };
        let model_path = dir.path().join("detector.onnx");
        fs::copy(
            std::env::var_os("PULSAR_AUTHORED_CUDA_DETECTOR").expect("pin authored detector"),
            &model_path,
        )
        .unwrap();
        let WorkerOperation::Generate { settings, .. } = example.operation else {
            panic!("generate template")
        };
        let mut profile = settings.inference_profile.unwrap();
        profile.model = worker::pin_artifact(&model_path).unwrap().identity;
        profile.runtime = policy.runtime_identity().unwrap();
        profile.preprocessing.input.channels = 3;
        profile.preprocessing.input.decoder = DetectorDecoder::YoloChannelMajor;
        profile.preprocessing.resize = InferenceResize::Stretch;
        profile.preprocessing.color = InferenceColor::Rgb;
        profile.preprocessing.scale = vec![1.0 / 255.0; 3];
        profile.preprocessing.bias = vec![0.0; 3];
        profile.minimum_output_bytes = 8 << 20;
        profile
            .cuda_resources
            .as_mut()
            .unwrap()
            .device_reservation_bytes = 96 << 20;
        let budget = ResourceBudget {
            memory_bytes: WORKER_MEMORY,
            output_bytes: 8 << 20,
            wall_time_ms: 30000,
            cpu_threads: 2,
        };
        assert!(policy
            .authorize_profile(&profile, &budget, crate::resources::GpuPriority::Offline)
            .is_err());
        policy
            .authorize_profile(
                &profile,
                &budget,
                crate::resources::GpuPriority::Interactive,
            )
            .unwrap();
        let context = FrameContext {
            source_version: source,
            source_placement: SourcePlacementId::new("preview-fixture").unwrap(),
            frame: FrameId::new(0),
            transform: video_view_transform(None).unwrap(),
            seek_generation: 2,
            request_generation: 3,
        };
        let command = Command::PreviewFrame {
            time_basis: PreviewTimeBasis::SourcePts,
            video_view: None,
            context: context.clone(),
            source_time: SourceTimestamp::new(0, 1).unwrap(),
            model_path: Some(model_path),
            model_input: Some(profile.preprocessing.input.clone()),
            inference_profile: Some(profile.clone()),
        };
        let ResponseBody::Preview(preview) = engine
            .handle(request(&session, Some(&project), None, command.clone()))
            .result
            .unwrap()
        else {
            panic!("preview")
        };
        assert_eq!(preview.context, context);
        assert!(matches!(preview.analysis, AnalysisStatus::Available));
        assert_eq!(preview.observations.len(), 1);
        let frame = preview.frame.as_ref().unwrap();
        let rgb = fs::read(&frame.artifact.path).unwrap();
        let mean = rgb.iter().map(|v| f64::from(*v)).sum::<f64>() / rgb.len() as f64 / 255.0;
        let observed = serde_json::to_value(&preview.observations[0]).unwrap();
        let confidence: f64 = observed["confidence"]["value"].as_f64().unwrap();
        assert!((confidence - (0.6 + 0.1 * mean)).abs() < 1e-5, "{observed}");
        let output = frame.artifact.path.parent().unwrap();
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("cuda-inference.json")).unwrap()).unwrap();
        let trace_bytes = fs::read(output.join("cuda-trace.json")).unwrap();
        let trace =
            crate::inference_trace::validate_node_trace(&trace_bytes, InferenceProvider::Cuda, 1)
                .unwrap();
        assert_eq!(serde_json::to_value(trace).unwrap(), receipt["trace"]);
        assert_eq!(receipt["successful_runs"], 1);
        assert_eq!(receipt["trace"]["computation_events"], 3);
        quiescent(&engine);
        if let Some(root) = std::env::var_os("PULSAR_ENGINE_CUDA_PREVIEW_EVIDENCE") {
            let root = PathBuf::from(root);
            fs::create_dir_all(&root).unwrap();
            for name in ["frame.raw", "cuda-inference.json", "cuda-trace.json"] {
                fs::copy(output.join(name), root.join(name)).unwrap();
            }
            fs::write(
                root.join("preview.json"),
                serde_json::to_vec_pretty(&preview).unwrap(),
            )
            .unwrap();
            fs::write(
                root.join("profile.json"),
                serde_json::to_vec_pretty(&profile).unwrap(),
            )
            .unwrap();
        }
        let mut invalid = command;
        if let Command::PreviewFrame {
            inference_profile: Some(profile),
            ..
        } = &mut invalid
        {
            profile.model.sha256 = "f".repeat(64);
        }
        assert!(engine
            .handle(request(&session, Some(&project), None, invalid))
            .result
            .is_err());
        quiescent(&engine);
        let db = engine.db.lock().unwrap();
        let jobs: i64 = db
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jobs, 0, "transient Preview is not a durable generation job");
    }
}
