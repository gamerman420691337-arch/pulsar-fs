//! Parent-side CUDA evidence consistency. This does not grant launch authority.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::PathBuf};

/// Shared bounded egress reader. Caller owns admission, private output custody,
/// deadline and child retirement; this function cannot authorize a GPU job.
pub fn receive_worker_evidence(
    reader: &mut impl Read,
    request: &WorkerRequest,
) -> std::result::Result<WorkerResult, ProtocolError> {
    request.validate()?;
    crate::launch::receive_artifacts(reader, request)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    provider: InferenceProvider,
    provider_evidence: String,
    device: Device,
    successful_runs: u64,
    trace_path: PathBuf,
    trace: serde_json::Value,
    worker_binding: Binding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Device {
    device_uuid: String,
    pci_bus_address: String,
    device_ordinal: u16,
    visible_device_count: u32,
    total_bytes: u64,
    driver_version: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    attempt_id: AttemptId,
    job_id: JobId,
    request_sha256: String,
    profile_sha256: String,
    model: ArtifactIdentity,
    runtime: ArtifactIdentity,
    owner_policy: ArtifactIdentity,
}

/// Reparse original trace bytes and bind their declaration to one exact request.
/// A valid receipt is not evidence of engine admission or trace authenticity.
pub fn verify_bytes(request: &WorkerRequest, receipt: &[u8], trace: &[u8]) -> Result<()> {
    verify_bytes_mode(request, receipt, trace, None)
}
fn verify_bytes_mode(
    request: &WorkerRequest,
    receipt: &[u8],
    trace: &[u8],
    preview_runs: Option<u64>,
) -> Result<()> {
    request.validate()?;
    ensure!(
        !receipt.is_empty() && receipt.len() <= 65536,
        "CUDA receipt byte admission"
    );
    let execution = request
        .cuda_execution
        .as_ref()
        .context("CUDA result on a CPU request")?;
    let profile = match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        _ => None,
    }
    .context("CUDA result requires selected profile")?;
    let receipt: Receipt = serde_json::from_slice(receipt)?;
    let initialization_only = preview_runs == Some(0);
    ensure!(
        !initialization_only
            || matches!(
                &request.operation,
                WorkerOperation::Preview {
                    annotated: Some(_),
                    ..
                }
            ),
        "no-inference evidence requires annotated preview"
    );
    let expected_evidence = if initialization_only {
        "cuda-session-initialized-without-inference"
    } else {
        "strict-cuda-node-trace"
    };
    ensure!(
        receipt.provider == InferenceProvider::Cuda
            && receipt.provider_evidence == expected_evidence,
        "CUDA result provider/evidence differs"
    );
    if let Some(runs) = preview_runs {
        ensure!(
            receipt.successful_runs == runs,
            "CUDA preview run count differs from pair availability"
        );
    }
    let binding = receipt.worker_binding;
    ensure!(
        binding.attempt_id == request.attempt_id
            && binding.job_id == request.job_id
            && binding.request_sha256
                == format!("{:x}", Sha256::digest(serde_json::to_vec(request)?))
            && binding.profile_sha256
                == format!("{:x}", Sha256::digest(serde_json::to_vec(profile)?))
            && binding.model == profile.model
            && binding.runtime == profile.runtime
            && binding.owner_policy == execution.owner_policy,
        "CUDA receipt request/profile identity differs"
    );
    let device = receipt.device;
    ensure!(
        device
            .device_uuid
            .eq_ignore_ascii_case(&execution.device_uuid)
            && device
                .pci_bus_address
                .eq_ignore_ascii_case(&execution.pci_bus_address)
            && u32::from(device.device_ordinal) == execution.device_ordinal
            && device.visible_device_count > u32::from(device.device_ordinal)
            && device.total_bytes > 0
            && device.driver_version > 0,
        "CUDA receipt SDK device differs"
    );
    ensure!(
        receipt.trace_path.parent() == Some(request.output_dir.as_path())
            && receipt
                .trace_path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("cuda_model_")),
        "CUDA trace path differs from attempt"
    );
    let checked = if initialization_only {
        crate::inference_trace::validate_initialization_trace(trace)?
    } else {
        crate::inference_trace::validate_node_trace(
            trace,
            InferenceProvider::Cuda,
            receipt.successful_runs,
        )?
    };
    ensure!(
        serde_json::to_value(checked)? == receipt.trace,
        "CUDA declared trace differs from original bytes"
    );
    Ok(())
}

