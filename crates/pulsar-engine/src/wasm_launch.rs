//! Engine-side Linux WASM supervision. No project mutation or grant authority.
use crate::{resources::ResourcePool, worker_fence::WorkerFence, EngineConfig};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[path = "wasm_executable.rs"]
mod executable;

pub(crate) const MAX_EXECUTABLE_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) fn executable_size_admitted(bytes: u64) -> bool {
    bytes > 0 && bytes <= MAX_EXECUTABLE_BYTES
}

pub(crate) struct Execution {
    pub output: WasmProcessingResult,
    pub worker_peak_memory_bytes: u64,
    pub worker_executable_sha256: String,
    pub worker_executable_bytes: u64,
}
fn failure(code: ErrorCode, error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(code, error.to_string())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn check(deadline: Instant, cancel: &AtomicBool) -> Result<(), ProtocolError> {
    if cancel.load(Ordering::Acquire) {
        return Err(failure(ErrorCode::Cancelled, "WASM attempt cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(failure(
            ErrorCode::Unavailable,
            "WASM whole-worker deadline exceeded",
        ));
    }
    Ok(())
}

/// The caller supplies the Engine's shared pool and durable fence. This function
/// grants no command scope and publishes no artifacts or edits. Authenticated
/// dispatch and late-result grant/revision admission remain the caller's work.
pub(crate) fn execute(
    config: &EngineConfig,
    pool: &ResourcePool,
    fence: Arc<WorkerFence>,
    envelope: &WasmWorkerRequest,
    expected_worker: &ArtifactIdentity,
    cancel: &AtomicBool,
) -> Result<Execution, ProtocolError> {
    let started = Instant::now();
    envelope.validate()?;
    expected_worker.validate()?;
    let request = &envelope.request;
    if digest(&request.module) != request.manifest.artifact.sha256 {
        return Err(failure(
            ErrorCode::DependencyMismatch,
            "captured WASM module bytes changed",
        ));
    }
    let deadline = started
        .checked_add(Duration::from_millis(request.manifest.budget.wall_time_ms))
        .ok_or_else(|| ProtocolError::invalid("WASM deadline overflow"))?;
    check(deadline, cancel)?;
    if !crate::launch::available() {
        return Err(ProtocolError::unsupported(
            "Linux WASM worker confinement unavailable",
        ));
    }
    let executable_source = executable::Source::open(&config.worker_executable)?;
    // Reserve the sealed executable, native child and parent transport separately.
    let memory = request
        .manifest
        .budget
        .memory_bytes
        .checked_add(16 * 1024 * 1024)
        .and_then(|n| n.checked_add(executable_source.byte_len()))
        .ok_or_else(|| ProtocolError::invalid("WASM reservation overflow"))?;
    let reservation = Arc::new(pool.reserve_worker(memory, request.manifest.budget.cpu_threads)?);
    let image = executable_source.capture(deadline, cancel)?;
    if image.sha256 != expected_worker.sha256 || image.byte_len != expected_worker.byte_len {
        return Err(failure(
            ErrorCode::DependencyMismatch,
            "WASM native worker differs from the approved build",
        ));
    }
    let worker_executable_sha256 = image.sha256.clone();
    let worker_executable_bytes = image.byte_len;
    let mut encoded = Vec::new();
    serde_json::to_writer(&mut encoded, envelope)
        .map_err(|e| failure(ErrorCode::InvalidRequest, e))?;
    if encoded.len() as u64 > WASM_REQUEST_MAX_BYTES {
        return Err(failure(
            ErrorCode::ResourceExhausted,
            "encoded WASM request exceeds transport budget",
        ));
    }
    check(deadline, cancel)?;
    let mut custody = fence
        .admit_group(
            reservation,
            request.manifest.budget.memory_bytes,
            request.manifest.budget.cpu_threads,
        )
        .map_err(|e| failure(ErrorCode::Unavailable, e))?
        .into_worker();
    let group = custody.group_handle();
    // Wasmtime uses a bounded 16 MiB reservation, rather than the default wasm32
    // multi-GiB reservation. Physical memory is independently capped by cgroup.
    let address_space = request
        .manifest
        .budget
        .memory_bytes
        .checked_mul(4)
        .and_then(|n| n.checked_add(128 * 1024 * 1024))
        .ok_or_else(|| ProtocolError::invalid("WASM address-space budget overflow"))?;
    let mut command = crate::confinement::wasm_command(
        config,
        &request.manifest.budget,
        address_space,
        &group,
        image.fd(),
        image.byte_len,
    )
    .map_err(|e| failure(ErrorCode::Unavailable, e))?;
    // Both the descriptor and its full memory reservation follow worker custody,
    // including unresolved retirement handed to the reaper.
    custody.retain_asset(image).inherit(&mut command)?;
    check(deadline, cancel)?;
    custody
        .spawn(&mut command)
        .map_err(|e| failure(ErrorCode::Unavailable, e))?;
    let transport = (|| -> Result<_, ProtocolError> {
        let (mut stdin, mut stdout, stderr) = {
            let mut child = custody.child();
            (
                child
                    .stdin
                    .take()
                    .ok_or_else(|| failure(ErrorCode::Internal, "WASM stdin missing"))?,
                child
                    .stdout
                    .take()
                    .ok_or_else(|| failure(ErrorCode::Internal, "WASM stdout missing"))?,
                child
                    .stderr
                    .take()
                    .ok_or_else(|| failure(ErrorCode::Internal, "WASM stderr missing"))?,
            )
        };
        crate::launch_pipes::nonblocking(&stdin).map_err(|e| failure(ErrorCode::Unavailable, e))?;
        crate::launch_pipes::nonblocking(&stdout)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        crate::launch_pipes::nonblocking(&stderr)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        let mut monitor = crate::launch_pipes::Monitor::new(deadline, cancel, stderr);
        monitor
            .write(&mut stdin)
            .write_all(&encoded)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        drop(stdin); // EOF is part of this one-request protocol, not a second request.
        let mut bytes = Vec::new();
        monitor
            .read(&mut stdout)
            .take(WASM_RESPONSE_MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        if bytes.len() as u64 > WASM_RESPONSE_MAX_BYTES {
            return Err(failure(
                ErrorCode::ResourceExhausted,
                "WASM response exceeds transport budget",
            ));
        }
        let status = loop {
            monitor
                .tick()
                .map_err(|e| failure(ErrorCode::Unavailable, e))?;
            if let Some(status) = custody
                .child()
                .try_wait()
                .map_err(|e| failure(ErrorCode::Unavailable, e))?
            {
                break status;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        monitor
            .check()
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        if !status.success() {
            return Err(failure(
                ErrorCode::Unavailable,
                format!(
                    "WASM worker exited {status}: {}",
                    String::from_utf8_lossy(monitor.diagnostics())
                ),
            ));
        }
        let response: WasmWorkerResponse = serde_json::from_slice(&bytes)
            .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?;
        validate_response(envelope, &response)?;
        Ok(response.output)
    })();
    // Never return success from a direct-child exit alone. On error, ownership
    // stays with the same custody/reaper if retirement cannot yet be confirmed.
    let retirement = match &transport {
        Ok(_) => deadline,
        Err(_) => Instant::now() + Duration::from_secs(2),
    };
    let retired = custody
        .retire(retirement)
        .map_err(|e| failure(ErrorCode::Unavailable, e));
    if let Err(error) = transport {
        if cancel.load(Ordering::Acquire) {
            return Err(failure(ErrorCode::Cancelled, "WASM attempt cancelled"));
        }
        if Instant::now() >= deadline {
            return Err(failure(
                ErrorCode::Unavailable,
                "WASM whole-worker deadline exceeded",
            ));
        }
        return Err(error);
    }
    retired?;
    check(deadline, cancel)?;
    let peak = group
        .peak_memory_bytes()
        .map_err(|e| failure(ErrorCode::Unavailable, e))?;
    Ok(Execution {
        output: transport.expect("checked WASM transport result"),
        worker_peak_memory_bytes: peak,
        worker_executable_sha256,
        worker_executable_bytes,
    })
}

fn validate_response(
    expected: &WasmWorkerRequest,
    actual: &WasmWorkerResponse,
) -> Result<(), ProtocolError> {
    let output = &actual.output;
    if actual.version != PROTOCOL_VERSION
        || actual.attempt_id != expected.attempt_id
        || output.abi != WASM_PROCESSING_ABI
        || output.module_sha256 != expected.request.manifest.artifact.sha256
        || output.input_sha256 != digest(&expected.request.input)
        || output.output.len() as u64 > expected.request.manifest.budget.output_bytes
        || output.output_sha256 != digest(&output.output)
        || output.fuel_consumed > WASM_FUEL_LIMIT
    {
        return Err(failure(
            ErrorCode::DependencyMismatch,
            "WASM response identity, bytes or limits do not match admitted attempt",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "wasm_launch_tests.rs"]
mod tests;
