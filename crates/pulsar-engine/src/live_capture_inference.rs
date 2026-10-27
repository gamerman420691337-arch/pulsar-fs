//! Lease-owned automatic analysis. No native work occurs under authority locks.
use super::*;
#[cfg(target_os = "linux")]
#[path = "live_capture_cpu.rs"]
mod captured_cpu;
#[cfg(target_os = "linux")]
#[path = "live_capture_cuda.rs"]
mod cuda;
#[cfg(target_os = "linux")]
#[path = "live_capture_finalize.rs"]
mod finalize;
use crate::{live_frames::LiveRgbWork, live_inference::LiveInferenceWorker};
pub(super) enum CpuStep {
    Work(Box<LiveRgbWork>),
    Wait,
    Join(CpuJoinTicket),
    Retired,
}
pub(super) struct CpuJoinTicket {
    pub(super) identity: LiveIdentity,
    pub(super) attempt: AttemptId,
    pub(super) close_request: RequestId,
    pub(super) completed: u64,
    pub(super) transfer: Option<ArtifactId>,
}
pub(super) fn cpu_join_matches(
    joined: &crate::live_inference::CpuInputJoined,
    info: &LiveCaptureInferenceReport,
    ticket: &CpuJoinTicket,
) -> PResult<bool> {
    Ok(match &info.cpu_manifest {
        Some(manifest) => joined.matches_captured(
            &ticket.identity,
            &ticket.attempt,
            &info.profile_sha256,
            &manifest.identity_sha256()?,
            ticket.completed,
        ),
        None => joined.matches(
            &ticket.identity,
            &ticket.attempt,
            &info.profile_sha256,
            ticket.completed,
        ),
    })
}
pub(super) fn same_cpu_mode(
    previous: Option<&LiveCaptureInferenceReport>,
    next: Option<&LiveCaptureInferenceReport>,
) -> PResult<bool> {
    Ok(
        match (
            previous.and_then(|i| i.cpu_manifest.as_ref()),
            next.and_then(|i| i.cpu_manifest.as_ref()),
        ) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.image.identity()? == b.image.identity()? && a.stream.worker == b.stream.worker
            }
            _ => false,
        },
    )
}
fn same_final(header: &LiveFrameHeader, stamp: &NativeAcceptedCapture) -> bool {
    header.frame == stamp.frame.frame
        && header.source_time == stamp.frame.source_time
        && header.capture_tick == stamp.frame.capture_tick
        && header.sha256 == stamp.frame.artifact.as_str()
}
pub(super) fn manifest(
    engine: &Engine,
    options: &LiveCaptureOptions,
    identity: &LiveIdentity,
    clock: &LivePresentationClock,
) -> PResult<Option<LiveInferenceManifest>> {
    let Some(inference) = &options.inference else {
        return Ok(None);
    };
    #[cfg(not(target_os = "linux"))]
    if inference.profile.provider == InferenceProvider::Cpu {
        crate::live_inference::require_host_cpu_runtime(&engine.config)?;
    }
    let runtime = engine.selected_profile_runtime(&inference.profile)?;
    if !launch::available() {
        return Err(ProtocolError::unsupported("live inference confinement"));
    }
    let gpu = inference.profile.provider == InferenceProvider::Cuda;
    let budget = crate::inference_budget::admit(
        ResourceBudget {
            memory_bytes: WORKER_MEMORY,
            output_bytes: if gpu { OUTPUT_LIMIT } else { 1024 * 1024 },
            wall_time_ms: u64::from(options.lease_ms),
            cpu_threads: if gpu {
                inference.profile.cpu_threads
            } else {
                2
            },
        },
        Some(&inference.profile),
        None,
        engine.config.max_ram_bytes,
    )?;
    let manifest = LiveInferenceManifest {
        version: PROTOCOL_VERSION,
        attempt_id: id!(AttemptId),
        identity: identity.clone(),
        clock: clock.clone(),
        model: ArtifactReference {
            path: inference.model_path.clone(),
            identity: inference.profile.model.clone(),
        },
        runtime,
        worker: engine.binary_identity.clone(),
        profile: inference.profile.clone(),
        budget,
    };
    if manifest.profile.provider == InferenceProvider::Cuda {
        manifest.validate_cuda_metadata()?;
        #[cfg(target_os = "linux")]
        cuda::preflight(engine, &manifest)?;
        #[cfg(not(target_os = "linux"))]
        return Err(ProtocolError::unsupported(
            "live CUDA requires Linux confinement",
        ));
    } else {
        manifest.validate()?;
    }
    Ok(Some(manifest))
}
pub(super) fn initial_report(
    manifest: &LiveInferenceManifest,
) -> PResult<LiveCaptureInferenceReport> {
    Ok(LiveCaptureInferenceReport {
        cpu_manifest: None,
        cuda_manifest_sha256: None,
        cuda_evidence: None,
        attempt_id: manifest.attempt_id.clone(),
        profile_sha256: crate::inference_profiles::profile_identity(&manifest.profile)
            .map_err(unavailable)?
            .sha256,
        state: LiveCaptureInferenceState::Starting,
        completed: 0,
        accepted: 0,
        stale: 0,
        latest: None,
        error: None,
    })
}
pub(super) fn initial_report_with_mode(
    engine: &Engine,
    manifest: &LiveInferenceManifest,
) -> PResult<LiveCaptureInferenceReport> {
    let report = initial_report(manifest)?;
    #[cfg(target_os = "linux")]
    {
        let mut report = report;
        report.cpu_manifest = captured_cpu::declaration(engine, manifest)?;
        Ok(report)
    }
    #[cfg(not(target_os = "linux"))]
    {
        if engine.config.cpu_owner_policy.is_some()
            && manifest.profile.provider == InferenceProvider::Cpu
        {
            return Err(ProtocolError::unsupported(
                "captured CPU live requires Linux confinement",
            ));
        }
        Ok(report)
    }
}