/// Files must be the parent's own bounded egress copies, never worker paths.
pub fn verify_files(request: &WorkerRequest, evidence: &WorkerCudaArtifacts) -> Result<()> {
    verify_files_mode(request, evidence, None)
}
/// Only this entry point admits initialization-only evidence, after validating
/// original target pixels and pair support. Historical/generated readers stay strict.
pub(crate) fn verify_preview_files(
    request: &WorkerRequest,
    evidence: &WorkerCudaArtifacts,
    preview: &PreviewResult,
) -> Result<()> {
    let WorkerOperation::Preview {
        annotated: Some(spec),
        model: Some(model),
        inference_profile: Some(profile),
        context,
        source_time,
        time_basis,
        placement_seek,
        ..
    } = &request.operation
    else {
        return verify_files(request, evidence);
    };
    let annotated = preview
        .annotated
        .as_ref()
        .context("annotated CUDA preview evidence absent")?;
    annotated.validate(preview)?;
    ensure!(
        serde_json::to_vec(&annotated.spec)? == serde_json::to_vec(spec)?
            && annotated.model == model.identity
            && annotated.profile == crate::inference_profiles::profile_identity(profile)?
            && request
                .tools
                .onnx_runtime
                .as_ref()
                .is_some_and(|r| r.identity == annotated.runtime),
        "CUDA preview model/profile/view binding differs"
    );
    ensure!(
        if let Some(placed) = placement_seek {
            preview.matches_placed_seek_request(context, placed)
        } else {
            preview.matches_timed_seek_request(context, source_time, *time_basis)
        },
        "CUDA preview request/clock binding differs"
    );
    crate::worker::validate_annotated_preview_pair(preview)?;
    verify_files_mode(
        request,
        evidence,
        Some(u64::from(annotated.position.is_some())),
    )
}
fn verify_files_mode(
    request: &WorkerRequest,
    evidence: &WorkerCudaArtifacts,
    preview_runs: Option<u64>,
) -> Result<()> {
    evidence.validate()?;
    fn read(reference: &ArtifactReference, limit: u64) -> Result<Vec<u8>> {
        let mut file = crate::artifacts::open_regular(&reference.path)?;
        ensure!(
            file.metadata()?.len() == reference.identity.byte_len
                && reference.identity.byte_len <= limit,
            "CUDA egress file length differs"
        );
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(limit + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == reference.identity.byte_len
                && format!("{:x}", Sha256::digest(&bytes)) == reference.identity.sha256,
            "CUDA egress file bytes differ"
        );
        Ok(bytes)
    }
    for reference in [&evidence.receipt, &evidence.trace] {
        ensure!(
            reference.path.parent() == Some(request.output_dir.as_path()),
            "CUDA evidence outside parent attempt"
        );
    }
    verify_bytes_mode(
        request,
        &read(&evidence.receipt, 65536)?,
        &read(&evidence.trace, 8 * 1024 * 1024)?,
        preview_runs,
    )
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (WorkerRequest, Vec<u8>, serde_json::Value) {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let WorkerOperation::Generate {
            settings, model, ..
        } = request.operation.clone()
        else {
            panic!()
        };
        let mut profile = settings.inference_profile.unwrap();
        profile.preprocessing.resize = InferenceResize::AnnotatedTemporalView;
        let view: AnnotatedGenerationView = serde_json::from_value(json!({"source_sha256":request.source.identity.sha256,
            "declaration":{"schema_version":1,"source_dimensions":[64,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],
            "target_id":"authored","video_view":{"layout":"mono","eye":"left","projection":{"kind":"flat"}}}})).unwrap();
        request.operation = WorkerOperation::Preview {
            still_image: false,
            context: FrameContext {
                source_version: request.source.source_version.clone(),
                source_placement: SourcePlacementId::new("primary").unwrap(),
                frame: FrameId::new(0),
                transform: annotated_preview_transform(&view).unwrap(),
                seek_generation: 1,
                request_generation: 1,
            },
            annotated: Some(Into::into(AnnotatedPreviewSpec {
                view,
                source_sample_stride: 1,
            })),
            placement_seek: None,
            time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
            video_view: None,
            source_time: SourceTimestamp::new(0, 1).unwrap(),
            max_width: 64,
            max_height: 64,
            model,
            model_input: Some(profile.preprocessing.input.clone()),
            inference_profile: Some(Into::into(profile.clone())),
        };
        request.validate().unwrap();
        let execution = request.cuda_execution.as_ref().unwrap();
        let trace = serde_json::to_vec(&json!([{"cat":"Session","name":"session_initialization","ph":"X","ts":1,"dur":5,"pid":1,"tid":2}])).unwrap();
        let receipt = json!({"provider":"cuda","provider_evidence":"cuda-session-initialized-without-inference",
            "device":{"device_uuid":execution.device_uuid,"pci_bus_address":execution.pci_bus_address,"device_ordinal":execution.device_ordinal,
                "visible_device_count":1,"total_bytes":1024,"driver_version":1},"successful_runs":0,
            "trace_path":request.output_dir.join("cuda_model_initialization.json"),
            "trace":crate::inference_trace::validate_initialization_trace(&trace).unwrap(),
            "worker_binding":{"attempt_id":request.attempt_id,"job_id":request.job_id,
                "request_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&request).unwrap())),
                "profile_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&profile).unwrap())),
                "model":profile.model,"runtime":profile.runtime,"owner_policy":execution.owner_policy}});
        (request, trace, receipt)
    }
    #[test]
    fn supported_pair_cannot_claim_no_inference_or_extra_runs() {
        let (mut request, _, _) = fixture();
        let root = tempfile::tempdir().unwrap();
        request.output_dir = root.path().to_path_buf();
        if let WorkerOperation::Preview { source_time, .. } = &mut request.operation {
            *source_time = SourceTimestamp::new(1, 8).unwrap();
        }
        let WorkerOperation::Preview {
            context,
            annotated: Some(spec),
            model: Some(model),
            inference_profile: Some(profile),
            ..
        } = &request.operation
        else {
            panic!()
        };
        let write_frame = |name: &str, value: u8| {
            let bytes = vec![value; 64 * 64 * 3];
            let path = root.path().join(name);
            std::fs::write(&path, &bytes).unwrap();
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            FrameArtifact {
                artifact: ArtifactReference {
                    identity: ArtifactIdentity {
                        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                        sha256,
                        byte_len: bytes.len() as u64,
                    },
                    path,
                },
                width: 64,
                height: 64,
                stride_bytes: 192,
                format: PixelFormat::Rgb8,
            }
        };
        let mut context = context.clone();
        context.frame = FrameId::new(1);
        let preview = PreviewResult {
            context,
            time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
            requested_source_time: SourceTimestamp::new(1, 8).unwrap(),
            source_time: SourceTimestamp::new(1, 8).unwrap(),
            source_frame_index: 1,
            elapsed_source_time: Some(SourceTimestamp::new(1, 8).unwrap()),
            source_origin: Some(SourceTimestamp::new(0, 1).unwrap()),
            coordinate_map: None,
            placed: None,
            observations: vec![],
            frame: Some(write_frame("current.rgb", 9)),
            analysis: AnalysisStatus::Unavailable {
                reason: "temporal_pair_without_continuous_support".into(),
            },
            annotated: Some(AnnotatedPreviewEvidence {
                spec: *spec.clone(),
                model: model.identity.clone(),
                runtime: profile.runtime.clone(),
                profile: crate::inference_profiles::profile_identity(profile).unwrap(),
                contract: AnnotatedTemporalModelContract::expected(),
                training_intake_sha256: "b".repeat(64),
                qualified: false,
                position: None,
                previous: Some(AnnotatedPreviewPrevious {
                    source_frame_index: 0,
                    source_time: SourceTimestamp::new(0, 1).unwrap(),
                    elapsed_source_time: SourceTimestamp::new(0, 1).unwrap(),
                    frame: write_frame("previous.rgb", 7),
                }),
            }),
        };
        // Direct support check is also used by the only no-inference parent gate.
        assert!(crate::worker::validate_annotated_preview_pair(&preview).is_err());
        let fake = WorkerCudaArtifacts {
            receipt: preview.frame.as_ref().unwrap().artifact.clone(),
            trace: preview
                .annotated
                .as_ref()
                .unwrap()
                .previous
                .as_ref()
                .unwrap()
                .frame
                .artifact
                .clone(),
        };
        let error = verify_preview_files(&request, &fake, &preview).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("availability differs from original pair support"),
            "{error:#}"
        );
        let mut inferred = preview.clone();
        inferred.annotated.as_mut().unwrap().position =
            Some(NormalizedPosition::new(0.25).unwrap());
        inferred.analysis = AnalysisStatus::Available;
        crate::worker::validate_annotated_preview_pair(&inferred).unwrap();
        let (request, trace, receipt) = fixture();
        assert!(verify_bytes_mode(
            &request,
            &serde_json::to_vec(&receipt).unwrap(),
            &trace,
            Some(1)
        )
        .is_err());
        let mut extra = receipt;
        extra["successful_runs"] = 2.into();
        extra["provider_evidence"] = "strict-cuda-node-trace".into();
        let error = verify_bytes_mode(
            &request,
            &serde_json::to_vec(&extra).unwrap(),
            &trace,
            Some(1),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("run count differs from pair availability"));
    }
    #[test]
    fn no_inference_receipt_is_separate_and_exactly_request_bound() {
        let (request, trace, receipt) = fixture();
        let encode = |r: &serde_json::Value| serde_json::to_vec(r).unwrap();
        verify_bytes_mode(&request, &encode(&receipt), &trace, Some(0)).unwrap();
        assert!(verify_bytes(&request, &encode(&receipt), &trace).is_err());
        assert!(verify_bytes_mode(&request, &encode(&receipt), &trace, Some(1)).is_err());
        for (pointer, value) in [
            ("/provider_evidence", json!("strict-cuda-node-trace")),
            ("/successful_runs", json!(1)),
            ("/worker_binding/request_sha256", json!("a".repeat(64))),
            ("/worker_binding/profile_sha256", json!("a".repeat(64))),
            ("/device/device_uuid", json!("GPU-wrong")),
            ("/trace/schema_version", json!(1)),
        ] {
            let mut bad = receipt.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                verify_bytes_mode(&request, &encode(&bad), &trace, Some(0)).is_err(),
                "{pointer}"
            );
        }
        let generation: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        assert!(verify_bytes_mode(&generation, &encode(&receipt), &trace, Some(0)).is_err());
        let mut executed: serde_json::Value = serde_json::from_slice(&trace).unwrap();
        executed[0]["name"] = "model_run".into();
        assert!(
            verify_bytes_mode(&request, &encode(&receipt), &encode(&executed), Some(0)).is_err()
        );
    }
}
