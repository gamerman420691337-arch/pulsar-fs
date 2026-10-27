use super::*;

fn fixture() -> (WorkerRequest, MemoryReply) {
    let request: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap();
    let execution = request.cuda_execution.as_ref().unwrap();
    let reply = MemoryReply {
        schema_version: 1,
        request_sha256: cuda_mounts::request_hash(&request).unwrap(),
        device: CudaSdkObservation {
            device_uuid: execution.device_uuid.clone(),
            pci_bus_address: execution.pci_bus_address.clone(),
            device_ordinal: execution.device_ordinal as u16,
            visible_device_count: 1,
            total_bytes: 90,
            driver_version: 13040,
        },
        nvml_total_bytes: 100,
        nvml_free_bytes: 95,
        nvml_used_bytes: 5,
        sample_started_ns: 1,
    };
    (request, reply)
}
#[test]
fn measured_capacity_is_conservative_and_timestamp_is_not_renewed() {
    let (request, mut reply) = fixture();
    let now = Instant::now();
    let age = Duration::from_millis(700);
    let memory = reply.observation_at(&request, now, age).unwrap();
    assert_eq!(memory.total_bytes, 90);
    assert_eq!(memory.available_bytes, 90);
    assert_eq!(now.duration_since(memory.observed_at), age);
    reply.nvml_free_bytes = 60;
    reply.nvml_used_bytes = 40;
    assert_eq!(
        reply
            .observation_at(&request, now, age)
            .unwrap()
            .available_bytes,
        60
    );
    assert!(reply
        .observation_at(&request, now, Duration::from_millis(1001))
        .is_err());
    reply.sample_started_ns = cuda_bootstrap::now_ns().unwrap() + 1_000_000_000;
    assert!(reply.observation(&request).is_err());
}
#[test]
fn hostile_measurement_identity_capacity_and_schema_refuse() {
    let (request, reply) = fixture();
    let value = serde_json::to_value(&reply).unwrap();
    for pointer in [
        "/request_sha256",
        "/device/device_uuid",
        "/device/pci_bus_address",
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(pointer).unwrap() = serde_json::json!("wrong");
        let bad: MemoryReply = serde_json::from_value(bad).unwrap();
        assert!(bad
            .observation_at(&request, Instant::now(), Duration::ZERO)
            .is_err());
    }
    for (pointer, v) in [
        ("/schema_version", 2),
        ("/device/device_ordinal", 1),
        ("/device/visible_device_count", 0),
        ("/device/driver_version", 0),
        ("/device/total_bytes", 0),
        ("/nvml_total_bytes", 0),
        ("/nvml_free_bytes", 101),
        ("/nvml_used_bytes", u64::MAX),
        ("/sample_started_ns", 0),
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(pointer).unwrap() = serde_json::json!(v);
        let bad: MemoryReply = serde_json::from_value(bad).unwrap();
        assert!(
            bad.observation_at(&request, Instant::now(), Duration::ZERO)
                .is_err(),
            "{pointer}"
        );
    }
    let mut bad = value;
    bad["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MemoryReply>(bad).is_err());
    let mut changed = request;
    changed.attempt_id = AttemptId::new(uuid::Uuid::new_v4().to_string()).unwrap();
    assert!(reply
        .observation_at(&changed, Instant::now(), Duration::ZERO)
        .is_err());
}

#[test]
#[ignore = "requires explicit real native dependency spec/NVML path, rebuilt worker, actual NVIDIA device and delegated cgroups; run serially"]
fn native_memory_probe_returns_image_then_admits_fresh_gpu_claim_and_cleans_cancelled_probe() {
    native_memory_case(None, false);
}

#[test]
#[cfg(feature = "cuda")]
#[ignore = "requires explicit CUDA-enabled worker, real libraries/NVML, authored ONNX and NVIDIA/delegated cgroups; run serially"]
fn native_owned_cuda_launch_uses_measured_claim_shared_egress_and_exact_retirement() {
    for case in ["inference", "changed-request", "cancelled"] {
        native_memory_case(Some(case), false);
    }
}

#[test]
#[cfg(feature = "cuda")]
#[ignore = "requires newly built CUDA worker, actual captured libraries/NVML, authored ONNX and delegated cgroups; serial only"]
fn native_issued_live_cuda_stream_checks_scope_freshness_and_worker_failures() {
    for case in [
        "live-stream",
        "live-replay",
        "live-cancel",
        "live-expired",
        "live-foreign-work",
        "live-worker-replay",
        "live-worker-pixels",
        "live-worker-finish",
    ] {
        native_memory_case(Some(case), true);
    }
}

pub(super) fn native_memory_case(launch_case: Option<&str>, live_stream: bool) {
    use crate::{
        artifacts,
        cuda_owner_policy::CudaOwnerPolicy,
        native_capture,
        resources::{GpuDeviceBudget, GpuUsage},
        worker_fence::WorkerFence,
    };
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::{atomic::Ordering, Arc},
    };
    let worker = std::env::var_os(if launch_case.is_some() {
        "PULSAR_CUDA_WORKER"
    } else {
        "PULSAR_BENCHMARK_WORKER"
    })
    .expect("pin rebuilt worker for selected configuration");
    let spec_path = std::env::var_os("PULSAR_NATIVE_LIBRARY_SPEC").expect("pin real library spec");
    let nvml_path = std::env::var_os("PULSAR_NVML_LIBRARY").expect("pin explicit real NVML");
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let config = EngineConfig::new(root.path().join("unused-state"), worker.into());
    let mut spec: NativeDependencySpec =
        serde_json::from_slice(&fs::read(spec_path).unwrap()).unwrap();
    let path = std::path::PathBuf::from(nvml_path);
    let size = fs::metadata(&path).unwrap().len();
    let (sha256, byte_len) = artifacts::digest_file(&path, size).unwrap();
    spec.libraries.push(NativeLibrarySpec {
        path,
        sha256,
        byte_len,
        names: vec!["libnvidia-ml.so.1".into()],
    });
    let mut request: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap();
    request.attempt_id = AttemptId::new(uuid::Uuid::new_v4().to_string()).unwrap();
    request.output_dir = root.path().join("guest-output");
    if launch_case.is_some() {
        artifacts::private_directory(&request.output_dir).unwrap();
        request.budget.memory_bytes = 2 << 30;
        let model_path = root.path().join("authored.onnx");
        let model_source =
            std::env::var_os("PULSAR_AUTHORED_CUDA_MODEL").expect("pin authored CUDA model");
        fs::copy(model_source, &model_path).unwrap();
        let model = crate::worker::pin_artifact(&model_path).unwrap();
        let pair_path = root.path().join("authored.rgbpair");
        let mut pair = vec![0u8; 64 * 64 * 3];
        pair.extend(vec![128u8; 64 * 64 * 3]);
        fs::write(&pair_path, pair).unwrap();
        let pair = crate::worker::pin_artifact(&pair_path).unwrap();
        request.source.path = pair.path;
        request.source.identity = pair.identity;
        request.tools = crate::worker::discover_tools().unwrap();
        request.tools.onnx_runtime = Some(ArtifactReference {
            path: std::path::PathBuf::from("/placeholder/runtime"),
            identity: policy_runtime(&spec),
        });
        if let WorkerOperation::Generate {
            settings,
            model: reference,
            ..
        } = &mut request.operation
        {
            settings.inference_profile.as_mut().unwrap().model = model.identity.clone();
            *reference = Some(SourceArtifact {
                source_version: SourceVersionId::new("authored-model").unwrap(),
                path: model.path,
                identity: model.identity,
            });
            settings.raw_frame_pair = Some(RawFramePair {
                source_epoch: AttemptId::new("authored-pair-epoch").unwrap(),
                previous_frame: FrameId::new(0),
                current_frame: FrameId::new(1),
                previous_time: SourceTimestamp::new(0, 1).unwrap(),
                current_time: SourceTimestamp::new(1, 10).unwrap(),
                presentation_clock: None,
            });
        }
    }
    let execution = request.cuda_execution.as_ref().unwrap();
    let policy = CudaOwnerPolicy {
        schema_version: 1,
        device: crate::cuda_devices::CudaDeviceExpectation {
            device_uuid: execution.device_uuid.clone(),
            pci_bus_address: execution.pci_bus_address.clone(),
            device_minor: execution.device_minor,
        },
        accounting: GpuDeviceBudget {
            device_uuid: execution.device_uuid.clone(),
            device_ordinal: execution.device_ordinal,
            maximum_reserved_bytes: 128 << 20,
            interactive_reserved_bytes: 64 << 20,
            control_reserve_bytes: 256 << 20,
        },
        native_dependencies: spec,
    };
    let policy_path = root.path().join("policy.json");
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
    let loaded = LoadedCudaOwnerPolicy::load(&policy_path).unwrap();
    let capture_pool = ResourcePool::new(1 << 30, 1);
    let total = policy.native_dependencies.validate().unwrap();
    let image = native_capture::capture(
        &config,
        &capture_pool,
        &policy.native_dependencies,
        &root.path().join("captures"),
        ResourceBudget {
            memory_bytes: 512 << 20,
            output_bytes: total + (16 << 20),
            wall_time_ms: 30_000,
            cpu_threads: 1,
        },
        1536 << 20,
        &AtomicBool::new(false),
    )
    .unwrap();
    let image_path = image.captured().library_directory();
    let storage = capture_pool.reserved_storage();
    assert!(storage > 0);
    let profile = match &mut request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_mut().unwrap(),
        _ => unreachable!(),
    };
    profile.runtime = policy.runtime_identity().unwrap();
    let execution = loaded
        .worker_execution_supervised(&image, profile, &request.budget, GpuPriority::Offline)
        .unwrap();
    request.tools.onnx_runtime.as_mut().unwrap().identity = profile.runtime.clone();
    request.tools.onnx_runtime.as_mut().unwrap().path = image_path.join("libonnxruntime.so.1");
    request.dependencies = vec![
        request.source.identity.clone(),
        request.tools.ffmpeg.identity.clone(),
        request.tools.ffprobe.identity.clone(),
    ];
    if launch_case.is_some() {
        if let WorkerOperation::Generate {
            settings, model, ..
        } = &request.operation
        {
            request
                .dependencies
                .push(model.as_ref().unwrap().identity.clone());
            request.dependencies.push(
                crate::inference_profiles::profile_identity(
                    settings.inference_profile.as_ref().unwrap(),
                )
                .unwrap(),
            );
        }
    }
    request.dependencies.push(execution.owner_policy.clone());
    request
        .dependencies
        .extend(execution.libraries.iter().map(|lib| lib.identity.clone()));
    request.cuda_execution = Some(execution);
    if live_stream {
        if let Some(case) = launch_case {
            super::live::tests::stream::native_case(
                &config,
                &request,
                &policy,
                &loaded,
                super::live::tests::stream::NativeCaptureFixture {
                    image,
                    capture_pool: &capture_pool,
                    storage,
                },
                case,
            );
        } else {
            super::live::tests::native_case(
                &config,
                &request,
                &policy,
                &loaded,
                image,
                &capture_pool,
                storage,
            );
        }
        return;
    }
    let devices = PinnedCudaDevices::open(&policy.device).unwrap();
    let state = root.path().join("fence");
    artifacts::private_directory(&state).unwrap();
    let fence = Arc::new(WorkerFence::open(&state).unwrap());
    fence.enable_creation().unwrap();
    let pool = ResourcePool::new_with_gpu_devices(
        request.budget.memory_bytes * 2,
        2,
        vec![policy.accounting.clone()],
    )
    .unwrap();
    let make_custody = || {
        fence
            .admit_group(
                Arc::new(pool.reserve(request.budget.memory_bytes).unwrap()),
                request.budget.memory_bytes,
                request.budget.cpu_threads,
            )
            .unwrap()
            .into_worker()
    };
    let mut custody = make_custody();
    let group = custody.group_handle().path().to_owned();
    let observed = observe(
        &config,
        &request,
        image,
        devices,
        custody,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!group.exists());
    assert!(image_path.exists());
    assert_eq!(capture_pool.reserved_storage(), storage);
    let measured = serde_json::to_value(&observed.reply).unwrap();
    let fresh = observed.reply.observation(&request).unwrap();
    let admitted = observed
        .admit(&pool, &loaded, &request, GpuPriority::Offline)
        .unwrap();
    assert_eq!(
        pool.gpu_usage(&policy.device.device_uuid).unwrap(),
        GpuUsage {
            reserved_bytes: 64 << 20,
            offline_reserved_bytes: 64 << 20
        }
    );
    let profile = match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref().unwrap(),
        _ => unreachable!(),
    };
    let refusal = pool
        .reserve_cuda(
            request.budget.memory_bytes,
            profile.cuda_resources.as_ref().unwrap(),
            &fresh,
            GpuPriority::Offline,
        )
        .err()
        .expect("second offline claim must exceed the accounting budget");
    assert_eq!(refusal.code, ErrorCode::ResourceExhausted);
    assert_eq!(refusal.message, "aggregate GPU admission budget exhausted");
    if let Some(case) = launch_case {
        let before_groups = fence_group_count(&state);
        assert_eq!(before_groups, 0);
        let mut launch_request = request.clone();
        if case == "changed-request" {
            launch_request.attempt_id = AttemptId::new(uuid::Uuid::new_v4().to_string()).unwrap();
        }
        let result = crate::launch_cuda::execute_reserved(
            &config,
            &launch_request,
            &AtomicBool::new(case == "cancelled"),
            admitted,
            fence.clone(),
        );
        if case == "inference" {
            let result = result.unwrap();
            let evidence = result
                .cuda_artifacts
                .as_ref()
                .expect("CUDA evidence missing");
            crate::cuda_result::verify_files(&request, evidence).unwrap();
            if let Some(root) = std::env::var_os("PULSAR_OWNED_CUDA_EVIDENCE") {
                let root = std::path::PathBuf::from(root);
                assert!(
                    root.is_absolute() && !root.exists(),
                    "new absolute evidence root required"
                );
                artifacts::private_directory(&root).unwrap();
                fs::write(
                    root.join("request.json"),
                    serde_json::to_vec_pretty(&request).unwrap(),
                )
                .unwrap();
                let WorkerOutput::Generated {
                    program, receipt, ..
                } = &result.output
                else {
                    panic!("expected generation")
                };
                let mut saved = Vec::new();
                for (name, reference) in [
                    ("program.json", program),
                    ("receipt.json", receipt),
                    ("cuda-inference.json", &evidence.receipt),
                    ("cuda-trace.json", &evidence.trace),
                ] {
                    fs::copy(&reference.path, root.join(name)).unwrap();
                    let (sha256, byte_len) =
                        artifacts::digest_file(&root.join(name), reference.identity.byte_len)
                            .unwrap();
                    assert_eq!(sha256, reference.identity.sha256);
                    assert_eq!(byte_len, reference.identity.byte_len);
                    saved.push(serde_json::json!({"name":name,"identity":reference.identity}));
                }
                fs::write(
                    root.join("artifacts.json"),
                    serde_json::to_vec_pretty(&saved).unwrap(),
                )
                .unwrap();
                crate::cuda_result::verify_bytes(
                    &request,
                    &fs::read(root.join("cuda-inference.json")).unwrap(),
                    &fs::read(root.join("cuda-trace.json")).unwrap(),
                )
                .unwrap();
            }
            let WorkerOutput::Generated { program, .. } = result.output else {
                panic!("expected generation")
            };
            let motion: MotionProgram =
                serde_json::from_slice(&fs::read(&program.path).unwrap()).unwrap();
            let actions = motion.tracks()[0].actions();
            assert_eq!(actions.len(), 2);
            assert!((actions[1].position().value() - (0.25 + 0.25 * 128.0 / 255.0)).abs() < 1e-5);
        } else {
            let error = result.err().expect("invalid CUDA launch accepted");
            assert_eq!(
                error.code,
                if case == "cancelled" {
                    ErrorCode::Cancelled
                } else {
                    ErrorCode::DependencyMismatch
                }
            );
            if case == "changed-request" {
                assert!(error.message.contains("admitted image request differs"));
            }
            assert!(
                !request.output_dir.exists()
                    || fs::read_dir(&request.output_dir).unwrap().next().is_none()
            );
        }
        let end = Instant::now() + Duration::from_secs(5);
        while capture_pool.reserved_storage() > 0
            || pool.gpu_usage(&policy.device.device_uuid).unwrap() != GpuUsage::default()
        {
            assert!(
                Instant::now() < end,
                "CUDA launch cleanup did not retire claims"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!image_path.exists());
        assert_eq!(fence_group_count(&state), 0);
        let ledger: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("worker-fence.json")).unwrap()).unwrap();
        for field in ["retired", "staging"] {
            assert!(ledger[field].as_object().unwrap().is_empty());
        }
        assert!(ledger["creation"]["records"]
            .as_object()
            .unwrap()
            .is_empty());
        assert!(pool.reserve(request.budget.memory_bytes * 2).is_ok());
        eprintln!(
            "owned CUDA launch: {}",
            serde_json::json!({"case":case,"measured_admission":true,"group_retired":true,"image_removed":true,"gpu_claim_released":true,"committed_engine_job":false})
        );
        return;
    }
    let AdmittedCudaImage {
        image,
        devices,
        reservation,
        ..
    } = admitted;
    drop(reservation);
    assert_eq!(
        pool.gpu_usage(&policy.device.device_uuid).unwrap(),
        GpuUsage::default()
    );
    let cancel = AtomicBool::new(true);
    assert!(cancel.load(Ordering::Acquire));
    let mut custody = make_custody();
    let cancelled_group = custody.group_handle().path().to_owned();
    assert!(observe(&config, &request, image, devices, custody, &cancel).is_err());
    let end = Instant::now() + Duration::from_secs(5);
    while capture_pool.reserved_storage() != 0
        || pool.reserve(request.budget.memory_bytes * 2).is_err()
    {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!image_path.exists() && !cancelled_group.exists());
    let ledger: serde_json::Value =
        serde_json::from_slice(&fs::read(state.join("worker-fence.json")).unwrap()).unwrap();
    for field in ["groups", "retired", "staging"] {
        assert!(ledger[field].as_object().unwrap().is_empty());
    }
    assert!(ledger["creation"]["records"]
        .as_object()
        .unwrap()
        .is_empty());
    eprintln!(
        "CUDA measured memory admission: {}",
        serde_json::json!({"measurement":measured,"captured_libraries":policy.native_dependencies.libraries.len(),"captured_bytes":total,"image_survived_probe_retirement":true,"measured_gpu_claim_bytes":64u64<<20,"second_offline_refused":true,"second_offline_refusal":"aggregate GPU admission budget exhausted","claim_released":true,"cancelled_probe_cleaned":true,"production_inference":false,"nvml_header_sha256":format!("{:x}",Sha256::digest(fs::read("/opt/cuda/include/nvml.h").unwrap()))})
    );
}

fn policy_runtime(spec: &NativeDependencySpec) -> ArtifactIdentity {
    let library = spec
        .libraries
        .iter()
        .find(|library| {
            library
                .names
                .iter()
                .any(|name| name == "libonnxruntime.so.1")
        })
        .unwrap();
    ArtifactIdentity {
        artifact_id: ArtifactId::new(library.sha256.clone()).unwrap(),
        sha256: library.sha256.clone(),
        byte_len: library.byte_len,
    }
}

fn fence_group_count(state: &std::path::Path) -> usize {
    let ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state.join("worker-fence.json")).unwrap()).unwrap();
    ledger["groups"].as_object().unwrap().len()
}