pub(super) fn fail_entry(entry: &mut Entry, error: ProtocolError) -> PResult<()> {
    entry.retire_native_delivery();
    entry.native_accepted = None;
    if entry.runtime.is_none() {
        return Ok(());
    }
    let mut bounded = error;
    bounded.message = bounded.message.chars().take(512).collect();
    if let Some(info) = &mut entry.report.inference {
        info.error = Some(bounded);
    }
    retire(entry, LiveCaptureRetirement::WorkerFailed)
}
#[cfg(target_os = "linux")]
pub(super) fn spawn(
    engine: &Arc<Engine>,
    epoch: AttemptId,
    manifest: LiveInferenceManifest,
    cancel: Arc<AtomicBool>,
    origin: Instant,
    lease_end: Instant,
    intent: Request,
) -> PResult<()> {
    if manifest.profile.provider == InferenceProvider::Cuda {
        return cuda::spawn(engine, epoch, manifest, cancel, origin, lease_end, intent);
    }
    spawn_tracked(
        engine,
        epoch,
        manifest,
        cancel,
        CpuLaunchLifetime {
            origin,
            lease_end,
            retirement: None,
        },
    )
}
pub(super) struct CpuLaunchLifetime {
    pub origin: Instant,
    pub lease_end: Instant,
    pub retirement: Option<Arc<crate::live_inference::CpuRetirementGuard>>,
}
#[cfg(target_os = "linux")]
pub(super) fn spawn_tracked(
    engine: &Arc<Engine>,
    epoch: AttemptId,
    manifest: LiveInferenceManifest,
    cancel: Arc<AtomicBool>,
    lifetime: CpuLaunchLifetime,
) -> PResult<()> {
    if manifest.profile.provider != InferenceProvider::Cpu {
        return Err(refused_cpu());
    }
    let CpuLaunchLifetime {
        origin,
        lease_end,
        mut retirement,
    } = lifetime;
    let weak = Arc::downgrade(engine);
    engine
        .spawn_task("pulsar-live-analysis", None, move || {
            let result = (|| -> PResult<()> {
                let Some(engine) = weak.upgrade() else {
                    return Ok(());
                };
                if !engine.live_analysis_launch_allowed(&epoch, &manifest.attempt_id)? {
                    return Ok(());
                }
                let assets: crate::worker_retirement::RetirementAssets = retirement
                    .take()
                    .map(|asset| vec![asset as Arc<dyn std::any::Any + Send + Sync>])
                    .unwrap_or_default();
                let mut worker = if engine.cpu_owner_policy.is_some() {
                    captured_cpu::start(
                        &engine,
                        &manifest,
                        cancel.clone(),
                        origin,
                        lease_end,
                        assets,
                    )?
                } else if assets.is_empty() {
                    LiveInferenceWorker::start_tracked_in_lease(
                        &engine.config,
                        &engine.pool,
                        manifest.clone(),
                        cancel.clone(),
                        origin,
                        lease_end,
                        engine.worker_fence.clone(),
                    )?
                } else {
                    LiveInferenceWorker::start_with_tracking(
                        &engine.config,
                        &engine.pool,
                        manifest.clone(),
                        cancel.clone(),
                        origin,
                        lease_end,
                        crate::live_inference::CpuWorkerTracking {
                            fence: Some(engine.worker_fence.clone()),
                            assets,
                        },
                    )?
                };
                if !engine.live_analysis_ready(
                    &epoch,
                    &manifest.attempt_id,
                    worker.captured_manifest_sha256(),
                )? {
                    return Ok(());
                }
                drop(engine);
                loop {
                    if cancel.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let Some(engine) = weak.upgrade() else {
                        return Ok(());
                    };
                    let step = engine.live_cpu_step(&epoch, &manifest.attempt_id)?;
                    drop(engine);
                    let work = match step {
                        CpuStep::Work(work) => *work,
                        CpuStep::Wait => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        CpuStep::Retired => return Ok(()),
                        CpuStep::Join(ticket) => {
                            let joined = worker.close_input_and_join(lease_end)?;
                            if let Some(engine) = weak.upgrade() {
                                engine.live_cpu_input_joined(ticket, joined)?;
                            }
                            return Ok(());
                        }
                    };
                    let result = worker.estimate(&work)?;
                    let Some(engine) = weak.upgrade() else {
                        return Ok(());
                    };
                    if !engine.live_analysis_complete(&epoch, &manifest.attempt_id, work, result)? {
                        return Ok(());
                    }
                }
            })();
            if let Err(error) = result {
                if let Some(engine) = weak.upgrade() {
                    let _ = engine.live_analysis_failed(&epoch, &manifest.attempt_id, error);
                }
            }
        })
        .map_err(internal)?;
    Ok(())
}
fn refused_cpu() -> ProtocolError {
    ProtocolError::unsupported("successor retirement requires owned CPU worker")
}
#[cfg(not(target_os = "linux"))]
pub(super) fn spawn_tracked(
    _engine: &Arc<Engine>,
    _epoch: AttemptId,
    _manifest: LiveInferenceManifest,
    _cancel: Arc<AtomicBool>,
    _lifetime: CpuLaunchLifetime,
) -> PResult<()> {
    Err(refused_cpu())
}
#[cfg(not(target_os = "linux"))]
pub(super) fn spawn(
    _engine: &Arc<Engine>,
    _epoch: AttemptId,
    _manifest: LiveInferenceManifest,
    _cancel: Arc<AtomicBool>,
    _origin: Instant,
    _lease_end: Instant,
    _intent: Request,
) -> PResult<()> {
    Err(ProtocolError::unsupported(
        "persistent native confinement on this platform",
    ))
}

