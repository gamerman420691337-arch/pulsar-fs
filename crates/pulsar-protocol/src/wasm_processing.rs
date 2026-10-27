//! Data-only worker ABI. These declarations do not authorize engine effects.
use crate::{ConfinementRequirement, ExtensionManifest, ExtensionRole, ProtocolError};
use serde::{Deserialize, Serialize};

pub const WASM_PROCESSING_ABI: &str = "pulsar.buffer.v1";
pub const WASM_MODULE_MAX_BYTES: usize = 512 * 1024;
pub const WASM_BUFFER_MAX_BYTES: usize = 64 * 1024;
pub const WASM_REQUEST_MAX_BYTES: u64 = 4 * 1024 * 1024;
pub const WASM_FUEL_LIMIT: u64 = 1_000_000;
pub const WASM_RESPONSE_MAX_BYTES: u64 = (WASM_BUFFER_MAX_BYTES as u64) * 4 + 4096;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmWorkerRequest {
    pub version: u16,
    pub attempt_id: pulsar_core::AttemptId,
    pub request: WasmProcessingRequest,
}
impl WasmWorkerRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != crate::PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("WASM worker protocol version"));
        }
        self.request.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmWorkerResponse {
    pub version: u16,
    pub attempt_id: pulsar_core::AttemptId,
    pub output: WasmProcessingResult,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmProcessingRequest {
    pub manifest: ExtensionManifest,
    pub module: Vec<u8>,
    pub input: Vec<u8>,
    pub guest_memory_bytes: u64,
}

impl WasmProcessingRequest {
    /// Also enforced by the worker, before compilation. Byte identity and native
    /// process confinement require separate effectful checks.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.manifest.validate()?;
        if self.manifest.abi != WASM_PROCESSING_ABI
            || !matches!(
                self.manifest.role,
                ExtensionRole::Processor | ExtensionRole::Editor
            )
            || !matches!(self.manifest.confinement, ConfinementRequirement::Wasm)
            || !self.manifest.requested_scopes.is_empty()
        {
            return Err(ProtocolError::invalid(
                "data-only WASM ABI requires a processor/editor with no effect scopes",
            ));
        }
        if self.module.is_empty()
            || self.module.len() > WASM_MODULE_MAX_BYTES
            || self.manifest.artifact.byte_len != self.module.len() as u64
            || self.input.len() > WASM_BUFFER_MAX_BYTES
        {
            return Err(ProtocolError::invalid(
                "WASM module or input exceeds buffer admission",
            ));
        }
        let b = &self.manifest.budget;
        if !(64 * 1024 * 1024..=512 * 1024 * 1024).contains(&b.memory_bytes)
            || !(65536..=16 * 1024 * 1024).contains(&self.guest_memory_bytes)
            || self.guest_memory_bytes > b.memory_bytes
            || b.output_bytes > WASM_BUFFER_MAX_BYTES as u64
            || b.wall_time_ms > 5000
            || b.cpu_threads != 2
        {
            return Err(ProtocolError::invalid(
                "WASM worker budget exceeds data-only admission",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmProcessingResult {
    pub abi: String,
    pub module_sha256: String,
    pub input_sha256: String,
    pub output_sha256: String,
    pub output: Vec<u8>,
    pub fuel_consumed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArtifactIdentity, ResourceBudget, Scope, PROTOCOL_VERSION};
    use pulsar_core::ArtifactId;

    fn request() -> WasmProcessingRequest {
        WasmProcessingRequest {
            manifest: ExtensionManifest {
                schema_version: PROTOCOL_VERSION,
                name: "buffer processor".into(),
                role: ExtensionRole::Processor,
                abi: WASM_PROCESSING_ABI.into(),
                artifact: ArtifactIdentity {
                    artifact_id: ArtifactId::new("module").unwrap(),
                    sha256: "a".repeat(64),
                    byte_len: 8,
                },
                requested_scopes: vec![],
                budget: ResourceBudget {
                    memory_bytes: 64 * 1024 * 1024,
                    output_bytes: 1,
                    wall_time_ms: 5000,
                    cpu_threads: 2,
                },
                confinement: ConfinementRequirement::Wasm,
            },
            module: b"\0asm\x01\0\0\0".to_vec(),
            input: vec![],
            guest_memory_bytes: 65536,
        }
    }
    #[test]
    fn buffer_budgets_are_bounded_before_compilation() {
        let r = request();
        assert!(r.validate().is_ok());
        for field in [
            "memory_low",
            "memory_high",
            "worker_memory_low",
            "worker_memory_high",
            "output",
            "wall",
            "threads",
            "module",
            "input",
            "identity_length",
        ] {
            let mut bad = r.clone();
            match field {
                "memory_low" => bad.guest_memory_bytes = 65535,
                "memory_high" => bad.guest_memory_bytes = 16 * 1024 * 1024 + 1,
                "worker_memory_low" => bad.manifest.budget.memory_bytes = 64 * 1024 * 1024 - 1,
                "worker_memory_high" => bad.manifest.budget.memory_bytes = 512 * 1024 * 1024 + 1,
                "output" => bad.manifest.budget.output_bytes = WASM_BUFFER_MAX_BYTES as u64 + 1,
                "wall" => bad.manifest.budget.wall_time_ms = 5001,
                "threads" => bad.manifest.budget.cpu_threads = 1,
                "module" => {
                    bad.module.resize(WASM_MODULE_MAX_BYTES + 1, 0);
                    bad.manifest.artifact.byte_len = bad.module.len() as u64;
                }
                "input" => bad.input.resize(WASM_BUFFER_MAX_BYTES + 1, 0),
                _ => bad.manifest.artifact.byte_len += 1,
            }
            assert!(bad.validate().is_err(), "{field}");
        }
    }
    #[test]
    fn role_confinement_and_scopes_are_specific_to_this_abi() {
        let r = request();
        let mut editor = r.clone();
        editor.manifest.role = ExtensionRole::Editor;
        assert!(editor.validate().is_ok());
        for role in [ExtensionRole::Runtime, ExtensionRole::Driver] {
            let mut bad = r.clone();
            bad.manifest.role = role;
            assert!(bad.validate().is_err());
        }
        let mut bad = r.clone();
        bad.manifest.confinement = ConfinementRequirement::NativeSandbox;
        assert!(bad.validate().is_err());
        let mut bad = r.clone();
        bad.manifest.requested_scopes = vec![Scope::Read];
        assert!(bad.validate().is_err());
        let mut bad = r;
        bad.manifest.abi = "pulsar.buffer.v2".into();
        assert!(bad.validate().is_err());
    }
    #[test]
    fn transport_does_not_accept_an_invented_authorization_field() {
        let mut value = serde_json::to_value(request()).unwrap();
        value["approved"] = serde_json::json!(true);
        assert!(serde_json::from_value::<WasmProcessingRequest>(value).is_err());
    }
    #[test]
    fn worker_envelope_requires_a_version_and_an_attempt() {
        let mut envelope = WasmWorkerRequest {
            version: crate::PROTOCOL_VERSION,
            attempt_id: pulsar_core::AttemptId::new("attempt").unwrap(),
            request: request(),
        };
        envelope.validate().unwrap();
        envelope.version += 1;
        assert!(envelope.validate().is_err());
        assert!(serde_json::from_value::<WasmWorkerRequest>(
            serde_json::to_value(request()).unwrap()
        )
        .is_err());
        let mut value = serde_json::to_value(envelope).unwrap();
        value["approved"] = serde_json::json!(true);
        assert!(serde_json::from_value::<WasmWorkerRequest>(value).is_err());
    }
    #[test]
    fn worst_case_output_fits_the_separate_response_transport_bound() {
        let output = WasmProcessingResult {
            abi: WASM_PROCESSING_ABI.into(),
            module_sha256: "a".repeat(64),
            input_sha256: "b".repeat(64),
            output_sha256: "c".repeat(64),
            output: vec![255; WASM_BUFFER_MAX_BYTES],
            fuel_consumed: WASM_FUEL_LIMIT,
        };
        let response = WasmWorkerResponse {
            version: crate::PROTOCOL_VERSION,
            attempt_id: pulsar_core::AttemptId::new("attempt").unwrap(),
            output,
        };
        let bytes = serde_json::to_vec(&response).unwrap();
        assert!(bytes.len() as u64 <= WASM_RESPONSE_MAX_BYTES);
        assert_eq!(
            serde_json::from_slice::<WasmWorkerResponse>(&bytes)
                .unwrap()
                .output
                .output
                .len(),
            WASM_BUFFER_MAX_BYTES
        );
    }
}
