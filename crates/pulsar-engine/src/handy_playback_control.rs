//! Original play requests and native authority. History never enqueues work.
use super::*;
use crate::device_effect_journal::{
    PlayClockRecord, PlayOperationObservation, PlayOperationRecord, PlayOperationState,
    PreparedPlayReceipt,
};
use crate::playback_runtime::{
    dispatch::{ClaimGate, ClaimedHandoff, DispatchAuthority, DispatchFailure},
    PlaybackEffect, PlaybackFailure,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(in crate::authority::handy_playback_preparation) type Clock =
    Arc<dyn Fn() -> PResult<u64> + Send + Sync>;
pub(in crate::authority::handy_playback_preparation) struct PlayPins {
    pub record: PlayOperationRecord,
    pub id: String,
    pub captured: handy_setup::Captured,
    pub preparation: HandyPlaybackPreparation,
    pub sync: Arc<native_sync::Context>,
    pub device_clock: crate::handy_playback_clock::NativeDeviceClock,
    pub server_error_ns: u64,
    pub route: String,
    pub namespace: String,
    pub browser: Option<Arc<browser_play::BrowserPlay>>,
}
impl PlayPins {
    pub(super) fn continuous_reference(&self) -> HandyContinuousTimeReference {
        HandyContinuousTimeReference {
            start_time_ms: self.record.start_time_ms,
            server_time_ms: self.record.clock.estimated_server_ms,
            uncertainty_ns: self.record.clock.uncertainty_ns,
        }
    }
    pub(super) fn clock_current(
        &self,
        now: u64,
        syncs: &HashMap<ArtifactId, native_sync::Slot>,
    ) -> bool {
        syncs
            .get(&self.sync.id)
            .is_some_and(|s| s.native.is_some() && Arc::ptr_eq(&s.context, &self.sync))
            && self.device_clock.error_at(now).is_ok_and(|device| {
                u128::from(device) + u128::from(self.server_error_ns)
                    <= u128::from(self.device_clock.qualified.maximum_total_error_ns)
            })
    }
}
pub(in crate::authority::handy_playback_preparation) struct PlayRequest {
    pub pins: Arc<PlayPins>,
    pub effect: PlaybackEffect,
    pub prepared: PreparedPlayReceipt,
    pub request: Request,
    pub clock: Clock,
}
pub(in crate::authority::handy_playback_preparation) struct PlayAuthority {
    pub shutdown_requested: Arc<AtomicBool>,
    pub db: Arc<Mutex<Connection>>,
    pub registry: Arc<Mutex<Registry>>,
    pub journal: Arc<Mutex<crate::device_effect_journal::DeviceEffectJournal>>,
    pub pool: ResourcePool,
    pub runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    pub pins: Arc<PlayPins>,
    pub request: Request,
    pub clock: Clock,
}
fn unavailable() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "original Handy play authority is unavailable or changed",
    )
}
fn history(
    project: ProjectId,
    request: RequestId,
    observed: Option<PlayOperationObservation>,
) -> PResult<HandyPlaybackStatus> {
    let value = if let Some(observed) = observed {
        observed.status().map_err(internal)?
    } else {
        HandyPlaybackStatus {
            project_id: project.clone(),
            play_request_id: request.clone(),
            history: None,
        }
    };
    if value.project_id != project || value.play_request_id != request {
        return Err(unavailable());
    }
    value.validate()?;
    Ok(value)
}
fn save_play_witness(
    db: &mut Connection,
    r: &Request,
    actor: &SessionId,
    fingerprint: &str,
    value: &HandyPlaybackStatus,
) -> PResult<()> {
    value.validate()?;
    if let Some(body) = replay(db, actor.as_str(), r, fingerprint)? {
        let ResponseBody::HandyPlayback(previous) = body else {
            return Err(unavailable());
        };
        previous.validate()?;
        if previous.project_id != value.project_id
            || previous.play_request_id != value.play_request_id
            || previous.history.as_ref().is_some_and(|h| {
                value
                    .history
                    .as_ref()
                    .is_none_or(|v| v.record_id != h.record_id)
            })
        {
            return Err(unavailable());
        }
        if encode(&*previous)? == encode(value)? {
            return Ok(());
        }
        let tx = db.transaction().map_err(internal)?;
        let changed = tx
            .execute(
                "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(&ResponseBody::HandyPlayback(Box::new(value.clone())))?,
                    actor.as_str(),
                    r.request_id.as_str()
                ],
            )
            .map_err(internal)?;
        if changed != 1 {
            return Err(internal("play witness reservation disappeared"));
        }
        tx.commit().map_err(internal)
    } else {
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            actor.as_str(),
            r,
            fingerprint,
            &ResponseBody::HandyPlayback(Box::new(value.clone())),
        )?;
        tx.commit().map_err(internal)
    }
}
fn selected(r: &Request, record: &PlayOperationRecord) -> bool {
    if r.project.as_ref() != Some(&record.original_controller.project_id)
        || r.expected_revision != Some(record.original_controller.revision)
    {
        return false;
    }
    match &r.command {
        Command::StartHandyPlayback {
            acquisition_request_id,
            controller_id,
            expected_generation,
            clock_request_id,
            start_time_ms,
            ..
        } => {
            record.browser_start.is_none()
                && record.original_controller.acquisition_request_id == *acquisition_request_id
                && record.original_controller.controller_id == *controller_id
                && record.original_controller.generation == *expected_generation
                && record.clock_request_id == *clock_request_id
                && record.start_time_ms == *start_time_ms
        }
        Command::StartHandyPlaybackFromBrowser {
            acquisition_request_id,
            controller_id,
            expected_generation,
            clock_request_id,
            browser_timing_binding_id,
            qualification,
            ..
        } => {
            record.original_controller.acquisition_request_id == *acquisition_request_id
                && record.original_controller.controller_id == *controller_id
                && record.original_controller.generation >= *expected_generation
                && record.clock_request_id == *clock_request_id
                && record.browser_start.as_ref().is_some_and(|b| {
                    b.timing_binding.binding_id == *browser_timing_binding_id
                        && b.qualification == *qualification
                })
        }
        _ => false,
    }
}
pub(in crate::authority) fn validate_play_journal_links(
    db: &Connection,
    state_dir: &std::path::Path,
) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='requests')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut q=db.prepare("SELECT DISTINCT CASE WHEN length(CAST(session AS BLOB))<=128 THEN session ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=16384 THEN response ELSE NULL END FROM requests WHERE CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='handy_playback' AND json_type(response,'$.value.history') IS NOT 'null' ELSE response LIKE '%handy_playback%' END LIMIT 4097")?;
    let mut witnesses = Vec::new();
    for result in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (actor, raw) = result?;
        let ResponseBody::HandyPlayback(value) = serde_json::from_str(&raw)? else {
            anyhow::bail!("invalid play witness");
        };
        value.validate()?;
        if value.history.is_none() {
            anyhow::bail!("play witness missing");
        }
        witnesses.push((SessionId::new(actor)?, *value));
    }
    anyhow::ensure!(witnesses.len() <= 4096, "play witness capacity exceeded");
    crate::device_effect_journal::DeviceEffectJournal::verify_play_statuses(state_dir, &witnesses)
}
fn references(
    db: &Connection,
    pool: &ResourcePool,
    pins: &PlayPins,
) -> PResult<HandyPlaybackStatus> {
    let r = &pins.record;
    let c = &r.original_controller;
    require_grant(db, &r.actor_session, &c.project_id, Scope::Read)?;
    require_grant(db, &r.actor_session, &c.project_id, Scope::Export)?;
    let mut query = Request::new(
        RequestId::new("play-current-revision").expect("constant ID"),
        Command::GetSnapshot,
    );
    query.project = Some(c.project_id.clone());
    query.expected_revision = Some(c.revision);
    check_revision(db, &query)?;
    let registration = handy_registrations::current(
        db,
        &c.device_id,
        pins.captured.connection.registration.revision,
    )?;
    if registration.disabled || registration != pins.captured.connection.registration {
        return Err(unavailable());
    }
    let _memory = pool.reserve_memory(16 * 1024 * 1024)?;
    let timing = handy_qualification::accepted_timing(
        db,
        &r.qualification,
        &registration,
        &pins.preparation.correlation.inspection.device,
        &pins.captured.hosted,
    )?;
    if timing.configuration() != &r.effect.admission.configuration {
        return Err(unavailable());
    }
    if let Some(browser) = &pins.browser {
        crate::authority::handy_browser_start_qualification::revalidate_accepted_browser_start(
            db,
            pool,
            &browser.qualification,
        )?;
    }
    let (fingerprint,raw):(String,String)=db.query_row("SELECT CASE WHEN length(fingerprint)=64 THEN fingerprint ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=16384 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",params![r.actor_session.as_str(),r.request_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
    if fingerprint != r.request_fingerprint {
        return Err(unavailable());
    }
    let ResponseBody::HandyPlayback(saved) = decode(&raw)? else {
        return Err(unavailable());
    };
    saved.validate()?;
    if saved.project_id != c.project_id
        || saved.play_request_id != r.request_id
        || saved
            .history
            .as_ref()
            .is_none_or(|h| h.record_id.as_str() != pins.id)
    {
        return Err(unavailable());
    }
    Ok(*saved)
}
pub(super) fn owned_references(
    db: &Connection,
    pool: &ResourcePool,
    pins: &PlayPins,
) -> PResult<HandyPlaybackStatus> {
    references(db, pool, pins)
}
fn bounded_lock<'a, T>(
    lock: &'a Mutex<T>,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<std::sync::MutexGuard<'a, T>, DispatchFailure> {
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(DispatchFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(DispatchFailure::InvalidBudget);
        }
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(DispatchFailure::WorkerUnavailable)
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(1))
            }
        }
    }
}
impl PlayAuthority {
    pub fn claim(
        &self,
        deadline: Instant,
        cancel: &AtomicBool,
        gate: ClaimGate<'_>,
    ) -> Result<ClaimedHandoff, DispatchFailure> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(DispatchFailure::Cancelled);
        }
        // Fail before waiting for metadata if the native clock is unavailable;
        // authority still resamples under the mutation guard immediately before claim.
        (self.clock)().map_err(|_| DispatchFailure::Authority(PlaybackFailure::InvalidClock))?;
        let denied = || DispatchFailure::Authority(PlaybackFailure::AdmissionChanged);
        let db = bounded_lock(&self.db, deadline, cancel)?;
        authenticate_session(
            &db,
            &self.pins.record.actor_session,
            self.request.auth_token.as_deref(),
        )
        .map_err(|_| denied())?;
        let witness = references(&db, &self.pool, &self.pins).map_err(|_| denied())?;
        check_request_identity(
            &db,
            self.pins.record.actor_session.as_str(),
            &self.request,
            &self.pins.record.request_fingerprint,
        )
        .map_err(|_| denied())?;
        {
            let journal = bounded_lock(&self.journal, deadline, cancel)?;
            let h = journal
                .play_request_history(
                    &self.pins.record.actor_session,
                    &self.pins.record.request_id,
                )
                .map_err(|_| denied())?
                .ok_or_else(denied)?;
            h.check_witness(&witness).map_err(|_| denied())?;
            if h.id != self.pins.id || h.state != PlayOperationState::Claimed {
                return Err(denied());
            }
            journal
                .play_route_available(
                    &self.pins.route,
                    &self.pins.namespace,
                    &self.pins.record.original_controller.device_id,
                    Some(&self.pins.id),
                )
                .map_err(|_| denied())?;
            journal
                .clock_sync_route_available(
                    &self.pins.route,
                    &self.pins.namespace,
                    &self.pins.record.original_controller.device_id,
                    None,
                )
                .map_err(|_| denied())?;
        }
        let _browser_claim = self
            .pins
            .browser
            .as_ref()
            .map(|browser| {
                browser
                    .timing
                    .claim(deadline, cancel, || (self.clock)())
                    .map_err(|_| denied())
            })
            .transpose()?;
        let mut registry = bounded_lock(&self.registry, deadline, cancel)?;
        let now = registry
            .sweep_controllers(|| (self.clock)())
            .map_err(|_| denied())?;
        let original = &self.pins.record.original_controller;
        let e = registry
            .entries
            .get(&original.preparation_id)
            .ok_or_else(denied)?;
        let l = e.controller.as_ref().ok_or_else(denied)?;
        let c = e.playback_clock.as_ref().ok_or_else(denied)?;
        if e.owner != self.pins.record.actor_session
            || !Arc::ptr_eq(&e.runtime, &self.runtime)
            || e.play.as_ref().is_none_or(|p| !Arc::ptr_eq(p, &self.pins))
            || l.report.state != HandyControllerState::Playing
            || l.report.controller_id != original.controller_id
            || if let Some(browser) = &self.pins.browser {
                l.report.generation < original.generation || !browser.owner.current(e, now)
            } else {
                l.report.generation != original.generation
            }
            || now >= l.report.deadline_boottime_ns
            || c.report.request_id != self.pins.record.clock_request_id
            || !playback_clock::sync_current(&registry, &c.sync)
            || c.native.at(now).is_err()
            || !Arc::ptr_eq(&e.captured.credentials, &self.pins.captured.credentials)
            || !e.captured.hosted.same_location(&self.pins.captured.hosted)
        {
            return Err(denied());
        }
        if let Some(browser) = &self.pins.browser {
            browser
                .original_claim_current(&self.pins, now, l.report.deadline_boottime_ns)
                .map_err(|_| denied())?;
            browser
                .alignment(&self.pins, None, now, l.report.deadline_boottime_ns)
                .map_err(|_| denied())?;
        }
        // DB, exact browser lifetime and registry guards survive through claim.
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(DispatchFailure::Cancelled);
        }
        gate.apply(DispatchAuthority {
            admission: self.pins.record.effect.admission.clone(),
            controller: Some(original.controller_id.clone()),
            clock_domain: self.pins.record.clock.domain.clone(),
            observed_tick_ns: now,
            lease_deadline_ns: l.report.deadline_boottime_ns,
        })
    }
}
impl Engine {
    pub(in crate::authority) fn handy_playback_control(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_playback_with(r, actor, fingerprint, Arc::new(boottime_ns), |e| {
            if !e.check_handy_controller_monitor()? {
                return Err(unavailable());
            }
            Ok(())
        })
    }
    fn play_status(
        &self,
        db: &Connection,
        r: &Request,
        actor: &SessionId,
        id: &RequestId,
    ) -> PResult<ResponseBody> {
        let observed = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .play_request_history(actor, id)
            .map_err(internal)?;
        let saved:Option<String>=db.query_row("SELECT CASE WHEN length(CAST(response AS BLOB))<=16384 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",params![actor.as_str(),id.as_str()],|r|r.get(0)).optional().map_err(internal)?;
        if let Some(raw) = saved {
            let ResponseBody::HandyPlayback(witness) = decode(&raw)? else {
                return Err(unavailable());
            };
            witness.validate()?;
            if witness.project_id != *project(r)? || witness.play_request_id != *id {
                return Err(unavailable());
            }
            if witness.history.is_some() {
                observed
                    .as_ref()
                    .ok_or_else(unavailable)?
                    .check_witness(&witness)
                    .map_err(internal)?;
            }
        }
        Ok(ResponseBody::HandyPlayback(Box::new(history(
            project(r)?.clone(),
            id.clone(),
            observed,
        )?)))
    }
    pub(in crate::authority) fn handy_playback_with<F: FnOnce(&Engine) -> PResult<()>>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        clock: Clock,
        preflight: F,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        if let Command::HandyPlaybackStatus {
            play_request_id, ..
        } = &r.command
        {
            return self.play_status(&db, r, actor, play_request_id);
        }
        check_request_identity(&db, actor.as_str(), r, fingerprint)?;
        if let Command::CancelHandyPlayback {
            play_request_id,
            record_id,
            ..
        } = &r.command
        {
            let observed = self
                ._device_effect_journal
                .lock()
                .map_err(internal)?
                .play_request_history(actor, play_request_id)
                .map_err(internal)?
                .ok_or_else(unavailable)?;
            if observed.id != record_id.as_str()
                || observed.record.original_controller.project_id != *project(r)?
            {
                return Err(unavailable());
            }
            if !matches!(
                observed.state,
                PlayOperationState::Prepared | PlayOperationState::Cancelled
            ) {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "only an exact unclaimed play can be cancelled",
                ));
            }
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::HandyPlayback(witness) = body else {
                    return Err(unavailable());
                };
                observed.check_witness(&witness).map_err(internal)?;
            } else {
                save_play_witness(
                    &mut db,
                    r,
                    actor,
                    fingerprint,
                    &observed.status().map_err(internal)?,
                )?;
            }
            self._device_effect_journal
                .lock()
                .map_err(internal)?
                .cancel_unclaimed_play(actor, play_request_id, record_id.as_str())
                .map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::Forbidden,
                        "only an exact unclaimed play can be cancelled",
                    )
                })?;
            self.retire_handy_preparations(Retirement::Id(
                &observed.record.original_controller.preparation_id,
            ));
            let body = self.play_status(&db, r, actor, play_request_id)?;
            let ResponseBody::HandyPlayback(status) = &body else {
                return Err(unavailable());
            };
            save_play_witness(&mut db, r, actor, fingerprint, status)?;
            return Ok(body);
        }
        let (
            acquisition_request_id,
            controller_id,
            expected_generation,
            clock_request_id,
            manual_start,
        ) = match &r.command {
            Command::StartHandyPlayback {
                acquisition_request_id,
                controller_id,
                expected_generation,
                clock_request_id,
                start_time_ms,
                ..
            } => (
                acquisition_request_id,
                controller_id,
                expected_generation,
                clock_request_id,
                Some(*start_time_ms),
            ),
            Command::StartHandyPlaybackFromBrowser {
                acquisition_request_id,
                controller_id,
                expected_generation,
                clock_request_id,
                ..
            } => (
                acquisition_request_id,
                controller_id,
                expected_generation,
                clock_request_id,
                None,
            ),
            _ => return Err(unavailable()),
        };
        let replayed = replay(&db, actor.as_str(), r, fingerprint)?;
        let observed = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .play_request_history(actor, &r.request_id)
            .map_err(internal)?;
        if replayed.is_some() || observed.is_some() {
            let previous = match replayed {
                Some(ResponseBody::HandyPlayback(saved)) => {
                    saved.validate()?;
                    if saved.project_id != *project(r)? || saved.play_request_id != r.request_id {
                        return Err(unavailable());
                    }
                    Some(saved)
                }
                Some(_) => return Err(unavailable()),
                None => None,
            };
            if let Some(h) = &observed {
                if h.record.request_fingerprint != fingerprint || !selected(r, &h.record) {
                    return Err(unavailable());
                }
                if let Some(saved) = previous.as_ref().filter(|s| s.history.is_some()) {
                    h.check_witness(saved).map_err(internal)?;
                }
            } else if previous.as_ref().is_some_and(|s| s.history.is_some()) {
                return Err(unavailable());
            }
            let status = history(project(r)?.clone(), r.request_id.clone(), observed)?;
            if previous.as_ref().is_none_or(|s| s.history.is_none()) && status.history.is_some() {
                save_play_witness(&mut db, r, actor, fingerprint, &status)?;
            }
            return Ok(ResponseBody::HandyPlayback(Box::new(status)));
        }
        preflight(self)?;
        self.ensure_handy_execution()?;
        let browser_owner = if manual_start.is_none() {
            Some(self.browser_start_owner(r, fingerprint)?)
        } else {
            None
        };
        let saved = load(&db, actor, project(r)?, acquisition_request_id)?;
        let mut now_clock = || clock();
        let live = self.controller_view(&db, actor, saved, r, &mut now_clock)?;
        if live.state != HandyControllerState::Active
            || live.controller_id != *controller_id
            || live.generation
                != browser_owner
                    .as_ref()
                    .map_or(*expected_generation, |owner| owner.selected_generation())
        {
            return Err(unavailable());
        }
        let browser_capture = if let Command::StartHandyPlaybackFromBrowser {
            browser_timing_binding_id,
            qualification,
            ..
        } = &r.command
        {
            let timing = self.capture_browser_timing_locked(&db, r, browser_timing_binding_id)?;
            let (hosting, device_qualification) = {
                let registry = self.handy_preparations.lock().map_err(internal)?;
                let entry = registry
                    .entries
                    .get(&live.preparation_id)
                    .ok_or_else(unavailable)?;
                (
                    entry.captured.hosting.clone(),
                    entry.report.selection()?.qualification,
                )
            };
            let accepted = self.capture_handy_browser_start_locked(
                &db,
                r,
                qualification,
                &timing.binding().browser_media,
                &hosting,
                &device_qualification,
            )?;
            Some((timing, accepted))
        } else {
            None
        };
        let initial = history(project(r)?.clone(), r.request_id.clone(), None)?;
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            actor.as_str(),
            r,
            fingerprint,
            &ResponseBody::HandyPlayback(Box::new(initial)),
        )?;
        tx.commit().map_err(internal)?;
        let binding = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .receipt_binding();
        let (route, namespace) = {
            let registry = self.handy_preparations.lock().map_err(internal)?;
            let e = registry
                .entries
                .get(&live.preparation_id)
                .ok_or_else(unavailable)?;
            let route = crate::HandyRouteIdentity::from_snapshot(
                &self.handy_owner_token,
                &e.captured.credentials.connection,
            )?;
            (
                route.route_sha256().to_owned(),
                route.namespace_sha256().to_owned(),
            )
        };
        self._device_effect_journal
            .lock()
            .map_err(internal)?
            .play_route_available(&route, &namespace, &live.device_id, None)
            .map_err(|_| unavailable())?;
        let no_cancel = AtomicBool::new(false);
        let browser_claim = browser_capture
            .as_ref()
            .map(|(timing, _)| {
                timing.claim(Instant::now() + Duration::from_secs(1), &no_cancel, || {
                    clock()
                })
            })
            .transpose()?;
        let (pins, effect, intent) = {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let now = registry.sweep_controllers(|| clock())?;
            let e = registry
                .entries
                .get(&live.preparation_id)
                .ok_or_else(unavailable)?;
            let c = e.playback_clock.as_ref().ok_or_else(unavailable)?;
            if c.report.request_id != *clock_request_id
                || !playback_clock::sync_current(&registry, &c.sync)
            {
                return Err(unavailable());
            }
            let live = e
                .controller
                .as_ref()
                .ok_or_else(unavailable)?
                .report
                .clone();
            if live.state != HandyControllerState::Active
                || live.controller_id != *controller_id
                || live.generation
                    != browser_owner
                        .as_ref()
                        .map_or(*expected_generation, |owner| owner.selected_generation())
                || browser_owner.as_ref().is_some_and(|owner| {
                    !owner.current(e, now)
                        || e.browser_owner
                            .as_ref()
                            .is_none_or(|o| !Arc::ptr_eq(o, owner))
                })
                || (browser_owner.is_none() && e.browser_owner.is_some())
            {
                return Err(unavailable());
            }
            let mut native = c.native.at(now)?;
            let selection = e.report.selection()?;
            let server_error_ns = c.native.server.at(now)?.uncertainty_ns;
            let shape = e.captured.hosted.sample_shape();
            let browser = if let Some((timing, accepted)) = &browser_capture {
                if !accepted.current() {
                    return Err(unavailable());
                }
                let observation = browser_claim
                    .as_ref()
                    .ok_or_else(unavailable)?
                    .observation
                    .clone();
                let alignment = accepted
                    .subject()
                    .timing
                    .phase_request(
                        &observation,
                        now,
                        accepted.script_quantization().max_time_error_ns,
                        native.uncertainty_ns,
                        native.valid_until_ns.min(live.deadline_boottime_ns),
                    )?
                    .calculate()
                    .map_err(internal)?;
                browser_play::check_phase_coverage(&alignment, timing.binding(), &shape)?;
                if alignment.total_error_ns > c.native.device.qualified.maximum_total_error_ns {
                    return Err(unavailable());
                }
                let decision = HandyBrowserStartDecision {
                    qualification: accepted.binding().clone(),
                    subject: accepted.subject().clone(),
                    timing_binding: timing.binding().clone(),
                    observation,
                    reference_boottime_ns: now,
                    alignment,
                };
                decision.validate()?;
                native.uncertainty_ns = decision.alignment.total_error_ns;
                native.valid_until_ns = native
                    .valid_until_ns
                    .min(decision.alignment.valid_until_boottime_ns);
                Some(Arc::new(browser_play::BrowserPlay {
                    owner: browser_owner.as_ref().ok_or_else(unavailable)?.clone(),
                    timing: timing.clone(),
                    qualification: accepted.clone(),
                    decision,
                    original_server: c.native.server.clone(),
                }))
            } else {
                None
            };
            let start_time_ms = browser
                .as_ref()
                .map(|b| b.decision.alignment.start_time_ms)
                .or(manual_start)
                .ok_or_else(unavailable)?;
            if start_time_ms < shape.first_time_ms || start_time_ms > shape.last_time_ms {
                return Err(ProtocolError::invalid(
                    "play start lies outside the verified script",
                ));
            }
            let mut runtime = e.runtime.lock().map_err(internal)?;
            let start = if let Some(browser) = &browser {
                runtime.prepare_referenced_start(
                    controller_id,
                    native.clone(),
                    start_time_ms,
                    browser.decision.subject.timing.media_rate_error_ppm,
                )
            } else {
                runtime.prepare_start(controller_id, native.clone(), start_time_ms)
            }
            .map_err(|_| unavailable())?;
            let effect = runtime
                .dispatch_start(
                    start,
                    &e.report.admission,
                    controller_id,
                    &native.domain,
                    now,
                )
                .map_err(|_| unavailable())?;
            let captured = (|| {
                let record = PlayOperationRecord {
                    actor_session: actor.clone(),
                    request_id: r.request_id.clone(),
                    request_fingerprint: fingerprint.into(),
                    original_controller: live.clone(),
                    qualification: selection.qualification,
                    setup_record_id: selection.record_id,
                    clock_request_id: clock_request_id.clone(),
                    clock: PlayClockRecord::from(&native),
                    start_time_ms,
                    browser_start: browser.as_ref().map(|value| value.decision.clone()),
                    effect: binding.record_for_effect(&effect).map_err(internal)?,
                };
                let intent = binding
                    .capture_play_intent(&runtime, &effect, record.clone())
                    .map_err(internal)?;
                let pins = Arc::new(PlayPins {
                    id: record.id().map_err(internal)?,
                    record,
                    captured: e.captured.clone(),
                    preparation: e.report.clone(),
                    sync: c.sync.clone(),
                    device_clock: c.native.device.clone(),
                    server_error_ns,
                    route,
                    namespace,
                    browser,
                });
                Ok::<_, ProtocolError>((pins, intent))
            })();
            drop(runtime);
            let e = registry
                .entries
                .get_mut(&live.preparation_id)
                .expect("checked preparation");
            let (pins, intent) = match captured {
                Ok(captured) => captured,
                Err(error) => {
                    retire_entry(e);
                    return Err(error);
                }
            };
            let l = e.controller.as_mut().ok_or_else(unavailable)?;
            l.report.state = HandyControllerState::Playing;
            l.report.physical_stop_pending = true;
            e.play = Some(pins.clone());
            (pins, effect, intent)
        };
        drop(browser_claim);
        let linked = (|| {
            let prepared = self
                ._device_effect_journal
                .lock()
                .map_err(internal)?
                .try_prepare_play(&intent)
                .map_err(|_| {
                    ProtocolError::new(ErrorCode::Unavailable, "play intent could not be recorded")
                })?;
            let body = self.play_status(&db, r, actor, &r.request_id)?;
            let ResponseBody::HandyPlayback(status) = &body else {
                return Err(unavailable());
            };
            save_play_witness(&mut db, r, actor, fingerprint, status)?;
            Ok::<_, ProtocolError>((prepared, body))
        })();
        let (prepared, body) = match linked {
            Ok(linked) => linked,
            Err(error) => {
                self.retire_handy_preparations(Retirement::Id(&live.preparation_id));
                return Err(error);
            }
        };
        {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let swept = registry.sweep_controllers(|| clock()).unwrap_or(u64::MAX);
            if let Some(e) = registry.entries.get_mut(&live.preparation_id).filter(|e| {
                e.controller.as_ref().is_some_and(|l| {
                    l.report.state == HandyControllerState::Playing
                        && if let Some(browser) = &pins.browser {
                            l.report.generation >= pins.record.original_controller.generation
                                && browser.owner.current(e, swept)
                        } else {
                            l.report.generation == *expected_generation
                        }
                })
            }) {
                if e.pending_play.is_none()
                    && e.play.as_ref().is_some_and(|p| Arc::ptr_eq(p, &pins))
                {
                    e.pending_play = Some(PlayRequest {
                        pins,
                        effect,
                        prepared,
                        request: r.clone(),
                        clock,
                    });
                    e.execution_pending.store(true, Ordering::Release);
                }
            }
        }
        Ok(body)
    }
}
