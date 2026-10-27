//! Engine-owned one-shot inference. No session approval or application effects.
use crate::{
    resources::{Reservation, ResourcePool},
    worker_fence::WorkerFence,
    EngineConfig,
};
use pulsar_protocol::*;
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[path = "assistant_assets.rs"]
mod assets;

/// Trusted host input paths. Client requests must select issued identities;
/// this embedding API grants no permission to read arbitrary client paths.
#[derive(Clone, Debug)]
pub struct AssistantLaunchAssets {
    pub vision: Option<AssistantVisionAssets>,
    pub runtime_path: PathBuf,
    pub model_path: PathBuf,
    /// Cgroup process-tree RAM, including copied asset mounts and native state.
    pub worker_memory_bytes: u64,
}
/// Trusted host paths for exactly the request's projector and raw RGB identity.
#[derive(Clone, Debug)]
pub struct AssistantVisionAssets {
    pub projector_path: PathBuf,
    pub rgb_path: PathBuf,
}
#[derive(Clone, Debug)]
pub struct AssistantExecution {
    pub response: AssistantWorkerResponse,
    pub worker_identity: ArtifactIdentity,
    pub worker_peak_memory_bytes: u64,
}
fn failure(code: ErrorCode, error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(code, error.to_string())
}
fn check(deadline: Instant, cancel: &AtomicBool) -> Result<(), ProtocolError> {
    if cancel.load(Ordering::Acquire) {
        return Err(failure(ErrorCode::Cancelled, "assistant attempt cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(failure(
            ErrorCode::Unavailable,
            "assistant parent deadline exceeded",
        ));
    }
    Ok(())
}
fn validate_response(
    expected: &AssistantWorkerRequest,
    actual: &AssistantWorkerResponse,
) -> Result<(), ProtocolError> {
    if actual.version != expected.version
        || actual.attempt_id != expected.attempt_id
        || actual.runtime != expected.runtime
        || actual.model != expected.model
        || actual.vision != expected.vision
    {
        return Err(failure(
            ErrorCode::DependencyMismatch,
            "assistant response identity differs from original attempt",
        ));
    }
    let bytes = serde_json::to_vec(&actual.proposal)
        .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?;
    AssistantProposal::decode(&bytes, &expected.scope)?;
    Ok(())
}
fn same_image(image: &assets::Image, expected: &ArtifactIdentity) -> Result<(), ProtocolError> {
    if image.sha256 != expected.sha256 || image.byte_len != expected.byte_len {
        return Err(failure(
            ErrorCode::DependencyMismatch,
            "assistant sealed artifact differs from approved identity",
        ));
    }
    Ok(())
}

fn open_vision(
    request: &AssistantWorkerRequest,
    paths: &AssistantLaunchAssets,
) -> Result<Option<(assets::Source, assets::Source)>, ProtocolError> {
    match (&request.vision, &paths.vision) {
        (None, None) => Ok(None),
        (Some(binding), Some(paths)) => Ok(Some((
            assets::Source::open(&paths.projector_path, &binding.projector, false)?,
            assets::Source::open(&paths.rgb_path, &binding.rgb, false)?,
        ))),
        _ => Err(ProtocolError::invalid(
            "assistant vision mounts and request must agree",
        )),
    }
}
fn vision_bytes(vision: &Option<(assets::Source, assets::Source)>) -> u64 {
    vision
        .as_ref()
        .map_or(0, |(projector, rgb)| projector.byte_len() + rgb.byte_len())
}
fn capture_vision(
    vision: Option<(assets::Source, assets::Source)>,
    request: &AssistantWorkerRequest,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Option<(assets::Image, assets::Image)>, ProtocolError> {
    match (vision, &request.vision) {
        (None, None) => Ok(None),
        (Some((projector, rgb)), Some(binding)) => {
            let projector = projector.capture(deadline, cancel)?;
            same_image(&projector, &binding.projector)?;
            let rgb = rgb.capture(deadline, cancel)?;
            same_image(&rgb, &binding.rgb)?;
            Ok(Some((projector, rgb)))
        }
        _ => Err(ProtocolError::invalid(
            "assistant vision capture differs from request",
        )),
    }
}

pub(crate) struct Parent<'a> {
    pub config: &'a EngineConfig,
    pub pool: &'a ResourcePool,
    pub fence: Arc<WorkerFence>,
    pub worker_identity: &'a ArtifactIdentity,
    pub admission: Option<Reservation>,
}
pub(crate) fn admission_bytes(
    config: &EngineConfig,
    identity: &ArtifactIdentity,
    request: &AssistantWorkerRequest,
    paths: &AssistantLaunchAssets,
) -> Result<u64, ProtocolError> {
    validate_request(request)?;
    if !(512 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&paths.worker_memory_bytes) {
        return Err(ProtocolError::invalid(
            "assistant memory outside supported bounds",
        ));
    }
    let worker = assets::Source::open(&config.worker_executable, identity, true)?;
    let runtime = assets::Source::open(&paths.runtime_path, &request.runtime, true)?;
    let model = assets::Source::open(&paths.model_path, &request.model, false)?;
    let vision = open_vision(request, paths)?;
    let captured = worker
        .byte_len()
        .checked_add(runtime.byte_len())
        .and_then(|n| n.checked_add(model.byte_len()))
        .and_then(|n| n.checked_add(vision_bytes(&vision)))
        .ok_or_else(|| ProtocolError::invalid("assistant image accounting overflow"))?;
    if captured
        .checked_add(256 * 1024 * 1024)
        .is_none_or(|n| n > paths.worker_memory_bytes)
    {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "assistant child budget cannot hold asset mounts and minimum native state",
        ));
    }
    paths
        .worker_memory_bytes
        .checked_add(captured)
        .and_then(|n| n.checked_add(2 * 1024 * 1024))
        .ok_or_else(|| ProtocolError::invalid("assistant aggregate memory overflow"))
}

