//! Authored failure metadata and real child/cgroup custody; no CUDA execution,
//! provider observation, equivalence acceptance or physical qualification.
use super::*;

fn reply(manifest: &LiveCudaInferenceManifest) -> LiveCudaStreamReply {
    LiveCudaStreamReply::ProviderUnavailable {
        manifest_sha256: manifest.identity_sha256().unwrap(),
        observation: InferenceProviderUnavailable {
            schema_version: 1,
            provider: InferenceProvider::Cuda,
            profile_sha256: crate::inference_profiles::profile_identity(&manifest.stream.profile)
                .unwrap()
                .sha256,
            check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
        },
    }
}

#[test]
fn settled_failure_is_bound_to_original_manifest_and_deadline() {
    // Private authored boundary fixture only; constructing it here is not a
    // claim that a native worker or its resources actually retired.
    let manifest = crate::cuda_mounts::live_test_witness().manifest;
    let evidence = reply(&manifest)
        .validate_provider_unavailable(&manifest)
        .unwrap()
        .clone();
    let mut settled = SettledLiveCudaProviderFailure {
        manifest: manifest.clone(),
        observation: evidence,
        deadline: Instant::now() + Duration::from_secs(3),
    };
    settled.matches_manifest(&manifest).unwrap();
    let mut changed = manifest.clone();
    changed.stream.attempt_id = AttemptId::new("replacement-attempt").unwrap();
    assert!(settled.matches_manifest(&changed).is_err());
    settled.observation.profile_sha256 = "0".repeat(64);
    assert!(settled.matches_manifest(&manifest).is_err());
    settled.observation = reply(&manifest)
        .validate_provider_unavailable(&manifest)
        .unwrap()
        .clone();
    settled.deadline = Instant::now();
    assert!(settled.matches_manifest(&manifest).is_err());
    let generic = anyhow::anyhow!("{settled}");
    assert!(generic
        .downcast::<SettledLiveCudaProviderFailure>()
        .is_err());
}

struct ReleaseThread(Option<std::sync::mpsc::Sender<()>>);
impl Drop for ReleaseThread {
    fn drop(&mut self) {
        if let Some(send) = self.0.take() {
            let _ = send.send(());
        }
    }
}

