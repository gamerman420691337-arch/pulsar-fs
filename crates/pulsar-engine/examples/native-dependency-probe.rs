//! Experimental authored CUDA probe using the production byte-capture helper.
//! No engine admission, private models, GPU reservation or platform qualification.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use pulsar_engine::{
        artifacts,
        native_dependencies::{CapturedNativeDependencies, NativeDependencySpec},
    };
    use std::{
        fs::{self, OpenOptions},
        io::Read,
        os::unix::process::CommandExt,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "device-gate") {
        ensure!((2..=4).contains(&args.len()), "device gate arguments");
        let mut input = artifacts::open_regular(&PathBuf::from(&args[1]))?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut input)
            .take(8193)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 8192, "device identity table too large");
        let nodes: Vec<pulsar_engine::cuda_devices::DeviceNodeIdentity> =
            serde_json::from_slice(&bytes)?;
        pulsar_engine::cuda_devices::verify_mounted_nodes(&nodes)?;
        eprintln!("DEVICE_BINDING_VERIFIED={}", nodes.len());
        let mut command = Command::new("/runtime/ld-linux-x86-64.so.2");
        command
            .env_clear()
            .env("LD_PRELOAD", "/runtime/libcudnn.so.9");
        if args.len() >= 3 {
            let mut input = artifacts::open_regular(&PathBuf::from(&args[2]))?;
            let mut bytes = Vec::new();
            Read::by_ref(&mut input)
                .take(8193)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 8192, "SDK expectation too large");
            let sdk: pulsar_engine::worker::cuda_identity::CudaSdkExpectation =
                serde_json::from_slice(&bytes)?;
            sdk.validate()?;
            command.env("PULSAR_CUDA_SDK_EXPECTATION", serde_json::to_string(&sdk)?);
        }
        if args.len() == 4 {
            ensure!(
                [
                    "invalid-input",
                    "wrong-run-count",
                    "preinitialized-runtime",
                    "invalid-output",
                    "invalid-detector-output",
                    "detector"
                ]
                .iter()
                .any(|v| args[3] == *v),
                "invalid research negative case"
            );
            command.env("PULSAR_RESEARCH_CUDA_CASE", &args[3]);
        }
        let error = command
            .args([
                "--inhibit-cache",
                "--library-path",
                "/runtime",
                "/probe",
                "cuda",
                "/model",
                "/output/trace",
            ])
            .exec();
        return Err(error.into());
    }
    ensure!((6..=8).contains(&args.len()), "usage: native-dependency-probe SPEC PRIVATE-PARENT PROBE-BINARY AUTHORED-MODEL NEW-OUTPUT DEVICE-EXPECTATION [SDK-EXPECTATION [NEGATIVE-CASE]]");
    if args.len() == 8 {
        ensure!(
            [
                "invalid-input",
                "wrong-run-count",
                "preinitialized-runtime",
                "invalid-output",
                "invalid-detector-output",
                "detector"
            ]
            .iter()
            .any(|v| args[7] == *v),
            "invalid research negative case"
        );
    }
    let mut device_input = artifacts::open_regular(&PathBuf::from(&args[5]))?;
    let mut device_bytes = Vec::new();
    Read::by_ref(&mut device_input)
        .take(8193)
        .read_to_end(&mut device_bytes)?;
    ensure!(device_bytes.len() <= 8192, "device expectation too large");
    let expectation: pulsar_engine::cuda_devices::CudaDeviceExpectation =
        serde_json::from_slice(&device_bytes)?;
    let sdk = if args.len() >= 7 {
        let mut input = artifacts::open_regular(&PathBuf::from(&args[6]))?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut input)
            .take(8193)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 8192, "SDK expectation too large");
        let sdk: pulsar_engine::worker::cuda_identity::CudaSdkExpectation =
            serde_json::from_slice(&bytes)?;
        sdk.validate()?;
        ensure!(
            sdk.device_uuid
                .eq_ignore_ascii_case(&expectation.device_uuid)
                && sdk
                    .pci_bus_address
                    .eq_ignore_ascii_case(&expectation.pci_bus_address),
            "SDK expectation differs from held kernel device"
        );
        Some(sdk)
    } else {
        None
    };
    let devices = pulsar_engine::cuda_devices::PinnedCudaDevices::open(&expectation)?;
    let mut input = artifacts::open_regular(&PathBuf::from(&args[0]))?;
    ensure!(
        input.metadata()?.len() <= 1024 * 1024,
        "dependency specification too large"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut input)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "dependency specification grew");
    let spec: NativeDependencySpec = serde_json::from_slice(&bytes)?;
    let captured = CapturedNativeDependencies::capture(&spec, &PathBuf::from(&args[1]))?;
    let libraries = captured.library_directory();
    for name in [
        "ld-linux-x86-64.so.2",
        "libonnxruntime.so.1",
        "libonnxruntime_providers_cuda.so",
        "libcudnn.so.9",
    ] {
        ensure!(
            libraries.join(name).is_file(),
            "required probe library alias missing: {name}"
        );
    }
    // The byte-capture helper grants no executable permission. This research
    // caller explicitly enables its captured loader, never a host original.
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        libraries.join("ld-linux-x86-64.so.2"),
        fs::Permissions::from_mode(0o500),
    )?;
    let binary = PathBuf::from(&args[2]).canonicalize()?;
    let model = PathBuf::from(&args[3]).canonicalize()?;
    let output = PathBuf::from(&args[4]);
    fs::create_dir(&output)?;
    // Parent-provided probe/model remain outside this library-only capture scope.
    let output = output.canonicalize()?;
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("stdout.json"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("stderr.log"))?;
    captured.verify()?;
    let mut node_table = tempfile::NamedTempFile::new()?;
    use std::io::Write;
    node_table.write_all(&serde_json::to_vec(&devices.identities())?)?;
    node_table.as_file().sync_all()?;
    let mut sdk_table = tempfile::NamedTempFile::new()?;
    if let Some(sdk) = &sdk {
        sdk_table.write_all(&serde_json::to_vec(sdk)?)?;
        sdk_table.as_file().sync_all()?;
    }
    let mut command = Command::new("/usr/bin/bwrap");
    command
        .env_clear()
        .args([
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--clearenv",
            "--cap-drop",
            "ALL",
        ])
        .arg("--ro-bind")
        .arg(&libraries)
        .arg("/runtime")
        .arg("--ro-bind")
        .arg(PathBuf::from(&args[0]).canonicalize()?)
        .arg("/expected-native-image.json")
        .arg("--ro-bind")
        .arg(std::env::current_exe()?)
        .arg("/device-gate")
        .arg("--ro-bind")
        .arg(node_table.path())
        .arg("/expected-nodes.json")
        .arg("--ro-bind")
        .arg(&binary)
        .arg("/probe")
        .arg("--ro-bind")
        .arg(&model)
        .arg("/model")
        .arg("--bind")
        .arg(&output)
        .arg("/output")
        .args([
            "--proc", "/proc", "--dev", "/dev", "--size", "16777216", "--tmpfs", "/tmp",
        ]);
    if sdk.is_some() {
        command
            .arg("--ro-bind")
            .arg(sdk_table.path())
            .arg("/sdk-expectation.json");
    }
    devices.append_bindings(&mut command)?;
    for path in [
        "/sys/module/nvidia/initstate",
        "/sys/module/nvidia_uvm/initstate",
        "/sys/devices/system/memory/block_size_bytes",
        "/sys/devices/system/node",
        "/sys/devices/system/cpu",
    ] {
        command.args(["--ro-bind", path, path]);
    }
    command
        .args([
            "--chdir",
            "/output",
            "/runtime/ld-linux-x86-64.so.2",
            "--inhibit-cache",
            "--library-path",
            "/runtime",
            "/device-gate",
            "device-gate",
            "/expected-nodes.json",
        ])
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    if sdk.is_some() {
        command.arg("/sdk-expectation.json");
    }
    if args.len() == 8 {
        command.arg(&args[7]);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for (resource, value) in [
                (libc::RLIMIT_AS, 16u64 << 30),
                (libc::RLIMIT_FSIZE, 8u64 << 20),
                (libc::RLIMIT_CPU, 30),
            ] {
                let limits = libc::rlimit {
                    rlim_cur: value as libc::rlim_t,
                    rlim_max: value as libc::rlim_t,
                };
                if libc::setrlimit(resource, &limits) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("experimental CUDA probe timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    captured.verify()?;
    let mut receipt = serde_json::json!({"schema_version":1,"scope":"experimental authored graph, captured declared library set",
        "production_cuda_admitted":false,"dependency_closure_qualified":false,"whole_worker_vram_quota":false,
        "address_space_bytes":16u64 << 30,"physical_ram_cgroup":false,"exit_code":status.code(),
        "captured_specification":captured.specification(),"kernel_device_expectation":devices.expectation(),"held_device_nodes":devices.identities(),"host_runtime_directories_mounted":false,
        "captured_bytes_verified_after_execution":true,"capture_cleanup":"pending"});
    fs::write(
        output.join("capture.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    ensure!(
        status.success(),
        "experimental CUDA probe failed; retained output: {}",
        output.display()
    );
    let observed: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("stdout.json"))?)?;
    ensure!(observed["runs"] == 4, "authored CUDA run count differs");
    if observed["decoded_rgb_adapter"] == true {
        let values = observed["outputs"]
            .as_array()
            .context("decoded RGB outputs missing")?;
        ensure!(
            values.len() == 4
                && values
                    .iter()
                    .zip(if observed["decoded_detector_adapter"] == true {
                        [0.6, 0.625, 0.65, 0.7]
                    } else {
                        [0.25, 0.375, 0.5, 0.75]
                    })
                    .all(|(v, e)| v
                        .as_f64()
                        .is_some_and(|v| v.is_finite() && (v - e).abs() <= 1e-5)),
            "decoded RGB CUDA oracle differs"
        );
    } else {
        ensure!(
            observed["outputs"] == serde_json::json!([0.25, 0.375, 0.5, 0.75]),
            "authored CUDA oracle differs"
        );
    }
    if let Some(sdk) = &sdk {
        ensure!(
            observed["rust_session"]["device"]["device_uuid"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(&sdk.device_uuid))
                && observed["rust_session"]["device"]["pci_bus_address"].as_str()
                    == Some(sdk.pci_bus_address.as_str())
                && observed["rust_session"]["device"]["device_ordinal"].as_u64()
                    == Some(u64::from(sdk.device_ordinal))
                && observed["rust_session"]["successful_runs"] == 4,
            "missing Rust session SDK identity or runs"
        );
    }
    let stderr = fs::read_to_string(output.join("stderr.log"))?;
    ensure!(
        stderr
            .lines()
            .filter(|line| *line == "DEVICE_BINDING_VERIFIED=3")
            .count()
            == 1,
        "missing pre-library mounted device gate"
    );
    let profile = stderr
        .lines()
        .find_map(|line| line.strip_prefix("PROFILE_PATH="))
        .context("missing native trace")?;
    let name = profile
        .strip_prefix("/output/")
        .context("unexpected native trace directory")?;
    ensure!(
        !name.contains('/') && !name.is_empty(),
        "unexpected native trace name"
    );
    let trace = pulsar_engine::inference_trace::validate_node_trace(
        &fs::read(output.join(name))?,
        pulsar_protocol::InferenceProvider::Cuda,
        4,
    )?;
    ensure!(
        trace.computation_events == 12,
        "unexpected authored CUDA computation count"
    );
    let mapped: Vec<_> = stderr
        .lines()
        .filter_map(|line| line.strip_prefix("LIBRARY_PATH="))
        .collect();
    ensure!(!mapped.is_empty(), "missing mapped native libraries");
    for path in &mapped {
        let name = path
            .strip_prefix("/runtime/")
            .context("native library mapped outside captured set")?;
        ensure!(
            spec.libraries
                .iter()
                .any(|library| library.names.iter().any(|alias| alias == name)),
            "undeclared native library mapping"
        );
    }
    captured.close()?;
    receipt["capture_cleanup"] = serde_json::json!("closed_successfully");
    fs::write(
        output.join("capture.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    let result = serde_json::json!({"capture":receipt,"oracle_passed":true,"strict_trace":trace,"mapped_libraries":mapped,"rust_session":observed.get("rust_session")});
    fs::write(
        output.join("checked.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!(
        "{}",
        serde_json::to_string(&result).context("capture receipt")?
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("experimental CUDA dependency probe requires Linux");
}