pub(crate) fn validate_request(request: &AssistantWorkerRequest) -> Result<(), ProtocolError> {
    request.validate()?;
    let bytes = serde_json::to_vec(request).map_err(|e| failure(ErrorCode::InvalidRequest, e))?;
    if bytes.len() as u64 > MAX_ASSISTANT_WORKER_REQUEST_BYTES {
        return Err(failure(
            ErrorCode::ResourceExhausted,
            "assistant encoded request exceeds transport cap",
        ));
    }
    Ok(())
}
pub(crate) fn execute(
    parent: Parent<'_>,
    request: &AssistantWorkerRequest,
    paths: &AssistantLaunchAssets,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<AssistantExecution, ProtocolError> {
    let Parent {
        config,
        pool,
        fence,
        worker_identity,
        admission,
    } = parent;
    validate_request(request)?;
    worker_identity.validate()?;
    check(deadline, cancel)?;
    if !crate::launch::available() {
        return Err(ProtocolError::unsupported(
            "assistant Linux confinement unavailable",
        ));
    }
    if !(512 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&paths.worker_memory_bytes) {
        return Err(ProtocolError::invalid(
            "assistant process-tree memory outside supported bounds",
        ));
    }
    let worker = assets::Source::open(&config.worker_executable, worker_identity, true)?;
    let runtime = assets::Source::open(&paths.runtime_path, &request.runtime, true)?;
    let model = assets::Source::open(&paths.model_path, &request.model, false)?;
    let vision = open_vision(request, paths)?;
    let captured_bytes = worker
        .byte_len()
        .checked_add(runtime.byte_len())
        .and_then(|n| n.checked_add(model.byte_len()))
        .and_then(|n| n.checked_add(vision_bytes(&vision)))
        .ok_or_else(|| ProtocolError::invalid("assistant image accounting overflow"))?;
    if captured_bytes
        .checked_add(256 * 1024 * 1024)
        .is_none_or(|minimum| minimum > paths.worker_memory_bytes)
    {
        return Err(failure(
            ErrorCode::ResourceExhausted,
            "assistant child budget cannot hold asset mounts and minimum native state",
        ));
    }
    // Host sealed pages coexist with the child RAM limit and bounded transport.
    let memory = paths
        .worker_memory_bytes
        .checked_add(captured_bytes)
        .and_then(|n| n.checked_add(2 * 1024 * 1024))
        .ok_or_else(|| ProtocolError::invalid("assistant aggregate memory overflow"))?;
    let reservation = Arc::new(match admission {
        Some(value) => {
            value.require_cpu_worker(pool, memory, request.cpu_threads)?;
            value
        }
        None => pool.reserve_worker(memory, request.cpu_threads)?,
    });
    let worker = worker.capture(deadline, cancel)?;
    same_image(&worker, worker_identity)?;
    let runtime = runtime.capture(deadline, cancel)?;
    same_image(&runtime, &request.runtime)?;
    let model = model.capture(deadline, cancel)?;
    same_image(&model, &request.model)?;
    let vision = capture_vision(vision, request, deadline, cancel)?;
    check(deadline, cancel)?;
    let mut custody = fence
        .admit_group(reservation, paths.worker_memory_bytes, request.cpu_threads)
        .map_err(|e| failure(ErrorCode::Unavailable, e))?
        .into_worker();
    let group = custody.group_handle();
    let budget = ResourceBudget {
        memory_bytes: paths.worker_memory_bytes,
        output_bytes: 16 * 1024 * 1024,
        cpu_threads: request.cpu_threads,
        wall_time_ms: request.wall_time_ms,
    };
    let address_space = paths
        .worker_memory_bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(128 * 1024 * 1024))
        .ok_or_else(|| ProtocolError::invalid("assistant address-space cap overflow"))?;
    // Install every owner before fallible command preparation/inheritance/spawn.
    // Unconfirmed retirement keeps all images and the full admission claim.
    let images = custody.retain_asset((worker, runtime, model, vision));
    let mut command = crate::confinement::assistant_command(
        config,
        crate::confinement::ConfinementLimits {
            budget: &budget,
            address_space_bytes: address_space,
        },
        &group,
        (images.0.fd(), images.0.byte_len),
        (images.1.fd(), images.1.byte_len),
        (images.2.fd(), images.2.byte_len),
        images.3.as_ref().map(|(projector, rgb)| {
            (
                (projector.fd(), projector.byte_len),
                (rgb.fd(), rgb.byte_len),
            )
        }),
    )
    .map_err(|e| failure(ErrorCode::Unavailable, e))?;
    images.0.inherit(&mut command)?;
    images.1.inherit(&mut command)?;
    images.2.inherit(&mut command)?;
    if let Some((projector, rgb)) = &images.3 {
        projector.inherit(&mut command)?;
        rgb.inherit(&mut command)?;
    }
    check(deadline, cancel)?;
    let mut dispatched = request.clone();
    dispatched.wall_time_ms = u64::try_from(
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis(),
    )
    .map_err(|_| ProtocolError::invalid("assistant remaining deadline overflow"))?
    .min(request.wall_time_ms);
    dispatched.validate()?;
    let encoded =
        serde_json::to_vec(&dispatched).map_err(|e| failure(ErrorCode::InvalidRequest, e))?;
    if encoded.len() as u64 > MAX_ASSISTANT_WORKER_REQUEST_BYTES {
        return Err(failure(
            ErrorCode::ResourceExhausted,
            "assistant encoded request exceeds transport cap",
        ));
    }
    check(deadline, cancel)?;
    custody
        .spawn(&mut command)
        .map_err(|e| failure(ErrorCode::Unavailable, e))?;
    let transport = (|| -> Result<AssistantWorkerResponse, ProtocolError> {
        let (mut stdin, mut stdout, stderr) =
            {
                let mut child = custody.child();
                (
                    child.stdin.take().ok_or_else(|| {
                        failure(ErrorCode::Internal, "assistant stdin unavailable")
                    })?,
                    child.stdout.take().ok_or_else(|| {
                        failure(ErrorCode::Internal, "assistant stdout unavailable")
                    })?,
                    child.stderr.take().ok_or_else(|| {
                        failure(ErrorCode::Internal, "assistant stderr unavailable")
                    })?,
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
        drop(stdin);
        let mut bytes = Vec::new();
        monitor
            .read(&mut stdout)
            .take(MAX_ASSISTANT_WORKER_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| failure(ErrorCode::Unavailable, e))?;
        if bytes.len() as u64 > MAX_ASSISTANT_WORKER_RESPONSE_BYTES {
            return Err(failure(
                ErrorCode::ResourceExhausted,
                "assistant response exceeded byte cap",
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
                format!("assistant worker exited {status}"),
            ));
        }
        let response: AssistantWorkerResponse = serde_json::from_slice(&bytes)
            .map_err(|e| failure(ErrorCode::DependencyMismatch, e))?;
        validate_response(request, &response)?;
        Ok(response)
    })();
    let retirement = if transport.is_ok() {
        deadline
    } else {
        Instant::now() + Duration::from_secs(2)
    };
    let retired = custody
        .retire(retirement)
        .map_err(|e| failure(ErrorCode::Unavailable, e));
    match transport {
        Err(error) => {
            check(deadline, cancel)?;
            Err(error)
        }
        Ok(response) => {
            retired?;
            check(deadline, cancel)?;
            let peak = group
                .peak_memory_bytes()
                .map_err(|e| failure(ErrorCode::Unavailable, e))?;
            Ok(AssistantExecution {
                response,
                worker_identity: worker_identity.clone(),
                worker_peak_memory_bytes: peak,
            })
        }
    }
}
#[cfg(test)]
#[path = "assistant_launch_tests.rs"]
mod tests;
