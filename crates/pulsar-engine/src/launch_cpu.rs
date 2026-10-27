//! Captured CPU execution through the common bounded supervision and retirement path.
#![cfg(target_os = "linux")]
use crate::{
    cpu_mounts::OwnedCpuMountCommand,
    launch::{self, AttemptReport, LaunchExecution},
    native_capture::SupervisedNativeDependencies,
    resources::Reservation,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_reserved(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    image: SupervisedNativeDependencies,
    descriptor: CpuRuntimeImage,
    reservation: Arc<Reservation>,
    fence: Arc<WorkerFence>,
) -> Result<LaunchExecution, ProtocolError> {
    execute_reserved_attempt(
        config,
        request,
        cancel,
        image,
        descriptor,
        reservation,
        fence,
    )
    .into_result()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_reserved_attempt(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    image: SupervisedNativeDependencies,
    descriptor: CpuRuntimeImage,
    reservation: Arc<Reservation>,
    fence: Arc<WorkerFence>,
) -> AttemptReport {
    // Capture has its own preparation lease. This execution budget begins here,
    // before request validation, cgroup admission, mount setup or worker intake.
    let started = Instant::now();
    prepare_and_execute(
        config,
        request,
        cancel,
        image,
        descriptor,
        reservation,
        fence,
        started,
        None,
    )
    .unwrap_or_else(AttemptReport::host_failure)
}

/// Apply an already-issued permission deadline without renewing it during
/// capture consumption, mount setup, bootstrap or worker execution.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_reserved_attempt_before(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    image: SupervisedNativeDependencies,
    descriptor: CpuRuntimeImage,
    reservation: Arc<Reservation>,
    fence: Arc<WorkerFence>,
    deadline: Instant,
) -> AttemptReport {
    let started = Instant::now();
    prepare_and_execute(
        config,
        request,
        cancel,
        image,
        descriptor,
        reservation,
        fence,
        started,
        Some(deadline),
    )
    .unwrap_or_else(AttemptReport::host_failure)
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
        .ok_or_else(|| ProtocolError::invalid("closed CPU worker wall deadline overflow"))?;
    let deadline = retained.map_or(budget_end, |retained| retained.min(budget_end));
    if Instant::now() >= deadline {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "closed CPU execution deadline expired before preparation",
        ));
    }
    Ok(deadline)
}

fn require_cpu_request(request: &WorkerRequest) -> Result<(), ProtocolError> {
    request.validate()?;
    let explicit = match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
    }
    .ok_or_else(|| {
        ProtocolError::invalid("closed CPU launch requires an explicit inference profile")
    })?;
    if explicit.provider != InferenceProvider::Cpu || request.cuda_execution.is_some() {
        return Err(ProtocolError::invalid(
            "closed CPU launch cannot substitute a provider",
        ));
    }
    crate::inference_profiles::resolve_profile(request).map_err(|error| {
        ProtocolError::invalid(format!("closed CPU profile refused: {error:#}"))
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn prepare_and_execute(
    config: &EngineConfig,
    request: &WorkerRequest,
    cancel: &AtomicBool,
    image: SupervisedNativeDependencies,
    descriptor: CpuRuntimeImage,
    reservation: Arc<Reservation>,
    fence: Arc<WorkerFence>,
    started: Instant,
    retained_deadline: Option<Instant>,
) -> Result<AttemptReport, ProtocolError> {
    let dependency =
        |error| ProtocolError::new(ErrorCode::DependencyMismatch, format!("{error:#}"));
    if cancel.load(Ordering::Acquire) {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "closed CPU attempt cancelled before launch",
        ));
    }
    require_cpu_request(request)?;
    let deadline = execution_deadline(started, request.budget.wall_time_ms, retained_deadline)?;
    let progress = if crate::generation_progress::eligible(request) {
        Some(crate::generation_progress::envelope(request).map_err(dependency)?)
    } else {
        None
    };
    let custody = fence
        .admit_group(
            reservation,
            request.budget.memory_bytes,
            request.budget.cpu_threads,
        )
        .map_err(|error| ProtocolError::new(ErrorCode::Unavailable, error.to_string()))?
        .into_worker();
    // The mount owner retains capture storage and descriptors before any
    // fallible setup; it also verifies and remaps the original resume member.
    let plan = OwnedCpuMountCommand::prepare(
        config, request, image, descriptor, custody, progress, deadline,
    )
    .map_err(dependency)?;
    let (command, custody, witness) = plan.into_parts();
    Ok(launch::supervise_owned_attempt_with_progress(
        request,
        &witness,
        cancel,
        custody,
        command,
        deadline,
        crate::generation_progress::eligible(request),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_permission_deadline_caps_but_never_renews_the_execution_budget() {
        let started = Instant::now();
        let short = started + Duration::from_secs(1);
        let long = started + Duration::from_secs(60);
        assert_eq!(
            execution_deadline(started, 30_000, Some(short)).unwrap(),
            short
        );
        assert_eq!(
            execution_deadline(started, 2_000, Some(long)).unwrap(),
            started + Duration::from_secs(2)
        );
        assert_eq!(
            execution_deadline(started, 2_000, None).unwrap(),
            started + Duration::from_secs(2)
        );
        let expired = started.checked_sub(Duration::from_secs(1)).unwrap();
        assert!(execution_deadline(started, 30_000, Some(expired)).is_err());
        assert!(execution_deadline(expired, 1, Some(long)).is_err());
        assert!(execution_deadline(started, 0, None).is_err());
        assert!(execution_deadline(started, 86_400_001, None).is_err());
    }

    fn cpu_request() -> WorkerRequest {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.cuda_execution = None;
        let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
            unreachable!()
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        request
    }

    #[test]
    fn launch_refuses_implicit_or_non_cpu_selection_before_resources_are_admitted() {
        let mut request = cpu_request();
        require_cpu_request(&request).unwrap();
        let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
            unreachable!()
        };
        settings.inference_profile = None;
        assert!(require_cpu_request(&request).is_err());
        let cuda: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        assert!(require_cpu_request(&cuda).is_err());
        let mut request = cpu_request();
        request.operation = WorkerOperation::ProbeMedia;
        assert!(require_cpu_request(&request).is_err());
    }

    #[test]
    fn launch_preparation_error_cannot_claim_worker_retirement() {
        let report =
            AttemptReport::host_failure(ProtocolError::invalid("authored prelaunch refusal"));
        let (result, settled) = report.into_parts();
        assert!(result.is_err());
        assert!(settled.is_none());
    }
}
