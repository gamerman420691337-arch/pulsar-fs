//! Explicit native campaign using the shipped server, SessionClient and live bulk transport.
//! Authored gray RGB and synthetic ONNX prove custody/protocol execution, never useful model quality.
#![cfg(target_os = "linux")]
use anyhow::{bail, ensure, Context, Result};
use pulsar_clients::session::EngineApi;
use pulsar_clients::{ResponseBody as ClientBody, SessionClient};
use pulsar_engine::{
    cuda_devices::CudaDeviceExpectation,
    cuda_owner_policy::CudaOwnerPolicy,
    native_dependencies::{NativeDependencySpec, NativeLibrarySpec},
    resources::GpuDeviceBudget,
};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
fn identity(path: &Path) -> Result<ArtifactIdentity> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0; 1024 * 1024];
    let mut size = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        size += n as u64;
    }
    let sha256 = format!("{:x}", hash.finalize());
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())?,
        sha256,
        byte_len: size,
    })
}
fn id(label: &str) -> RequestId {
    RequestId::new(format!("native-rpc-{}-{label}", std::process::id())).unwrap()
}
struct Server {
    child: Child,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn call(client: &mut SessionClient, project: &ProjectId, command: Command) -> Result<ClientBody> {
    client.execute(command, Some(project.clone()), Some(RevisionId::new(0)))
}
fn prep(value: ClientBody) -> Result<NativeImagePreparationStatus> {
    let ClientBody::NativeImagePreparation(value) = value else {
        bail!("unexpected preparation response")
    };
    value.validate()?;
    Ok(*value)
}
fn capture(value: ClientBody) -> Result<LiveCaptureReport> {
    let ClientBody::LiveCapture(value) = value else {
        bail!("unexpected capture response")
    };
    value.validate()?;
    Ok(*value)
}
fn wait_prepared(
    client: &mut SessionClient,
    project: &ProjectId,
    operation: &AttemptId,
    deadline: Instant,
    output: &Path,
) -> Result<NativeImagePreparationStatus> {
    loop {
        let value = prep(call(
            client,
            project,
            Command::NativeImagePreparationStatus {
                operation_id: operation.clone(),
            },
        )?)?;
        fs::write(output, serde_json::to_vec_pretty(&value)?)?;
        if value.available {
            ensure!(
                value.report.state == NativeImagePreparationState::Retained
                    && !value.producer_active,
                "retention custody differs"
            );
            return Ok(value);
        }
        ensure!(
            !value.report.state.is_terminal(),
            "native preparation failed: {:?}: {:?}",
            value.report.state,
            value.report.error
        );
        ensure!(
            Instant::now() < deadline,
            "native preparation observation timed out"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[test]
#[ignore = "requires explicitly pinned native inputs, CUDA worker/model, reader and delegated cgroup; serial only"]
fn native_preparation_session_client_live_custody() -> Result<()> {
    let output = PathBuf::from(
        std::env::var_os("PULSAR_NATIVE_RPC_EVIDENCE")
            .context("pin fresh native RPC evidence path")?,
    );
    ensure!(!output.exists(), "native evidence must be fresh");
    fs::create_dir(&output)?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700))?;
    let private = output.join("private-runtime");
    fs::create_dir(&private)?;
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700))?;
    let mut spec: NativeDependencySpec = serde_json::from_slice(&fs::read(
        std::env::var_os("PULSAR_NATIVE_LIBRARY_SPEC").context("pin native spec")?,
    )?)?;
    let nvml = PathBuf::from(std::env::var_os("PULSAR_NVML_LIBRARY").context("pin NVML")?);
    let nvml_id = identity(&nvml)?;
    spec.libraries.push(NativeLibrarySpec {
        path: nvml,
        sha256: nvml_id.sha256,
        byte_len: nvml_id.byte_len,
        names: vec!["libnvidia-ml.so.1".into()],
    });
    spec.validate()?;
    let mut example: WorkerRequest = serde_json::from_str(include_str!(
        "../crates/pulsar-engine/tests/fixtures/cuda-intent-request.json"
    ))?;
    let execution = example
        .cuda_execution
        .take()
        .context("authored CUDA fixture execution")?;
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
    policy.validate()?;
    let runtime = policy
        .native_dependencies
        .libraries
        .iter()
        .find(|l| l.names.iter().any(|n| n == "libonnxruntime.so.1"))
        .context("runtime alias")?
        .path
        .clone();
    let policy_path = private.join("policy.json");
    fs::write(&policy_path, serde_json::to_vec(&policy)?)?;
    fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600))?;
    let state = private.join("state");
    let endpoint = state.join("engine.sock");
    let stderr = fs::File::create(output.join("engine.stderr"))?;
    let mut server = Server {
        child: ProcessCommand::new(env!("CARGO_BIN_EXE_pulsar"))
            .arg("engine")
            .env("PULSAR_STATE_DIR", &state)
            .env("PULSAR_CUDA_OWNER_POLICY", &policy_path)
            .env("PULSAR_ORT_DYLIB", &runtime)
            .env(
                "PULSAR_WORKER_EXECUTABLE",
                std::env::var_os("PULSAR_ENGINE_CUDA_WORKER").context("pin worker")?,
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()?,
    };
    let ready = Instant::now() + Duration::from_secs(120);
    while !endpoint.exists() {
        ensure!(
            server.child.try_wait()?.is_none(),
            "engine exited before readiness"
        );
        ensure!(Instant::now() < ready, "engine readiness timed out");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut client = SessionClient::connect(&endpoint, &state.join("pairing.token"))?;
    let ClientBody::Project(project) = client.execute(
        Command::CreateProject {
            name: "Authored native RPC custody".into(),
        },
        None,
        None,
    )?
    else {
        bail!("create project")
    };
    let project = project.project_id;
    let ClientBody::NativeImagePreparationLimits(limits) =
        call(&mut client, &project, Command::NativeImagePreparationLimits)?
    else {
        bail!("limits")
    };
    limits.validate()?;
    ensure!(limits.supported, "native preparation unavailable");
    fs::write(
        output.join("limits.json"),
        serde_json::to_vec_pretty(&limits)?,
    )?;
    let options = NativeImagePreparationOptions {
        preparation_ms: 180_000,
        retention_ms: 300_000,
    };
    let start = Command::StartNativeImagePreparation {
        engine_epoch: limits.engine_epoch.clone(),
        options,
    };
    let original = id("first-start");
    let deadline =
        Instant::now() + Duration::from_millis(options.preparation_ms) + Duration::from_secs(10);
    let ack = prep(client.execute_identified(
        original.clone(),
        start.clone(),
        Some(project.clone()),
        Some(RevisionId::new(0)),
    )?)?;
    let replay = prep(client.execute_identified(
        original.clone(),
        start.clone(),
        Some(project.clone()),
        Some(RevisionId::new(0)),
    )?)?;
    ensure!(ack == replay, "Start replay changed acknowledgement");
    let resolved = prep(call(
        &mut client,
        &project,
        Command::ResolveNativeImagePreparation {
            original_request_id: original,
        },
    )?)?;
    ensure!(
        resolved.report.intent.operation_id == ack.report.intent.operation_id,
        "lost-reply resolution differs"
    );
    fs::write(
        output.join("first-ack.json"),
        serde_json::to_vec_pretty(&ack)?,
    )?;
    fs::write(
        output.join("first-resolution.json"),
        serde_json::to_vec_pretty(&resolved)?,
    )?;
    let first = wait_prepared(
        &mut client,
        &project,
        &ack.report.intent.operation_id,
        deadline,
        &output.join("first-status.json"),
    )?;
    let second_ack = prep(client.execute_identified(
        id("second-start"),
        start,
        Some(project.clone()),
        Some(RevisionId::new(0)),
    )?)?;
    let second = wait_prepared(
        &mut client,
        &project,
        &second_ack.report.intent.operation_id,
        Instant::now() + Duration::from_secs(15),
        &output.join("second-status.json"),
    )?;
    ensure!(
        first.report.intent.image_sha256 == second.report.intent.image_sha256
            && first.report.intent.declared_bytes == second.report.intent.declared_bytes,
        "shared image identity differs"
    );
    let first_release = prep(call(
        &mut client,
        &project,
        Command::ReleaseNativeImagePreparation {
            operation_id: first.report.intent.operation_id.clone(),
        },
    )?)?;
    fs::write(
        output.join("first-release.json"),
        serde_json::to_vec_pretty(&first_release)?,
    )?;
    let until = Instant::now() + Duration::from_secs(5);
    let first_released = loop {
        let status = prep(call(
            &mut client,
            &project,
            Command::NativeImagePreparationStatus {
                operation_id: first.report.intent.operation_id.clone(),
            },
        )?)?;
        if status.report.state == NativeImagePreparationState::Released {
            break status;
        }
        ensure!(Instant::now() < until, "release transition timed out");
        std::thread::sleep(Duration::from_millis(25));
    };
    ensure!(
        !first_released.report.cleanup_confirmed,
        "other retained use must preserve storage custody"
    );
    fs::write(
        output.join("first-released-with-borrow.json"),
        serde_json::to_vec_pretty(&first_released)?,
    )?;
    let source_path = private.join("authored.rgbpair");
    let mut source_bytes = vec![0; 64 * 64 * 3];
    source_bytes.extend(vec![128; 64 * 64 * 3]);
    fs::write(&source_path, source_bytes)?;
    let ClientBody::Source { source_version: _ } = call(
        &mut client,
        &project,
        Command::ImportSource { path: source_path },
    )?
    else {
        bail!("import authored source")
    };
    let model = private.join("authored.onnx");
    fs::copy(
        std::env::var_os("PULSAR_AUTHORED_CUDA_MODEL").context("pin authored model")?,
        &model,
    )?;
    let WorkerOperation::Generate { settings, .. } = example.operation else {
        bail!("fixture generation")
    };
    let mut profile = settings.inference_profile.context("fixture profile")?;
    profile.model = identity(&model)?;
    profile.runtime = policy.runtime_identity()?;
    let stream: LiveInferenceManifest = serde_json::from_str(include_str!(
        "../crates/pulsar-protocol/tests/fixtures/live-cpu-manifest.json"
    ))?;
    profile.preprocessing = stream.profile.preprocessing;
    profile
        .cuda_resources
        .as_mut()
        .context("CUDA profile resources")?
        .device_reservation_bytes = 96 << 20;
    let options = LiveCaptureOptions {
        browser_analysis: None,
        browser_media: None,
        model_pack: None,
        source_time_basis: pulsar_protocol::LiveSourceTimeBasis::ProducerDeclared,
        continuation_mode: LiveContinuationMode::Immediate,
        encoded_source: None,
        inference: Some(LiveCaptureInference {
            model_path: model,
            profile,
        }),
        policy: LivePolicy {
            maximum_analysis_age_ns: 200_000_000,
            added_buffer_ns: 0,
        },
        source_anchor: SourceTimestamp::new(0, 1)?,
        project_anchor: ProjectTime::from_nanos(0),
        exact_project_anchor: None,
        presentation_anchor: LiveClockTick(0),
        lease_ms: 30_000,
    };
    let live_end = Instant::now() + Duration::from_secs(30);
    let initial = capture(call(
        &mut client,
        &project,
        Command::StartLiveCapture {
            options,
            engine_epoch: limits.engine_epoch.clone(),
        },
    )?)?;
    fs::write(
        output.join("live-initial.json"),
        serde_json::to_vec_pretty(&initial)?,
    )?;
    let epoch = initial.identity.epoch.clone();
    let running = loop {
        let value = capture(call(
            &mut client,
            &project,
            Command::LiveCaptureStatus {
                epoch: epoch.clone(),
            },
        )?)?;
        fs::write(
            output.join("live-status.json"),
            serde_json::to_vec_pretty(&value)?,
        )?;
        ensure!(value.active, "live capture retired: {:?}", value.inference);
        if value.inference.as_ref().context("live inference")?.state
            == LiveCaptureInferenceState::Running
        {
            break value;
        }
        ensure!(
            Instant::now() < live_end,
            "live readiness exceeded original deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut bulk = client.open_live_bulk(&running)?;
    let mut reports = Vec::new();
    for frame in 0..=3u64 {
        let current = capture(call(
            &mut client,
            &project,
            Command::LiveCaptureStatus {
                epoch: epoch.clone(),
            },
        )?)?;
        let bytes = vec![(frame * 64) as u8; 64 * 64 * 3];
        let header = LiveFrameHeader {
            frame: FrameId::new(frame),
            source_time: SourceTimestamp::new(frame as i64, 10)?,
            capture_tick: current.server_elapsed.context("capture elapsed")?,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        };
        bulk.upload(&header, &bytes)?;
        if frame == 0 {
            continue;
        }
        loop {
            let value = capture(call(
                &mut client,
                &project,
                Command::LiveCaptureStatus {
                    epoch: epoch.clone(),
                },
            )?)?;
            ensure!(value.active, "live capture retired while awaiting estimate");
            if value.inference.as_ref().context("inference")?.completed == frame {
                let latest = value
                    .inference
                    .as_ref()
                    .context("inference")?
                    .latest
                    .as_ref()
                    .context("GPU estimate")?;
                ensure!(
                    !latest.qualified
                        && matches!(&latest.result, LiveInferenceReply::CudaProvisional { .. }),
                    "estimate must retain provisional GPU evidence"
                );
                reports.push(value);
                break;
            }
            ensure!(
                Instant::now() < live_end,
                "inference exceeded original live deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    drop(bulk);
    fs::write(
        output.join("live-estimates.json"),
        serde_json::to_vec_pretty(&reports)?,
    )?;
    let requested = capture(call(
        &mut client,
        &project,
        Command::FinishLiveCapture {
            epoch: epoch.clone(),
        },
    )?)?;
    fs::write(
        output.join("live-finish-requested.json"),
        serde_json::to_vec_pretty(&requested)?,
    )?;
    let finished = loop {
        let value = capture(call(
            &mut client,
            &project,
            Command::LiveCaptureStatus {
                epoch: epoch.clone(),
            },
        )?)?;
        fs::write(
            output.join("live-finished.json"),
            serde_json::to_vec_pretty(&value)?,
        )?;
        if !value.active {
            ensure!(
                value.inference.as_ref().context("final inference")?.state
                    == LiveCaptureInferenceState::Retired,
                "live completion differs"
            );
            break value;
        }
        ensure!(
            Instant::now() < live_end,
            "Finish exceeded original live deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    ensure!(
        finished
            .inference
            .as_ref()
            .context("final inference")?
            .completed
            == 3,
        "three authored estimates required"
    );
    ensure!(
        finished.retirement == Some(LiveCaptureRetirement::Completed),
        "capture must finish successfully"
    );
    let evidence = finished
        .inference
        .as_ref()
        .context("inference")?
        .cuda_evidence
        .as_ref()
        .context("verified native receipt summary")?;
    ensure!(
        !evidence.qualified && evidence.summary.successful_runs == 3,
        "verified three-run native receipt required"
    );
    let second_release = prep(call(
        &mut client,
        &project,
        Command::ReleaseNativeImagePreparation {
            operation_id: second.report.intent.operation_id.clone(),
        },
    )?)?;
    fs::write(
        output.join("second-release.json"),
        serde_json::to_vec_pretty(&second_release)?,
    )?;
    let cleanup_end = Instant::now() + Duration::from_secs(15);
    for operation in [
        &first.report.intent.operation_id,
        &second.report.intent.operation_id,
    ] {
        loop {
            let status = prep(call(
                &mut client,
                &project,
                Command::NativeImagePreparationStatus {
                    operation_id: operation.clone(),
                },
            )?)?;
            fs::write(
                output.join(format!("cleanup-{}.json", operation)),
                serde_json::to_vec_pretty(&status)?,
            )?;
            if status.report.cleanup_confirmed {
                ensure!(
                    status.report.state == NativeImagePreparationState::Released
                        && !status.available
                        && !status.producer_active,
                    "cleanup receipt differs"
                );
                break;
            }
            ensure!(
                Instant::now() < cleanup_end,
                "native storage cleanup unconfirmed"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    fs::write(
        output.join("checked.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"authenticated_session_client":true,"shipped_engine":true,"frozen_start_replay":true,"original_request_resolution":true,"two_retained_uses":true,"release_preserves_other_use":true,"live_lease_ms":30000,"gpu_estimates":3,"finish_completed":true,"both_cleanup_confirmed":true,"authored_inputs":true,"qualified":false,"cold_post_finish_generation_test_replaced":false,"physical_dispatch":false}),
        )?,
    )?;
    // Runtime auth and SQLite stay private; the caller exports only explicit metadata/evidence files.
    Ok(())
}
