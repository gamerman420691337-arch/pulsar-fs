//! Owned CUDA inference dispatch through shared bounded worker supervision.
use crate::{
    cuda_memory::AdmittedCudaImage,
    cuda_mounts::OwnedCudaMountCommand,
    launch::{self, AttemptReport, LaunchExecution},
    worker_fence::WorkerFence,
    EngineConfig,
};
use pulsar_protocol::*;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub(crate) fn execute_reserved(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    admitted: AdmittedCudaImage,
    fence: Arc<WorkerFence>,
) -> Result<LaunchExecution, ProtocolError> {
    execute_reserved_attempt(config, request, cancel, admitted, fence).into_result()
}

pub(crate) fn execute_reserved_attempt(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    admitted: AdmittedCudaImage,
    fence: Arc<WorkerFence>,
) -> AttemptReport {
    prepare_and_execute(config, request, cancel, admitted, fence, None)
        .unwrap_or_else(AttemptReport::host_failure)
}

/// Preserve the caller's absolute permission deadline across admission and
/// supervision. The function-entry worker budget can only shorten that cap.
pub(crate) fn execute_reserved_attempt_before(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    admitted: AdmittedCudaImage,
    fence: Arc<WorkerFence>,
    deadline: Instant,
) -> AttemptReport {
    let started = Instant::now();
    let result = execution_deadline(started, request.budget.wall_time_ms, Some(deadline)).and_then(
        |deadline| prepare_and_execute(config, request, cancel, admitted, fence, Some(deadline)),
    );
    result.unwrap_or_else(AttemptReport::host_failure)
}

fn execution_deadline(
    started: Instant,
    budget_ms: u64,
    retained: Option<Instant>,
) -> Result<Instant, ProtocolError> {
    if !(1..=86_400_000).contains(&budget_ms) {
        return Err(ProtocolError::invalid(
            "worker execution deadline must be at most one day",
        ));
    }
    let budget_end = started
        .checked_add(Duration::from_millis(budget_ms))
        .ok_or_else(|| ProtocolError::invalid("CUDA worker wall deadline overflow"))?;
    let deadline = retained.map_or(budget_end, |retained| retained.min(budget_end));
    if Instant::now() >= deadline {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "CUDA execution deadline expired before preparation",
        ));
    }
    Ok(deadline)
}

fn prepare_and_execute(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    admitted: AdmittedCudaImage,
    fence: Arc<WorkerFence>,
    retained_deadline: Option<Instant>,
) -> Result<AttemptReport, ProtocolError> {
    let fail = |error| ProtocolError::new(ErrorCode::DependencyMismatch, format!("{error:#}"));
    if cancel.load(Ordering::Acquire) {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "CUDA attempt cancelled before launch",
        ));
    }
    if !cfg!(feature = "cuda") {
        return Err(ProtocolError::unsupported(
            "CUDA inference requires a CUDA-enabled worker build",
        ));
    }
    request.validate()?;
    let deadline = execution_deadline(
        Instant::now(),
        request.budget.wall_time_ms,
        retained_deadline,
    )?;
    let (image, devices, reservation) = admitted.into_launch_parts(request).map_err(fail)?;
    let custody = fence
        .admit_group(
            reservation,
            request.budget.memory_bytes,
            request.budget.cpu_threads,
        )
        .map_err(|e| ProtocolError::new(ErrorCode::Unavailable, e.to_string()))?
        .into_worker();
    let plan =
        OwnedCudaMountCommand::inference(config, request, image, devices, custody).map_err(fail)?;
    let (command, custody, mut witness) = plan.into_parts();
    if retained_deadline.is_some() {
        witness.execution_deadline_ns = Some(
            crate::cuda_bootstrap::deadline_ns_in_execution(deadline, request.budget.wall_time_ms)
                .map_err(fail)?,
        );
    }
    Ok(launch::supervise_owned_attempt(
        request, &witness, cancel, custody, command, deadline,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_cuda_permission_deadline_survives_later_preparation_checks() {
        let started = Instant::now();
        let retained = started + Duration::from_secs(1);
        let capped = execution_deadline(started, 30_000, Some(retained)).unwrap();
        assert_eq!(capped, retained);
        assert_eq!(
            execution_deadline(Instant::now(), 30_000, Some(capped)).unwrap(),
            retained
        );
        let later_permission = started + Duration::from_secs(60);
        assert_eq!(
            execution_deadline(started, 2_000, Some(later_permission)).unwrap(),
            started + Duration::from_secs(2)
        );
        let expired = started.checked_sub(Duration::from_secs(1)).unwrap();
        assert!(execution_deadline(started, 30_000, Some(expired)).is_err());
        assert!(execution_deadline(expired, 1, Some(later_permission)).is_err());
        assert!(execution_deadline(started, 0, None).is_err());
        assert!(execution_deadline(started, 86_400_001, None).is_err());
    }
}
