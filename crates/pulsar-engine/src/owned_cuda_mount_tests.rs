use super::*;
use crate::{
    cuda_owner_policy::{CudaOwnerPolicy, LoadedCudaOwnerPolicy},
    native_capture,
    resources::{GpuDeviceBudget, GpuPriority, ResourcePool},
    staging_obligation::Staging,
    worker_fence::WorkerFence,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[test]
#[ignore = "requires pinned native worker, actual NVIDIA nodes/modules and writable delegated cgroups; run serially"]
fn native_owned_mounts_keep_image_descriptors_and_admission_until_transport_retirement() {
    run_owned_mount_cases(false);
}
#[test]
#[ignore = "requires pinned native worker, actual NVIDIA nodes/modules and writable delegated cgroups; run serially"]
fn native_live_owned_mounts_keep_separate_identity_and_retirement_custody() {
    run_owned_mount_cases(true);
}
struct TestMountPlan {
    command: Command,
    custody: crate::worker_retirement::WorkerCustody,
    witness: serde_json::Value,
}
fn run_owned_mount_cases(live_stream: bool) {
    let worker = std::env::var_os("PULSAR_BENCHMARK_WORKER").expect("pin actual worker");
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let config = EngineConfig::new(root.path().join("unused-state"), worker.into());
    let template: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap();
    let expected = template.cuda_execution.as_ref().unwrap();
    let mut policy = CudaOwnerPolicy {
        schema_version: 1,
        device: crate::cuda_devices::CudaDeviceExpectation {
            device_uuid: expected.device_uuid.clone(),
            pci_bus_address: expected.pci_bus_address.clone(),
            device_minor: expected.device_minor,
        },
        accounting: GpuDeviceBudget {
            device_uuid: expected.device_uuid.clone(),
            device_ordinal: 0,
            maximum_reserved_bytes: 128 * 1024 * 1024,
            interactive_reserved_bytes: 64 * 1024 * 1024,
            control_reserve_bytes: 1024 * 1024,
        },
        native_dependencies: NativeDependencySpec {
            schema_version: 1,
            libraries: vec![],
        },
    };
    for (i, name) in [
        "ld-linux-x86-64.so.2",
        "libcuda.so.1",
        "libonnxruntime.so.1",
        "libonnxruntime_providers_cuda.so",
        "libonnxruntime_providers_shared.so",
        "libcudnn.so.9",
        "libnvidia-ml.so.1",
    ]
    .iter()
    .enumerate()
    {
        let bytes = vec![i as u8; i + 1];
        let path = root.path().join(format!("supplier-{i}"));
        fs::write(&path, &bytes).unwrap();
        policy
            .native_dependencies
            .libraries
            .push(NativeLibrarySpec {
                path,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                byte_len: bytes.len() as u64,
                names: vec![name.to_string()],
            });
    }
    let policy_path = root.path().join("policy.json");
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
    let loaded = LoadedCudaOwnerPolicy::load(&policy_path).unwrap();
    let capture_pool = ResourcePool::new(1024 * 1024 * 1024, 1);
    let worker_pool = ResourcePool::new(template.budget.memory_bytes, 1);
    let mut cases = vec![
        "baseline-replaced-path",
        "corrupt-backing",
        "failed-spawn",
        "invalid-request",
        "blocked-staging-cleanup",
    ];
    if live_stream {
        cases.extend(["wrong-manifest", "expired-deadline"]);
    }
    for case in cases {
        let case_root = root.path().join(case);
        fs::create_dir(&case_root).unwrap();
        fs::set_permissions(&case_root, fs::Permissions::from_mode(0o700)).unwrap();
        let image = native_capture::capture(
            &config,
            &capture_pool,
            &policy.native_dependencies,
            &case_root.join("captures"),
            ResourceBudget {
                memory_bytes: 512 * 1024 * 1024,
                output_bytes: 16 * 1024 * 1024 + 28,
                wall_time_ms: 10_000,
                cpu_threads: 1,
            },
            1536 * 1024 * 1024,
            &AtomicBool::new(false),
        )
        .unwrap();
        let image_path = image.captured().library_directory();
        let mut request = template.clone();
        request.attempt_id = AttemptId::new(uuid::Uuid::new_v4().to_string()).unwrap();
        request.output_dir = case_root.join("guest-output");
        let profile = match &mut request.operation {
            WorkerOperation::Generate { settings, .. } => {
                settings.inference_profile.as_mut().unwrap()
            }
            _ => unreachable!(),
        };
        profile.runtime = policy.runtime_identity().unwrap();
        let execution = loaded
            .worker_execution_supervised(&image, profile, &request.budget, GpuPriority::Offline)
            .unwrap();
        request.tools.onnx_runtime.as_mut().unwrap().identity = profile.runtime.clone();
        request.tools.onnx_runtime.as_mut().unwrap().path = image_path.join("libonnxruntime.so.1");
        request.dependencies.push(execution.owner_policy.clone());
        request
            .dependencies
            .extend(execution.libraries.iter().map(|lib| lib.identity.clone()));
        request.cuda_execution = Some(execution);
        let devices = PinnedCudaDevices::open(&policy.device).unwrap();
        let state = case_root.join("fence");
        crate::artifacts::private_directory(&state).unwrap();
        let fence = Arc::new(WorkerFence::open(&state).unwrap());
        fence.enable_creation().unwrap();
        let reservation = Arc::new(worker_pool.reserve(request.budget.memory_bytes).unwrap());
        let stage = Staging::Durable(fence.create_staging(reservation.clone()).unwrap());
        let stage_path = stage.path();
        let blocked = stage_path.join("blocked");
        if case == "blocked-staging-cleanup" {
            fs::create_dir(&blocked).unwrap();
            fs::write(
                blocked.join("authored"),
                b"must retain owners during failed deletion",
            )
            .unwrap();
            fs::set_permissions(&blocked, fs::Permissions::from_mode(0o500)).unwrap();
        }
        let mut custody = fence
            .admit_staged_group(
                reservation,
                request.budget.memory_bytes,
                request.budget.cpu_threads,
                stage,
            )
            .unwrap()
            .into_worker();
        let dropped = Arc::new(AtomicBool::new(false));
        custody.retain_asset(Dropped(dropped.clone()));
        let (release, wait) = std::sync::mpsc::channel();
        custody.retain_thread(std::thread::spawn(move || {
            let _ = wait.recv();
        }));
        let group = custody.group_handle();
        let group_path = group.path().to_owned();
        drop(group);
        drop(fence);
        if case == "invalid-request" {
            request.cuda_execution.as_mut().unwrap().library_directory =
                case_root.join("foreign-image");
        }
        let result = if live_stream {
            let mut stream: LiveInferenceManifest = serde_json::from_str(include_str!(
                "../../pulsar-protocol/tests/fixtures/live-cpu-manifest.json"
            ))
            .unwrap();
            stream.attempt_id = request.attempt_id.clone();
            stream.runtime = request.tools.onnx_runtime.clone().unwrap();
            stream.budget = request.budget.clone();
            let offline_profile = match &request.operation {
                WorkerOperation::Generate { settings, .. } => {
                    settings.inference_profile.as_ref().unwrap()
                }
                _ => unreachable!(),
            };
            stream.profile.schema_version = 2;
            stream.profile.provider = InferenceProvider::Cuda;
            stream.profile.runtime = stream.runtime.identity.clone();
            stream.profile.cpu_threads = stream.budget.cpu_threads;
            stream.profile.cuda_resources = offline_profile.cuda_resources.clone();
            let mut dependencies = request.dependencies.clone();
            dependencies.extend([stream.model.identity.clone(), stream.worker.clone()]);
            let manifest = LiveCudaInferenceManifest {
                schema_version: 1,
                stream,
                execution: request.cuda_execution.clone().unwrap(),
                dependencies,
                output_dir: request.output_dir.clone(),
            };
            let end =
                crate::cuda_bootstrap::deadline_ns(manifest.stream.budget.wall_time_ms).unwrap();
            live::OwnedLiveCudaMountCommand::inspection(
                &config, &manifest, end, image, devices, custody,
            )
            .map(|plan| {
                let (command, custody, witness) = plan.into_parts();
                TestMountPlan {
                    command,
                    custody,
                    witness: serde_json::to_value(witness).unwrap(),
                }
            })
        } else {
            OwnedCudaMountCommand::inspection(&config, &request, image, devices, custody).map(
                |plan| {
                    let (command, custody, witness) = plan.into_parts();
                    TestMountPlan {
                        command,
                        custody,
                        witness: serde_json::to_value(witness).unwrap(),
                    }
                },
            )
        };
        if case == "invalid-request" {
            assert!(result.is_err());
        } else {
            let mut plan = result.unwrap();
            if case == "wrong-manifest" {
                plan.witness["manifest_sha256"] = serde_json::json!("0".repeat(64));
            }
            if case == "expired-deadline" {
                plan.witness["deadline_ns"] = serde_json::json!(0);
            }
            let descriptors = plan
                .command
                .get_args()
                .filter_map(|a| a.to_str())
                .filter_map(|s| s.strip_prefix("/proc/self/fd/"))
                .map(|s| s.parse::<i32>().unwrap())
                .collect::<Vec<_>>();
            assert!(descriptors.len() >= 12);
            let alias = image_path.join("libcuda.so.1");
            if case == "baseline-replaced-path" {
                fs::rename(&alias, case_root.join("held-alias")).unwrap();
                fs::write(&alias, b"replacement is not the pinned inode").unwrap();
            }
            if case == "corrupt-backing" {
                fs::set_permissions(&alias, fs::Permissions::from_mode(0o600)).unwrap();
                fs::write(&alias, [9u8, 9]).unwrap();
                fs::set_permissions(&alias, fs::Permissions::from_mode(0o400)).unwrap();
            }
            if case == "failed-spawn" {
                plan.command = Command::new("/absent-pulsar-owned-mount-worker");
                assert!(plan.custody.spawn(&mut plan.command).is_err());
            } else {
                plan.custody.spawn(&mut plan.command).unwrap();
                let (mut stdin, mut stdout, mut stderr) = {
                    let mut child = plan.custody.child();
                    (
                        child.stdin.take().unwrap(),
                        child.stdout.take().unwrap(),
                        child.stderr.take().unwrap(),
                    )
                };
                let witness = serde_json::to_value(&plan.witness).unwrap();
                plan.custody.retain_thread(std::thread::spawn(move || {
                    write_message(&mut stdin, &witness).unwrap();
                }));
                let (send, recv) = std::sync::mpsc::channel();
                plan.custody.retain_thread(std::thread::spawn(move || {
                    let mut b = vec![];
                    let r = stdout.by_ref().take(65537).read_to_end(&mut b);
                    let _ = send.send((r, b));
                }));
                let (send_err, recv_err) = std::sync::mpsc::channel();
                plan.custody.retain_thread(std::thread::spawn(move || {
                    let mut b = vec![];
                    let r = stderr.by_ref().take(16385).read_to_end(&mut b);
                    let _ = send_err.send((r, b));
                }));
                let end = Instant::now() + Duration::from_secs(10);
                let status = loop {
                    if let Some(status) = plan.custody.child().try_wait().unwrap() {
                        break status;
                    }
                    assert!(Instant::now() < end, "mount checker child deadline");
                    std::thread::sleep(Duration::from_millis(10));
                };
                assert_eq!(
                    status.success(),
                    !matches!(
                        case,
                        "corrupt-backing" | "wrong-manifest" | "expired-deadline"
                    )
                );
                let (r, bytes) = recv.recv_timeout(Duration::from_secs(1)).unwrap();
                r.unwrap();
                let (r, errors) = recv_err.recv_timeout(Duration::from_secs(1)).unwrap();
                r.unwrap();
                if case == "corrupt-backing" {
                    assert!(String::from_utf8_lossy(&errors)
                        .contains("mounted native alias hash differs"));
                } else if matches!(case, "wrong-manifest" | "expired-deadline") {
                    assert!(bytes.is_empty());
                    let errors = String::from_utf8_lossy(&errors);
                    assert!(
                        errors.contains(if case == "wrong-manifest" {
                            "CUDA live mount manifest identity differs"
                        } else {
                            "CUDA bootstrap deadline expired"
                        }),
                        "{errors}"
                    );
                } else {
                    assert!(String::from_utf8_lossy(&bytes).contains("passed"));
                    if live_stream {
                        let checked: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                        assert_eq!(checked["manifest_sha256"], plan.witness["manifest_sha256"]);
                        assert_eq!(checked["deadline_ns"], plan.witness["deadline_ns"]);
                        assert_eq!(checked["native_execution"], false);
                        assert_eq!(checked["gpu_admission"], false);
                    }
                }
            }
            drop(plan);
            for fd in descriptors {
                assert!(
                    unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0,
                    "descriptor lost before transport retirement"
                );
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(!dropped.load(Ordering::Acquire));
        assert!(image_path.exists());
        assert!(capture_pool.reserved_storage() > 0);
        assert!(worker_pool.reserve(template.budget.memory_bytes).is_err());
        assert!(WorkerFence::open(&state).is_err());
        assert!(group_path.exists());
        release.send(()).unwrap();
        if case == "blocked-staging-cleanup" {
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                let ledger: serde_json::Value =
                    serde_json::from_slice(&fs::read(state.join("worker-fence.json")).unwrap())
                        .unwrap();
                if ledger["staging"]
                    .as_object()
                    .unwrap()
                    .values()
                    .any(|record| {
                        record["phase"]
                            .as_object()
                            .is_some_and(|phase| phase.contains_key("Retired"))
                    })
                {
                    break;
                }
                assert!(Instant::now() < end, "staging retirement was not published");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                fs::read(blocked.join("authored")).unwrap(),
                b"must retain owners during failed deletion"
            );
            assert!(!dropped.load(Ordering::Acquire));
            assert!(capture_pool.reserved_storage() > 0 && image_path.exists());
            assert!(worker_pool.reserve(template.budget.memory_bytes).is_err());
            assert!(WorkerFence::open(&state).is_err());
            fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let end = Instant::now() + Duration::from_secs(5);
        while !dropped.load(Ordering::Acquire)
            || capture_pool.reserved_storage() != 0
            || worker_pool.reserve(template.budget.memory_bytes).is_err()
        {
            assert!(Instant::now() < end, "owned CUDA cleanup stalled");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!group_path.exists() && !image_path.exists());
        let fence = WorkerFence::open(&state).unwrap();
        let ledger: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("worker-fence.json")).unwrap()).unwrap();
        for field in ["groups", "retired", "staging"] {
            assert!(ledger[field].as_object().unwrap().is_empty());
        }
        assert!(ledger["creation"]["records"]
            .as_object()
            .unwrap()
            .is_empty());
        drop(fence);
        eprintln!("owned CUDA mount case passed: live={live_stream} {case}; no device IO, native library loading or inference");
    }
}