#[test]
#[ignore = "requires owned writable delegated cgroup and durable proof path; authored metadata, no CUDA or device calls"]
fn native_provider_failure_requires_eof_exit_and_complete_resource_retirement() {
    use crate::{resources::ResourcePool, worker_fence::WorkerFence};
    use std::{
        fs,
        os::{
            fd::AsRawFd,
            unix::{fs::PermissionsExt, process::CommandExt},
        },
        process::{Command, Stdio},
    };
    let proof = std::path::PathBuf::from(
        std::env::var_os("PULSAR_LIVE_PROVIDER_SETTLEMENT_PROOF").expect("retained proof path"),
    );
    assert!(proof.is_absolute() && !proof.exists());
    let mut cases = Vec::new();
    for case in [
        "settled",
        "trailing",
        "wrong_exit",
        "signalled",
        "cancelled",
        "expired",
        "retirement_pending",
        "wrong_manifest",
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
        fence.enable_creation().unwrap();
        let mut manifest = crate::cuda_mounts::live_test_witness().manifest;
        manifest.stream.budget.wall_time_ms = 5000;
        let memory = manifest.stream.budget.memory_bytes;
        let total_memory = memory * 2 + (16 << 20);
        let pool = ResourcePool::new(total_memory, 2);
        let runtime = crate::live_frames::LiveRgbRuntime::new(
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
        let reservation = Arc::new(
            pool.reserve_worker(memory, manifest.stream.budget.cpu_threads)
                .unwrap()
                .with_storage(4096, total_memory)
                .unwrap(),
        );
        let staged = tempfile::tempdir().unwrap();
        let staged_path = staged.path().to_owned();
        let mut custody = fence
            .admit_staged_group_with_assets(
                reservation,
                memory,
                manifest.stream.budget.cpu_threads,
                staged.into(),
                Vec::new(),
            )
            .unwrap()
            .into_worker();
        let group = custody.group_handle();
        let mut authored = reply(&manifest);
        if case == "wrong_manifest" {
            let LiveCudaStreamReply::ProviderUnavailable {
                manifest_sha256, ..
            } = &mut authored
            else {
                unreachable!()
            };
            *manifest_sha256 = "0".repeat(64);
        }
        let mut framed = Vec::new();
        write_bulk_header(&mut framed, &authored).unwrap();
        if case == "trailing" {
            framed.push(1);
        }
        // Echo exactly one authored frame from this parent's pipe, then take
        // the selected real exit. These commands never load a CUDA library.
        let exit = match case {
            "wrong_exit" => "exit 0",
            "signalled" => "kill -TERM $$",
            _ => "exit 1",
        };
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("/usr/bin/head -c {}; {exit}", framed.len()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let fd = group.procs.as_raw_fd();
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) == 1 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        custody.spawn(&mut command).unwrap();
        let (mut stdin, stdout, stderr) = {
            let mut child = custody.child();
            (
                child.stdin.take().unwrap(),
                child.stdout.take().unwrap(),
                child.stderr.take().unwrap(),
            )
        };
        stdin.write_all(&framed).unwrap();
        stdin.flush().unwrap();
        crate::launch_pipes::nonblocking(&stdin).unwrap();
        crate::launch_pipes::nonblocking(&stdout).unwrap();
        crate::launch_pipes::nonblocking(&stderr).unwrap();
        let (release, receive) = std::sync::mpsc::channel();
        let release = ReleaseThread(Some(release));
        if case == "retirement_pending" {
            custody.retain_thread(std::thread::spawn(move || {
                let _ = receive.recv();
            }));
        }
        let child = custody.child_handle();
        let cancel = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + Duration::from_millis(manifest.stream.budget.wall_time_ms);
        let mut worker = LiveCudaWorker {
            transcript: Some(LiveCudaTranscript::new(&manifest).unwrap()),
            manifest: manifest.clone(),
            scope: runtime.inference_scope(),
            sequence: 0,
            stdin,
            stdout,
            monitor: Monitor::owned(deadline, cancel.clone(), stderr),
            stop: Arc::new(AtomicBool::new(false)),
            custody: Some(custody),
        };
        let received: LiveCudaStreamReply =
            read_bulk_header(&mut worker.monitor.read(&mut worker.stdout)).unwrap();
        match case {
            "cancelled" => cancel.store(true, Ordering::Release),
            "expired" => worker.monitor.deadline = Instant::now(),
            "retirement_pending" => {
                worker.monitor.deadline = Instant::now() + Duration::from_millis(20)
            }
            _ => {}
        }
        let outcome = worker.settle_provider_unavailable(received);
        assert_eq!(outcome.is_ok(), case == "settled", "{case}: {outcome:?}");
        if let Ok(settled) = &outcome {
            settled.matches_manifest(&manifest).unwrap();
            assert!(worker.custody.is_none());
            assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
            assert_eq!(pool.reserved_storage(), 0);
            assert!(!staged_path.exists());
        }
        if case == "retirement_pending" {
            assert!(pool.reserved_cpu_threads().unwrap() > 0);
            assert_eq!(pool.reserved_storage(), 4096);
        }
        drop(worker);
        drop(release);
        drop(runtime);
        let cleanup_deadline = Instant::now() + Duration::from_secs(5);
        while pool.reserved_storage() != 0
            || pool.reserved_cpu_threads().unwrap() != 0
            || staged_path.exists()
        {
            assert!(
                Instant::now() < cleanup_deadline,
                "{case}: resource cleanup incomplete"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(child.lock().unwrap().try_wait().unwrap().is_some());
        assert!(group.is_empty().unwrap());
        assert!(pool.reserve(total_memory).is_ok());
        let roster: serde_json::Value =
            serde_json::from_slice(&fs::read(root.path().join("worker-fence.json")).unwrap())
                .unwrap();
        assert!(roster["groups"].as_object().unwrap().is_empty());
        cases.push(serde_json::json!({"case":case,"settled_failure":outcome.is_ok(),"child_joined":true,"group_empty":true,"resources_readmitted":true,"staging_removed":true,"roster_empty":true}));
        root.close().unwrap();
    }
    let receipt = serde_json::json!({"status":"PASS","cases":cases,"authored_provider_observation":true,"actual_cuda_execution":false,"physical_device_contacted":false,"qualified":false});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(proof)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&receipt).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}
