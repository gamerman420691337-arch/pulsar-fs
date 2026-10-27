//! Actual RGB preprocessing and shared worker decoders; no production admission.
#[cfg(all(target_os = "linux", feature = "cuda"))]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use pulsar_engine::worker::{
        cuda_identity::CudaSdkExpectation, detector::Detector, temporal_position::TemporalPosition,
    };
    use pulsar_protocol::*;
    use std::{collections::BTreeSet, io::Read, path::PathBuf};
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 3 && args[0] == "cuda",
        "usage: rust-cuda-adapter-probe cuda MODEL PROFILE-PREFIX"
    );
    let case = std::env::var("PULSAR_RESEARCH_CUDA_CASE").ok();
    ensure!(
        case.as_deref().is_none_or(|c| [
            "invalid-input",
            "invalid-output",
            "invalid-detector-output",
            "detector",
            "preinitialized-runtime"
        ]
        .contains(&c)),
        "invalid adapter research case"
    );
    if case.as_deref() == Some("preinitialized-runtime") {
        ensure!(
            ort::init_from("/runtime/libonnxruntime.so.1")
                .map_err(|e| anyhow::anyhow!("initial runtime: {e}"))?
                .commit(),
            "initial runtime commit failed"
        );
    }
    let expected: CudaSdkExpectation =
        serde_json::from_str(&std::env::var("PULSAR_CUDA_SDK_EXPECTATION")?)?;
    let detector = case.as_deref().is_some_and(|c| c.contains("detector"));
    let contract = ModelInputContract {
        width: 64,
        height: 64,
        channels: if detector { 3 } else { 6 },
        layout: TensorLayout::Nchw,
        class_count: 1,
        decoder: if detector {
            DetectorDecoder::YoloChannelMajor
        } else {
            DetectorDecoder::TemporalScriptPosition
        },
    };
    use pulsar_engine::{
        artifacts,
        cuda_devices::CudaDeviceExpectation,
        cuda_owner_policy::{CudaOwnerPolicy, LoadedCudaOwnerPolicy},
        resources::{GpuDeviceBudget, GpuPriority},
    };
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let image: NativeDependencySpec =
        serde_json::from_slice(&std::fs::read("/expected-native-image.json")?)?;
    let policy = CudaOwnerPolicy {
        schema_version: 1,
        device: CudaDeviceExpectation {
            device_uuid: expected.device_uuid.clone(),
            pci_bus_address: expected.pci_bus_address.clone(),
            device_minor: 0,
        },
        accounting: GpuDeviceBudget {
            device_uuid: expected.device_uuid.clone(),
            device_ordinal: u32::from(expected.device_ordinal),
            maximum_reserved_bytes: 2 << 30,
            interactive_reserved_bytes: 1 << 30,
            control_reserve_bytes: 256 << 20,
        },
        native_dependencies: image.clone(),
    };
    let owner_path = std::path::Path::new("/output/authored-owner.json");
    std::fs::write(owner_path, serde_json::to_vec(&policy)?)?;
    std::fs::set_permissions(owner_path, std::fs::Permissions::from_mode(0o600))?;
    let loaded = LoadedCudaOwnerPolicy::load(owner_path)?;
    let model = PathBuf::from(&args[1]);
    let (sha256, byte_len) = artifacts::digest_file(&model, 1024 * 1024)?;
    let model_identity = ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())?,
        sha256,
        byte_len,
    };
    let runtime_identity = policy.runtime_identity()?;
    let profile = DeclaredInferenceProfile {
        schema_version: 2,
        model: model_identity.clone(),
        runtime: runtime_identity.clone(),
        preprocessing: InferencePreprocessing {
            input: contract.clone(),
            resize: if detector {
                InferenceResize::Stretch
            } else {
                InferenceResize::NearestStretch
            },
            color: if detector {
                InferenceColor::Rgb
            } else {
                InferenceColor::RgbFramePair
            },
            scale: vec![1.0 / 255.0; contract.channels as usize],
            bias: vec![0.0; contract.channels as usize],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cuda,
        minimum_memory_bytes: 512 << 20,
        minimum_output_bytes: 64 << 20,
        cpu_threads: 2,
        cuda_resources: Some(CudaResourceLimits {
            device_uuid: expected.device_uuid.clone(),
            device_ordinal: u32::from(expected.device_ordinal),
            address_space_bytes: 16 << 30,
            device_reservation_bytes: 64 << 20,
            arena_limit_bytes: 64 << 20,
        }),
    };
    let budget = ResourceBudget {
        memory_bytes: 512 << 20,
        output_bytes: 64 << 20,
        wall_time_ms: 30000,
        cpu_threads: 2,
    };
    policy.authorize_profile(&profile, &budget, GpuPriority::Offline)?;
    let libraries = image
        .libraries
        .iter()
        .map(|l| {
            Ok(CapturedNativeLibrary {
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new(l.sha256.clone())?,
                    sha256: l.sha256.clone(),
                    byte_len: l.byte_len,
                },
                names: l.names.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut dependencies = vec![
        loaded.status().source_identity.clone(),
        model_identity.clone(),
    ];
    dependencies.extend(libraries.iter().map(|l| l.identity.clone()));
    let source = SourceArtifact {
        source_version: SourceVersionId::new("authored-model")?,
        path: model,
        identity: model_identity,
    };
    let runtime = ArtifactReference {
        identity: runtime_identity,
        path: "/runtime/libonnxruntime.so.1".into(),
    };
    let request = WorkerRequest {
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new("authored-adapter-attempt")?,
        job_id: JobId::new("authored-adapter-job")?,
        project_id: ProjectId::new("authored-adapter-project")?,
        base_revision: RevisionId::new(0),
        source: source.clone(),
        independent_audio: None,
        output_dir: "/output".into(),
        cuda_execution: Some(WorkerCudaExecution {
            schema_version: 1,
            owner_policy: loaded.status().source_identity,
            device_uuid: expected.device_uuid,
            pci_bus_address: expected.pci_bus_address,
            device_minor: 0,
            device_ordinal: u32::from(expected.device_ordinal),
            library_directory: "/runtime".into(),
            libraries,
        }),
        operation: WorkerOperation::Generate {
            preset: "fast".into(),
            model: Some(source),
            settings: (GenerationSettings {
                analysis_width: 64,
                analysis_height: 64,
                model_input: Some(contract),
                inference_profile: Some(profile),
                ..Default::default()
            })
            .into(),
        },
        budget,
        dependencies,
        tools: WorkerTools {
            ffmpeg: runtime.clone(),
            ffprobe: runtime.clone(),
            onnx_runtime: Some(runtime),
        },
    };
    request.validate()?;
    std::fs::write(
        "/output/worker-request.json",
        serde_json::to_vec_pretty(&request)?,
    )?;
    let expected_request = format!("{:x}", Sha256::digest(serde_json::to_vec(&request)?));
    let mut temporal = if detector {
        None
    } else {
        Some(TemporalPosition::from_request(&request)?)
    };
    let mut boxes = if detector {
        Some(Detector::from_request(&request)?)
    } else {
        None
    };
    let frame = |index: usize| -> Vec<u8> {
        (0..64 * 64)
            .flat_map(|p| {
                let byte = match index {
                    0 => 0,
                    1 => {
                        if p % 4 == 0 {
                            63
                        } else {
                            64
                        }
                    }
                    2 => {
                        if p % 2 == 0 {
                            127
                        } else {
                            128
                        }
                    }
                    _ => 255,
                };
                [byte; 3]
            })
            .collect()
    };
    if case.as_deref().is_some_and(|c| c.starts_with("invalid-")) {
        let pixels = frame(0);
        let error = if let Some(estimator) = temporal.as_mut() {
            estimator
                .estimate(
                    if case.as_deref() == Some("invalid-input") {
                        &pixels[..1]
                    } else {
                        &pixels
                    },
                    &pixels,
                )
                .err()
        } else {
            boxes.as_mut().unwrap().detect(&pixels, 64, 64).err()
        }
        .context("invalid adapter data unexpectedly succeeded")?;
        let reuse = if let Some(estimator) = temporal.as_mut() {
            estimator.estimate(&pixels, &pixels).err()
        } else {
            boxes.as_mut().unwrap().detect(&pixels, 64, 64).err()
        }
        .context("failed adapter was reused")?;
        ensure!(
            reuse.to_string().contains("session is failed"),
            "unexpected adapter reuse refusal"
        );
        let finish = if let Some(estimator) = temporal {
            estimator.finish().err()
        } else {
            boxes.unwrap().finish().err()
        }
        .context("failed adapter produced receipt")?;
        ensure!(
            finish.to_string().contains("receipt refused"),
            "unexpected adapter receipt refusal"
        );
        eprintln!("CUDA_ADAPTER_REUSE_AND_RECEIPT_REFUSED; initial error: {error}");
        anyhow::bail!("intentional adapter refusal");
    }
    let mut values = Vec::new();
    for (index, level) in [0.0, 0.25, 0.5, 1.0].into_iter().enumerate() {
        let pixels = frame(index);
        let value = if let Some(estimator) = temporal.as_mut() {
            estimator.estimate(&pixels, &pixels)?.value()
        } else {
            let detections = boxes.as_mut().unwrap().detect(&pixels, 64, 64)?;
            ensure!(
                detections.len() == 1
                    && detections[0].class_id == 0
                    && detections[0].bbox == [22.0 / 64.0, 22.0 / 64.0, 42.0 / 64.0, 42.0 / 64.0],
                "authored detector geometry differs"
            );
            f64::from(detections[0].score)
        };
        let oracle = if detector {
            0.6 + 0.1 * level
        } else {
            0.25 + 0.5 * level
        };
        ensure!(
            (value - oracle).abs() <= 1e-5,
            "RGB adapter numerical oracle differs"
        );
        values.push(value);
    }
    let receipt = if let Some(estimator) = temporal {
        estimator.finish()?
    } else {
        boxes.unwrap().finish()?
    }
    .context("CUDA adapter receipt missing")?;
    ensure!(
        receipt["provider"] == "cuda" && receipt["successful_runs"] == 4,
        "CUDA adapter provider/count differs"
    );
    ensure!(
        receipt["worker_binding"]["request_sha256"] == expected_request,
        "worker request binding differs"
    );
    let receipt_bytes = serde_json::to_vec(&receipt)?;
    let trace_bytes = std::fs::read(receipt["trace_path"].as_str().context("trace path")?)?;
    pulsar_engine::cuda_result::verify_bytes(&request, &receipt_bytes, &trace_bytes)?;
    for field in [
        "successful_runs",
        "provider",
        "worker_binding",
        "device",
        "trace",
        "trace_path",
    ] {
        let mut bad = receipt.clone();
        bad[field] = serde_json::Value::Null;
        ensure!(
            pulsar_engine::cuda_result::verify_bytes(
                &request,
                &serde_json::to_vec(&bad)?,
                &trace_bytes
            )
            .is_err(),
            "altered CUDA receipt accepted"
        );
    }
    let mut bad = receipt.clone();
    bad["worker_binding"]["request_sha256"] = "0".repeat(64).into();
    ensure!(
        pulsar_engine::cuda_result::verify_bytes(
            &request,
            &serde_json::to_vec(&bad)?,
            &trace_bytes
        )
        .is_err(),
        "changed request accepted"
    );
    for (field, value) in [
        ("job_id", serde_json::json!("other-job")),
        ("profile_sha256", serde_json::json!("0".repeat(64))),
    ] {
        let mut bad = receipt.clone();
        bad["worker_binding"][field] = value;
        ensure!(
            pulsar_engine::cuda_result::verify_bytes(
                &request,
                &serde_json::to_vec(&bad)?,
                &trace_bytes
            )
            .is_err(),
            "changed binding accepted"
        );
    }
    let mut bad = receipt.clone();
    bad["device"]["device_ordinal"] = 1.into();
    ensure!(
        pulsar_engine::cuda_result::verify_bytes(
            &request,
            &serde_json::to_vec(&bad)?,
            &trace_bytes
        )
        .is_err(),
        "changed device accepted"
    );
    let mut bad = receipt.clone();
    bad["successful_runs"] = 3.into();
    ensure!(
        pulsar_engine::cuda_result::verify_bytes(
            &request,
            &serde_json::to_vec(&bad)?,
            &trace_bytes
        )
        .is_err(),
        "changed run count accepted"
    );
    let mut bad = receipt.clone();
    bad["authorized"] = true.into();
    ensure!(
        pulsar_engine::cuda_result::verify_bytes(
            &request,
            &serde_json::to_vec(&bad)?,
            &trace_bytes
        )
        .is_err(),
        "unknown receipt field accepted"
    );
    ensure!(
        pulsar_engine::cuda_result::verify_bytes(&request, &receipt_bytes, b"[]").is_err(),
        "changed trace accepted"
    );
    // Exercise the actual parent egress reader with authored transport output,
    // not an engine-generated motion program or admitted production job.
    let emitted = std::path::Path::new("/output/emitted-cuda-inference.json");
    std::fs::rename("/output/cuda-inference.json", emitted)?;
    let program_path = std::path::Path::new("/output/emitted-program.json");
    let output_receipt = std::path::Path::new("/output/emitted-output-receipt.json");
    std::fs::write(program_path, b"{\"authored_transport_only\":true}")?;
    std::fs::write(output_receipt, b"{\"authored_transport_only\":true}")?;
    let program = pulsar_engine::worker::pin_artifact(program_path)?;
    let output_receipt = pulsar_engine::worker::pin_artifact(output_receipt)?;
    let evidence = WorkerCudaArtifacts {
        receipt: pulsar_engine::worker::pin_artifact(emitted)?,
        trace: pulsar_engine::worker::pin_artifact(std::path::Path::new(
            receipt["trace_path"].as_str().unwrap(),
        ))?,
    };
    let result = WorkerResult {
        provider_unavailable: None,
        version: PROTOCOL_VERSION,
        attempt_id: request.attempt_id.clone(),
        job_id: request.job_id.clone(),
        cuda_artifacts: Some(evidence.clone()),
        result: Ok(WorkerOutput::Generated {
            program: program.clone(),
            receipt: output_receipt.clone(),
            lineage: vec![],
            review: vec![],
        }),
    };
    for mode in 0..4 {
        let mut bad = result.clone();
        match mode {
            0 => bad.cuda_artifacts = None,
            1 => bad.job_id = JobId::new("other-job")?,
            2 => bad.cuda_artifacts.as_mut().unwrap().trace.identity.byte_len = 8 * 1024 * 1024 + 1,
            _ => {
                if let Ok(WorkerOutput::Generated { program, .. }) = &mut bad.result {
                    program.identity.byte_len = request.budget.output_bytes + 1;
                }
            }
        }
        let mut header = Vec::new();
        write_message(&mut header, &bad)?;
        ensure!(
            pulsar_engine::cuda_result::receive_worker_evidence(&mut header.as_slice(), &request)
                .is_err(),
            "malformed CUDA envelope accepted"
        );
        ensure!(
            !std::path::Path::new("/output/program.json").exists(),
            "refused envelope wrote parent output"
        );
    }
    let mut wire = Vec::new();
    write_message(&mut wire, &result)?;
    for artifact in [
        &program,
        &output_receipt,
        &evidence.receipt,
        &evidence.trace,
    ] {
        for chunk in std::fs::read(&artifact.path)?.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
            write_artifact_chunk(&mut wire, chunk)?;
        }
    }
    let received =
        pulsar_engine::cuda_result::receive_worker_evidence(&mut wire.as_slice(), &request)?;
    let retained = received
        .cuda_artifacts
        .context("parent CUDA evidence missing")?;
    ensure!(
        retained.trace.path == request.output_dir.join("cuda-trace.json"),
        "parent trace not rebased"
    );
    pulsar_engine::cuda_result::verify_files(&request, &retained)?;
    let mut bad = retained.clone();
    bad.trace.identity.sha256 = "0".repeat(64);
    ensure!(
        pulsar_engine::cuda_result::verify_files(&request, &bad).is_err(),
        "changed parent trace identity accepted"
    );
    let mut bad = retained.clone();
    bad.trace.path = "/runtime/libcuda.so.1".into();
    ensure!(
        pulsar_engine::cuda_result::verify_files(&request, &bad).is_err(),
        "outside-attempt CUDA evidence accepted"
    );
    std::fs::write(
        "/output/parent-egress-checked.json",
        serde_json::to_vec_pretty(&retained)?,
    )?;
    eprintln!(
        "PROFILE_PATH={}",
        receipt["trace_path"]
            .as_str()
            .context("trace path missing")?
    );
    let mut maps = String::new();
    std::fs::File::open("/proc/self/maps")?
        .take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut maps)?;
    ensure!(maps.len() <= 2 * 1024 * 1024, "mapping bound exceeded");
    let mut paths = BTreeSet::new();
    for line in maps.lines() {
        ensure!(line.len() <= 8192, "mapping line exceeds bound");
        if let Some(i) = line.find('/') {
            if line[i..].contains(".so") {
                paths.insert(&line[i..]);
            }
        }
    }
    for path in paths {
        eprintln!("LIBRARY_PATH={path}");
    }
    println!(
        "{}",
        serde_json::json!({"runs":4,"outputs":values,"rust_session":receipt,
        "decoded_rgb_adapter":true,"decoded_detector_adapter":detector,"production_cuda_admitted":false})
    );
    Ok(())
}
#[cfg(not(all(target_os = "linux", feature = "cuda")))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("CUDA adapter probe requires Linux and cuda feature")
}
