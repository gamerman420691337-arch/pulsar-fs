#[path = "live_cuda_native_tests.rs"]
pub(in crate::cuda_memory) mod stream;
use super::*;

#[test]
fn live_measurement_is_manifest_bound_and_does_not_renew_sample_time() {
    let manifest = crate::cuda_mounts::live_test_witness().manifest;
    let device = &manifest.execution;
    let reply = LiveMemoryReply {
        schema_version: 1,
        manifest_sha256: manifest.identity_sha256().unwrap(),
        sample: MemorySample {
            device: CudaSdkObservation {
                device_uuid: device.device_uuid.clone(),
                pci_bus_address: device.pci_bus_address.clone(),
                device_ordinal: device.device_ordinal as u16,
                visible_device_count: 1,
                total_bytes: 90,
                driver_version: 13040,
            },
            nvml_total_bytes: 100,
            nvml_free_bytes: 60,
            nvml_used_bytes: 40,
            sample_started_ns: cuda_bootstrap::now_ns().unwrap(),
        },
    };
    let observed = reply.observation(&manifest).unwrap();
    assert_eq!(observed.total_bytes, 90);
    assert_eq!(observed.available_bytes, 60);
    let age = Duration::from_millis(700);
    let now = Instant::now();
    assert_eq!(
        now.duration_since(
            reply
                .sample
                .observation_at(device, now, age)
                .unwrap()
                .observed_at
        ),
        age
    );
    assert!(reply
        .sample
        .observation_at(device, now, Duration::from_millis(1001))
        .is_err());
    let mut changed = manifest.clone();
    changed.stream.attempt_id = AttemptId::new("other").unwrap();
    assert!(reply.observation(&changed).is_err());
    let mut value = serde_json::to_value(&reply).unwrap();
    value["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<LiveMemoryReply>(value)
        .unwrap()
        .observation(&manifest)
        .is_err());
    let old = MemoryReply {
        schema_version: 1,
        request_sha256: "a".repeat(64),
        device: reply.sample.device.clone(),
        nvml_total_bytes: reply.sample.nvml_total_bytes,
        nvml_free_bytes: reply.sample.nvml_free_bytes,
        nvml_used_bytes: reply.sample.nvml_used_bytes,
        sample_started_ns: reply.sample.sample_started_ns,
    };
    assert!(serde_json::from_value::<LiveMemoryReply>(serde_json::to_value(old).unwrap()).is_err());
    let value = serde_json::to_value(reply).unwrap();
    assert!(serde_json::from_value::<MemoryReply>(value.clone()).is_err());
    for (pointer, replacement) in [
        ("/sample/device/device_uuid", serde_json::json!("wrong")),
        ("/sample/nvml_free_bytes", serde_json::json!(101)),
        (
            "/sample/sample_started_ns",
            serde_json::json!(cuda_bootstrap::now_ns().unwrap() + 1_000_000_000),
        ),
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(pointer).unwrap() = replacement;
        assert!(serde_json::from_value::<LiveMemoryReply>(bad)
            .unwrap()
            .observation(&manifest)
            .is_err());
    }
}

#[test]
#[ignore = "requires explicit real native spec/NVML, rebuilt worker, NVIDIA and delegated cgroups; run serially"]
fn native_live_memory_probe_admits_interactive_claim_keeps_lease_and_retires_cancelled_probe() {
    crate::cuda_memory::tests::native_memory_case(None, true);
}

