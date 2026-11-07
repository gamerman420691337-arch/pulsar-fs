//! Bounded original evidence retained through readback/publication by its caller.
//! Consistency is neither Engine authorization nor model/device qualification.
use crate::resources::{Reservation, ResourcePool};
use anyhow::{ensure, Result};
use pulsar_protocol::{
    ArtifactId, ArtifactIdentity, LiveCudaEvidenceRecord, LiveCudaInferenceManifest,
    LiveCudaStreamSummary,
};

pub(crate) const EVIDENCE_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_SUMMARY_BYTES: usize = 4096;
const MAX_RECEIPT_BYTES: usize = 65536;

/// Acquire before receiving native bytes or loading database blobs. The guard
/// covers parsed JSON and retained originals, and does not occupy a worker slot.
pub(crate) struct EvidenceAdmission {
    _reservation: Reservation,
}
pub(crate) struct LiveCudaEvidence {
    manifest: LiveCudaInferenceManifest,
    manifest_bytes: Vec<u8>,
    summary: LiveCudaStreamSummary,
    summary_bytes: Vec<u8>,
    receipt: Vec<u8>,
    trace: Vec<u8>,
    _admission: EvidenceAdmission,
    storage_reserved: bool,
    storage_extra: u64,
}
impl EvidenceAdmission {
    pub(crate) fn reserve(pool: &ResourcePool) -> Result<Self> {
        Ok(Self {
            _reservation: pool.reserve_memory(EVIDENCE_MEMORY_BYTES)?,
        })
    }
    pub(crate) fn finished(
        self,
        manifest: &LiveCudaInferenceManifest,
        summary: &LiveCudaStreamSummary,
        receipt: Vec<u8>,
        trace: Vec<u8>,
    ) -> Result<LiveCudaEvidence> {
        manifest.validate()?;
        summary.validate(manifest)?;
        self.read(
            serde_json::to_vec(manifest)?,
            serde_json::to_vec(summary)?,
            receipt,
            trace,
        )
    }
    /// Canonical control bytes and original native bytes have different rules:
    /// the latter must never be normalized, even when the JSON is equivalent.
    pub(crate) fn read(
        self,
        manifest_bytes: Vec<u8>,
        summary_bytes: Vec<u8>,
        receipt: Vec<u8>,
        trace: Vec<u8>,
    ) -> Result<LiveCudaEvidence> {
        for (bytes, limit) in [
            (&manifest_bytes, MAX_MANIFEST_BYTES),
            (&summary_bytes, MAX_SUMMARY_BYTES),
            (&receipt, MAX_RECEIPT_BYTES),
            (&trace, crate::inference_trace::MAX_TRACE_BYTES),
        ] {
            ensure!(
                !bytes.is_empty() && bytes.len() <= limit,
                "CUDA retained evidence byte admission"
            );
        }
        let manifest: LiveCudaInferenceManifest = serde_json::from_slice(&manifest_bytes)?;
        let summary: LiveCudaStreamSummary = serde_json::from_slice(&summary_bytes)?;
        ensure!(
            serde_json::to_vec(&manifest)? == manifest_bytes
                && serde_json::to_vec(&summary)? == summary_bytes,
            "CUDA retained control bytes are not canonical"
        );
        crate::live_cuda_result::verify_bytes(&manifest, &summary, &receipt, &trace)?;
        Ok(LiveCudaEvidence {
            manifest,
            manifest_bytes,
            summary,
            summary_bytes,
            receipt,
            trace,
            _admission: self,
            storage_reserved: false,
            storage_extra: 0,
        })
    }
}
impl LiveCudaEvidence {
    pub(crate) fn report_record(&self) -> Result<LiveCudaEvidenceRecord> {
        use sha2::{Digest, Sha256};
        fn identity(bytes: &[u8]) -> Result<ArtifactIdentity> {
            let sha256 = format!("{:x}", Sha256::digest(bytes));
            Ok(ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone())?,
                sha256,
                byte_len: bytes.len() as u64,
            })
        }
        let record = LiveCudaEvidenceRecord {
            schema_version: 1,
            qualified: false,
            manifest: identity(self.manifest_bytes())?,
            summary: self.summary().clone(),
            receipt: identity(self.receipt())?,
            trace: identity(self.trace())?,
        };
        record.validate()?;
        Ok(record)
    }
    /// Call before opening the publication transaction, with actual free space
    /// from the database filesystem. Keep this object through transaction commit.
    #[cfg(test)]
    pub(crate) fn reserve_storage(&mut self, available: u64) -> Result<()> {
        self.reserve_storage_extra(available, 0)
    }
    pub(crate) fn reserve_storage_extra(&mut self, available: u64, extra: u64) -> Result<()> {
        let charge = self
            .byte_len()
            .checked_add(extra)
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(1024 * 1024))
            .ok_or_else(|| anyhow::anyhow!("CUDA storage charge overflow"))?;
        self._admission
            ._reservation
            .attach_storage(charge, available)?;
        self.storage_reserved = true;
        self.storage_extra = extra;
        Ok(())
    }
    pub(crate) fn storage_extra(&self) -> u64 {
        self.storage_extra
    }
    pub(crate) fn storage_reserved(&self) -> bool {
        self.storage_reserved
    }
    pub(crate) fn manifest(&self) -> &LiveCudaInferenceManifest {
        &self.manifest
    }
    pub(crate) fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }
    pub(crate) fn summary(&self) -> &LiveCudaStreamSummary {
        &self.summary
    }
    pub(crate) fn summary_bytes(&self) -> &[u8] {
        &self.summary_bytes
    }
    pub(crate) fn receipt(&self) -> &[u8] {
        &self.receipt
    }
    pub(crate) fn trace(&self) -> &[u8] {
        &self.trace
    }
    pub(crate) fn byte_len(&self) -> u64 {
        (self.manifest_bytes.len()
            + self.summary_bytes.len()
            + self.receipt.len()
            + self.trace.len()) as u64
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// Authored parser/storage inputs only. No native execution, GPU, report
    /// directory or qualification receipt is consulted or established here.
    pub(crate) fn original() -> (
        LiveCudaInferenceManifest,
        LiveCudaStreamSummary,
        Vec<u8>,
        Vec<u8>,
    ) {
        use pulsar_protocol::{FrameId, InferenceProvider, SourceTimestamp};
        use serde_json::json;
        let mut manifest: LiveCudaInferenceManifest = serde_json::from_str(include_str!(
            "../tests/fixtures/authored-live-cuda-manifest.json"
        ))
        .unwrap();
        // These are declarations, never opened. Use the host's absolute path
        // syntax so portable parser tests do not depend on Unix path semantics.
        let root = if cfg!(windows) {
            "C:/authored"
        } else {
            "/authored"
        };
        let root = std::path::Path::new(root);
        manifest.stream.model.path = root.join("input/model");
        manifest.execution.library_directory = root.join("libraries");
        manifest.stream.runtime.path = manifest
            .execution
            .library_directory
            .join("libonnxruntime.so.1");
        manifest.output_dir = root.join("output");
        let summary = LiveCudaStreamSummary {
            schema_version: 1,
            manifest_sha256: manifest.identity_sha256().unwrap(),
            successful_runs: 1,
            last_sequence: 1,
            last_frame: FrameId::new(1),
            last_source_time: SourceTimestamp::new(1, 1000).unwrap(),
            transcript_sha256: "b".repeat(64),
        };
        // Preserve deliberately noncanonical original bytes through the real
        // parser and database paths; hashing a reserialized trace would differ.
        let trace = br#" [
            {"cat":"Node","name":"Authored_0_kernel_time","ph":"X","ts":11,"dur":2,"pid":1,"tid":2,
                "args":{"provider":"CUDAExecutionProvider","op_name":"Conv"}},
            {"cat":"Session","name":"model_run","ph":"X","ts":10,"dur":10,"pid":1,"tid":2}
        ]
"#
        .to_vec();
        let receipt = serde_json::to_vec_pretty(&json!({
            "provider":"cuda", "provider_evidence":"strict-cuda-node-trace",
            "device":{
                "device_uuid":manifest.execution.device_uuid,
                "pci_bus_address":manifest.execution.pci_bus_address,
                "device_ordinal":manifest.execution.device_ordinal,
                "visible_device_count":1, "total_bytes":1u64 << 30, "driver_version":13040
            },
            "successful_runs":1,
            "trace_path":manifest.output_dir.join("live_cuda_model_authored.json"),
            "trace":crate::inference_trace::validate_node_trace(&trace, InferenceProvider::Cuda, 1).unwrap(),
            "live_binding":summary
        }))
        .unwrap();
        (manifest, summary, receipt, trace)
    }
    #[test]
    fn retained_native_bytes_roundtrip_without_cuda_loading_and_keep_memory_claim() {
        let pool = ResourcePool::new(EVIDENCE_MEMORY_BYTES + 1, 1);
        let control = pool.reserve(1).unwrap();
        let admission = EvidenceAdmission::reserve(&pool).unwrap();
        assert!(pool.reserve_memory(1).is_err());
        let (manifest, summary, receipt, trace) = original();
        let evidence = admission
            .finished(&manifest, &summary, receipt.clone(), trace.clone())
            .unwrap();
        assert_eq!(evidence.receipt(), receipt);
        assert_eq!(evidence.trace(), trace);
        assert_eq!(evidence.summary(), &summary);
        assert_eq!(
            evidence.manifest().identity_sha256().unwrap(),
            summary.manifest_sha256
        );
        let bytes = (
            evidence.manifest_bytes().to_vec(),
            evidence.summary_bytes().to_vec(),
            evidence.receipt().to_vec(),
            evidence.trace().to_vec(),
        );
        assert_eq!(
            evidence.byte_len(),
            (bytes.0.len() + bytes.1.len() + bytes.2.len() + bytes.3.len()) as u64
        );
        assert!(EvidenceAdmission::reserve(&pool).is_err());
        drop(evidence);
        let reread = EvidenceAdmission::reserve(&pool)
            .unwrap()
            .read(bytes.0, bytes.1, bytes.2, bytes.3)
            .unwrap();
        assert_eq!(reread.trace(), trace);
        assert!(pool.reserve_memory(1).is_err());
        drop(reread);
        drop(control);
        assert!(pool.reserve(EVIDENCE_MEMORY_BYTES + 1).is_ok());
    }
    #[test]
    fn refused_readback_releases_memory_and_rejects_noncanonical_or_changed_evidence() {
        let pool = ResourcePool::new(EVIDENCE_MEMORY_BYTES, 1);
        let (manifest, summary, receipt, trace) = original();
        let canonical = (
            serde_json::to_vec(&manifest).unwrap(),
            serde_json::to_vec(&summary).unwrap(),
        );
        for case in 0..10 {
            let (mut m, mut s, mut r, mut t) = (
                canonical.0.clone(),
                canonical.1.clone(),
                receipt.clone(),
                trace.clone(),
            );
            match case {
                0 => m.push(b' '),
                1 => s.push(b' '),
                2 => t.push(b' '),
                3 => r = b"{}".to_vec(),
                4 => m.clear(),
                5 => m = vec![b' '; MAX_MANIFEST_BYTES + 1],
                6 => s = vec![b' '; MAX_SUMMARY_BYTES + 1],
                7 => r = vec![b' '; MAX_RECEIPT_BYTES + 1],
                8 => t = vec![b' '; crate::inference_trace::MAX_TRACE_BYTES + 1],
                9 => {
                    let mut bad = summary.clone();
                    bad.last_sequence += 1;
                    s = serde_json::to_vec(&bad).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                EvidenceAdmission::reserve(&pool)
                    .unwrap()
                    .read(m, s, r, t)
                    .is_err(),
                "case {case}"
            );
            assert!(
                pool.reserve_memory(EVIDENCE_MEMORY_BYTES).is_ok(),
                "case {case}"
            );
        }
    }
    #[test]
    fn insufficient_memory_refuses_before_any_input_is_read() {
        let pool = ResourcePool::new(EVIDENCE_MEMORY_BYTES - 1, 1);
        assert!(EvidenceAdmission::reserve(&pool).is_err());
        assert!(pool.reserve(EVIDENCE_MEMORY_BYTES - 1).is_ok());
    }
}
