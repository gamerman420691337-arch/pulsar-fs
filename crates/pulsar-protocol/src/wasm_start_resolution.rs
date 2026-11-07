//! Resolving historical start metadata never approves executable bytes or cancels accepted work.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmStartReference {
    pub original_request_id: RequestId,
    pub base_revision: RevisionId,
    pub manifest: ExtensionManifest,
    pub worker: ArtifactIdentity,
    pub guest_memory_bytes: u64,
    pub label: String,
}
impl WasmStartReference {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.base_revision.value() > i64::MAX as u64
            || self.manifest.role != ExtensionRole::Editor
            || !(1..=WASM_MODULE_MAX_BYTES as u64).contains(&self.manifest.artifact.byte_len)
        {
            return Err(ProtocolError::invalid(
                "invalid historical WASM start reference",
            ));
        }
        self.worker.validate()?;
        bounded_text(&self.label, 256, "WASM start reference label")?;
        // Exercise only the shared declaration shape, with one dummy byte; no executable bytes are stored or authorized.
        let mut manifest = self.manifest.clone();
        manifest.artifact.byte_len = 1;
        WasmProcessingRequest {
            manifest,
            module: vec![0],
            input: vec![],
            guest_memory_bytes: self.guest_memory_bytes,
        }
        .validate()?;
        if serde_json::to_vec(self)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            .len()
            > 16 * 1024
        {
            return Err(ProtocolError::invalid(
                "WASM start reference exceeds metadata admission",
            ));
        }
        Ok(())
    }
    pub fn matches(&self, other: &Self) -> Result<bool, ProtocolError> {
        Ok(
            serde_json::to_vec(self).map_err(|e| ProtocolError::invalid(e.to_string()))?
                == serde_json::to_vec(other).map_err(|e| ProtocolError::invalid(e.to_string()))?,
        )
    }
    /// Stable schema-1 metadata-derived replay ID. This is an identity, never a grant.
    pub fn resolution_request_id(&self, project: &ProjectId) -> Result<RequestId, ProtocolError> {
        use sha2::{Digest, Sha256};
        self.validate()?;
        let bytes = serde_json::to_vec(&("pulsar-wasm-start-resolution-v1", project, self))
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        RequestId::new(format!("wasm-resolve-{:x}", Sha256::digest(bytes)))
            .map_err(|e| ProtocolError::invalid(e.to_string()))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum WasmStartOutcome {
    Accepted { report: Box<WasmEditReport> },
    Withdrawn,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmStartResolution {
    pub project_id: ProjectId,
    pub reference: WasmStartReference,
    pub outcome: WasmStartOutcome,
}
impl WasmStartResolution {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.reference.validate()?;
        if let WasmStartOutcome::Accepted { report } = &self.outcome {
            report.validate()?;
            if report.project_id != self.project_id
                || report.request_id != self.reference.original_request_id
                || report.base_revision != self.reference.base_revision
            {
                return Err(ProtocolError::invalid(
                    "resolved start report scope differs",
                ));
            }
            if report.receipt.as_ref().is_some_and(|r| {
                r.module != self.reference.manifest.artifact || r.worker != self.reference.worker
            }) {
                return Err(ProtocolError::invalid(
                    "resolved start receipt identity differs",
                ));
            }
        }
        Ok(())
    }
    pub fn validate_for(&self, request: &Request) -> Result<(), ProtocolError> {
        self.validate()?;
        match &request.command {
            Command::ResolveWasmStart { reference }
                if request.project.as_ref() == Some(&self.project_id)
                    && self.reference.matches(reference)? =>
            {
                Ok(())
            }
            _ => Err(ProtocolError::invalid(
                "WASM start resolution request differs",
            )),
        }
    }
}
