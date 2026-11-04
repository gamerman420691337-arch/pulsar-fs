//! Explicit mount inspection only; no inference or production job admission.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use pulsar_engine::{
        artifacts,
        cuda_devices::{CudaDeviceExpectation, PinnedCudaDevices},
        cuda_mounts::CudaMountCommand,
        native_dependencies::CapturedNativeDependencies,
        EngineConfig,
    };
    use pulsar_protocol::*;
    use std::{
        fs,
        io::Read,
        path::PathBuf,
        time::{Duration, Instant},
    };
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 4,
        "usage: cuda-mount-check WORKER NATIVE-SPEC DEVICE-EXPECTATION NEW-ROOT"
    );
    let root = PathBuf::from(&args[3]);
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
    // This research helper performs capture itself. Production must instead use
    // supervised capture under aggregate admission; no qualification is implied.
    let captured = CapturedNativeDependencies::capture(&spec, &root.join("capture"))?;
    let devices = PinnedCudaDevices::open(&device)?;
    let config = EngineConfig::new(root.join("unused-state"), PathBuf::from(&args[0]));
    let mut request: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))?;
    request.cuda_execution.as_mut().unwrap().library_directory = captured.library_directory();
    request.tools.onnx_runtime.as_mut().unwrap().path =
        captured.library_directory().join("libonnxruntime.so.1");
    request.output_dir = root.join("guest-output");
    let mut results = Vec::new();
    for case in [
        "baseline",
        "replacement",
        "wrong-alias-inode",
        "wrong-device-inode",
        "corrupt-backing",
    ] {
        request.attempt_id = AttemptId::new(uuid::Uuid::new_v4().to_string())?;
        let mut plan = CudaMountCommand::inspection(&config, &request, &captured, &devices)?;
        let mut witness = serde_json::to_value(plan.witness())?;
        let alias_path = captured.library_directory().join(
            witness["aliases"][0]["name"]
                .as_str()
                .context("alias name missing")?,
        );
        let backup = root.join("old-alias");
        match case {
            "replacement" => {
                fs::rename(&alias_path, &backup)?;
                fs::write(&alias_path, b"authored replacement must not be mounted")?;
            }
            "wrong-alias-inode" => {
                witness["aliases"][0]["inode"] =
                    serde_json::json!(witness["aliases"][0]["inode"].as_u64().unwrap() + 1);
            }
            "wrong-device-inode" => {
                witness["devices"][0]["inode"] =
                    serde_json::json!(witness["devices"][0]["inode"].as_u64().unwrap() + 1);
            }
            "corrupt-backing" => {
                use std::{
                    io::{Read, Seek, SeekFrom, Write},
                    os::unix::fs::PermissionsExt,
                };
                fs::set_permissions(&alias_path, fs::Permissions::from_mode(0o600))?;
                let mut file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&alias_path)?;
                let mut byte = [0];
                file.read_exact(&mut byte)?;
                file.seek(SeekFrom::Start(0))?;
                byte[0] ^= 1;
                file.write_all(&byte)?;
                file.sync_all()?;
                fs::set_permissions(&alias_path, fs::Permissions::from_mode(0o400))?;
            }
            _ => {}
        }
        let mut child = plan.command_mut().spawn()?;
        let mut stdin = child.stdin.take().context("inspection stdin missing")?;
        let mut stdout = child.stdout.take().context("inspection stdout missing")?;
        let stderr = child.stderr.take().context("inspection stderr missing")?;
        let writer = std::thread::spawn(move || write_message(&mut stdin, &witness));
        let reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            stdout.by_ref().take(65537).read_to_end(&mut bytes)?;
            Ok(bytes)
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
        let retired = Instant::now();
        while !plan.tree_is_empty()? {
            plan.kill_tree();
            ensure!(
                retired.elapsed() < Duration::from_secs(10),
                "inspection child tree retirement unconfirmed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("inspection writer panicked"))??;
        let output = reader
            .join()
            .map_err(|_| anyhow::anyhow!("inspection reader panicked"))??;
        let errors = diagnostics
            .join()
            .map_err(|_| anyhow::anyhow!("inspection diagnostics panicked"))??;
        ensure!(!timed_out, "inspection deadline exceeded");
        ensure!(
            output.len() <= 65536 && errors.len() <= 16384,
            "inspection output capacity"
        );
        let positive = matches!(case, "baseline" | "replacement");
        ensure!(
            status.success() == positive,
            "unexpected {case} outcome: {}",
            String::from_utf8_lossy(&errors)
        );
        let checked = if positive {
            Some(serde_json::from_slice::<serde_json::Value>(&output)?)
        } else {
            None
        };
        if let Some(checked) = &checked {
            ensure!(
                checked["native_execution"] == false && checked["gpu_admission"] == false,
                "incorrect inspection scope"
            );
        }
        results.push(serde_json::json!({"case":case,"exit_code":status.code(),"tree_empty":true,"elapsed_ms":start.elapsed().as_millis(),
            "checked":checked,"stderr":String::from_utf8_lossy(&errors)}));
        drop(plan);
        if case == "replacement" {
            fs::remove_file(&alias_path)?;
            fs::rename(&backup, &alias_path)?;
            captured.verify()?;
        }
    }
    captured.close()?;
    let report = serde_json::json!({"state":"passed","scope":"actual confined mount and hash inspection only","native_execution":false,
        "production_gpu_admission":false,"capture_removed":true,"parent_capture_not_resource_qualified":true,"cases":results});
    fs::write(
        root.join("checked.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("CUDA mount inspection requires Linux")
}
