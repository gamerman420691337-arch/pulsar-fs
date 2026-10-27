//! Renewable native controller ownership and retirement.
use super::*;
#[path = "handy_browser_playback.rs"]
pub(super) mod browser_play;
#[path = "handy_clock_sync_control.rs"]
pub(super) mod clock_sync;
#[path = "handy_continuous_timing_control.rs"]
pub(super) mod continuous_timing;
#[path = "handy_device_timing_control.rs"]
mod device_timing;
#[path = "handy_playback_control.rs"]
pub(super) mod play;
#[path = "handy_playback_clock_control.rs"]
pub(super) mod playback_clock;
#[path = "handy_player_guard_control.rs"]
pub(super) mod player_guard;
#[path = "handy_clock_control.rs"]
pub(super) mod server_clock;
#[path = "handy_stop_control.rs"]
mod stop_status;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::Duration,
};
pub(super) struct Lease {
    pub(super) report: HandyControllerLease,
}
pub(in crate::authority) struct ControllerMonitor {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
#[cfg(test)]
thread_local! {static FAIL_MONITOR:std::cell::Cell<bool>=const {std::cell::Cell::new(false)};}
pub(super) use crate::handy_clock::boottime_ns;
fn retired(mut value: HandyControllerLease) -> HandyControllerLease {
    value.state = HandyControllerState::Retired;
    value.armed = false;
    value
}
/// Retirement creates at most one stop intent. The execution owner consumes it;
/// repeated sweeps cannot silently retry a completed or rejected physical effect.
pub(super) fn retire_entry(entry: &mut Entry) -> bool {
    entry.server_clock = None;
    entry.playback_clock = None;
    entry.continuous_timing = None;
    entry.player_guard = None;
    if let Some(owner) = &entry.browser_owner {
        owner.cancelled.store(true, Ordering::Release);
    }
    if let Some(lease) = &mut entry.controller {
        let first_retirement = lease.report.state != HandyControllerState::Retired;
        lease.report.state = HandyControllerState::Retired;
        lease.report.armed = false;
        let Ok(mut runtime) = entry.runtime.lock() else {
            lease.report.physical_stop_pending = true;
            return false;
        };
        let clean = !first_retirement
            || match runtime.revoke_controller() {
                Ok(None) => true,
                Ok(Some(effect)) => {
                    entry.pending_controller_stop = Some(effect);
                    false
                }
                Err(_) => false,
            };
        lease.report.physical_stop_pending = runtime.session().physical_stop_pending();
        return clean
            && !lease.report.physical_stop_pending
            && entry.pending_controller_stop.is_none()
            && !entry.execution_pending.load(Ordering::Acquire);
    }
    !entry.execution_pending.load(Ordering::Acquire)
}
impl Registry {
    fn sweep_controllers<F: FnOnce() -> PResult<u64>>(&mut self, clock: F) -> PResult<u64> {
        let now = clock();
        if now.as_ref().is_err()
            || now
                .as_ref()
                .is_ok_and(|n| self.controller_tick.is_some_and(|last| *n < last))
        {
            self.controller_clock_failed = true;
        }
        if self.controller_clock_failed {
            self.syncs.clear();
            self.entries
                .retain(|_, e| e.controller.is_none() || !retire_entry(e));
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "controller clock domain failed; restart required",
            ));
        }
        let now = now?;
        self.controller_tick = Some(now);
        self.prune_syncs(now);
        let syncs = &self.syncs;
        self.entries.retain(|_, e| {
            !e.controller.as_ref().is_some_and(|l| {
                l.report.state == HandyControllerState::Retired
                    || now >= l.report.deadline_boottime_ns
                    || e.browser_owner
                        .as_ref()
                        .is_some_and(|owner| !owner.current(e, now))
                    || e.player_guard.as_ref().is_some_and(|g| !g.current(e, now))
                    || (l.report.state == HandyControllerState::Playing
                        && !e.playing_clock_current(now, syncs))
            }) || !retire_entry(e)
        });
        Ok(now)
    }
    fn retire_controller(
        &mut self,
        preparation: &ArtifactId,
        controller: &ClientId,
    ) -> Option<HandyControllerLease> {
        let e = self.entries.get_mut(preparation)?;
        if e.controller.as_ref()?.report.controller_id != *controller {
            return None;
        }
        let remove = retire_entry(e);
        let report = e.controller.as_ref()?.report.clone();
        if remove {
            self.entries.remove(preparation);
        }
        Some(report)
    }
}
impl ControllerMonitor {
    fn start(registry: Arc<Mutex<Registry>>) -> PResult<Self> {
        Self::start_with_clock(registry, |_| boottime_ns())
    }
    fn start_with_clock<F>(registry: Arc<Mutex<Registry>>, clock: F) -> PResult<Self>
    where
        F: Fn(&Registry) -> PResult<u64> + Send + 'static,
    {
        #[cfg(test)]
        if FAIL_MONITOR.with(|v| v.replace(false)) {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "injected controller monitor spawn failure",
            ));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let thread = thread::Builder::new()
            .name("pulsar-handy-lease".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                while !cancelled.load(Ordering::Acquire) {
                    thread::park_timeout(Duration::from_millis(50));
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let mut r = registry.lock().unwrap_or_else(|e| {
                        let mut r = e.into_inner();
                        r.controller_clock_failed = true;
                        r
                    });
                    let now = clock(&r);
                    let _ = r.sweep_controllers(|| now);
                }
            })
            .map_err(|_| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "controller expiry worker unavailable",
                )
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
    fn wake(&self) {
        if let Some(t) = &self.thread {
            t.thread().unpark();
        }
    }
    pub(in crate::authority) fn shutdown(&mut self) -> PResult<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            t.join().map_err(|_| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "controller monitor panicked during retirement",
                )
            })?;
        }
        Ok(())
    }
    #[cfg(test)]
    pub(in crate::authority) fn panicked_for_test() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            thread: Some(thread::spawn(|| {
                panic!("synthetic controller-monitor failure")
            })),
        }
    }
}
impl Drop for ControllerMonitor {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn deadline(now: u64, duration: u64) -> PResult<u64> {
    now.checked_add(
        duration
            .checked_mul(1_000_000)
            .ok_or_else(|| internal("lease duration overflow"))?,
    )
    .ok_or_else(|| internal("lease deadline overflow"))
}
fn receipt(id: &RequestId, lease: HandyControllerLease) -> PResult<ResponseBody> {
    let value = HandyControllerReceipt {
        request_id: id.clone(),
        lease,
    };
    value.validate()?;
    Ok(ResponseBody::HandyController(Box::new(value)))
}
fn load(
    db: &Connection,
    actor: &SessionId,
    project_id: &ProjectId,
    original: &RequestId,
) -> PResult<HandyControllerLease> {
    let raw:Option<String>=db.query_row("SELECT CASE WHEN length(CAST(response AS BLOB))<=16384 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",params![actor.as_str(),original.as_str()],|r|r.get(0)).optional().map_err(internal)?;
    let raw = raw.ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, "controller acquisition not found")
    })?;
    let ResponseBody::HandyController(value) = decode(&raw)? else {
        return Err(internal("request is not a controller acquisition"));
    };
    value.validate()?;
    if value.request_id != *original
        || value.lease.acquisition_request_id != *original
        || value.lease.project_id != *project_id
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "controller acquisition selection differs",
        ));
    }
    Ok(value.lease)
}
impl Engine {
    fn check_handy_controller_monitor(&self) -> PResult<bool> {
        let slot = self.handy_controller_monitor.lock().map_err(internal)?;
        let healthy = slot
            .as_ref()
            .is_some_and(|m| m.thread.as_ref().is_some_and(|t| !t.is_finished()))
            && self
                .handy_execution
                .lock()
                .map_err(internal)?
                .as_ref()
                .is_some_and(ExecutionPump::is_running);
        if !healthy {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            if registry.entries.values().any(|e| {
                e.controller
                    .as_ref()
                    .is_some_and(|l| l.report.state != HandyControllerState::Retired)
            }) {
                registry.controller_clock_failed = true;
                let _ = registry.sweep_controllers(boottime_ns);
                return Ok(false);
            }
        }
        Ok(true)
    }
    #[cfg(test)]
    pub(in crate::authority) fn fail_next_handy_controller_monitor(&self) {
        FAIL_MONITOR.with(|v| v.set(true));
    }

