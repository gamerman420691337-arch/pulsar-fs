//! Private consuming finalization. Transport/custody retirement occurs outside
//! Engine locks; durable bytes alone never supply current capture authority.
use super::*;

#[cfg(test)]
type FinalizerHook = Box<dyn FnMut(&str)>;
#[cfg(test)]
thread_local! {
    static FINALIZER_HOOK: std::cell::RefCell<Option<FinalizerHook>> =
        std::cell::RefCell::new(None);
}
#[cfg(test)]
fn checkpoint(point: &str) {
    FINALIZER_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(point);
        }
    });
}

fn checked<'a>(
    engine: &Engine,
    db: &Connection,
    registry: &'a mut Registry,
    request: &Request,
    epoch: &AttemptId,
    attempt: &AttemptId,
) -> PResult<&'a mut Entry> {
    request.validate()?;
    let session = request
        .session
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("CUDA finalization session absent"))?;
    let (boot, options) = match &request.command {
        Command::StartLiveCapture {
            engine_epoch,
            options,
        }
        | Command::ContinueLiveCapture {
            engine_epoch,
            options,
            ..
        } => (engine_epoch, options),
        _ => {
            return Err(ProtocolError::invalid(
                "CUDA finalization requires original explicit capture intent",
            ))
        }
    };
    engine.authorize(db, request, session)?;
    engine.require_worker_fence(db)?;
    registry.sweep(db)?;
    let entry = active(registry, epoch, attempt).ok_or_else(|| {
        ProtocolError::new(ErrorCode::Unavailable, "CUDA finalization capture retired")
    })?;
    if boot.as_str() != engine.engine_epoch
        || serde_json::to_vec(options).map_err(internal)?
            != serde_json::to_vec(&entry.report.options).map_err(internal)?
        || entry.owner != *session
        || &entry.report.identity.project != project(request)?
        || request.expected_revision != Some(entry.report.identity.revision)
        || entry.report.engine_epoch.as_str() != engine.engine_epoch
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "CUDA finalization owner, project, revision or boot differs",
        ));
    }
    Ok(entry)
}
fn completed_report(
    entry: &Entry,
    record: &LiveCudaEvidenceRecord,
    now: LiveClockTick,
) -> PResult<LiveCaptureReport> {
    let mut proposed = entry.report.clone();
    proposed.active = false;
    proposed.queue = None;
    proposed.admitted_pixel_bytes = 0;
    proposed.retirement = Some(LiveCaptureRetirement::Completed);
    proposed.server_elapsed = Some(now);
    if let Some(source) = &mut proposed.encoded_source {
        source.state = LiveEncodedSourceState::Retired;
    }
    let info = proposed.inference.as_mut().expect("admitted");
    info.state = LiveCaptureInferenceState::Retired;
    info.latest = None;
    info.error = None;
    info.cuda_evidence = Some(record.clone());
    proposed.validate()?;
    Ok(proposed)
}
impl Engine {
    #[cfg(all(test, feature = "cuda"))]
    pub(in crate::authority) fn live_cuda_finalizer_hook_for_test(hook: Box<dyn FnMut(&str)>) {
        FINALIZER_HOOK.with(|slot| *slot.borrow_mut() = Some(hook));
    }
    pub(in crate::authority) fn live_cuda_finalize(
        &self,
        request: &Request,
        epoch: &AttemptId,
        attempt: &AttemptId,
        worker: crate::live_cuda_stream::LiveCudaWorker,
    ) -> PResult<LiveCaptureReport> {
        let mut began = false;
        let result = (|| -> PResult<LiveCaptureReport> {
            let (manifest_hash, origin) = {
                let db = self.db.lock().map_err(internal)?;
                let mut registry = self.live_captures.lock().map_err(internal)?;
                let entry = checked(self, &db, &mut registry, request, epoch, attempt)?;
                let info = entry.report.inference.as_ref().expect("admitted");
                let runtime = entry.runtime.as_ref().expect("active");
                let manifest = worker
                    .ready_manifest(&runtime.inference_scope())
                    .map_err(unavailable)?;
                let hash = manifest.identity_sha256()?;
                let profile = &entry
                    .report
                    .options
                    .inference
                    .as_ref()
                    .expect("admitted")
                    .profile;
                if !matches!(
                    info.state,
                    LiveCaptureInferenceState::Running | LiveCaptureInferenceState::FinishRequested
                ) || info.completed == 0
                    || runtime.status().queue.in_flight_frames != 0
                    || info.cuda_manifest_sha256.as_ref() != Some(&hash)
                    || profile.provider != InferenceProvider::Cuda
                    || manifest.stream.attempt_id != *attempt
                    || manifest.stream.identity != entry.report.identity
                    || manifest.stream.clock != entry.report.clock
                    || manifest.stream.worker != self.binary_identity
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
                        "CUDA finalization differs from owned Ready/completed work",
                    ));
                }
                began = true;
                let info = entry.report.inference.as_mut().expect("admitted");
                info.state = LiveCaptureInferenceState::Finishing;
                info.latest = None;
                report(entry)?;
                (hash, entry.origin.clone())
            };
            // The worker is consumed here. No caller-supplied finished envelope
            // can substitute for its scope-bound transcript and native custody.
            #[cfg(test)]
            checkpoint("before_finish");
            let mut evidence = worker.finish(&self.pool).map_err(unavailable)?;
            let record = evidence.report_record().map_err(unavailable)?;
            if record.manifest.sha256 != manifest_hash {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "CUDA finalization manifest changed",
                ));
            }
            let origin_size = origin
                .as_ref()
                .map(|o| o.bytes().map(|b| b.len() as u64))
                .transpose()
                .map_err(unavailable)?
                .unwrap_or(0);
            evidence
                .reserve_storage_extra(
                    fs2::available_space(&self.config.state_dir).map_err(unavailable)?,
                    origin_size,
                )
                .map_err(unavailable)?;
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            let mut registry = self.live_captures.lock().map_err(internal)?;
            let entry = checked(self, &tx, &mut registry, request, epoch, attempt)?;
            if !finishing(entry)
                || entry
                    .report
                    .inference
                    .as_ref()
                    .expect("admitted")
                    .cuda_manifest_sha256
                    .as_ref()
                    != Some(&manifest_hash)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "CUDA closing membership changed",
                ));
            }
            completed_report(entry, &record, tick(entry)?)?;
            if let Some(origin) = &origin {
                crate::live_cuda_capture_db::retain(&tx, origin, &evidence).map_err(unavailable)?;
            } else {
                crate::live_cuda_evidence_db::retain(&tx, &evidence).map_err(unavailable)?;
            }
            self.require_worker_fence(&tx)?;
            #[cfg(test)]
            checkpoint("before_commit");
            if entry.created.elapsed()
                >= Duration::from_millis(u64::from(entry.report.options.lease_ms))
            {
                retire(entry, LiveCaptureRetirement::Expired)?;
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "CUDA finalization expired before commit",
                ));
            }
            tx.commit().map_err(internal)?;
            #[cfg(test)]
            checkpoint("after_commit");
            // A blocking filesystem commit cannot be preempted by the lease.
            // Never publish Completed if that commit crossed its deadline. Its
            // already durable bytes remain unqualified historical diagnostics.
            let now = tick(entry)?;
            if now.0 >= u64::from(entry.report.options.lease_ms) * 1_000_000 {
                retire(entry, LiveCaptureRetirement::Expired)?;
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "CUDA finalization commit exceeded lease",
                ));
            }
            retire_at(entry, LiveCaptureRetirement::Completed, now)?;
            entry
                .report
                .inference
                .as_mut()
                .expect("admitted")
                .cuda_evidence = Some(record);
            report(entry)
        })();
        if began {
            if let Err(error) = &result {
                let _ = self.live_analysis_failed(epoch, attempt, error.clone());
            }
        }
        result
    }
}
