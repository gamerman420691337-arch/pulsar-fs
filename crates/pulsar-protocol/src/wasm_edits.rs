//! Per-invocation editor intent. Approval is not a grant or project commit.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmEditSpec {
    pub manifest: ExtensionManifest,
    pub module: Vec<u8>,
    pub guest_memory_bytes: u64,
    pub expected_worker: ArtifactIdentity,
    pub approve_execution: bool,
    pub label: String,
}
impl std::fmt::Debug for WasmEditSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmEditSpec")
            .field("module_identity", &self.manifest.artifact)
            .finish_non_exhaustive()
    }
}
impl WasmEditSpec {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !self.approve_execution || self.manifest.role != ExtensionRole::Editor {
            return Err(ProtocolError::invalid(
                "WASM editing requires explicit per-invocation approval and the Editor role",
            ));
        }
        self.expected_worker.validate()?;
        bounded_text(&self.label, 256, "WASM edit label")?;
        WasmProcessingRequest {
            manifest: self.manifest.clone(),
            module: self.module.clone(),
            input: vec![],
            guest_memory_bytes: self.guest_memory_bytes,
        }
        .validate()?;
        if serde_json::to_vec(self)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            .len()
            > MAX_CONTROL_BYTES - 16 * 1024
        {
            return Err(ProtocolError::invalid(
                "WASM module declaration exceeds control-frame admission",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmEditorInfo {
    pub worker: ArtifactIdentity,
    pub supported: bool,
    pub abi: String,
    pub maximum_buffer_bytes: u64,
    pub native_runtime_qualified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WasmEditState {
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmEditReceipt {
    pub schema_version: u16,
    pub request_sha256: String,
    pub module: ArtifactIdentity,
    pub worker: ArtifactIdentity,
    pub base_program: ProgramDescriptor,
    pub input_sha256: String,
    pub input_byte_len: u64,
    pub output_sha256: String,
    pub output_byte_len: u64,
    pub result_program: ProgramDescriptor,
    pub edit_receipt: ArtifactIdentity,
    pub fuel_consumed: u64,
    pub worker_peak_memory_bytes: u64,
}
impl WasmEditReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != PROTOCOL_VERSION
            || self.input_byte_len > WASM_BUFFER_MAX_BYTES as u64
            || self.output_byte_len > WASM_BUFFER_MAX_BYTES as u64
            || self.fuel_consumed > WASM_FUEL_LIMIT
        {
            return Err(ProtocolError::invalid(
                "invalid WASM edit receipt limits or version",
            ));
        }
        self.module.validate()?;
        self.worker.validate()?;
        self.base_program.validate()?;
        self.result_program.validate()?;
        self.edit_receipt.validate()?;
        validate_sha256(&self.request_sha256)?;
        validate_sha256(&self.input_sha256)?;
        validate_sha256(&self.output_sha256)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmEditReport {
    pub attempt_id: AttemptId,
    pub request_id: RequestId,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub engine_epoch: String,
    pub state: WasmEditState,
    pub candidate_id: Option<CandidateId>,
    pub receipt: Option<WasmEditReceipt>,
    pub error: Option<ProtocolError>,
}
impl WasmEditReport {
    pub fn validate_for(&self, request: &Request) -> Result<(), ProtocolError> {
        self.validate()?;
        if request.project.as_ref() != Some(&self.project_id) {
            return Err(ProtocolError::invalid("WASM edit response project differs"));
        }
        match &request.command {
            Command::StartWasmEdit { spec } => {
                if self.request_id != request.request_id
                    || request.expected_revision != Some(self.base_revision)
                {
                    return Err(ProtocolError::invalid(
                        "WASM edit response invocation differs",
                    ));
                }
                if let Some(receipt) = &self.receipt {
                    use sha2::{Digest, Sha256};
                    let fingerprint = format!(
                        "{:x}",
                        Sha256::digest(
                            request_fingerprint(request)
                                .map_err(|e| ProtocolError::invalid(e.to_string()))?
                        )
                    );
                    if receipt.module != spec.manifest.artifact
                        || receipt.worker != spec.expected_worker
                        || receipt.request_sha256 != fingerprint
                    {
                        return Err(ProtocolError::invalid(
                            "WASM edit response approval identity differs",
                        ));
                    }
                }
            }
            Command::WasmEditStatus { attempt_id } | Command::CancelWasmEdit { attempt_id }
                if attempt_id == &self.attempt_id => {}
            _ => {
                return Err(ProtocolError::invalid(
                    "WASM edit response operation differs",
                ))
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        bounded_text(&self.engine_epoch, 128, "WASM operation engine epoch")?;
        let completed = self.state == WasmEditState::Completed;
        if completed != self.candidate_id.is_some()
            || completed != self.receipt.is_some()
            || completed && self.error.is_some()
            || self.state == WasmEditState::Running && self.error.is_some()
            || matches!(
                self.state,
                WasmEditState::Failed | WasmEditState::Cancelled | WasmEditState::Interrupted
            ) && self.error.is_none()
        {
            return Err(ProtocolError::invalid(
                "inconsistent WASM edit operation state",
            ));
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
        }
        Ok(())
    }
}

/// Bounded discovery contains status facts, never captured module/input bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmEditCatalog {
    pub project_id: ProjectId,
    pub operations: Vec<WasmEditReport>,
}
impl WasmEditState {
    pub fn is_unsuccessful_terminal(&self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::Interrupted)
    }
}
impl WasmEditCatalog {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.operations.len() > 64 {
            return Err(ProtocolError::invalid(
                "WASM catalog response exceeds operation bound",
            ));
        }
        let mut attempts = std::collections::BTreeSet::new();
        let mut requests = std::collections::BTreeSet::new();
        for report in &self.operations {
            report.validate()?;
            if report.project_id != self.project_id
                || !attempts.insert(&report.attempt_id)
                || !requests.insert(&report.request_id)
            {
                return Err(ProtocolError::invalid(
                    "WASM catalog contains foreign or duplicate operations",
                ));
            }
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("WASM catalog cannot be encoded"))?;
        if encoded.len() > MAX_CONTROL_BYTES - 16 * 1024 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "WASM catalog exceeds control response admission",
            ));
        }
        Ok(())
    }
    pub fn validate_for(&self, request: &Request) -> Result<(), ProtocolError> {
        self.validate()?;
        if !matches!(request.command, Command::ListWasmEdits)
            || request.project.as_ref() != Some(&self.project_id)
        {
            return Err(ProtocolError::invalid(
                "WASM catalog response differs from request",
            ));
        }
        Ok(())
    }
}
impl WasmEditReport {
    pub fn validate_removal_for(&self, request: &Request) -> Result<(), ProtocolError> {
        self.validate()?;
        match &request.command {
            Command::RemoveUnsuccessfulWasmEdit {
                attempt_id,
                original_request_id,
                expected_state,
            } if request.project.as_ref() == Some(&self.project_id)
                && attempt_id == &self.attempt_id
                && original_request_id == &self.request_id
                && expected_state == &self.state
                && self.state.is_unsuccessful_terminal() =>
            {
                Ok(())
            }
            _ => Err(ProtocolError::invalid(
                "WASM removal response differs from explicit intent",
            )),
        }
    }
}

impl WasmEditReport {
    pub fn validate_archive_for(&self, request: &Request) -> Result<(), ProtocolError> {
        self.validate()?;
        match &request.command {
            Command::ArchiveCompletedWasmEdit {
                attempt_id,
                original_request_id,
            } if request.project.as_ref() == Some(&self.project_id)
                && attempt_id == &self.attempt_id
                && original_request_id == &self.request_id
                && self.state == WasmEditState::Completed =>
            {
                Ok(())
            }
            _ => Err(ProtocolError::invalid(
                "WASM archive response differs from explicit completed intent",
            )),
        }
    }
}
