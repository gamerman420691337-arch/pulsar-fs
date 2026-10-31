//! Captured CPU preparation stays inside the original live capture lifetime.
use super::*;
use crate::{
    live_cpu_mounts::{LiveCpuMountInputs, OwnedLiveCpuMountCommand},
    live_frames::LiveRgbScope,
    worker_retirement::RetirementAssets,
};
use std::os::unix::fs::OpenOptionsExt;

pub(super) fn declaration(
    engine: &Engine,
    stream: &LiveInferenceManifest,
) -> PResult<Option<Box<LiveCpuInferenceManifest>>> {
    if stream.profile.provider != InferenceProvider::Cpu {
        return Ok(None);
    }
    let Some(owner) = &engine.cpu_owner_policy else {
        crate::live_inference::require_host_cpu_runtime(&engine.config)?;
        return Ok(None);
    };
    owner.match_profile(&stream.profile).map_err(unavailable)?;
    crate::inference_profiles::admit_tensor_adapter(&stream.profile, &stream.budget)
        .map_err(unavailable)?;
    let mut guest = stream.clone();
    guest.model.path = LIVE_CPU_MODEL_PATH.into();
    guest.runtime.path = LIVE_CPU_RUNTIME_PATH.into();
    Ok(Some(Box::new(LiveCpuInferenceManifest::new(
        guest,
        owner.image().map_err(unavailable)?,
    )?)))
}

struct Admission {
    manifest: Box<LiveCpuInferenceManifest>,
    scope: LiveRgbScope,
}

fn changed(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}

/// Re-read original ownership, current grants and the exact registry instance.
/// Matching serialized metadata alone cannot replace the minted RGB scope.
fn admission(
    engine: &Engine,
    stream: &LiveInferenceManifest,
    cancel: &Arc<AtomicBool>,
    origin: Instant,
    lease_end: Instant,
    previous: Option<&Admission>,
) -> PResult<Admission> {
    let observed_fallback =
        fallback::recheck_io(engine, &stream.identity.epoch, &stream.attempt_id)?;
    let db = engine.db.lock().map_err(internal)?;
    engine.require_worker_fence(&db)?;
    let mut registry = engine.live_captures.lock().map_err(internal)?;
    registry.sweep(&db)?;
    let entry = active(&mut registry, &stream.identity.epoch, &stream.attempt_id)
        .ok_or_else(|| changed("captured CPU original capture is retired"))?;
    let info = entry.report.inference.as_ref().expect("active inference");
    let options = &entry.report.options;
    stream.validate_browser_source(&entry.report.engine_epoch, options)?;
    fallback::require_bound(&db, entry, stream, observed_fallback.as_ref())?;
    let selected = options
        .inference
        .as_ref()
        .ok_or_else(|| changed("captured CPU inference declaration is absent"))?;
    // Match the actual held admission, including its original fallback cap.
    // An arbitrary earlier or later caller deadline is still refused.
    let expected_end = entry
        .created
        .checked_add(Duration::from_millis(u64::from(options.lease_ms)))
        .map(|end| {
            entry
                .fallback
                .as_ref()
                .map_or(end, |held| held.end_cap(end))
        });
    if finishing(entry)
        || info.state != LiveCaptureInferenceState::Starting
        || entry.report.engine_epoch.as_str() != engine.engine_epoch
        || entry.report.identity != stream.identity
        || entry.report.clock != stream.clock
        || entry.created != origin
        || expected_end != Some(lease_end)
        || !Arc::ptr_eq(&entry.cancel, cancel)
        || cancel.load(Ordering::Acquire)
        || Instant::now() >= lease_end
        || stream.model.path != selected.model_path
        || serde_json::to_vec(&stream.profile).map_err(internal)?
            != serde_json::to_vec(&selected.profile).map_err(internal)?
        || stream.worker != engine.binary_identity
        || stream.profile.provider != InferenceProvider::Cpu
        || stream.budget.wall_time_ms != u64::from(options.lease_ms)
        || stream.budget.memory_bytes > engine.config.max_ram_bytes
        || Duration::from_millis(stream.budget.wall_time_ms) > engine.config.worker_timeout
    {
        return Err(changed(
            "captured CPU boot, scope, options or original lifetime differs",
        ));
    }
    if let Some(choice) = &options.model_pack {
        engine.require_current_pack_choice(&db, choice)?;
    }
    let manifest = info
        .cpu_manifest
        .clone()
        .ok_or_else(|| changed("captured CPU mode was not declared at admission"))?;
    let expected = declaration(engine, stream)?
        .ok_or_else(|| changed("captured CPU owner policy is absent"))?;
    if manifest.identity()? != expected.identity()? {
        return Err(changed("captured CPU image changed since admission"));
    }
    let scope = entry
        .runtime
        .as_ref()
        .expect("active runtime")
        .inference_scope();
    scope.validate_manifest(stream)?;
    if let Some(previous) = previous {
        previous.scope.validate_scope(&scope)?;
        if previous.manifest.identity()? != manifest.identity()? {
            return Err(changed("captured CPU preparation was rebound"));
        }
    }
    if cancel.load(Ordering::Acquire) || Instant::now() >= lease_end {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "captured CPU admission expired",
        ));
    }
    Ok(Admission { manifest, scope })
}

