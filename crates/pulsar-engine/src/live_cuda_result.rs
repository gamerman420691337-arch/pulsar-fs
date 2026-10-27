//! Exact live transcript/original trace consistency; no motion or launch authority.
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    provider: InferenceProvider,
    provider_evidence: String,
    device: Device,
    successful_runs: u64,
    trace_path: PathBuf,
    trace: serde_json::Value,
    live_binding: LiveCudaStreamSummary,
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
pub(crate) fn verify_bytes(
    manifest: &LiveCudaInferenceManifest,
    expected: &LiveCudaStreamSummary,
    receipt: &[u8],
    trace: &[u8],
) -> Result<()> {
    expected.validate(manifest)?;
    ensure!(
        !receipt.is_empty() && receipt.len() <= 65536,
        "CUDA live receipt byte admission"
    );
    let receipt: Receipt = serde_json::from_slice(receipt)?;
    ensure!(
        receipt.provider == InferenceProvider::Cuda
            && receipt.provider_evidence == "strict-cuda-node-trace"
            && receipt.live_binding == *expected
            && receipt.successful_runs == expected.successful_runs,
        "CUDA live receipt stream correspondence differs"
    );
    let device = &receipt.device;
    let execution = &manifest.execution;
    ensure!(
        device
            .device_uuid
            .eq_ignore_ascii_case(&execution.device_uuid)
            && device
                .pci_bus_address
                .eq_ignore_ascii_case(&execution.pci_bus_address)
            && u32::from(device.device_ordinal) == execution.device_ordinal
            && device.visible_device_count > u32::from(device.device_ordinal)
            && device.visible_device_count <= 1024
            && (1..=1u64 << 40).contains(&device.total_bytes)
            && device.driver_version > 0,
        "CUDA live receipt SDK device differs"
    );
    ensure!(receipt.trace_path.parent() == Some(manifest.output_dir.as_path())
        && receipt.trace_path.file_name().and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("live_cuda_model_") && name.ends_with(".json")),
        "CUDA live original trace escaped stream output");
    let checked = crate::inference_trace::validate_node_trace(
        trace,
        InferenceProvider::Cuda,
        expected.successful_runs,
    )?;
    ensure!(
        serde_json::to_value(checked)? == receipt.trace,
        "CUDA live receipt trace differs from original bytes"
    );
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (
        LiveCudaInferenceManifest,
        LiveCudaStreamSummary,
        serde_json::Value,
        Vec<u8>,
    ) {
        let manifest = crate::cuda_mounts::live_test_witness().manifest;
        let summary = LiveCudaStreamSummary {
            schema_version: 1,
            manifest_sha256: manifest.identity_sha256().unwrap(),
            successful_runs: 1,
            last_sequence: 1,
            last_frame: FrameId::new(1),
            last_source_time: SourceTimestamp::new(1, 1000).unwrap(),
            transcript_sha256: "b".repeat(64),
        };
        let trace = serde_json::to_vec(&json!([
            {"cat":"Node","name":"Conv_0_kernel_time","ph":"X","ts":11,"dur":2,"pid":1,"tid":2,
                "args":{"provider":"CUDAExecutionProvider","op_name":"Conv"}},
            {"cat":"Session","name":"model_run","ph":"X","ts":10,"dur":10,"pid":1,"tid":2}
        ]))
        .unwrap();
        let receipt = json!({"provider":"cuda","provider_evidence":"strict-cuda-node-trace",
            "device":{"device_uuid":manifest.execution.device_uuid,"pci_bus_address":manifest.execution.pci_bus_address,
                "device_ordinal":manifest.execution.device_ordinal,"visible_device_count":1,"total_bytes":1<<30,"driver_version":13040},
            "successful_runs":1,"trace_path":manifest.output_dir.join("live_cuda_model_authored.json"),
            "trace":crate::inference_trace::validate_node_trace(&trace,InferenceProvider::Cuda,1).unwrap(),
            "live_binding":summary});
        (manifest, summary, receipt, trace)
    }
    #[test]
    fn exact_live_receipt_and_original_trace_correspond() {
        let (manifest, summary, receipt, trace) = fixture();
        verify_bytes(
            &manifest,
            &summary,
            &serde_json::to_vec(&receipt).unwrap(),
            &trace,
        )
        .unwrap();
    }
    #[test]
    fn live_receipt_refuses_rebound_stream_device_runs_paths_and_trace() {
        let (manifest, summary, receipt, trace) = fixture();
        for (pointer, replacement) in [
            ("/provider", json!("cpu")),
            ("/provider_evidence", json!("declared")),
            ("/successful_runs", json!(2)),
            ("/live_binding/transcript_sha256", json!("c".repeat(64))),
            ("/live_binding/manifest_sha256", json!("0".repeat(64))),
            ("/live_binding/last_sequence", json!(2)),
            (
                "/device/device_uuid",
                json!("GPU-00000000-0000-0000-0000-000000000000"),
            ),
            ("/device/pci_bus_address", json!("0000:02:00.0")),
            ("/device/device_ordinal", json!(1)),
            ("/device/visible_device_count", json!(0)),
            ("/device/visible_device_count", json!(1025)),
            ("/device/total_bytes", json!(0)),
            ("/device/total_bytes", json!((1u64 << 40) + 1)),
            ("/device/driver_version", json!(0)),
            ("/trace_path", json!("/outside/live_cuda_model_x.json")),
            ("/trace_path", json!("/work/cuda_model_x.json")),
            ("/trace_path", json!("/work/live_cuda_model_x.log")),
            ("/trace/trace_sha256", json!("0".repeat(64))),
            ("/trace/computation_events", json!(2)),
        ] {
            let mut bad = receipt.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                verify_bytes(
                    &manifest,
                    &summary,
                    &serde_json::to_vec(&bad).unwrap(),
                    &trace
                )
                .is_err(),
                "{pointer}"
            );
        }
        let mut bad = receipt.clone();
        bad["worker_binding"] = json!({});
        assert!(verify_bytes(
            &manifest,
            &summary,
            &serde_json::to_vec(&bad).unwrap(),
            &trace
        )
        .is_err());
        let mut changed = manifest.clone();
        changed.stream.attempt_id = AttemptId::new("other-stream").unwrap();
        assert!(verify_bytes(
            &changed,
            &summary,
            &serde_json::to_vec(&receipt).unwrap(),
            &trace
        )
        .is_err());
        let mut altered: serde_json::Value = serde_json::from_slice(&trace).unwrap();
        altered[0]["args"]["provider"] = json!("CPUExecutionProvider");
        assert!(verify_bytes(
            &manifest,
            &summary,
            &serde_json::to_vec(&receipt).unwrap(),
            &serde_json::to_vec(&altered).unwrap()
        )
        .is_err());
        let mut raw = trace.clone();
        raw.push(b' ');
        assert!(verify_bytes(
            &manifest,
            &summary,
            &serde_json::to_vec(&receipt).unwrap(),
            &raw
        )
        .is_err());
        for bytes in [Vec::new(), vec![b' '; 65537]] {
            assert!(verify_bytes(&manifest, &summary, &bytes, &trace).is_err());
        }
    }
}
