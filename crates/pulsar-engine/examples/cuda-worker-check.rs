//! Actual confined authored generation/egress; not engine GPU job admission.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use pulsar_engine::{
        artifacts,
        cuda_devices::{CudaDeviceExpectation, PinnedCudaDevices},
        cuda_mounts::CudaMountCommand,
        cuda_result,
        native_dependencies::CapturedNativeDependencies,
        worker, EngineConfig,
    };
    use pulsar_protocol::*;
    use std::{
        fs,
        io::Read,
        path::PathBuf,
        time::{Duration, Instant},
    };
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    ensure!(args.len()==5,"usage: cuda-worker-check CUDA-WORKER NATIVE-SPEC DEVICE-EXPECTATION AUTHORED-MODEL NEW-ROOT");
    let root = PathBuf::from(&args[4]);
    ensure!(
        root.is_absolute() && !root.exists(),
        "new absolute inspection root required"
    );
    let read = |path: &std::ffi::OsStr| -> anyhow::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        artifacts::open_regular(std::path::Path::new(path))?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 1024 * 1024, "inspection metadata capacity");
        Ok(bytes)
    };
    let spec: NativeDependencySpec = serde_json::from_slice(&read(&args[1])?)?;
    let device: CudaDeviceExpectation = serde_json::from_slice(&read(&args[2])?)?;
    artifacts::private_directory(&root)?;
    let model_path = root.join("authored.onnx");
    fs::write(&model_path, read(&args[3])?)?;
    let pair_path = root.join("authored.rgbpair");
    let mut pixels = vec![0; 64 * 64 * 3];
    pixels.extend(vec![128; 64 * 64 * 3]);
    fs::write(&pair_path, pixels)?;
    // Parent capture is intentionally a research helper, not aggregate admission.
    let captured = CapturedNativeDependencies::capture(&spec, &root.join("capture"))?;
    let devices = PinnedCudaDevices::open(&device)?;
    let config = EngineConfig::new(root.join("unused-state"), PathBuf::from(&args[0]));
    let mut request: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))?;
    request.budget.memory_bytes = 2 * 1024 * 1024 * 1024;
    request.cuda_execution.as_mut().unwrap().library_directory = captured.library_directory();
    let source = worker::pin_artifact(&pair_path)?;
    request.source.path = source.path;
    request.source.identity = source.identity;
    let model = worker::pin_artifact(&model_path)?;
    let mut tools = worker::discover_tools()?;
    tools.onnx_runtime = Some(ArtifactReference {
        path: captured.library_directory().join("libonnxruntime.so.1"),
        identity: request
            .tools
            .onnx_runtime
            .as_ref()
            .unwrap()
            .identity
            .clone(),
    });
    request.tools = tools;
    if let WorkerOperation::Generate {
        settings,
        model: reference,
        ..
    } = &mut request.operation
    {
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.model = model.identity.clone();
        *reference = Some(SourceArtifact {
            source_version: SourceVersionId::new("authored-model")?,
            path: model.path,
            identity: model.identity,
        });
        settings.raw_frame_pair = Some(RawFramePair {
            source_epoch: AttemptId::new("authored-rgb-epoch")?,
            previous_frame: FrameId::new(0),
            current_frame: FrameId::new(1),
            previous_time: SourceTimestamp::new(0, 1)?,
            current_time: SourceTimestamp::new(1, 10)?,
            presentation_clock: None,
        });
    }
    request.dependencies = vec![
        request.source.identity.clone(),
        request.tools.ffmpeg.identity.clone(),
        request.tools.ffprobe.identity.clone(),
        request
            .tools
            .onnx_runtime
            .as_ref()
            .unwrap()
            .identity
            .clone(),
        request
            .cuda_execution
            .as_ref()
            .unwrap()
            .owner_policy
            .clone(),
    ];
    if let WorkerOperation::Generate {
        settings, model, ..
    } = &request.operation
    {
        request
            .dependencies
            .push(model.as_ref().unwrap().identity.clone());
        request
            .dependencies
            .push(pulsar_engine::inference_profiles::profile_identity(
                settings.inference_profile.as_ref().unwrap(),
            )?);
    }
    for lib in &request.cuda_execution.as_ref().unwrap().libraries {
        if !request.dependencies.contains(&lib.identity) {
            request.dependencies.push(lib.identity.clone());
        }
    }
    let mut cases = Vec::new();
    for case in [
        "baseline",
        "wrong-request",
        "wrong-alias-inode",
        "wrong-device-inode",
    ] {
        request.attempt_id = AttemptId::new(uuid::Uuid::new_v4().to_string())?;
        request.job_id = JobId::new(uuid::Uuid::new_v4().to_string())?;
        request.output_dir = root.join(case);
        artifacts::private_directory(&request.output_dir)?;
        request.validate()?;
        let mut plan = CudaMountCommand::worker(&config, &request, &captured, &devices)?;
        let mut witness = serde_json::to_value(plan.witness())?;
        match case {
            "wrong-request" => witness["request_sha256"] = serde_json::json!("0".repeat(64)),
            "wrong-alias-inode" => {
                witness["aliases"][0]["inode"] =
                    serde_json::json!(witness["aliases"][0]["inode"].as_u64().unwrap() + 1)
            }
            "wrong-device-inode" => {
                witness["devices"][0]["inode"] =
                    serde_json::json!(witness["devices"][0]["inode"].as_u64().unwrap() + 1)
            }
            _ => {}
        }
        fs::write(
            root.join(format!("{case}-request.json")),
            serde_json::to_vec_pretty(&request)?,
        )?;
        let mut child = plan.command_mut().spawn()?;
        let mut stdin = child.stdin.take().context("worker stdin missing")?;
        let mut stdout = child.stdout.take().context("worker stdout missing")?;
        let stderr = child.stderr.take().context("worker stderr missing")?;
        let writer = std::thread::spawn(move || write_message(&mut stdin, &witness));
        let expected = request.clone();
        let reader = std::thread::spawn(move || {
            cuda_result::receive_worker_evidence(&mut stdout, &expected)
        });
        let diagnostics = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            stderr.take(16385).read_to_end(&mut bytes)?;
            Ok(bytes)
        });
        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if start.elapsed() >= Duration::from_millis(request.budget.wall_time_ms) {
                timed_out = true;
                plan.kill_tree();
                let _ = child.kill();
            }
            if let Some(status) = child.try_wait()? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        plan.kill_tree();
        let retirement = Instant::now();
        while !plan.tree_is_empty()? {
            plan.kill_tree();
            ensure!(
                retirement.elapsed() < Duration::from_secs(10),
                "worker child tree retirement unconfirmed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        // Egress joins can still block on host filesystem IO. This research
        // helper does not establish a production independent reader deadline.
        let write = writer
            .join()
            .map_err(|_| anyhow::anyhow!("worker writer panicked"))?;
        let response = reader
            .join()
            .map_err(|_| anyhow::anyhow!("worker reader panicked"))?;
        let errors = diagnostics
            .join()
            .map_err(|_| anyhow::anyhow!("worker diagnostics panicked"))??;
        ensure!(
            !timed_out && errors.len() <= 16384,
            "worker deadline/output limit exceeded"
        );
        let positive = case == "baseline";
        if positive {
            write?;
            ensure!(
                status.success(),
                "CUDA worker exited {status}: {}",
                String::from_utf8_lossy(&errors)
            );
            let result = response.map_err(|error| anyhow::anyhow!("parent egress: {error}"))?;
            let output = result
                .result
                .map_err(|error| anyhow::anyhow!("generation: {error}"))?;
            let evidence = result
                .cuda_artifacts
                .context("dedicated CUDA evidence missing")?;
            cuda_result::verify_files(&request, &evidence)?;
            let (program, receipt) = match output {
                WorkerOutput::Generated {
                    program, receipt, ..
                } => (program, receipt),
                _ => anyhow::bail!("unexpected preview"),
            };
            let motion: MotionProgram = serde_json::from_slice(&fs::read(&program.path)?)?;
            let actions = &motion.tracks()[0].actions();
            ensure!(actions.len() == 2, "authored pair action count");
            ensure!(
                (actions[1].position().value() - (0.25 + 0.25 * 128.0 / 255.0)).abs() < 1e-5,
                "authored CUDA position mismatch"
            );
            cases.push(serde_json::json!({"case":case,"exit_code":status.code(),"tree_empty":true,"elapsed_ms":start.elapsed().as_millis(),
                "program":program,"receipt":receipt,"cuda_artifacts":evidence,"position":actions[1].position().value(),"stderr":String::from_utf8_lossy(&errors)}));
        } else {
            ensure!(
                !status.success() && response.is_err(),
                "invalid bootstrap unexpectedly succeeded"
            );
            ensure!(
                fs::read_dir(&request.output_dir)?.next().is_none(),
                "refused bootstrap published artifacts"
            );
            cases.push(serde_json::json!({"case":case,"exit_code":status.code(),"tree_empty":true,"elapsed_ms":start.elapsed().as_millis(),"stderr":String::from_utf8_lossy(&errors),"published_artifacts":false}));
        }
        drop(plan);
    }
    captured.close()?;
    let report = serde_json::json!({"state":"passed","scope":"actual authored CUDA worker generation and shared parent egress only", "native_execution":true,
        "production_gpu_admission":false,"capture_removed":true,"parent_capture_not_resource_qualified":true,"cases":cases});
    fs::write(
        root.join("checked.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("CUDA worker inspection requires Linux")
}
