//! Pure archive admission. It deliberately never invokes a compiler or worker.
use pulsar_core::{Axis, EditValuesProgram, MotionProgram, ProvenanceRef, TimeRange};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};

pub(crate) struct CheckedValues {
    pub authored_ranges: Vec<(Axis, TimeRange)>,
    pub inherited_ranges: Vec<(Axis, TimeRange)>,
    pub authored_provenance: ProvenanceRef,
}
pub(crate) fn validate(
    invocation: &PackageWasmInvocation,
    module: &[u8],
    input: &[u8],
    output: &[u8],
    base: &MotionProgram,
    result: &MotionProgram,
) -> Result<CheckedValues, ProtocolError> {
    invocation.validate()?;
    for (bytes, sha, len, max) in [
        (
            module,
            invocation.manifest.artifact.sha256.as_str(),
            invocation.manifest.artifact.byte_len,
            WASM_MODULE_MAX_BYTES,
        ),
        (
            input,
            invocation.receipt.input_sha256.as_str(),
            invocation.receipt.input_byte_len,
            WASM_BUFFER_MAX_BYTES,
        ),
        (
            output,
            invocation.receipt.output_sha256.as_str(),
            invocation.receipt.output_byte_len,
            WASM_BUFFER_MAX_BYTES,
        ),
    ] {
        if bytes.len() > max
            || bytes.len() as u64 != len
            || format!("{:x}", Sha256::digest(bytes)) != sha
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "archived WASM buffer identity differs",
            ));
        }
    }
    let input: EditValuesProgram = serde_json::from_slice(input)
        .map_err(|_| ProtocolError::invalid("WASM original input is not values-only"))?;
    let expected = EditValuesProgram::from_program_values(base)
        .map_err(|_| ProtocolError::invalid("invalid WASM original base"))?;
    if input != expected {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "WASM input differs from original revision values",
        ));
    }
    let output: EditValuesProgram = serde_json::from_slice(output)
        .map_err(|_| ProtocolError::invalid("WASM original output is not values-only"))?;
    let provenance = ProvenanceRef::new(format!("edit:{}", invocation.attempt_id))
        .map_err(|_| ProtocolError::invalid("invalid WASM authored provenance"))?;
    let recomputed = pulsar_core::reconcile_edit_values(base, &output, &provenance)
        .map_err(|_| ProtocolError::invalid("WASM archived values cannot be reconciled"))?;
    if &recomputed.program != result {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "WASM result differs from the edit kernel",
        ));
    }
    Ok(CheckedValues {
        authored_ranges: recomputed.authored_ranges,
        inherited_ranges: recomputed.inherited_ranges,
        authored_provenance: recomputed.authored_provenance,
    })
}
