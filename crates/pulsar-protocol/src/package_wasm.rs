//! Inert original invocation facts. Importing these never authorizes execution.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageWasmInvocation {
    pub format: String,
    pub attempt_id: AttemptId,
    pub request_id: RequestId,
    pub engine_epoch: String,
    pub origin_candidate_id: CandidateId,
    pub base_revision: RevisionId,
    pub manifest: ExtensionManifest,
    pub guest_memory_bytes: u64,
    /// Retained declaration, not a current grant or human acceptance signature.
    pub approve_execution: bool,
    pub approved_worker: ArtifactIdentity,
    pub receipt: WasmEditReceipt,
    /// Original guest input; the authored lineage input is the guest output.
    pub input: PackageObjectRef,
}
impl PackageWasmInvocation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.manifest.validate()?;
        self.receipt.validate()?;
        self.input.validate()?;
        self.approved_worker.validate()?;
        bounded_text(&self.engine_epoch, 128, "archived WASM epoch")?;
        if self.format != "pulsar-wasm-invocation-v1"
            || !self.approve_execution
            || self.manifest.role != ExtensionRole::Editor
            || self.manifest.abi != WASM_PROCESSING_ABI
            || !matches!(self.manifest.confinement, ConfinementRequirement::Wasm)
            || !self.manifest.requested_scopes.is_empty()
            || self.manifest.artifact != self.receipt.module
            || self.approved_worker != self.receipt.worker
            || self.manifest.artifact.byte_len == 0
            || self.manifest.artifact.byte_len > WASM_MODULE_MAX_BYTES as u64
            || self.input.sha256 != self.receipt.input_sha256
            || self.input.byte_len != self.receipt.input_byte_len
            || self.guest_memory_bytes < 65536
            || self.guest_memory_bytes > 16 * 1024 * 1024
            || self.guest_memory_bytes > self.manifest.budget.memory_bytes
            || self.manifest.budget.wall_time_ms > 5000
            || self.manifest.budget.cpu_threads != 2
            || self.manifest.budget.memory_bytes < 64 * 1024 * 1024
            || self.manifest.budget.memory_bytes > 512 * 1024 * 1024
            || self.manifest.budget.output_bytes > WASM_BUFFER_MAX_BYTES as u64
            || self.receipt.output_byte_len > self.manifest.budget.output_bytes
            || self.receipt.base_program.byte_len > 8 * WASM_BUFFER_MAX_BYTES as u64
            || self.approved_worker.byte_len == 0
            || self.approved_worker.byte_len > 128 * 1024 * 1024
        {
            return Err(ProtocolError::invalid(
                "archived WASM invocation binding or admission differs",
            ));
        }
        Ok(())
    }
}