    pub(in crate::authority) fn ensure_handy_controller_monitor(&self) -> PResult<()> {
        let mut slot = self.handy_controller_monitor.lock().map_err(internal)?;
        if let Some(m) = slot.as_ref() {
            if m.thread.as_ref().is_none_or(|t| t.is_finished()) {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "controller monitor stopped",
                ));
            }
        } else {
            *slot = Some(ControllerMonitor::start(self.handy_preparations.clone())?);
        }
        self.ensure_handy_execution()?;
        Ok(())
    }
    fn wake_controller_monitor(&self) {
        if let Ok(slot) = self.handy_controller_monitor.lock() {
            if let Some(m) = slot.as_ref() {
                m.wake();
            }
        }
    }
    pub(in crate::authority) fn handy_controller_control(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
        }
        self.check_handy_controller_monitor()?;
        if matches!(r.command, Command::AcquireHandyController { .. }) {
            boottime_ns()?;
            self.ensure_handy_controller_monitor()?;
        }
        self.handy_controller_with(
            r,
            actor,
            fingerprint,
            |h, d| {
                h.recheck(d).map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "controller hosted script recheck failed",
                    )
                })
            },
            crate::handy_online::inspect_program,
            boottime_ns,
        )
    }
    fn controller_view<F: FnMut() -> PResult<u64>>(
        &self,
        db: &Connection,
        actor: &SessionId,
        saved: HandyControllerLease,
        r: &Request,
        clock: &mut F,
    ) -> PResult<HandyControllerLease> {
        saved.validate()?;
        let prep = {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let _ = registry.sweep_controllers(&mut *clock);
            registry
                .entries
                .get(&saved.preparation_id)
                .filter(|e| {
                    e.owner == *actor
                        && e.controller
                            .as_ref()
                            .is_some_and(|l| l.report.controller_id == saved.controller_id)
                })
                .map(|e| (e.report.clone(), e.play.clone()))
        };
        let Some((prep, owned)) = prep else {
            return Ok(retired(saved));
        };
        let checked = if let Some(pins) = &owned {
            play::owned_references(db, &self.pool, pins).and_then(|witness| {
                let h = self
                    ._device_effect_journal
                    .lock()
                    .map_err(internal)?
                    .play_request_history(actor, &pins.record.request_id)
                    .map_err(internal)?;
                let h = h.ok_or_else(|| {
                    ProtocolError::new(ErrorCode::DependencyMismatch, "owned play history missing")
                })?;
                h.check_witness(&witness).map_err(internal)?;
                if h.id != pins.id {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "owned play history missing",
                    ));
                }
                Ok((pins.captured.clone(), true))
            })
        } else {
            self.preparation_references(db, actor, &prep, r)
        };
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let _ = registry.sweep_controllers(clock);
        let valid = registry
            .entries
            .get(&saved.preparation_id)
            .is_some_and(|e| {
                let Some(l) = e.controller.as_ref() else {
                    return false;
                };
                let Ok(runtime) = e.runtime.lock() else {
                    return false;
                };
                checked.as_ref().is_ok_and(|(current, history)| {
                    ((!*history
                        && l.report.state == HandyControllerState::Active
                        && runtime.session().state() == DeviceState::Armed)
                        || (*history
                            && l.report.state == HandyControllerState::Playing
                            && runtime.session().state() == DeviceState::Playing
                            && e.play
                                .as_ref()
                                .zip(owned.as_ref())
                                .is_some_and(|(a, b)| Arc::ptr_eq(a, b))))
                        && Arc::ptr_eq(&e.captured.credentials, &current.credentials)
                        && e.captured.hosted.same_location(&current.hosted)
                        && runtime.session().controller() == Some(&l.report.controller_id)
                        && runtime.session().admission() == Some(&e.report.admission)
                }) && l.report.controller_id == saved.controller_id
                    && l.report.generation >= saved.generation
                    && l.report.state != HandyControllerState::Retired
            });
        if !valid {
            return Ok(registry
                .retire_controller(&saved.preparation_id, &saved.controller_id)
                .unwrap_or_else(|| retired(saved)));
        }
        Ok(registry.entries[&saved.preparation_id]
            .controller
            .as_ref()
            .expect("checked controller")
            .report
            .clone())
    }
    pub(in crate::authority) fn handy_controller_with<R, D, F>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        recheck: R,
        device: D,
        mut clock: F,
    ) -> PResult<ResponseBody>
    where
        R: FnOnce(&VerifiedHosted, Instant) -> PResult<ArtifactIdentity>,
        D: FnMut(
            Arc<Credentials>,
            Instant,
        ) -> PResult<(HandySetupDeviceObservation, HandyHspBuffer)>,
        F: FnMut() -> PResult<u64>,
    {
        r.validate()?;
        {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            if let Command::HandyControllerStatus {
                acquisition_request_id,
                ..
            } = &r.command
            {
                let saved = load(&db, actor, project(r)?, acquisition_request_id)?;
                return receipt(
                    &r.request_id,
                    self.controller_view(&db, actor, saved, r, &mut clock)?,
                );
            }
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::HandyController(saved) = body else {
                    return Err(internal("controller replay type differs"));
                };
                saved.validate()?;
                if saved.request_id != r.request_id || saved.lease.project_id != *project(r)? {
                    return Err(internal("controller replay identity differs"));
                }
                let matches = match &r.command {
                    Command::AcquireHandyController {
                        preparation_request_id,
                        preparation_id,
                        duration_ms,
                        ..
                    } => {
                        saved.lease.acquisition_request_id == r.request_id
                            && saved.lease.preparation_request_id == *preparation_request_id
                            && saved.lease.preparation_id == *preparation_id
                            && saved.lease.duration_ms == *duration_ms
                            && Some(saved.lease.revision) == r.expected_revision
                    }
                    Command::RenewHandyController {
                        acquisition_request_id,
                        controller_id,
                        expected_generation,
                        ..
                    } => {
                        saved.lease.acquisition_request_id == *acquisition_request_id
                            && saved.lease.controller_id == *controller_id
                            && saved.lease.generation == expected_generation + 1
                            && Some(saved.lease.revision) == r.expected_revision
                    }
                    Command::ReleaseHandyController {
                        acquisition_request_id,
                        controller_id,
                        ..
                    } => {
                        saved.lease.acquisition_request_id == *acquisition_request_id
                            && saved.lease.controller_id == *controller_id
                            && saved.lease.state == HandyControllerState::Retired
                    }
                    _ => false,
                };
                if !matches {
                    return Err(internal("controller replay selection differs"));
                }
                if matches!(r.command, Command::ReleaseHandyController { .. }) {
                    self.handy_preparations
                        .lock()
                        .map_err(internal)?
                        .retire_controller(&saved.lease.preparation_id, &saved.lease.controller_id);
                    return Ok(ResponseBody::HandyController(saved));
                }
                return receipt(
                    &r.request_id,
                    self.controller_view(&db, actor, saved.lease, r, &mut clock)?,
                );
            }
            if let Command::ReleaseHandyController {
                acquisition_request_id,
                controller_id,
                ..
            } = &r.command
            {
                let mut saved = load(&db, actor, project(r)?, acquisition_request_id)?;
                if saved.controller_id != *controller_id {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "controller release identity differs",
                    ));
                }
                {
                    let registry = self.handy_preparations.lock().map_err(internal)?;
                    if let Some(l) = registry
                        .entries
                        .get(&saved.preparation_id)
                        .and_then(|e| e.controller.as_ref())
                        .filter(|l| l.report.controller_id == *controller_id)
                    {
                        saved = l.report.clone();
                    }
                }
                let body = receipt(&r.request_id, retired(saved.clone()))?;
                let tx = db.transaction().map_err(internal)?;
                remember(&tx, actor.as_str(), r, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                self.handy_preparations
                    .lock()
                    .map_err(internal)?
                    .retire_controller(&saved.preparation_id, controller_id);
                return Ok(body);
            }
            if let Command::RenewHandyController {
                acquisition_request_id,
                controller_id,
                expected_generation,
                ..
            } = &r.command
            {
                let saved = load(&db, actor, project(r)?, acquisition_request_id)?;
                if saved.controller_id != *controller_id {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "controller renewal identity differs",
                    ));
                }
                let live = self.controller_view(&db, actor, saved, r, &mut clock)?;
                if live.state == HandyControllerState::Retired
                    || live.generation != *expected_generation
                    || Some(live.revision) != r.expected_revision
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "controller expired, retired or renewal generation changed",
                    ));
                }
                let now = self
                    .handy_preparations
                    .lock()
                    .map_err(internal)?
                    .sweep_controllers(&mut clock)?;
                let mut next = live.clone();
                next.generation += 1;
                next.deadline_boottime_ns = deadline(now, next.duration_ms)?;
                let body = receipt(&r.request_id, next.clone())?;
                let tx = db.transaction().map_err(internal)?;
                remember(&tx, actor.as_str(), r, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                {
                    let mut registry = self.handy_preparations.lock().map_err(internal)?;
                    let _ = registry.sweep_controllers(&mut clock);
                    if let Some(l) = registry
                        .entries
                        .get_mut(&next.preparation_id)
                        .and_then(|e| e.controller.as_mut())
                        .filter(|l| {
                            l.report.controller_id == *controller_id
                                && l.report.generation == *expected_generation
                                && l.report.state != HandyControllerState::Retired
                        })
                    {
                        l.report = next.clone();
                        registry
                            .entries
                            .get_mut(&next.preparation_id)
                            .expect("checked lease")
                            .server_clock = None;
                        registry
                            .entries
                            .get_mut(&next.preparation_id)
                            .expect("checked lease")
                            .playback_clock = None;
                    } else {
                        return receipt(&r.request_id, retired(next));
                    }
                }
                self.wake_controller_monitor();
                return Ok(body);
            }
        }
        let Command::AcquireHandyController {
            preparation_request_id,
            preparation_id,
            duration_ms,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::unsupported("Handy controller command"));
        };
        let prepared = {
            let db = self.db.lock().map_err(internal)?;
            let saved = Self::saved_preparation(&db, actor, project(r)?, preparation_request_id)?;
            self.preparation_view(&db, actor, saved, r)?
        };
        if prepared.preparation_id != *preparation_id
            || prepared.state != HandyPlaybackPreparationState::Prepared
            || prepared.physical_stop_pending
            || Some(prepared.admission.revision) != r.expected_revision
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "controller needs a current nonplaying preparation without stopping uncertainty",
            ));
        }
        let query = self.preparation_inspection(r, &prepared.selection()?);
        self.inspect_handy_program_with(&query, actor, recheck, device)?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        check_request_identity(&db, actor.as_str(), r, fingerprint)?;
        if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
            let ResponseBody::HandyController(saved) = body else {
                return Err(internal("controller acquisition race differs"));
            };
            return receipt(
                &r.request_id,
                self.controller_view(&db, actor, saved.lease, r, &mut clock)?,
            );
        }
        let prepared = self.preparation_view(&db, actor, prepared, r)?;
        if prepared.state != HandyPlaybackPreparationState::Prepared
            || prepared.physical_stop_pending
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "preparation retired before controller installation",
            ));
        }
        let now = self
            .handy_preparations
            .lock()
            .map_err(internal)?
            .sweep_controllers(&mut clock)?;
        let i = &prepared.correlation.inspection;
        let controller_id = ClientId::new(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(actor, &self.engine_epoch, &r.request_id, preparation_id))
                    .map_err(internal)?
            )
        ))
        .map_err(internal)?;
        let report = HandyControllerLease {
            acquisition_request_id: r.request_id.clone(),
            controller_id: controller_id.clone(),
            preparation_request_id: preparation_request_id.clone(),
            preparation_id: preparation_id.clone(),
            project_id: project(r)?.clone(),
            revision: prepared.admission.revision,
            engine_epoch: i.engine_epoch.clone(),
            device_session: i.device_session.clone(),
            device_id: i.registration.device_id.clone(),
            generation: 0,
            duration_ms: *duration_ms,
            deadline_boottime_ns: deadline(now, *duration_ms)?,
            state: HandyControllerState::Active,
            armed: true,
            physical_stop_pending: false,
            playback_enabled: false,
        };
        let body = receipt(&r.request_id, report.clone())?;
        let tx = db.transaction().map_err(internal)?;
        remember(&tx, actor.as_str(), r, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let current = registry.sweep_controllers(&mut clock);
            if current.as_ref().is_err()
                || current
                    .as_ref()
                    .is_ok_and(|n| *n >= report.deadline_boottime_ns)
            {
                registry.entries.remove(preparation_id);
                return receipt(&r.request_id, retired(report));
            }
            let Some(e) = registry
                .entries
                .get_mut(preparation_id)
                .filter(|e| e.owner == *actor && e.controller.is_none())
            else {
                return receipt(&r.request_id, retired(report));
            };
            if e.runtime
                .lock()
                .map_err(internal)?
                .arm(controller_id, e.report.admission.clone(), None)
                .is_err()
            {
                registry.entries.remove(preparation_id);
                return receipt(&r.request_id, retired(report));
            }
            e.controller = Some(Lease { report });
        }
        self.wake_controller_monitor();
        Ok(body)
    }
}