pub(super) fn start(
    engine: &Engine,
    stream: &LiveInferenceManifest,
    cancel: Arc<AtomicBool>,
    origin: Instant,
    lease_end: Instant,
    assets: RetirementAssets,
) -> PResult<LiveInferenceWorker> {
    let admitted = admission(engine, stream, &cancel, origin, lease_end, None)?;
    let deadline = crate::live_worker_lifetime::deadline(
        origin,
        lease_end,
        stream.budget.wall_time_ms,
        Instant::now(),
    )?;
    let owner = engine.cpu_owner_policy.as_ref().expect("admitted owner");
    let total = owner
        .policy()
        .native_dependencies
        .validate()
        .map_err(unavailable)?;
    let image = engine.native_image_reuse.capture_with(
        &owner.policy().native_dependencies,
        &cancel,
        deadline,
        || {
            crate::native_capture::capture_tracked_in_lease_with_assets(
                &engine.config,
                &engine.pool,
                &owner.policy().native_dependencies,
                crate::native_capture::CaptureLimits {
                    budget: crate::native_capture::preparation_budget(
                        total,
                        stream.budget.wall_time_ms,
                    )?,
                    address_space_bytes: 1536 << 20,
                },
                &cancel,
                engine.worker_fence.clone(),
                crate::native_capture::CaptureLease {
                    deadline,
                    retirement_assets: assets.clone(),
                },
            )
        },
    )?;
    if owner
        .captured_image_supervised(&image)
        .map_err(unavailable)?
        .identity()?
        != admitted.manifest.image.identity()?
    {
        return Err(changed("captured CPU bytes differ from admitted image"));
    }
    admission(engine, stream, &cancel, origin, lease_end, Some(&admitted))?;
    let prepared = crate::live_staging::prepare_tracked(
        &engine.config,
        &engine.pool,
        [
            ArtifactReference {
                path: engine.config.worker_executable.clone(),
                identity: stream.worker.clone(),
            },
            stream.model.clone(),
        ],
        crate::live_staging::PreparationBudget {
            memory_bytes: stream.budget.memory_bytes,
            cpu_threads: stream.budget.cpu_threads,
        },
        cancel.clone(),
        deadline,
        crate::live_staging::PreparationTracking {
            fence: Some(engine.worker_fence.clone()),
            assets: assets.clone(),
        },
    )?;
    admission(engine, stream, &cancel, origin, lease_end, Some(&admitted))?;
    let paths = prepared.paths().clone();
    let custody = prepared.into_worker(
        stream.attempt_id.as_str(),
        stream.budget.memory_bytes,
        stream.budget.cpu_threads,
    )?;
    // Staging custody retains this use's guards; the separate captured image
    // owner retains its storage through any cleanup after preparation failure.
    let open = |path: &std::path::Path| {
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(path)
            .map_err(unavailable)
    };
    let inputs = LiveCpuMountInputs {
        image,
        worker: open(&paths[0])?,
        model: open(&paths[1])?,
        assets,
    };
    let plan = OwnedLiveCpuMountCommand::prepare(
        &engine.config,
        &admitted.manifest,
        inputs,
        custody,
        deadline,
    )
    .map_err(unavailable)?;
    admission(engine, stream, &cancel, origin, lease_end, Some(&admitted))?;
    let worker = LiveInferenceWorker::start_captured(plan, cancel.clone())?;
    admission(engine, stream, &cancel, origin, lease_end, Some(&admitted))?;
    if worker.captured_manifest_sha256() != Some(admitted.manifest.identity_sha256()?.as_str()) {
        return Err(changed(
            "captured CPU Ready differs from original admission",
        ));
    }
    Ok(worker)
}