pub(in crate::cuda_memory) fn native_case(
    config: &EngineConfig,
    request: &WorkerRequest,
    policy: &crate::cuda_owner_policy::CudaOwnerPolicy,
    loaded: &LoadedCudaOwnerPolicy,
    image: SupervisedNativeDependencies,
    capture_pool: &ResourcePool,
    storage: u64,
) {
    use crate::{artifacts, resources::GpuUsage, worker_fence::WorkerFence};
    let root = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    artifacts::private_directory(root.path()).unwrap();
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
        path: model.path.clone(),
        identity: model.identity.clone(),
    };
    stream.runtime = request.tools.onnx_runtime.clone().unwrap();
    stream.attempt_id = request.attempt_id.clone();
    stream.budget = request.budget.clone();
    stream.worker = crate::worker::pin_artifact(&config.worker_executable)
        .unwrap()
        .identity;
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
    assert!(
        policy
            .authorize_profile(
                &manifest.stream.profile,
                &manifest.stream.budget,
                GpuPriority::Offline
            )
            .is_err(),
        "96 MiB must exceed the offline 64 MiB ceiling"
    );
    let image_path = image.captured().library_directory();
    let deadline_ns = cuda_bootstrap::deadline_ns(manifest.stream.budget.wall_time_ms).unwrap();
    let state = root.path().join("fence");
    artifacts::private_directory(&state).unwrap();
    let fence = Arc::new(WorkerFence::open(&state).unwrap());
    fence.enable_creation().unwrap();
    let pool = ResourcePool::new_with_gpu_devices(
        manifest.stream.budget.memory_bytes * 2,
        2,
        vec![policy.accounting.clone()],
    )
    .unwrap();
    let make_custody = || {
        fence
            .admit_group(
                Arc::new(pool.reserve(manifest.stream.budget.memory_bytes).unwrap()),
                manifest.stream.budget.memory_bytes,
                manifest.stream.budget.cpu_threads,
            )
            .unwrap()
            .into_worker()
    };
    let mut custody = make_custody();
    let group = custody.group_handle().path().to_owned();
    let observed = observe(
        config,
        &manifest,
        deadline_ns,
        image,
        PinnedCudaDevices::open(&policy.device).unwrap(),
        custody,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!group.exists() && image_path.exists());
    assert_eq!(capture_pool.reserved_storage(), storage);
    assert_eq!(observed.deadline_ns, deadline_ns);
    let measured = serde_json::to_value(&observed.reply).unwrap();
    let fresh = observed.reply.observation(&manifest).unwrap();
    let mut changed = manifest.clone();
    changed.stream.attempt_id = AttemptId::new("changed").unwrap();
    assert!(observed.reply.observation(&changed).is_err());
    let mut admitted = observed.admit(&pool, loaded, &manifest).unwrap();
    assert!(admitted.validate(&changed).is_err());
    admitted.deadline_ns = 0;
    assert!(admitted.validate(&manifest).is_err());
    admitted.deadline_ns = deadline_ns;
    admitted.validate(&manifest).unwrap();
    assert_eq!(
        pool.gpu_usage(&policy.device.device_uuid).unwrap(),
        GpuUsage {
            reserved_bytes: 96 << 20,
            offline_reserved_bytes: 0
        }
    );
    assert!(pool
        .reserve_cuda(
            manifest.stream.budget.memory_bytes,
            manifest.stream.profile.cuda_resources.as_ref().unwrap(),
            &fresh,
            GpuPriority::Interactive
        )
        .is_err());
    let (image, devices, reservation, inherited_end) =
        admitted.into_launch_parts(&manifest).unwrap();
    assert_eq!(inherited_end, deadline_ns);
    drop(reservation);
    assert_eq!(
        pool.gpu_usage(&policy.device.device_uuid).unwrap(),
        GpuUsage::default()
    );
    let mut custody = make_custody();
    let cancelled_group = custody.group_handle().path().to_owned();
    assert!(observe(
        config,
        &manifest,
        deadline_ns,
        image,
        devices,
        custody,
        &AtomicBool::new(true)
    )
    .is_err());
    let end = Instant::now() + Duration::from_secs(5);
    while capture_pool.reserved_storage() > 0
        || pool
            .reserve(manifest.stream.budget.memory_bytes * 2)
            .is_err()
    {
        assert!(Instant::now() < end, "live memory custody did not retire");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!image_path.exists() && !cancelled_group.exists());
    let ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state.join("worker-fence.json")).unwrap()).unwrap();
    for field in ["groups", "retired", "staging"] {
        assert!(ledger[field].as_object().unwrap().is_empty());
    }
    assert!(ledger["creation"]["records"]
        .as_object()
        .unwrap()
        .is_empty());
    let evidence = serde_json::json!({"measurement":measured,"deadline_ns":deadline_ns,
        "inherited_deadline_unchanged":true,"captured_libraries":policy.native_dependencies.libraries.len(),
        "measured_interactive_claim_bytes":96u64<<20,"offline_ceiling_refused":true,
        "second_interactive_claim_refused":true,"image_survived_probe_retirement":true,
        "cancelled_probe_cleaned":true,"persistent_live_inference":false,"model_loaded":false});
    if let Some(path) = std::env::var_os("PULSAR_LIVE_CUDA_MEMORY_EVIDENCE") {
        let path = std::path::PathBuf::from(path);
        assert!(path.is_absolute() && !path.exists());
        artifacts::private_directory(&path).unwrap();
        std::fs::write(
            path.join("checked.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
        std::fs::write(
            path.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }
    eprintln!("CUDA live measured memory admission: {evidence}");
}

#[test]
fn stale_live_memory_error_preserves_sample_age_and_validation_stage() {
    let manifest = crate::cuda_mounts::live_test_witness().manifest;
    let device = &manifest.execution;
    let reply = LiveMemoryReply {
        schema_version: 1,
        manifest_sha256: manifest.identity_sha256().unwrap(),
        sample: MemorySample {
            device: CudaSdkObservation {
                device_uuid: device.device_uuid.clone(),
                pci_bus_address: device.pci_bus_address.clone(),
                device_ordinal: device.device_ordinal as u16,
                visible_device_count: 1,
                total_bytes: 90,
                driver_version: 13040,
            },
            nvml_total_bytes: 100,
            nvml_free_bytes: 60,
            nvml_used_bytes: 40,
            sample_started_ns: cuda_bootstrap::now_ns()
                .unwrap()
                .saturating_sub(2_000_000_000),
        },
    };
    for stage in [
        "completed observer transport",
        "image retirement",
        "pool admission",
    ] {
        let error = reply
            .checked_observation(&manifest, stage)
            .err()
            .expect("stale sample must be refused")
            .to_string();
        assert!(error.contains(stage) && error.contains("sample_age_ns=Some("));
        assert!(error.contains("CUDA memory observation is stale"));
    }
}