fn active<'a>(
    registry: &'a mut Registry,
    epoch: &AttemptId,
    attempt: &AttemptId,
) -> Option<&'a mut Entry> {
    registry.entries.get_mut(epoch.as_str()).filter(|entry| {
        entry.runtime.is_some()
            && !entry.cancel.load(Ordering::Acquire)
            && entry
                .report
                .inference
                .as_ref()
                .is_some_and(|info| info.attempt_id == *attempt)
    })
}
impl Engine {
    /// Authored state-machine fixture only. Actual custody retirement is covered
    /// by the separately gated native worker campaign.
    #[cfg(test)]
    pub(in crate::authority) fn authored_cpu_input_join_for_test(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
        join: bool,
    ) -> PResult<bool> {
        match self.live_cpu_step(epoch, attempt)? {
            CpuStep::Join(ticket) if join => {
                let profile = {
                    let registry = self.live_captures.lock().map_err(internal)?;
                    registry
                        .entries
                        .get(epoch.as_str())
                        .expect("test entry")
                        .report
                        .inference
                        .as_ref()
                        .expect("test inference")
                        .profile_sha256
                        .clone()
                };
                let proof = crate::live_inference::CpuInputJoined::authored_for_test(
                    ticket.identity.clone(),
                    ticket.attempt.clone(),
                    profile,
                    ticket.completed,
                );
                self.live_cpu_input_joined(ticket, proof)?;
                Ok(true)
            }
            CpuStep::Work(_) => panic!("authored join requested before original work completed"),
            _ => Ok(false),
        }
    }
    fn live_cpu_step(&self, epoch: &AttemptId, attempt: &AttemptId) -> PResult<CpuStep> {
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let Some(entry) = active(&mut registry, epoch, attempt) else {
            return Ok(CpuStep::Retired);
        };
        if finishing(entry)
            || entry.report.inference.as_ref().expect("active").state
                == LiveCaptureInferenceState::InputSealed
            || entry.native_transfer_joined
        {
            return Ok(CpuStep::Retired);
        }
        let now = tick(entry)?;
        if let Some(work) = entry.runtime.as_mut().expect("active").take(now)? {
            return Ok(CpuStep::Work(Box::new(work)));
        }
        if entry.native_transfer.is_some() {
            return registry.cpu_transfer_step(epoch, attempt);
        }
        let Some(input) = entry.native_input_close.as_mut() else {
            return Ok(CpuStep::Wait);
        };
        if !entry.runtime.as_ref().expect("active").input_closed() {
            return Err(internal("native input close lacks frozen ingress"));
        }
        let close = entry
            .report
            .input_close
            .as_mut()
            .expect("native close report");
        let settled = entry.native_delivery.as_ref().is_none_or(|sink| {
            sink.pending.load(Ordering::Acquire) == 0 && !sink.delivering.load(Ordering::Acquire)
        });
        close.delivery_settled = settled;
        let Some(mut disposition) = entry.runtime.as_ref().expect("active").input_disposition()
        else {
            return Ok(CpuStep::Wait);
        };
        if !settled {
            return Ok(CpuStep::Wait);
        }
        if disposition == LiveCaptureInputFinalDisposition::Accepted {
            let accepted = entry.native_accepted.as_ref().filter(|stamp| {
                close
                    .final_frame
                    .as_ref()
                    .is_some_and(|header| same_final(header, stamp))
                    && stamp.continuity_generation == input.continuity_generation
            });
            if accepted.is_none() {
                disposition = LiveCaptureInputFinalDisposition::Failed;
            }
            if let Some(sink) = &entry.native_delivery {
                let last = match sink.last_delivered.try_lock() {
                    Ok(last) => last,
                    Err(std::sync::TryLockError::WouldBlock) => return Ok(CpuStep::Wait),
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        disposition = LiveCaptureInputFinalDisposition::Failed;
                        input.disposition = Some(disposition);
                        close.phase = LiveCaptureInputPhase::Joining;
                        return Ok(CpuStep::Join(CpuJoinTicket {
                            identity: entry.report.identity.clone(),
                            attempt: attempt.clone(),
                            close_request: close.original_request_id.clone(),
                            completed: entry.report.inference.as_ref().expect("active").completed,
                            transfer: None,
                        }));
                    }
                };
                if !sink.current()
                    || accepted.is_none_or(|stamp| last.as_ref() != Some(&stamp.frame))
                {
                    disposition = LiveCaptureInputFinalDisposition::Failed;
                }
            }
        }
        input.disposition = Some(disposition);
        close.phase = LiveCaptureInputPhase::Joining;
        Ok(CpuStep::Join(CpuJoinTicket {
            identity: entry.report.identity.clone(),
            attempt: attempt.clone(),
            close_request: close.original_request_id.clone(),
            completed: entry.report.inference.as_ref().expect("active").completed,
            transfer: None,
        }))
    }

    fn live_cpu_input_joined(
        &self,
        ticket: CpuJoinTicket,
        joined: crate::live_inference::CpuInputJoined,
    ) -> PResult<()> {
        if ticket.transfer.is_some() {
            return self.live_cpu_transfer_joined(ticket, joined);
        }
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let Some(entry) = registry.entries.get_mut(ticket.identity.epoch.as_str()) else {
            return Ok(());
        };
        let info = entry
            .report
            .inference
            .as_ref()
            .ok_or_else(|| internal("closed input lacks inference"))?;
        if entry.report.identity != ticket.identity
            || info.attempt_id != ticket.attempt
            || !cpu_join_matches(&joined, info, &ticket)?
            || info.completed != ticket.completed
            || entry
                .report
                .input_close
                .as_ref()
                .is_none_or(|close| close.original_request_id != ticket.close_request)
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "CPU join differs from original input close",
            ));
        }
        if !entry.report.active || entry.cancel.load(Ordering::Acquire) {
            if let Some(close) = &mut entry.report.input_close {
                close.worker_joined = true;
            }
            return Ok(());
        }
        let input = entry
            .native_input_close
            .as_ref()
            .ok_or_else(|| internal("native input close absent"))?;
        if input.end.is_some() {
            return Err(internal("native input end already sealed"));
        }
        let disposition = input
            .disposition
            .ok_or_else(|| internal("input drain disposition absent"))?;
        let close = entry.report.input_close.as_ref().expect("checked close");
        if close.phase != LiveCaptureInputPhase::Joining
            || entry
                .native_delivery
                .as_ref()
                .is_some_and(|sink| sink.pending.load(Ordering::Acquire) != 0)
        {
            return Err(internal("CPU join preceded delivery settlement"));
        }
        let selection = NativeCaptureSelection {
            owner: entry.owner.clone(),
            identity: entry.report.identity.clone(),
            attempt_id: info.attempt_id.clone(),
            profile_sha256: info.profile_sha256.clone(),
            cpu_manifest_sha256: cpu_manifest_hash(info)?,
            browser_media: entry
                .report
                .options
                .browser_media
                .clone()
                .ok_or_else(|| internal("input close media absent"))?,
        };
        let final_accepted = (disposition == LiveCaptureInputFinalDisposition::Accepted)
            .then(|| entry.native_accepted.clone())
            .flatten();
        let end = NativeCaptureEnd {
            selection,
            close_request_id: ticket.close_request,
            final_frame: close.final_frame.clone(),
            final_accepted: final_accepted.clone(),
            disposition,
            closed_at_boottime_ns: input.closed_at_boottime_ns,
            continuity_generation: input.continuity_generation,
        };
        let mut proposed = report(entry)?;
        let close = proposed.input_close.as_mut().expect("checked close");
        close.phase = LiveCaptureInputPhase::Sealed;
        close.disposition = Some(disposition);
        close.worker_joined = true;
        close.delivery_settled = true;
        let info = proposed.inference.as_mut().expect("checked inference");
        info.state = LiveCaptureInferenceState::InputSealed;
        if final_accepted.is_none() {
            info.latest = None;
        }
        proposed.validate()?;
        entry.report = proposed;
        entry.native_input_close.as_mut().expect("native close").end = Some(end.clone());
        entry.native_accepted = final_accepted;
        let sink = entry.native_delivery.clone();
        drop(registry);
        drop(db);
        if let Some(sink) = sink {
            sink.finish(end);
        }
        Ok(())
    }
    fn live_analysis_launch_allowed(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
    ) -> PResult<bool> {
        let db = self.db.lock().map_err(internal)?;
        self.require_worker_fence(&db)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        Ok(active(&mut registry, epoch, attempt).is_some_and(|entry| !finishing(entry)))
    }
    #[cfg(test)]
    pub(in crate::authority) fn live_analysis_ready_for_test(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
    ) -> PResult<bool> {
        self.live_analysis_ready(epoch, attempt, None)
    }
    fn live_analysis_ready(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
        cpu_manifest_sha256: Option<&str>,
    ) -> PResult<bool> {
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let Some(entry) = active(&mut registry, epoch, attempt) else {
            return Ok(false);
        };
        if finishing(entry) {
            return Ok(false);
        }
        let expected_cpu = entry
            .report
            .inference
            .as_ref()
            .expect("admitted")
            .cpu_manifest
            .as_ref()
            .map(|manifest| manifest.identity_sha256())
            .transpose()?;
        if expected_cpu.as_deref() != cpu_manifest_sha256 {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "CPU Ready runtime mode differs from original admission",
            ));
        }
        registry.activate_prepared(epoch)?;
        registry
            .entries
            .get_mut(epoch.as_str())
            .expect("validated Ready")
            .report
            .inference
            .as_mut()
            .expect("admitted")
            .state = LiveCaptureInferenceState::Running;
        Ok(true)
    }
    #[cfg(target_os = "linux")]
    pub(in crate::authority) fn live_cuda_analysis_ready(
        &self,
        epoch: &AttemptId,
        worker: &crate::live_cuda_stream::LiveCudaWorker,
        attempt: &AttemptId,
    ) -> PResult<bool> {
        let db = self.db.lock().map_err(internal)?;
        self.require_worker_fence(&db)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let Some(entry) = active(&mut registry, epoch, attempt) else {
            return Ok(false);
        };
        if finishing(entry) {
            return Ok(false);
        }
        let scope = entry
            .runtime
            .as_ref()
            .expect("active runtime")
            .inference_scope();
        let manifest = worker.ready_manifest(&scope).map_err(unavailable)?;
        let info = entry.report.inference.as_ref().expect("admitted");
        let profile = &entry
            .report
            .options
            .inference
            .as_ref()
            .expect("admitted")
            .profile;
        if manifest.stream.identity != entry.report.identity
            || manifest.stream.clock != entry.report.clock
            || manifest.stream.attempt_id != *attempt
            || manifest.stream.worker != self.binary_identity
            || profile.provider != InferenceProvider::Cuda
            || crate::inference_profiles::profile_identity(profile)
                .map_err(unavailable)?
                .sha256
                != info.profile_sha256
            || crate::inference_profiles::profile_identity(&manifest.stream.profile)
                .map_err(unavailable)?
                .sha256
                != info.profile_sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "CUDA Ready differs from active capture admission",
            ));
        }
        let hash = manifest.identity_sha256()?;
        if info
            .cuda_manifest_sha256
            .as_ref()
            .is_some_and(|old| old != &hash)
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "CUDA Ready changed the capture manifest",
            ));
        }
        registry.activate_prepared(epoch)?;
        let info = registry
            .entries
            .get_mut(epoch.as_str())
            .expect("validated Ready")
            .report
            .inference
            .as_mut()
            .expect("admitted");
        info.cuda_manifest_sha256 = Some(hash);
        info.state = LiveCaptureInferenceState::Running;
        Ok(true)
    }
    #[cfg(target_os = "linux")]
    pub(in crate::authority) fn live_cuda_analysis_complete(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
        worker: &crate::live_cuda_stream::LiveCudaWorker,
        work: LiveRgbWork,
        reply: LiveCudaStreamReply,
    ) -> PResult<bool> {
        let result = match worker.provisional(&work, reply).map_err(unavailable) {
            Ok(result) => result,
            Err(error) => {
                self.live_analysis_failed(epoch, attempt, error.clone())?;
                return Err(error);
            }
        };
        self.live_analysis_complete(epoch, attempt, work, result)
    }
    pub(in crate::authority) fn live_analysis_take(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
    ) -> PResult<Option<LiveRgbWork>> {
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let Some(entry) = active(&mut registry, epoch, attempt) else {
            return Ok(None);
        };
        if finishing(entry) {
            return Ok(None);
        }
        let now = tick(entry)?;
        entry.runtime.as_mut().expect("active").take(now)
    }
    pub(in crate::authority) fn live_analysis_complete(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
        work: LiveRgbWork,
        result: LiveInferenceReply,
    ) -> PResult<bool> {
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let boundary_seed = registry.successor_seed(epoch);
        let original_seed_used = boundary_seed
            .as_ref()
            .is_some_and(|seed| work.uses_boundary(seed));
        let Some(entry) = active(&mut registry, epoch, attempt) else {
            return Ok(false);
        };
        if entry
            .report
            .inference
            .as_ref()
            .is_some_and(|i| i.state == LiveCaptureInferenceState::Finishing)
        {
            return Ok(false);
        }
        let info = entry.report.inference.as_ref().expect("admitted");
        let provider = entry
            .report
            .options
            .inference
            .as_ref()
            .expect("admitted")
            .profile
            .provider;
        let binding = match &result {
            LiveInferenceReply::Estimated {
                attempt_id,
                identity,
                profile_sha256,
                input,
                pair_analysis_elapsed_ns,
                ..
            } if provider == InferenceProvider::Cpu
                && info.cuda_manifest_sha256.is_none()
                && info.cpu_manifest.is_none() =>
            {
                Some((
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    *pair_analysis_elapsed_ns,
                ))
            }
            LiveInferenceReply::CapturedCpuEstimated {
                attempt_id,
                identity,
                profile_sha256,
                input,
                pair_analysis_elapsed_ns,
                ..
            } if provider == InferenceProvider::Cpu
                && info.cuda_manifest_sha256.is_none()
                && info.cpu_manifest.as_ref().is_some_and(|manifest| {
                    manifest.validate_estimated(input, &result).is_ok()
                }) =>
            {
                Some((
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    *pair_analysis_elapsed_ns,
                ))
            }
            LiveInferenceReply::CudaProvisional {
                attempt_id,
                identity,
                profile_sha256,
                manifest_sha256,
                input,
                pair_analysis_elapsed_ns,
                ..
            } if provider == InferenceProvider::Cuda
                && info.cpu_manifest.is_none()
                && info.cuda_manifest_sha256.as_ref() == Some(manifest_sha256) =>
            {
                Some((
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    *pair_analysis_elapsed_ns,
                ))
            }
            _ => None,
        };
        let Some((attempt_id, identity, profile_sha256, input, elapsed)) = binding else {
            let error = ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "live result provider or provisional CUDA binding differs",
            );
            fail_entry(entry, error.clone())?;
            return Err(error);
        };
        if info.completed.checked_add(1) != Some(input.sequence)
            || elapsed == 0
            || elapsed > u64::from(entry.report.options.lease_ms) * 1_000_000
            || *attempt_id != *attempt
            || *identity != entry.report.identity
            || *profile_sha256 != info.profile_sha256
            || input.pair != work.declaration()
            || input.current.artifact_id != work.token().frame().artifact
            || input.previous.sha256 != format!("{:x}", Sha256::digest(work.previous()))
            || input.current.sha256 != format!("{:x}", Sha256::digest(work.current()))
        {
            let error = ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "live result differs from lease-issued work",
            );
            fail_entry(entry, error.clone())?;
            return Err(error);
        }
        let original_previous_frame = work.previous_frame().clone();
        let original_frame = work.token().frame().clone();
        let continuity_generation = work.continuity_generation();
        let capture_tick = original_frame.capture_tick;
        let now = tick(entry)?;
        let completed = info
            .completed
            .checked_add(1)
            .ok_or_else(|| ProtocolError::invalid("live completion count overflow"))?;
        let completion = entry
            .runtime
            .as_mut()
            .expect("active")
            .complete(work, now)
            .map_err(|error| error.error)?;
        let info = entry.report.inference.as_mut().expect("admitted");
        let mut delivery = None;
        match completion {
            LiveCompletion::Accepted {
                capture_to_analysis_ns,
                ..
            } => {
                info.accepted = info
                    .accepted
                    .checked_add(1)
                    .ok_or_else(|| ProtocolError::invalid("live accepted count overflow"))?;
                let estimate = LiveCaptureEstimate {
                    age_basis: entry.report.age_basis,
                    evidence: EvidenceKind::Inferred,
                    qualified: false,
                    result,
                    capture_tick,
                    accepted_at: now,
                    capture_to_analysis_ns,
                    capture_age_ns: capture_to_analysis_ns,
                    fresh: true,
                };
                // Recheck the same native clock after completion. Keep the
                // original acceptance tick: scheduling/metadata work cannot
                // rejuvenate an already accepted result.
                if let Err(error) = tick(entry) {
                    retire(entry, LiveCaptureRetirement::AuthorityLost)?;
                    return Err(error);
                }
                let native = if entry
                    .runtime
                    .as_ref()
                    .expect("active")
                    .status()
                    .pair_discontinuities
                    == continuity_generation
                {
                    match clock::NativeAcceptedCapture::capture(
                        &entry.native_clock,
                        original_previous_frame,
                        original_frame,
                        estimate.clone(),
                        continuity_generation,
                        entry.native_accepted.as_ref(),
                    ) {
                        Ok(native) => native,
                        Err(error) => {
                            retire(entry, LiveCaptureRetirement::AuthorityLost)?;
                            return Err(error);
                        }
                    }
                } else {
                    None
                };
                if let Some(recording) = &mut entry.recording {
                    if let Err(error) = recording.append(&estimate) {
                        entry.report.inference.as_mut().expect("admitted").completed = completed;
                        fail_entry(entry, error.clone())?;
                        return Err(error);
                    }
                }
                if let Some(sink) = &entry.native_delivery {
                    match &native {
                        Some(stamp)
                            if stamp.continuity_generation == sink.continuity_generation
                                && sink.current() =>
                        {
                            delivery =
                                Some(PendingNativeDelivery::new(sink.clone(), stamp.clone()));
                        }
                        _ => sink.retire(),
                    }
                }
                entry.native_accepted = native;
                entry.report.inference.as_mut().expect("admitted").latest = Some(estimate);
            }
            LiveCompletion::Stale { .. } => {
                info.stale = info
                    .stale
                    .checked_add(1)
                    .ok_or_else(|| ProtocolError::invalid("live stale count overflow"))?;
                entry.native_accepted = None;
                entry.retire_native_delivery();
            }
            LiveCompletion::Retired => {
                entry.retire_native_delivery();
                return Ok(false);
            }
        }
        let info = entry.report.inference.as_mut().expect("admitted");
        info.completed = completed;
        if info.state == LiveCaptureInferenceState::FinishRequested {
            info.latest = None;
            entry.native_accepted = None;
            entry.retire_native_delivery();
            delivery = None;
        }
        let successor_accepted = entry.native_accepted.clone();
        if boundary_seed.is_some() {
            registry.observe_successor_acceptance(epoch, original_seed_used, successor_accepted)?;
        }
        // Deliver the retained original exactly once without owning either
        // authority mutex. Consumer failure only retires that native sink;
        // capture preview and recording remain available.
        drop(registry);
        drop(db);
        if let Some(delivery) = delivery {
            delivery.deliver();
        }
        self.advance_browser_analysis_transfers()?;
        Ok(true)
    }
    #[cfg(test)]
    pub(in crate::authority) fn live_analysis_failure_for_test(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
    ) -> PResult<()> {
        self.live_analysis_failed(
            epoch,
            attempt,
            ProtocolError::new(ErrorCode::Unavailable, "authored Finish failure"),
        )
    }
    fn live_analysis_failed(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
        error: ProtocolError,
    ) -> PResult<()> {
        // Failure retirement must not depend on a healthy project database.
        let mut registry = self.live_captures.lock().map_err(internal)?;
        if let Some(entry) = active(&mut registry, epoch, attempt) {
            if entry.created.elapsed()
                >= Duration::from_millis(u64::from(entry.report.options.lease_ms))
            {
                retire(entry, LiveCaptureRetirement::Expired)?;
            } else {
                fail_entry(entry, error)?;
            }
        }
        Ok(())
    }
}
