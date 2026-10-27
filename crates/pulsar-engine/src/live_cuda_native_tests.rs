//! Authored native mechanism checks, not real-media/model/device qualification.
use super::*;
use crate::{artifacts, resources::GpuUsage, worker_fence::WorkerFence};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;

pub(in crate::cuda_memory) struct NativeCaptureFixture<'a> {
    pub(in crate::cuda_memory) image: SupervisedNativeDependencies,
    pub(in crate::cuda_memory) capture_pool: &'a ResourcePool,
    pub(in crate::cuda_memory) storage: u64,
}

pub(in crate::cuda_memory) fn native_case(
    config: &EngineConfig,
    request: &WorkerRequest,
    policy: &crate::cuda_owner_policy::CudaOwnerPolicy,
    loaded: &LoadedCudaOwnerPolicy,
    fixture: NativeCaptureFixture<'_>,
    case: &str,
) {
    let NativeCaptureFixture {
        image,
        capture_pool,
        storage,
    } = fixture;
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut stream: LiveInferenceManifest = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/live-cpu-manifest.json"
    ))
    .unwrap();
    let (profile, model) = match &request.operation {
        WorkerOperation::Generate {
            settings, model, ..
        } => (
            settings.inference_profile.as_ref().unwrap(),
            model.as_ref().unwrap(),
        ),
        _ => unreachable!(),
    };
    let preprocessing = stream.profile.preprocessing.clone();
    stream.profile = profile.clone();
    stream.profile.preprocessing = preprocessing;
    stream
        .profile
        .cuda_resources
        .as_mut()
        .unwrap()
        .device_reservation_bytes = 96 << 20;
    stream.model = ArtifactReference {
        path: "/input/model".into(),
        identity: model.identity.clone(),
    };
    stream.runtime = request.tools.onnx_runtime.clone().unwrap();
    stream.attempt_id = request.attempt_id.clone();
    stream.budget = request.budget.clone();
    let worker = crate::worker::pin_artifact(&config.worker_executable).unwrap();
    stream.worker = worker.identity.clone();
    let execution = loaded
        .worker_execution_supervised(
            &image,
            &stream.profile,
            &stream.budget,
            GpuPriority::Interactive,
        )
        .unwrap();
    let mut dependencies = request.dependencies.clone();
    dependencies.extend([stream.model.identity.clone(), stream.worker.clone()]);
    let manifest = LiveCudaInferenceManifest {
        schema_version: 1,
        stream,
        execution,
        dependencies,
        output_dir: root.path().join("guest-evidence"),
    };
    manifest.validate().unwrap();
    assert!(policy
        .authorize_profile(
            &manifest.stream.profile,
            &manifest.stream.budget,
            GpuPriority::Offline
        )
        .is_err());
    let image_path = image.captured().library_directory();
    let deadline_ns = cuda_bootstrap::deadline_ns(manifest.stream.budget.wall_time_ms).unwrap();
    let deadline =
        cuda_bootstrap::deadline_instant(deadline_ns, manifest.stream.budget.wall_time_ms).unwrap();
    let state = root.path().join("fence");
    artifacts::private_directory(&state).unwrap();
    let fence = Arc::new(WorkerFence::open(&state).unwrap());
    fence.enable_creation().unwrap();
    // Stage, observer, and admitted worker share one accounting domain. The
    // preparation CPU claim retires after join; snapshot storage stays held.
    let pool = ResourcePool::new_with_gpu_devices(
        manifest.stream.budget.memory_bytes * 2,
        3,
        vec![policy.accounting.clone()],
    )
    .unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut runtime = crate::live_frames::LiveRgbRuntime::new(
        &pool,
        manifest.stream.identity.clone(),
        LivePolicy {
            maximum_analysis_age_ns: 2_000_000,
            added_buffer_ns: manifest.stream.clock.added_buffer_ns,
        },
        Some(manifest.stream.clock.clone()),
        LiveClockTick(0),
    )
    .unwrap();
    let scope = runtime.inference_scope();
    let prepared = crate::live_staging::prepare(
        config,
        &pool,
        [
            worker,
            ArtifactReference {
                path: model.path.clone(),
                identity: model.identity.clone(),
            },
        ],
        crate::live_staging::PreparationBudget {
            memory_bytes: 64 << 20,
            cpu_threads: 1,
        },
        cancel.clone(),
        deadline,
        Some(fence.clone()),
    )
    .unwrap()
    .into_gpu()
    .unwrap();
    let staged = prepared.paths().clone();
    let custody = fence
        .admit_group(
            Arc::new(pool.reserve(manifest.stream.budget.memory_bytes).unwrap()),
            manifest.stream.budget.memory_bytes,
            manifest.stream.budget.cpu_threads,
        )
        .unwrap()
        .into_worker();
    let observed = observe(
        config,
        &manifest,
        deadline_ns,
        image,
        PinnedCudaDevices::open(&policy.device).unwrap(),
        custody,
        &cancel,
    )
    .unwrap();
    assert!(image_path.exists());
    assert_eq!(capture_pool.reserved_storage(), storage);
    let measured = serde_json::to_value(&observed.reply).unwrap();
    let admitted = observed.admit(&pool, loaded, &manifest).unwrap();
    assert_eq!(
        pool.gpu_usage(&policy.device.device_uuid).unwrap(),
        GpuUsage {
            reserved_bytes: 96 << 20,
            offline_reserved_bytes: 0
        }
    );
    let mut worker = crate::live_cuda_stream::LiveCudaWorker::start(
        config,
        manifest.clone(),
        admitted,
        prepared,
        scope,
        cancel.clone(),
    )
    .unwrap();
    let offer = |runtime: &mut crate::live_frames::LiveRgbRuntime, frame: u64| {
        let tick = LiveClockTick(frame * 100_000_000);
        runtime
            .offer(
                FrameId::new(frame),
                SourceTimestamp::new(frame as i64, 10).unwrap(),
                tick,
                ProjectTime::from_nanos(frame as i64 * 100_000_000),
                &vec![(frame * 64) as u8; LIVE_FRAME_BYTES],
                tick,
            )
            .unwrap();
    };
    offer(&mut runtime, 0);
    let mut accepted = Vec::new();
    let mut freshness_accepted = 0;
    let mut freshness_stale = 0;
    let mut held_work = None;
    let mut last_input = None;
    for sequence in 1..=if case == "live-stream" { 3 } else { 1 } {
        offer(&mut runtime, sequence);
        let tick = LiveClockTick(sequence * 100_000_000);
        let work = runtime.take(tick).unwrap().unwrap();
        let pixels = [work.previous(), work.current()].concat();
        let reply = worker.estimate(&work).unwrap();
        if let LiveCudaStreamReply::Estimated {
            position, input, ..
        } = &reply
        {
            let expected = 0.25 + 0.25 * ((sequence - 1) * 64 + sequence * 64) as f64 / 255.0;
            assert!(
                (position.value() - expected).abs() < 1e-5,
                "authored pair {sequence}: actual {} expected {expected}",
                position.value()
            );
            assert_eq!(input.pair, work.declaration());
            last_input = Some((input.clone(), pixels));
        } else {
            panic!("expected estimate");
        }
        accepted.push(reply);
        if case == "live-replay" {
            held_work = Some(work);
        } else {
            let age = if sequence == 2 { 3_000_000 } else { 1_000_000 };
            match runtime.complete(work, LiveClockTick(tick.0 + age)).unwrap() {
                LiveCompletion::Accepted { .. } => freshness_accepted += 1,
                LiveCompletion::Stale { .. } => freshness_stale += 1,
                LiveCompletion::Retired => panic!("active runtime retired"),
            }
        }
    }
    let mut refusal = None;
    let evidence = match case {
        "live-stream" => {
            assert_eq!((freshness_accepted, freshness_stale), (2, 1));
            Some(worker.finish(&pool).unwrap())
        }
        "live-replay" => {
            let work = held_work.take().unwrap();
            assert!(worker.estimate(&work).is_err());
            assert!(worker.estimate(&work).is_err());
            drop(work);
            assert!(worker.finish(&pool).is_err());
            None
        }
        "live-foreign-work" => {
            let mut foreign = crate::live_frames::LiveRgbRuntime::new(
                &pool,
                manifest.stream.identity.clone(),
                LivePolicy {
                    maximum_analysis_age_ns: 2_000_000,
                    added_buffer_ns: manifest.stream.clock.added_buffer_ns,
                },
                Some(manifest.stream.clock.clone()),
                LiveClockTick(0),
            )
            .unwrap();
            offer(&mut foreign, 0);
            offer(&mut foreign, 1);
            let work = foreign.take(LiveClockTick(100_000_000)).unwrap().unwrap();
            let error = worker.estimate(&work).unwrap_err();
            assert!(error.to_string().contains("another runtime scope"));
            refusal = Some(error.to_string());
            drop(work);
            drop(foreign);
            assert!(worker.finish(&pool).is_err());
            None
        }
        "live-cancel" | "live-expired" => {
            if case == "live-cancel" {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            worker
                .await_idle_exit_for_test(if case == "live-cancel" {
                    Instant::now() + Duration::from_secs(2)
                } else {
                    deadline + Duration::from_secs(2)
                })
                .unwrap();
            if case == "live-expired" {
                assert!(Instant::now() >= deadline);
            }
            // Child exit alone must not release the held image/storage/GPU claim.
            assert_eq!(
                pool.gpu_usage(&policy.device.device_uuid)
                    .unwrap()
                    .reserved_bytes,
                96 << 20
            );
            assert!(image_path.exists());
            assert_eq!(capture_pool.reserved_storage(), storage);
            assert!(worker.finish(&pool).is_err());
            None
        }
        "live-worker-replay" | "live-worker-pixels" | "live-worker-finish" => {
            let (mut input, mut pixels) = last_input.unwrap();
            let command = match case {
                "live-worker-replay" => {
                    pixels.clear();
                    LiveCudaStreamCommand::FramePair {
                        input: (input).into(),
                    }
                }
                "live-worker-pixels" => {
                    offer(&mut runtime, 2);
                    let work = runtime.take(LiveClockTick(200_000_000)).unwrap().unwrap();
                    pixels = [work.previous(), work.current()].concat();
                    let identity = |bytes: &[u8]| {
                        let sha = format!("{:x}", Sha256::digest(bytes));
                        ArtifactIdentity {
                            artifact_id: ArtifactId::new(sha.clone()).unwrap(),
                            sha256: sha,
                            byte_len: bytes.len() as u64,
                        }
                    };
                    input = LiveInferenceInput {
                        sequence: 2,
                        pair: work.declaration(),
                        previous: identity(work.previous()),
                        current: identity(work.current()),
                    };
                    input.current.sha256 = "0".repeat(64);
                    input.current.artifact_id = ArtifactId::new("0".repeat(64)).unwrap();
                    drop(work);
                    LiveCudaStreamCommand::FramePair {
                        input: (input).into(),
                    }
                }
                "live-worker-finish" => {
                    let mut transcript = LiveCudaTranscript::new(&manifest).unwrap();
                    let LiveCudaStreamReply::Estimated {
                        position,
                        pair_analysis_elapsed_ns,
                        ..
                    } = &accepted[0]
                    else {
                        unreachable!()
                    };
                    transcript
                        .append(&input, *position, *pair_analysis_elapsed_ns)
                        .unwrap();
                    let mut expected = transcript.finish().unwrap();
                    expected.transcript_sha256 = "0".repeat(64);
                    pixels.clear();
                    LiveCudaStreamCommand::Finish { expected }
                }
                _ => unreachable!(),
            };
            let diagnostic = worker.reject_command_for_test(&command, &pixels).unwrap();
            let required = match case {
                "live-worker-replay" => "previous frame identity or clock",
                "live-worker-pixels" => "pixels differ",
                _ => "transcripts differ",
            };
            assert!(
                diagnostic.contains(required),
                "native refusal differs: {diagnostic}"
            );
            refusal = Some(diagnostic);
            assert!(worker.finish(&pool).is_err());
            None
        }
        _ => panic!("unknown native stream case"),
    };
    runtime.retire(LiveClockTick(2_000_000_000)).unwrap();
    drop(runtime);
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if capture_pool.reserved_storage() == 0
            && pool.gpu_usage(&policy.device.device_uuid).unwrap() == GpuUsage::default()
            && pool
                .reserve(
                    manifest.stream.budget.memory_bytes * 2
                        - if evidence.is_some() {
                            crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES
                        } else {
                            0
                        },
                )
                .is_ok()
        {
            break;
        }
        assert!(
            Instant::now() < end,
            "native stream admission did not retire"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!image_path.exists());
    for path in &staged {
        assert!(!path.exists());
    }
    let ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state.join("worker-fence.json")).unwrap()).unwrap();
    for field in ["groups", "retired", "staging"] {
        assert!(ledger[field].as_object().unwrap().is_empty());
    }
    assert!(ledger["creation"]["records"]
        .as_object()
        .unwrap()
        .is_empty());
    if let Some(base) = std::env::var_os("PULSAR_LIVE_CUDA_STREAM_EVIDENCE") {
        let path = std::path::PathBuf::from(base).join(case);
        assert!(path.is_absolute() && !path.exists());
        artifacts::private_directory(&path).unwrap();
        std::fs::write(
            path.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(
            path.join("exchanges.json"),
            serde_json::to_vec_pretty(&accepted).unwrap(),
        )
        .unwrap();
        if let Some(evidence) = &evidence {
            assert_eq!(evidence.summary().successful_runs, 3);
            crate::live_cuda_result::verify_bytes(
                &manifest,
                evidence.summary(),
                evidence.receipt(),
                evidence.trace(),
            )
            .unwrap();
            std::fs::write(
                path.join("summary.json"),
                serde_json::to_vec_pretty(evidence.summary()).unwrap(),
            )
            .unwrap();
            std::fs::write(path.join("receipt.json"), evidence.receipt()).unwrap();
            std::fs::write(path.join("original-trace.json"), evidence.trace()).unwrap();
        }
        std::fs::write(path.join("checked.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "case":case,"measurement":measured,"inherited_deadline_ns":deadline_ns,
            "issued_runtime_scope":true,"freshness_accepted":freshness_accepted,"freshness_stale":freshness_stale,
            "authored_capture_ticks":true,"refusal":refusal,
            "gpu_claim_bytes":96u64<<20,"successful_exchanges":accepted.len(),"successful_finish":evidence.is_some(),
            "captured_libraries":policy.native_dependencies.libraries.len(),"all_custody_retired":true,
            "authored_model_only":true,"physical_dispatch":false,"real_media":false})).unwrap()).unwrap();
    }
    drop(evidence);
    assert!(pool
        .reserve(manifest.stream.budget.memory_bytes * 2)
        .is_ok());
}
